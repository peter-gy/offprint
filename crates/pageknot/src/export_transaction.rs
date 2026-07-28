mod commit;
mod durable;
mod fault;
mod journal;
mod plan;
mod recovery;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use pageknot_export::{MarkdownBundle, VariantEvidence};
use pageknot_model::{
    ArtifactVariantKind, ArtifactVariantVerification, ConflictPolicy, ContentDigest, ErrorStage,
    ExportedArtifact, PageKnotError, PortablePath, Result,
};

use self::commit::{CommitRequest, commit_transaction};
use self::durable::cleanup_transaction_root;
use self::fault::{FaultAction, FaultInjector, FaultPoint, NoFault, injected_fault};
use self::journal::{JournalPhase, JournalStore, TransactionNamespace};
use self::plan::{StagedVariant, plan_destinations, stage_variants, validate_staged_variants};
use self::recovery::recover_pending_transactions;

pub(super) fn markdown_bundle_digest(bundle: &MarkdownBundle) -> (u64, ContentDigest) {
    plan::markdown_bundle_digest(bundle)
}

pub(super) fn validate_markdown_bundle_limits(
    bundle: &MarkdownBundle,
    maximum_assets: usize,
    maximum_bytes: u64,
    stage: ErrorStage,
) -> Result<()> {
    plan::validate_markdown_bundle_limits(bundle, maximum_assets, maximum_bytes, stage)
}

pub(super) fn markdown_file_limit_error(stage: ErrorStage) -> PageKnotError {
    plan::markdown_file_limit_error(stage)
}

pub(super) fn markdown_byte_limit_error(stage: ErrorStage) -> PageKnotError {
    plan::markdown_byte_limit_error(stage)
}

#[derive(Debug)]
pub(super) enum PreparedVariant {
    File {
        name: String,
        kind: ArtifactVariantKind,
        bytes: Vec<u8>,
        verification: ArtifactVariantVerification,
    },
    Markdown {
        name: String,
        bundle: MarkdownBundle,
        verification: ArtifactVariantVerification,
    },
}

impl PreparedVariant {
    pub(super) fn file(
        name: String,
        kind: ArtifactVariantKind,
        bytes: Vec<u8>,
        evidence: VariantEvidence,
        maximum_bytes: u64,
    ) -> Result<Self> {
        let byte_count = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if byte_count > maximum_bytes {
            return Err(PageKnotError::new(
                "pageknot.export.size",
                ErrorStage::Encoding,
                "artifact variant exceeds the export byte limit",
            ));
        }
        let sha256 = ContentDigest::sha256(&bytes);
        Ok(Self::File {
            name,
            kind,
            bytes,
            verification: variant_verification(kind, byte_count, sha256, evidence),
        })
    }

    pub(super) fn markdown(
        name: String,
        bundle: MarkdownBundle,
        maximum_assets: usize,
        maximum_bytes: u64,
    ) -> Result<Self> {
        validate_markdown_bundle_limits(
            &bundle,
            maximum_assets,
            maximum_bytes,
            ErrorStage::Encoding,
        )?;
        let evidence = pageknot_export::verify_markdown(&bundle)?;
        let (bytes, sha256) = markdown_bundle_digest(&bundle);
        Ok(Self::Markdown {
            name,
            bundle,
            verification: variant_verification(
                ArtifactVariantKind::Markdown,
                bytes,
                sha256,
                evidence,
            ),
        })
    }
}

#[derive(Debug)]
pub(super) struct ExportTransaction {
    _namespace: TransactionNamespace,
    output_directory: PathBuf,
    root: PathBuf,
    journal: JournalStore,
    variants: Vec<StagedVariant>,
    conflict: ConflictPolicy,
    mode: journal::CommitMode,
    maximum_assets: usize,
    maximum_bytes: u64,
}

impl ExportTransaction {
    pub(super) fn stage(
        output_directory: &PortablePath,
        conflict: ConflictPolicy,
        prepared: Vec<PreparedVariant>,
        maximum_assets: usize,
        maximum_bytes: u64,
    ) -> Result<Self> {
        let mut injector = NoFault;
        Self::stage_with_injector(
            output_directory,
            conflict,
            prepared,
            maximum_assets,
            maximum_bytes,
            &mut injector,
        )
    }

