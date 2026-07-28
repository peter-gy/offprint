use std::path::Path;

use pageknot_model::{ErrorStage, ExportedArtifact, PageKnotError, PortablePath, Result};

use super::durable::{
    cleanup_transaction_root, create_directory, durable_hard_link, durable_rename, sync_directory,
};
use super::fault::{FaultAction, FaultInjector, FaultPoint, injected_fault};
use super::journal::{CommitMode, JournalPhase, JournalStore, OutputKind};
use super::plan::{
    StagedVariant, output_directory_is_empty, recheck_destinations, validate_complete_output_set,
    validate_entry_path, validate_staged_variants,
};
use super::recovery::{finalize_committed, rollback_transaction};

pub(super) struct CommitRequest<'a> {
    pub(super) output_directory: &'a Path,
    pub(super) root: &'a Path,
    pub(super) journal: &'a mut JournalStore,
    pub(super) variants: &'a [StagedVariant],
    pub(super) conflict: pageknot_model::ConflictPolicy,
    pub(super) mode: CommitMode,
    pub(super) maximum_assets: usize,
    pub(super) maximum_bytes: u64,
}

pub(super) fn commit_transaction(
    mut request: CommitRequest<'_>,
    injector: &mut impl FaultInjector,
) -> Result<Vec<ExportedArtifact>> {
    if let Err(error) = validate_before_mutation(&request) {
        return Err(clean_prepared_failure(&request, error));
    }
    if let Err(error) = request.journal.persist(JournalPhase::Mutating) {
        return Err(clean_prepared_failure(&request, error));
    }
    if let Some(action) = injector.action(FaultPoint::AfterJournal(JournalPhase::Mutating)) {
        let error = injected_fault(FaultPoint::AfterJournal(JournalPhase::Mutating), action);
        return handle_mutating_failure(&mut request, injector, error, action);
    }

    let mutation = match request.mode {
        CommitMode::Entries => commit_entries(&request, injector),
        CommitMode::DirectorySwap => commit_directory_swap(&request, injector),
    };
    if let Err((error, action)) = mutation {
        return handle_mutating_failure(&mut request, injector, error, action);
    }
    if let Err(error) = validate_committed_outputs(&request) {
        return handle_mutating_failure(&mut request, injector, error, FaultAction::Error);
    }
    if let Err(error) = sync_directory(request.output_directory, ErrorStage::Commit) {
        return handle_mutating_failure(&mut request, injector, error, FaultAction::Error);
    }
    if let Err(error) = request.journal.persist(JournalPhase::Committed) {
        return Err(commit_confirmation_error(request.root, error));
    }
    if let Some(action) = injector.action(FaultPoint::AfterJournal(JournalPhase::Committed)) {
        return Err(committed_recovery_error(
            request.root,
            injected_fault(FaultPoint::AfterJournal(JournalPhase::Committed), action),
        ));
    }
    if let Some(action) = injector.action(FaultPoint::BeforeCleanup) {
        return Err(committed_recovery_error(
            request.root,
            injected_fault(FaultPoint::BeforeCleanup, action),
        ));
    }

    let results = build_results(request.output_directory, request.variants)
        .map_err(|error| committed_recovery_error(request.root, error))?;
    finalize_committed(
        request.output_directory,
        request.root,
        request.journal.snapshot(),
        request.maximum_assets,
        request.maximum_bytes,
    )?;
    Ok(results)
}

fn validate_before_mutation(request: &CommitRequest<'_>) -> Result<()> {
    validate_staged_variants(
        request.variants,
        request.maximum_assets,
        request.maximum_bytes,
    )?;
    recheck_destinations(request.conflict, request.variants)?;
    if request.mode == CommitMode::DirectorySwap
        && !output_directory_is_empty(request.output_directory, ErrorStage::Commit)?
    {
        return Err(PageKnotError::new(
            "pageknot.export.output",
            ErrorStage::Commit,
            "artifact export directory changed before its directory-swap commit",
        ));
    }
    Ok(())
}

