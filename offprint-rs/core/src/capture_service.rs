use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

use futures_util::FutureExt as _;
use offprint_capture::{CaptureStateMachine, ValidatedCaptureRequest};
use offprint_model::{
    BatchRequest, BatchResult, CaptureEvent, CaptureId, CaptureReceipt, CaptureRequest,
    CaptureStatus, CrawlRequest, CrawlResult, ErrorStage, OffprintError, Result,
};
use tokio::sync::watch;

use crate::diagnostics::CaptureDiagnostics;
use crate::pipeline::run_capture_job;
use crate::runtime::RuntimeState;

mod diagnostics;
mod events;

use diagnostics::DiagnosticsRecorder;
pub use events::CaptureEvents;
use events::EventJournal;

#[derive(Clone, Debug)]
/// Starts validated capture requests through one Offprint runtime.
pub struct CaptureService {
    state: Arc<RuntimeState>,
}

impl CaptureService {
    pub(crate) const fn new(state: Arc<RuntimeState>) -> Self {
        Self { state }
    }

    /// Starts `request` and returns a cancellable job.
    ///
    /// Request validation and diagnostic-directory setup complete before this
    /// method returns. Browser acquisition and capture continue in the job.
    pub async fn start(&self, mut request: CaptureRequest) -> Result<CaptureJob> {
        self.state.ensure_open()?;
        if matches!(request.browser, offprint_model::BrowserSpec::Auto) {
            request.browser = self.state.default_browser().clone();
        }
        let request = ValidatedCaptureRequest::new(request)?;
        let capture_id = self.state.next_capture_id();
        let diagnostics = CaptureDiagnostics::prepare(request.get())
            .await?
            .map(DiagnosticsRecorder::new);
        let control = Arc::new(JobControl::new(self.state.operation_cancellation()));
        let (result, result_receiver) = watch::channel(None);
        let job_state = Arc::new(JobState {
            lifecycle: Mutex::new(CaptureStateMachine::new()),
            control: Arc::clone(&control),
            events: Arc::new(EventJournal::new()),
            diagnostics,
            result,
        });
        self.state.register_job(capture_id.clone(), control)?;
        let (start_tx, start_rx) = tokio::sync::oneshot::channel();
        let runtime = Arc::clone(&self.state);
        let task_capture_id = capture_id.clone();
        let task_state = Arc::clone(&job_state);
        let task = tokio::spawn(async move {
            if start_rx.await.is_err() {
                runtime.unregister_job(&task_capture_id);
                return;
            }
            let outcome = std::panic::AssertUnwindSafe(run_capture_job(
                Arc::clone(&runtime),
                Arc::clone(&task_state),
                task_capture_id.clone(),
                request,
            ))
            .catch_unwind()
            .await;
            if outcome.is_err() {
                task_state.finish_after_panic(task_capture_id.clone()).await;
            }
            runtime.unregister_job(&task_capture_id);
        });
        if let Err(error) = self.state.set_job_task(&capture_id, task) {
            self.state.unregister_job(&capture_id);
            return Err(error);
        }
        let _ignored = start_tx.send(());
        Ok(CaptureJob {
            id: capture_id,
            state: job_state,
            result: result_receiver,
        })
    }

    /// Runs independent capture requests with bounded concurrency and
    /// per-request outcomes.
    pub async fn batch(&self, request: BatchRequest) -> Result<BatchResult> {
        crate::scheduler::SchedulerService::new(Arc::clone(&self.state))
            .batch(request)
            .await
    }

    /// Captures a bounded breadth-first link graph from one seed request.
    pub async fn crawl(&self, request: CrawlRequest) -> Result<CrawlResult> {
        crate::scheduler::SchedulerService::new(Arc::clone(&self.state))
            .crawl(request)
            .await
    }

    pub(crate) fn profile(&self, name: &str) -> Result<offprint_model::CaptureProfile> {
        self.state.capture_profile(name)
    }
}

#[derive(Debug)]
pub(crate) struct JobControl {
    cancellation: tokio_util::sync::CancellationToken,
    terminal_decision: AtomicU8,
}

const TERMINAL_UNDECIDED: u8 = 0;
const TERMINAL_CANCELLATION: u8 = 1;
const TERMINAL_COMMIT: u8 = 2;

impl JobControl {
    pub(crate) fn new(cancellation: tokio_util::sync::CancellationToken) -> Self {
        Self {
            cancellation,
            terminal_decision: AtomicU8::new(TERMINAL_UNDECIDED),
        }
    }

    pub(crate) fn cancellation(&self) -> &tokio_util::sync::CancellationToken {
        &self.cancellation
    }

