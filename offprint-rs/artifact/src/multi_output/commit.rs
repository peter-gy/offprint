use std::path::Path;

use offprint_model::{ErrorStage, ExportedArtifact, OffprintError, PortablePath, Result};

use super::durable::{
    cleanup_transaction_root, create_directory, durable_hard_link, durable_rename, sync_directory,
};
use super::fault::{FaultAction, FaultInjector, FaultPoint, injected_fault};
use super::journal::{CommitMode, JournalPhase, JournalStore, OutputKind};
use super::payload::ArtifactTransactionLimits;
use super::plan::{
    StagedArtifact, output_directory_is_empty, recheck_destinations, validate_complete_output_set,
    validate_entry_path, validate_staged_artifacts,
};
use super::recovery::{finalize_committed, rollback_transaction};

pub(super) struct CommitRequest<'a> {
    pub(super) output_directory: &'a Path,
    pub(super) root: &'a Path,
    pub(super) journal: &'a mut JournalStore,
    pub(super) artifacts: &'a [StagedArtifact],
    pub(super) conflict: offprint_model::ConflictPolicy,
    pub(super) mode: CommitMode,
    pub(super) limits: ArtifactTransactionLimits,
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
        return Err(commit_confirmation_error(request.root, error));
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

    let results = build_results(request.output_directory, request.artifacts)
        .map_err(|error| committed_recovery_error(request.root, error))?;
    finalize_committed(
        request.output_directory,
        request.root,
        request.journal.snapshot(),
        request.limits,
    )?;
    Ok(results)
}

fn validate_before_mutation(request: &CommitRequest<'_>) -> Result<()> {
    validate_staged_artifacts(request.artifacts, request.limits)?;
    recheck_destinations(request.conflict, request.artifacts)?;
    if request.mode == CommitMode::DirectorySwap
        && !output_directory_is_empty(request.output_directory, ErrorStage::Commit)?
    {
        return Err(OffprintError::new(
            "offprint.export.output",
            ErrorStage::Commit,
            "artifact export directory changed before its directory-swap commit",
        ));
    }
    Ok(())
}

fn commit_entries(
    request: &CommitRequest<'_>,
    injector: &mut impl FaultInjector,
) -> std::result::Result<(), (OffprintError, FaultAction)> {
    if request
        .artifacts
        .iter()
        .any(|artifact| artifact.entry.existed)
    {
        let backups = request.root.join("backups");
        if let Err(error) = create_directory(&backups, ErrorStage::Commit) {
            return Err((error, FaultAction::Error));
        }
        for (index, artifact) in request.artifacts.iter().enumerate() {
            if !artifact.entry.existed {
                continue;
            }
            let backup = backups.join(format!("entry-{index}"));
            if let Err(error) = durable_rename(&artifact.destination, &backup) {
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
    for (index, artifact) in request.artifacts.iter().enumerate() {
        let result = match artifact.entry.output_kind {
            OutputKind::File => durable_hard_link(&artifact.staged_path, &artifact.destination),
            OutputKind::Directory => durable_rename(&artifact.staged_path, &artifact.destination),
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
) -> std::result::Result<(), (OffprintError, FaultAction)> {
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
                OffprintError::new(
                    "offprint.export.output",
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
            for artifact in request.artifacts {
                validate_entry_path(&artifact.destination, &artifact.entry, request.limits)?;
            }
            Ok(())
        }
        CommitMode::DirectorySwap => validate_complete_output_set(
            request.output_directory,
            request.journal.snapshot().entries.as_slice(),
            request.limits,
        ),
    }
}

fn handle_mutating_failure(
    request: &mut CommitRequest<'_>,
    _injector: &mut impl FaultInjector,
    source: OffprintError,
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
        request.limits,
    ) {
        Ok(()) => Err(OffprintError::new(
            "offprint.export.output",
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

fn clean_prepared_failure(request: &CommitRequest<'_>, source: OffprintError) -> OffprintError {
    match cleanup_transaction_root(request.root) {
        Ok(()) => source,
        Err(cleanup) => OffprintError::new(
            "offprint.export.output",
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

fn committed_recovery_error(root: &Path, source: OffprintError) -> OffprintError {
    OffprintError::new(
        "offprint.export.output",
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

fn commit_confirmation_error(root: &Path, source: OffprintError) -> OffprintError {
    OffprintError::new(
        "offprint.export.output",
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

fn recovery_pending_error(root: &Path, source: OffprintError) -> OffprintError {
    OffprintError::new(
        "offprint.export.output",
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
    artifacts: &[StagedArtifact],
) -> Result<Vec<ExportedArtifact>> {
    artifacts
        .iter()
        .map(|artifact| {
            let path = PortablePath::from_path_buf(
                output_directory.join(&artifact.entry.destination_name),
            )?;
            let entrypoint = match artifact.entry.entrypoint.as_deref() {
                Some(relative) => {
                    PortablePath::from_path_buf(path.join(relative).into_std_path_buf())?
                }
                None => path.clone(),
            };
            Ok(ExportedArtifact {
                format: artifact.entry.format,
                path,
                entrypoint,
                bytes: artifact.entry.verification.bytes,
                sha256: artifact.entry.verification.sha256,
                verification: artifact.entry.verification.clone(),
            })
        })
        .collect()
}