fn commit_entries(
    request: &CommitRequest<'_>,
    injector: &mut impl FaultInjector,
) -> std::result::Result<(), (PageKnotError, FaultAction)> {
    if request.variants.iter().any(|variant| variant.entry.existed) {
        let backups = request.root.join("backups");
        if let Err(error) = create_directory(&backups, ErrorStage::Commit) {
            return Err((error, FaultAction::Error));
        }
        for (index, variant) in request.variants.iter().enumerate() {
            if !variant.entry.existed {
                continue;
            }
            let backup = backups.join(format!("entry-{index}"));
            if let Err(error) = durable_rename(&variant.destination, &backup) {
                return Err((error, FaultAction::Error));
            }
            if let Some(action) = injector.action(FaultPoint::AfterBackup(index)) {
                return Err((
                    injected_fault(FaultPoint::AfterBackup(index), action),
                    action,
                ));
            }
        }
    }
    for (index, variant) in request.variants.iter().enumerate() {
        let result = match variant.entry.output_kind {
            OutputKind::File => durable_hard_link(&variant.staged_path, &variant.destination),
            OutputKind::Directory => durable_rename(&variant.staged_path, &variant.destination),
        };
        if let Err(error) = result {
            return Err((error, FaultAction::Error));
        }
        if let Some(action) = injector.action(FaultPoint::AfterCommit(index)) {
            return Err((
                injected_fault(FaultPoint::AfterCommit(index), action),
                action,
            ));
        }
    }
    Ok(())
}

fn commit_directory_swap(
    request: &CommitRequest<'_>,
    injector: &mut impl FaultInjector,
) -> std::result::Result<(), (PageKnotError, FaultAction)> {
    let previous = request.root.join("previous-output");
    if let Err(error) = durable_rename(request.output_directory, &previous) {
        return Err((error, FaultAction::Error));
    }
    if let Some(action) = injector.action(FaultPoint::AfterBackup(0)) {
        return Err((injected_fault(FaultPoint::AfterBackup(0), action), action));
    }
    match output_directory_is_empty(&previous, ErrorStage::Commit) {
        Ok(true) => {}
        Ok(false) => {
            return Err((
                PageKnotError::new(
                    "pageknot.export.output",
                    ErrorStage::Commit,
                    "artifact export directory changed during its directory-swap commit",
                ),
                FaultAction::Error,
            ));
        }
        Err(error) => return Err((error, FaultAction::Error)),
    }
    let staged_output = request.root.join("staged-output");
    if let Err(error) = durable_rename(&staged_output, request.output_directory) {
        return Err((error, FaultAction::Error));
    }
    if let Some(action) = injector.action(FaultPoint::AfterCommit(0)) {
        return Err((injected_fault(FaultPoint::AfterCommit(0), action), action));
    }
    Ok(())
}

fn validate_committed_outputs(request: &CommitRequest<'_>) -> Result<()> {
    match request.mode {
        CommitMode::Entries => {
            for variant in request.variants {
                validate_entry_path(
                    &variant.destination,
                    &variant.entry,
                    request.maximum_assets,
                    request.maximum_bytes,
                )?;
            }
            Ok(())
        }
        CommitMode::DirectorySwap => validate_complete_output_set(
            request.output_directory,
            request.journal.snapshot().entries.as_slice(),
            request.maximum_assets,
            request.maximum_bytes,
        ),
    }
}