    pub(crate) fn request_cancel(&self) -> bool {
        match self.terminal_decision.compare_exchange(
            TERMINAL_UNDECIDED,
            TERMINAL_CANCELLATION,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                self.cancellation.cancel();
                true
            }
            Err(TERMINAL_CANCELLATION) => {
                self.cancellation.cancel();
                false
            }
            Err(TERMINAL_COMMIT) => false,
            Err(_) => unreachable!("terminal decision uses a closed value set"),
        }
    }

    pub(crate) fn claim_commit(&self) -> Result<()> {
        if self.cancellation.is_cancelled() {
            self.request_cancel();
        }
        match self.terminal_decision.compare_exchange(
            TERMINAL_UNDECIDED,
            TERMINAL_COMMIT,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => Ok(()),
            Err(TERMINAL_CANCELLATION) => Err(cancellation_error()),
            Err(TERMINAL_COMMIT) => Err(OffprintError::new(
                "offprint.runtime.job",
                ErrorStage::Internal,
                "capture commit decision was already claimed",
            )),
            Err(_) => unreachable!("terminal decision uses a closed value set"),
        }
    }

    pub(crate) fn cancellation_won(&self) -> bool {
        self.terminal_decision.load(Ordering::Acquire) == TERMINAL_CANCELLATION
    }
}

fn cancellation_error() -> OffprintError {
    OffprintError::new(
        "offprint.runtime.cancelled",
        ErrorStage::Shutdown,
        "capture cancellation requested",
    )
}

#[derive(Debug)]
pub(crate) struct JobState {
    lifecycle: Mutex<CaptureStateMachine>,
    control: Arc<JobControl>,
    events: Arc<EventJournal>,
    diagnostics: Option<DiagnosticsRecorder>,
    result: watch::Sender<Option<Result<CaptureReceipt>>>,
}

impl JobState {
    pub(crate) fn status(&self) -> CaptureStatus {
        self.lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .status()
    }

    pub(crate) fn transition(&self, status: CaptureStatus) -> Result<()> {
        self.lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .transition(status)
    }

    pub(crate) fn emit(&self, event: CaptureEvent) {
        self.events.emit_with(event, |event| {
            if let Some(diagnostics) = &self.diagnostics {
                diagnostics.record(event);
            }
        });
    }

    pub(crate) fn finish(&self, result: Result<CaptureReceipt>) {
        self.result.send_replace(Some(result));
    }

    pub(crate) fn cancellation(&self) -> &tokio_util::sync::CancellationToken {
        self.control.cancellation()
    }

    pub(crate) fn request_cancel(&self) -> bool {
        self.control.request_cancel()
    }

    pub(crate) fn claim_commit(&self) -> Result<()> {
        self.control.claim_commit()
    }

    pub(crate) fn cancellation_won(&self) -> bool {
        self.control.cancellation_won()
    }

    pub(crate) async fn attach_diagnostics(
        &self,
        capture_id: &CaptureId,
        error: OffprintError,
    ) -> OffprintError {
        let Some(diagnostics) = &self.diagnostics else {
            return error;
        };
        diagnostics.attach(capture_id, self.status(), error).await
    }

    async fn finish_after_panic(&self, capture_id: CaptureId) {
        if self.result.borrow().is_some() {
            return;
        }
        let error = OffprintError::new(
            "offprint.internal.panic",
            ErrorStage::Internal,
            "the capture task stopped after an unexpected internal failure",
        );
        let error = match self.transition(CaptureStatus::Failed) {
            Ok(()) => error,
            Err(transition_error) => transition_error.with_source(error),
        };
        let error = self.attach_diagnostics(&capture_id, error).await;
        self.emit(CaptureEvent::CaptureFailed {
            capture_id,
            error: error.clone(),
        });
        self.finish(Err(error));
    }
}

#[derive(Clone, Debug)]
/// A running or completed capture.
pub struct CaptureJob {
    id: CaptureId,
    state: Arc<JobState>,
    result: watch::Receiver<Option<Result<CaptureReceipt>>>,
}

impl CaptureJob {
    /// Returns the stable identifier assigned when the job started.
    #[must_use]
    pub const fn id(&self) -> &CaptureId {
        &self.id
    }

    /// Returns the latest observable lifecycle state.
    #[must_use]
    pub fn status(&self) -> CaptureStatus {
        self.state.status()
    }

    /// Subscribes to ordered progress and terminal events.
    ///
    /// A late subscriber receives retained lifecycle events and the latest
    /// resource progress record before live events.
    #[must_use]
    pub fn events(&self) -> CaptureEvents {
        CaptureEvents::new(Arc::clone(&self.state.events))
    }

    /// Requests cancellation.
    ///
    /// Cancellation is idempotent. A verified artifact whose atomic commit
    /// already won the terminal decision remains successful.
    pub fn cancel(&self) {
        self.state.request_cancel();
    }

