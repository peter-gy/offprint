mod commit;
mod durable;
mod fault;
mod journal;
mod payload;
mod plan;
mod recovery;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use pageknot_model::{
    ConflictPolicy, ErrorStage, ExportedArtifact, PageKnotError, PortablePath, Result,
};

use self::commit::{CommitRequest, commit_transaction};
use self::durable::cleanup_transaction_root;
use self::fault::{FaultAction, FaultInjector, FaultPoint, NoFault, injected_fault};
use self::journal::{JournalPhase, JournalStore, TransactionNamespace};
pub use self::payload::{ArtifactDirectory, ArtifactTransactionLimits, PreparedArtifact};
use self::plan::{
    StagedArtifact, plan_destinations, stage_artifacts, validate_prepared_artifacts,
    validate_staged_artifacts,
};
use self::recovery::recover_pending_transactions;

#[derive(Debug)]
/// A staged set of verified artifacts committed as one recoverable transaction.
pub struct ArtifactTransaction {
    _namespace: TransactionNamespace,
    output_directory: PathBuf,
    root: PathBuf,
    journal: JournalStore,
    artifacts: Vec<StagedArtifact>,
    conflict: ConflictPolicy,
    mode: journal::CommitMode,
    limits: ArtifactTransactionLimits,
}

impl ArtifactTransaction {
    /// Recovers prior work for `output_directory`, then stages every prepared artifact.
    pub fn stage(
        output_directory: &PortablePath,
        conflict: ConflictPolicy,
        prepared: Vec<PreparedArtifact>,
        limits: ArtifactTransactionLimits,
    ) -> Result<Self> {
        let mut injector = NoFault;
        Self::stage_with_injector(output_directory, conflict, prepared, limits, &mut injector)
    }

    fn stage_with_injector(
        output_directory: &PortablePath,
        conflict: ConflictPolicy,
        prepared: Vec<PreparedArtifact>,
        limits: ArtifactTransactionLimits,
        injector: &mut impl FaultInjector,
    ) -> Result<Self> {
        let output_directory = output_directory.as_utf8_path().as_std_path().to_owned();
        validate_prepared_artifacts(&prepared, limits)?;
        let namespace = TransactionNamespace::acquire(&output_directory)?;
        recover_pending_transactions(&namespace, limits)?;
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
        let artifacts = match stage_artifacts(&root, prepared, &plans, limits) {
            Ok(artifacts) => artifacts,
            Err(error) => return Err(clean_stage_failure(&root, error)),
        };
        if let Err(error) = validate_staged_artifacts(&artifacts, limits) {
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
            artifacts,
            conflict,
            mode,
            limits,
        })
    }

    /// Commits every staged artifact and returns its final destination.
    pub fn commit(mut self) -> Result<Vec<ExportedArtifact>> {
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
                artifacts: &self.artifacts,
                conflict: self.conflict,
                mode: self.mode,
                limits: self.limits,
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