fn handle_mutating_failure(
    request: &mut CommitRequest<'_>,
    _injector: &mut impl FaultInjector,
    source: PageKnotError,
    action: FaultAction,
) -> Result<Vec<ExportedArtifact>> {
    if action == FaultAction::Crash {
        return Err(source
            .with_detail("transactionPath", request.root.display().to_string())
            .with_detail("recoveryPending", true));
    }
    let persist = request.journal.persist(JournalPhase::RollingBack);
    if let Err(error) = persist {
        return Err(recovery_pending_error(
            request.root,
            source.with_source(error),
        ));
    }
    match rollback_transaction(
        request.output_directory,
        request.root,
        request.journal.snapshot(),
        request.maximum_assets,
        request.maximum_bytes,
    ) {
        Ok(()) => Err(PageKnotError::new(
            "pageknot.export.output",
            ErrorStage::Commit,
            "artifact export commit failed and every destination was restored",
        )
        .with_detail("rollbackAttempted", true)
        .with_detail("rollbackComplete", true)
        .with_detail("partialCommit", false)
        .with_detail("recoveryPending", false)
        .with_source(source)),
        Err(recovery) => Err(recovery.with_source(source)),
    }
}

fn clean_prepared_failure(request: &CommitRequest<'_>, source: PageKnotError) -> PageKnotError {
    match cleanup_transaction_root(request.root) {
        Ok(()) => source,
        Err(cleanup) => PageKnotError::new(
            "pageknot.export.output",
            ErrorStage::Commit,
            "artifact export failed before commit and transaction cleanup remains pending",
        )
        .retryable(true)
        .with_detail("transactionPath", request.root.display().to_string())
        .with_detail("commitComplete", false)
        .with_detail("rollbackComplete", true)
        .with_detail("cleanupComplete", false)
        .with_detail("recoveryPending", true)
        .with_source(source.with_source(cleanup)),
    }
}

fn committed_recovery_error(root: &Path, source: PageKnotError) -> PageKnotError {
    PageKnotError::new(
        "pageknot.export.output",
        ErrorStage::Commit,
        "artifact export committed every destination and cleanup remains pending",
    )
    .retryable(true)
    .with_detail("transactionPath", root.display().to_string())
    .with_detail("commitComplete", true)
    .with_detail("rollbackComplete", false)
    .with_detail("partialCommit", false)
    .with_detail("recoveryPending", true)
    .with_source(source)
}

fn commit_confirmation_error(root: &Path, source: PageKnotError) -> PageKnotError {
    PageKnotError::new(
        "pageknot.export.output",
        ErrorStage::Commit,
        "artifact export installed every destination but durable commit recovery remains pending",
    )
    .retryable(true)
    .with_detail("transactionPath", root.display().to_string())
    .with_detail("commitComplete", false)
    .with_detail("outputsInstalled", true)
    .with_detail("commitStatus", "indeterminate")
    .with_detail("rollbackComplete", false)
    .with_detail("partialCommit", false)
    .with_detail("recoveryPending", true)
    .with_source(source)
}

fn recovery_pending_error(root: &Path, source: PageKnotError) -> PageKnotError {
    PageKnotError::new(
        "pageknot.export.output",
        ErrorStage::Commit,
        "artifact export rollback could not establish a durable recovery phase",
    )
    .with_detail("transactionPath", root.display().to_string())
    .with_detail("commitComplete", false)
    .with_detail("rollbackComplete", false)
    .with_detail("partialCommit", true)
    .with_detail("recoveryPending", true)
    .with_source(source)
}

fn build_results(
    output_directory: &Path,
    variants: &[StagedVariant],
) -> Result<Vec<ExportedArtifact>> {
    variants
        .iter()
        .map(|variant| {
            let path = PortablePath::from_path_buf(
                output_directory.join(&variant.entry.destination_name),
            )?;
            let entrypoint = if variant.entry.output_kind == OutputKind::Directory {
                PortablePath::from_path_buf(path.join("index.md").into_std_path_buf())?
            } else {
                path.clone()
            };
            Ok(ExportedArtifact {
                kind: variant.entry.kind,
                path,
                entrypoint,
                bytes: variant.entry.verification.bytes,
                sha256: variant.entry.verification.sha256,
                verification: variant.entry.verification.clone(),
            })
        })
        .collect()
}