    /// Waits for the single terminal capture result.
    ///
    /// Multiple callers may await clones of the same job.
    pub async fn result(&self) -> Result<CaptureReceipt> {
        let mut result = self.result.clone();
        loop {
            if let Some(result) = result.borrow().clone() {
                return result;
            }
            result.changed().await.map_err(|_| {
                OffprintError::new(
                    "offprint.runtime.job",
                    offprint_model::ErrorStage::Internal,
                    "capture job ended without a terminal result",
                )
            })?;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use offprint_model::{BatchJob, BatchRequest};

    use super::*;

    #[derive(Debug, Default)]
    struct CountingCaptureIds(AtomicUsize);

    impl crate::CaptureIdGenerator for CountingCaptureIds {
        fn next_capture_id(&self) -> CaptureId {
            self.0.fetch_add(1, Ordering::AcqRel);
            CaptureId::new()
        }
    }

    fn job_state() -> Arc<JobState> {
        let (result, _) = watch::channel(None);
        Arc::new(JobState {
            lifecycle: Mutex::new(CaptureStateMachine::new()),
            control: Arc::new(JobControl::new(tokio_util::sync::CancellationToken::new())),
            events: Arc::new(EventJournal::new()),
            diagnostics: None,
            result,
        })
    }

    #[test]
    fn job_status_observes_validated_lifecycle_transitions() -> Result<()> {
        let state = job_state();

        assert_eq!(state.status(), CaptureStatus::Created);
        state.transition(CaptureStatus::Validating)?;
        assert_eq!(state.status(), CaptureStatus::Validating);

        let result = state.transition(CaptureStatus::Encoding);

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.runtime.state_transition")
        );
        assert_eq!(state.status(), CaptureStatus::Validating);
        Ok(())
    }

    #[tokio::test]
    async fn configured_remote_policy_is_validated_before_job_registration() -> Result<()> {
        let capture_ids = Arc::new(CountingCaptureIds::default());
        let offprint = crate::Offprint::builder()
            .cdp_url(url::Url::parse("http://127.0.0.1:9222").map_err(|error| {
                OffprintError::new(
                    "offprint.input.cdp_url",
                    ErrorStage::Validation,
                    error.to_string(),
                )
            })?)
            .capture_id_generator(capture_ids.clone())
            .build()?;
        let request = CaptureRequest::builder("https://example.com")?.build()?;

        let result = offprint.captures().start(request).await;

        assert_eq!(
            result.as_ref().err().map(|error| error.code.as_str()),
            Some("offprint.input.remote_network_policy")
        );
        assert_eq!(capture_ids.0.load(Ordering::Acquire), 0);
        offprint.close().await
    }

    #[tokio::test]
    async fn configured_remote_batch_is_validated_before_any_job_registration() -> Result<()> {
        let capture_ids = Arc::new(CountingCaptureIds::default());
        let offprint = crate::Offprint::builder()
            .cdp_url(url::Url::parse("http://127.0.0.1:9222").map_err(|error| {
                OffprintError::new(
                    "offprint.input.cdp_url",
                    ErrorStage::Validation,
                    error.to_string(),
                )
            })?)
            .capture_id_generator(capture_ids.clone())
            .build()?;
        let request = BatchRequest {
            schema_version: offprint_model::PUBLIC_SCHEMA_VERSION,
            jobs: vec![BatchJob {
                id: "remote-policy".to_owned(),
                request: CaptureRequest::builder("https://example.com")?.build()?,
            }],
            concurrency: 1,
            resume: None,
        };

        let result = offprint.captures().batch(request).await;

        assert_eq!(
            result.as_ref().err().map(|error| error.code.as_str()),
            Some("offprint.input.remote_network_policy")
        );
        assert_eq!(capture_ids.0.load(Ordering::Acquire), 0);
        offprint.close().await
    }

    #[test]
    fn cancellation_claim_prevents_artifact_commit() -> Result<()> {
        let control = JobControl::new(tokio_util::sync::CancellationToken::new());

        assert!(control.request_cancel());
        assert!(control.cancellation().is_cancelled());
        let Err(error) = control.claim_commit() else {
            return Err(OffprintError::new(
                "offprint.internal.test",
                ErrorStage::Internal,
                "cancellation lost the terminal commit decision",
            ));
        };

        assert_eq!(error.code.as_str(), "offprint.runtime.cancelled");
        assert!(control.cancellation_won());
        Ok(())
    }

    #[test]
    fn commit_claim_rejects_late_cancellation() -> Result<()> {
        let control = JobControl::new(tokio_util::sync::CancellationToken::new());

        control.claim_commit()?;
        assert!(!control.request_cancel());

        assert!(!control.cancellation().is_cancelled());
        assert!(!control.cancellation_won());
        Ok(())
    }
}
