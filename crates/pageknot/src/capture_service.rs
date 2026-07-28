use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::task::{Context, Poll};

use futures_core::Stream;
use futures_util::FutureExt as _;
use pageknot_capture::{CaptureStateMachine, ValidatedCaptureRequest};
use pageknot_model::{
    BatchRequest, BatchResult, CaptureEvent, CaptureId, CaptureRequest, CaptureResult,
    CaptureStatus, CrawlRequest, CrawlResult, ErrorStage, PageKnotError, Result,
};
use serde_json::Value;
use tokio::sync::{broadcast, watch};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use crate::pipeline::run_capture_job;
use crate::runtime::RuntimeState;
use crate::{diagnostics::CaptureDiagnostics, diagnostics::sanitized_event};

const EVENT_CAPACITY: usize = 16_384;
const DIAGNOSTIC_EVENT_CAPACITY: usize = 4_096;

#[derive(Clone, Debug)]
/// Starts validated capture requests through one PageKnot runtime.
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
        if matches!(request.browser, pageknot_model::BrowserSpec::Auto) {
            request.browser = self.state.default_browser().clone();
        }
        let request = ValidatedCaptureRequest::new(request)?.into_inner();
        let capture_id = self.state.next_capture_id();
        let diagnostics = CaptureDiagnostics::prepare(&request).await?;
        let control = Arc::new(JobControl::new());
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let (result, result_receiver) = watch::channel(None);
        let job_state = Arc::new(JobState {
            lifecycle: Mutex::new(CaptureStateMachine::new()),
            control: Arc::clone(&control),
            event_sequence: AtomicU64::new(0),
            event_gate: Mutex::new(()),
            events,
            retained_events: Mutex::new(RetainedEvents::default()),
            diagnostic_events: Mutex::new(VecDeque::new()),
            dropped_diagnostic_events: AtomicU64::new(0),
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

    pub(crate) fn profile(&self, name: &str) -> Result<pageknot_model::CaptureProfile> {
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
    fn new() -> Self {
        Self {
            cancellation: tokio_util::sync::CancellationToken::new(),
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
            Err(TERMINAL_COMMIT) => Err(PageKnotError::new(
                "pageknot.runtime.job",
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

fn cancellation_error() -> PageKnotError {
    PageKnotError::new(
        "pageknot.runtime.cancelled",
        ErrorStage::Shutdown,
        "capture cancellation requested",
    )
}

#[derive(Debug)]
pub(crate) struct JobState {
    lifecycle: Mutex<CaptureStateMachine>,
    control: Arc<JobControl>,
    event_sequence: AtomicU64,
    event_gate: Mutex<()>,
    events: broadcast::Sender<Arc<EventEnvelope>>,
    retained_events: Mutex<RetainedEvents>,
    diagnostic_events: Mutex<VecDeque<Value>>,
    dropped_diagnostic_events: AtomicU64,
    diagnostics: Option<CaptureDiagnostics>,
    result: watch::Sender<Option<Result<CaptureResult>>>,
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
        // Keep subscription baselines and sequence assignment in one order so a
        // subscriber can recover every retained event published after it joins.
        let _gate = self
            .event_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let sequence = self.event_sequence.fetch_add(1, Ordering::AcqRel) + 1;
        let progress = matches!(event, CaptureEvent::ResourceProgress { .. });
        let retained = !progress;
        {
            let mut diagnostic_events = self
                .diagnostic_events
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if diagnostic_events.len() == DIAGNOSTIC_EVENT_CAPACITY {
                diagnostic_events.pop_front();
                self.dropped_diagnostic_events
                    .fetch_add(1, Ordering::AcqRel);
            }
            diagnostic_events.push_back(sanitized_event(&event));
        }
        let envelope = Arc::new(EventEnvelope { sequence, event });
        if retained || progress {
            let mut retained_events = self.retained_events();
            if retained {
                if retained_events.important.len() == EVENT_CAPACITY {
                    retained_events.important.pop_front();
                }
                retained_events.important.push_back(Arc::clone(&envelope));
            }
            if progress {
                retained_events.latest_progress = Some(Arc::clone(&envelope));
            }
        }
        let _ignored = self.events.send(envelope);
    }

    pub(crate) fn finish(&self, result: Result<CaptureResult>) {
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

    fn retained_events(&self) -> std::sync::MutexGuard<'_, RetainedEvents> {
        self.retained_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn retained_after(&self, sequence: u64, before: Option<u64>) -> Vec<Arc<EventEnvelope>> {
        let _gate = self
            .event_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let retained = self.retained_events();
        let mut events = retained
            .important
            .iter()
            .filter(|event| event_is_between(event, sequence, before))
            .cloned()
            .collect::<Vec<_>>();
        if let Some(progress) = retained
            .latest_progress
            .as_ref()
            .filter(|event| event_is_between(event, sequence, before))
        {
            events.push(Arc::clone(progress));
        }
        events.sort_unstable_by_key(|event| event.sequence);
        events
    }

    pub(crate) async fn attach_diagnostics(
        &self,
        capture_id: &CaptureId,
        error: PageKnotError,
    ) -> PageKnotError {
        let Some(diagnostics) = &self.diagnostics else {
            return error;
        };
        let events = self
            .diagnostic_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect();
        match diagnostics
            .write(
                capture_id,
                self.status(),
                events,
                self.dropped_diagnostic_events.load(Ordering::Acquire),
                &error,
            )
            .await
        {
            Ok(path) => error.with_diagnostics_path(path),
            Err(diagnostic_error) => {
                error.with_detail("diagnosticsError", diagnostic_error.code.to_string())
            }
        }
    }

    async fn finish_after_panic(&self, capture_id: CaptureId) {
        if self.result.borrow().is_some() {
            return;
        }
        let error = PageKnotError::new(
            "pageknot.internal.panic",
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
struct EventEnvelope {
    sequence: u64,
    event: CaptureEvent,
}

#[derive(Debug, Default)]
struct RetainedEvents {
    important: VecDeque<Arc<EventEnvelope>>,
    latest_progress: Option<Arc<EventEnvelope>>,
}

fn event_is_between(event: &EventEnvelope, after: u64, before: Option<u64>) -> bool {
    event.sequence > after && before.is_none_or(|limit| event.sequence < limit)
}

#[derive(Clone, Debug)]
/// A running or completed capture.
pub struct CaptureJob {
    id: CaptureId,
    state: Arc<JobState>,
    result: watch::Receiver<Option<Result<CaptureResult>>>,
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
        CaptureEvents::new(Arc::clone(&self.state))
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
    pub async fn wait(&self) -> Result<CaptureResult> {
        let mut result = self.result.clone();
        loop {
            if let Some(result) = result.borrow().clone() {
                return result;
            }
            result.changed().await.map_err(|_| {
                PageKnotError::new(
                    "pageknot.runtime.job",
                    pageknot_model::ErrorStage::Internal,
                    "capture job ended without a terminal result",
                )
            })?;
        }
    }
}

#[derive(Debug)]
/// An ordered stream of [`CaptureEvent`] records for one capture job.
///
/// The stream ends after its terminal event. Resource progress may be
/// coalesced under backpressure. Lifecycle and terminal events are retained.
pub struct CaptureEvents {
    state: Arc<JobState>,
    inner: BroadcastStream<Arc<EventEnvelope>>,
    pending: VecDeque<Arc<EventEnvelope>>,
    last_sequence: u64,
    terminal_seen: bool,
}

impl CaptureEvents {
    fn new(state: Arc<JobState>) -> Self {
        let gate = state
            .event_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let inner = BroadcastStream::new(state.events.subscribe());
        let pending = {
            let retained = state.retained_events();
            let mut events = retained.important.iter().cloned().collect::<Vec<_>>();
            if let Some(progress) = &retained.latest_progress {
                events.push(Arc::clone(progress));
            }
            events.sort_unstable_by_key(|event| event.sequence);
            VecDeque::from(events)
        };
        drop(gate);
        Self {
            state,
            inner,
            pending,
            last_sequence: 0,
            terminal_seen: false,
        }
    }

    fn next_pending(&mut self) -> Option<CaptureEvent> {
        let envelope = self.pending.pop_front()?;
        self.last_sequence = self.last_sequence.max(envelope.sequence);
        self.terminal_seen |= envelope.event.is_terminal();
        Some(envelope.event.clone())
    }
}

impl Stream for CaptureEvents {
    type Item = CaptureEvent;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            if let Some(event) = self.next_pending() {
                return Poll::Ready(Some(event));
            }
            if self.terminal_seen {
                return Poll::Ready(None);
            }
            match Pin::new(&mut self.inner).poll_next(context) {
                Poll::Ready(Some(Ok(envelope))) => {
                    if envelope.sequence <= self.last_sequence {
                        continue;
                    }
                    let retained = self
                        .state
                        .retained_after(self.last_sequence, Some(envelope.sequence));
                    self.pending.extend(retained);
                    self.pending.push_back(envelope);
                }
                Poll::Ready(Some(Err(BroadcastStreamRecvError::Lagged(_)))) => {
                    let retained = self.state.retained_after(self.last_sequence, None);
                    self.pending.extend(retained);
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use pageknot_model::{BatchJob, BatchRequest, CaptureWarning, ResourceId};
    use tokio_stream::StreamExt as _;

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
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        let (result, _) = watch::channel(None);
        Arc::new(JobState {
            lifecycle: Mutex::new(CaptureStateMachine::new()),
            control: Arc::new(JobControl::new()),
            event_sequence: AtomicU64::new(0),
            event_gate: Mutex::new(()),
            events,
            retained_events: Mutex::new(RetainedEvents::default()),
            diagnostic_events: Mutex::new(VecDeque::new()),
            dropped_diagnostic_events: AtomicU64::new(0),
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
            Err("pageknot.runtime.state_transition")
        );
        assert_eq!(state.status(), CaptureStatus::Validating);
        Ok(())
    }

    #[tokio::test]
    async fn configured_remote_policy_is_validated_before_job_registration() -> Result<()> {
        let capture_ids = Arc::new(CountingCaptureIds::default());
        let pageknot = crate::PageKnot::builder()
            .cdp_url(url::Url::parse("http://127.0.0.1:9222").map_err(|error| {
                PageKnotError::new(
                    "pageknot.input.cdp_url",
                    ErrorStage::Validation,
                    error.to_string(),
                )
            })?)
            .capture_id_generator(capture_ids.clone())
            .build()?;
        let request = CaptureRequest::builder("https://example.com")?.build()?;

        let result = pageknot.captures().start(request).await;

        assert_eq!(
            result.as_ref().err().map(|error| error.code.as_str()),
            Some("pageknot.input.remote_network_policy")
        );
        assert_eq!(capture_ids.0.load(Ordering::Acquire), 0);
        pageknot.close().await
    }

    #[tokio::test]
    async fn configured_remote_batch_is_validated_before_any_job_registration() -> Result<()> {
        let capture_ids = Arc::new(CountingCaptureIds::default());
        let pageknot = crate::PageKnot::builder()
            .cdp_url(url::Url::parse("http://127.0.0.1:9222").map_err(|error| {
                PageKnotError::new(
                    "pageknot.input.cdp_url",
                    ErrorStage::Validation,
                    error.to_string(),
                )
            })?)
            .capture_id_generator(capture_ids.clone())
            .build()?;
        let request = BatchRequest {
            jobs: vec![BatchJob {
                id: "remote-policy".to_owned(),
                request: CaptureRequest::builder("https://example.com")?.build()?,
            }],
            concurrency: 1,
            resume: None,
        };

        let result = pageknot.captures().batch(request).await;

        assert_eq!(
            result.as_ref().err().map(|error| error.code.as_str()),
            Some("pageknot.input.remote_network_policy")
        );
        assert_eq!(capture_ids.0.load(Ordering::Acquire), 0);
        pageknot.close().await
    }

    #[tokio::test]
    async fn slow_event_consumers_retain_warning_and_terminal_events() {
        let state = job_state();
        let mut events = CaptureEvents::new(Arc::clone(&state));
        let capture_id = CaptureId::new();
        for completed in 0..u32::try_from(EVENT_CAPACITY + 32).unwrap_or(u32::MAX) {
            state.emit(CaptureEvent::ResourceProgress {
                capture_id: capture_id.clone(),
                completed,
                discovered: completed,
                bytes: u64::from(completed),
            });
        }
        state.emit(CaptureEvent::Warning {
            capture_id: capture_id.clone(),
            warning: CaptureWarning {
                code: "pageknot.resource.fixture".to_owned(),
                message: "fixture warning".to_owned(),
                frame_id: None,
                resource_id: Some(ResourceId::new(1)),
            },
        });
        state.emit(CaptureEvent::CaptureCancelled { capture_id });

        let mut retained = Vec::new();
        while let Some(event) = events.next().await {
            if matches!(
                event,
                CaptureEvent::ResourceProgress { .. }
                    | CaptureEvent::Warning { .. }
                    | CaptureEvent::CaptureCancelled { .. }
            ) {
                retained.push(event);
            }
        }

        assert_eq!(retained.len(), 3);
        assert!(matches!(
            retained[0],
            CaptureEvent::ResourceProgress {
                completed,
                discovered,
                ..
            } if completed == u32::try_from(EVENT_CAPACITY + 31).unwrap_or(u32::MAX)
                && discovered == completed
        ));
        assert!(matches!(retained[1], CaptureEvent::Warning { .. }));
        assert!(matches!(retained[2], CaptureEvent::CaptureCancelled { .. }));
    }

    #[tokio::test]
    async fn subscriber_created_after_completion_observes_the_terminal_event() {
        let state = job_state();
        let capture_id = CaptureId::new();
        state.emit(CaptureEvent::CaptureStarted {
            capture_id: capture_id.clone(),
        });
        state.emit(CaptureEvent::CaptureCancelled { capture_id });
        let mut events = CaptureEvents::new(state);

        assert!(matches!(
            events.next().await,
            Some(CaptureEvent::CaptureStarted { .. })
        ));
        assert!(matches!(
            events.next().await,
            Some(CaptureEvent::CaptureCancelled { .. })
        ));
        assert!(events.next().await.is_none());
    }

    #[test]
    fn cancellation_claim_prevents_artifact_commit() -> Result<()> {
        let control = JobControl::new();

        assert!(control.request_cancel());
        assert!(control.cancellation().is_cancelled());
        let Err(error) = control.claim_commit() else {
            return Err(PageKnotError::new(
                "pageknot.internal.test",
                ErrorStage::Internal,
                "cancellation lost the terminal commit decision",
            ));
        };

        assert_eq!(error.code.as_str(), "pageknot.runtime.cancelled");
        assert!(control.cancellation_won());
        Ok(())
    }

    #[test]
    fn commit_claim_rejects_late_cancellation() -> Result<()> {
        let control = JobControl::new();

        control.claim_commit()?;
        assert!(!control.request_cancel());

        assert!(!control.cancellation().is_cancelled());
        assert!(!control.cancellation_won());
        Ok(())
    }
}