    fn stage_with_injector(
        output_directory: &PortablePath,
        conflict: ConflictPolicy,
        prepared: Vec<PreparedVariant>,
        maximum_assets: usize,
        maximum_bytes: u64,
        injector: &mut impl FaultInjector,
    ) -> Result<Self> {
        let output_directory = output_directory.as_utf8_path().as_std_path().to_owned();
        let namespace = TransactionNamespace::acquire(&output_directory)?;
        recover_pending_transactions(&namespace, maximum_assets, maximum_bytes)?;
        let (mode, plans) = plan_destinations(&output_directory, conflict, &prepared)?;
        let root = namespace.create_transaction_root()?;
        let entries = plans.iter().map(|plan| plan.entry.clone()).collect();
        let mut journal =
            match JournalStore::new(&root, namespace.identity().to_owned(), mode, entries) {
                Ok(journal) => journal,
                Err(error) => return Err(clean_stage_failure(&root, error)),
            };
        if let Err(error) = journal.persist(JournalPhase::Staging) {
            return Err(clean_stage_failure(&root, error));
        }
        if let Some(action) = injector.action(FaultPoint::AfterJournal(JournalPhase::Staging)) {
            return Err(stage_fault(
                &root,
                FaultPoint::AfterJournal(JournalPhase::Staging),
                action,
            ));
        }
        let variants = match stage_variants(&root, prepared, &plans, maximum_assets, maximum_bytes)
        {
            Ok(variants) => variants,
            Err(error) => return Err(clean_stage_failure(&root, error)),
        };
        if let Err(error) = validate_staged_variants(&variants, maximum_assets, maximum_bytes) {
            return Err(clean_stage_failure(&root, error));
        }
        if let Err(error) = journal.persist(JournalPhase::Prepared) {
            return Err(clean_stage_failure(&root, error));
        }
        if let Some(action) = injector.action(FaultPoint::AfterJournal(JournalPhase::Prepared)) {
            return Err(stage_fault(
                &root,
                FaultPoint::AfterJournal(JournalPhase::Prepared),
                action,
            ));
        }
        Ok(Self {
            _namespace: namespace,
            output_directory,
            root,
            journal,
            variants,
            conflict,
            mode,
            maximum_assets,
            maximum_bytes,
        })
    }

    pub(super) fn commit(mut self) -> Result<Vec<ExportedArtifact>> {
        let mut injector = NoFault;
        self.commit_with_injector(&mut injector)
    }

    fn commit_with_injector(
        &mut self,
        injector: &mut impl FaultInjector,
    ) -> Result<Vec<ExportedArtifact>> {
        commit_transaction(
            CommitRequest {
                output_directory: &self.output_directory,
                root: &self.root,
                journal: &mut self.journal,
                variants: &self.variants,
                conflict: self.conflict,
                mode: self.mode,
                maximum_assets: self.maximum_assets,
                maximum_bytes: self.maximum_bytes,
            },
            injector,
        )
    }

    #[cfg(test)]
    fn root(&self) -> &Path {
        &self.root
    }
}

fn clean_stage_failure(root: &Path, source: PageKnotError) -> PageKnotError {
    match cleanup_transaction_root(root) {
        Ok(()) => source,
        Err(cleanup) => PageKnotError::new(
            "pageknot.export.output",
            ErrorStage::Encoding,
            "artifact export staging failed and durable cleanup remains pending",
        )
        .retryable(true)
        .with_detail("transactionPath", root.display().to_string())
        .with_detail("recoveryPending", true)
        .with_source(source.with_source(cleanup)),
    }
}

fn stage_fault(root: &Path, point: FaultPoint, action: FaultAction) -> PageKnotError {
    let source = injected_fault(point, action);
    if action == FaultAction::Crash {
        source
            .with_detail("transactionPath", root.display().to_string())
            .with_detail("recoveryPending", true)
    } else {
        clean_stage_failure(root, source)
    }
}

fn variant_verification(
    kind: ArtifactVariantKind,
    bytes: u64,
    sha256: ContentDigest,
    evidence: VariantEvidence,
) -> ArtifactVariantVerification {
    ArtifactVariantVerification {
        kind,
        passed: evidence.structure_valid && evidence.content_valid,
        bytes,
        sha256,
        structure_valid: evidence.structure_valid,
        content_valid: evidence.content_valid,
    }
}
