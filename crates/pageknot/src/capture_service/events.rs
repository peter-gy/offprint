use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures_core::Stream;
use pageknot_model::CaptureEvent;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

const EVENT_CAPACITY: usize = 16_384;

#[derive(Debug)]
pub(super) struct EventJournal {
    sequence: AtomicU64,
    gate: Mutex<()>,
    sender: broadcast::Sender<Arc<EventEnvelope>>,
    retained: Mutex<RetainedEvents>,
}

impl EventJournal {
    pub(super) fn new() -> Self {
        let (sender, _) = broadcast::channel(EVENT_CAPACITY);
        Self {
            sequence: AtomicU64::new(0),
            gate: Mutex::new(()),
            sender,
            retained: Mutex::new(RetainedEvents::default()),
        }
    }

    pub(super) fn emit_with(&self, event: CaptureEvent, observe: impl FnOnce(&CaptureEvent)) {
        // Subscription baselines, diagnostic observation, retention, and
        // publication share one order.
        let _gate = self
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        observe(&event);
        let sequence = self.sequence.fetch_add(1, Ordering::AcqRel) + 1;
        let envelope = Arc::new(EventEnvelope { sequence, event });
        {
            let mut retained = self
                .retained
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match retention(&envelope.event) {
                Retention::Guaranteed => retained.guaranteed.push(Arc::clone(&envelope)),
                Retention::Detail => {
                    if retained.details.len() == EVENT_CAPACITY {
                        retained.details.pop_front();
                    }
                    retained.details.push_back(Arc::clone(&envelope));
                }
                Retention::Progress => retained.latest_progress = Some(Arc::clone(&envelope)),
            }
        }
        let _ignored = self.sender.send(envelope);
    }

    #[cfg(test)]
    fn emit(&self, event: CaptureEvent) {
        self.emit_with(event, |_| {});
    }

    fn subscribe(self: &Arc<Self>) -> CaptureEvents {
        let gate = self
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let inner = BroadcastStream::new(self.sender.subscribe());
        let pending = {
            let retained = self
                .retained
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            VecDeque::from(retained.after(0, None))
        };
        drop(gate);
        CaptureEvents {
            journal: Arc::clone(self),
            inner,
            pending,
            last_sequence: 0,
            terminal_seen: false,
        }
    }

    fn retained_after(&self, sequence: u64, before: Option<u64>) -> Vec<Arc<EventEnvelope>> {
        let _gate = self
            .gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.retained
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .after(sequence, before)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Retention {
    Guaranteed,
    Detail,
    Progress,
}

const fn retention(event: &CaptureEvent) -> Retention {
    match event {
        CaptureEvent::CaptureStarted { .. }
        | CaptureEvent::BrowserReady { .. }
        | CaptureEvent::NavigationStarted { .. }
        | CaptureEvent::ReadinessChanged { .. }
        | CaptureEvent::TransformStarted { .. }
        | CaptureEvent::ArtifactEncoding { .. }
        | CaptureEvent::VerificationStarted { .. }
        | CaptureEvent::CaptureSucceeded { .. }
        | CaptureEvent::CaptureFailed { .. }
        | CaptureEvent::CaptureCancelled { .. } => Retention::Guaranteed,
        CaptureEvent::ResourceProgress { .. } => Retention::Progress,
        CaptureEvent::NavigationRedirected { .. }
        | CaptureEvent::FrameCollected { .. }
        | CaptureEvent::ResourceDiscovered { .. }
        | CaptureEvent::Warning { .. } => Retention::Detail,
    }
}

#[derive(Clone, Debug)]
struct EventEnvelope {
    sequence: u64,
    event: CaptureEvent,
}

#[derive(Debug, Default)]
struct RetainedEvents {
    guaranteed: Vec<Arc<EventEnvelope>>,
    details: VecDeque<Arc<EventEnvelope>>,
    latest_progress: Option<Arc<EventEnvelope>>,
}

impl RetainedEvents {
    fn after(&self, sequence: u64, before: Option<u64>) -> Vec<Arc<EventEnvelope>> {
        let mut events = self
            .guaranteed
            .iter()
            .chain(&self.details)
            .filter(|event| event_is_between(event, sequence, before))
            .cloned()
            .collect::<Vec<_>>();
        if let Some(progress) = self
            .latest_progress
            .as_ref()
            .filter(|event| event_is_between(event, sequence, before))
        {
            events.push(Arc::clone(progress));
        }
        events.sort_unstable_by_key(|event| event.sequence);
        events
    }
}

fn event_is_between(event: &EventEnvelope, after: u64, before: Option<u64>) -> bool {
    event.sequence > after && before.is_none_or(|limit| event.sequence < limit)
}

#[derive(Debug)]
/// An ordered stream of [`CaptureEvent`] records for one capture job.
///
/// The stream ends after its terminal event. Resource progress may be
/// coalesced under backpressure. Lifecycle and terminal events are retained.
pub struct CaptureEvents {
    journal: Arc<EventJournal>,
    inner: BroadcastStream<Arc<EventEnvelope>>,
    pending: VecDeque<Arc<EventEnvelope>>,
    last_sequence: u64,
    terminal_seen: bool,
}

impl CaptureEvents {
    pub(super) fn new(journal: Arc<EventJournal>) -> Self {
        journal.subscribe()
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
                        .journal
                        .retained_after(self.last_sequence, Some(envelope.sequence));
                    self.pending.extend(retained);
                    self.pending.push_back(envelope);
                }
                Poll::Ready(Some(Err(BroadcastStreamRecvError::Lagged(_)))) => {
                    let retained = self.journal.retained_after(self.last_sequence, None);
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
    use pageknot_model::{CaptureId, ResourceId};
    use tokio_stream::StreamExt as _;

    use super::*;

    #[tokio::test]
    async fn slow_consumers_retain_warning_terminal_and_latest_progress() {
        let journal = Arc::new(EventJournal::new());
        let mut events = CaptureEvents::new(Arc::clone(&journal));
        let capture_id = CaptureId::new();
        for completed in 0..u32::try_from(EVENT_CAPACITY + 32).unwrap_or(u32::MAX) {
            journal.emit(CaptureEvent::ResourceProgress {
                capture_id: capture_id.clone(),
                completed,
                discovered: completed,
                bytes: u64::from(completed),
            });
        }
        journal.emit(CaptureEvent::Warning {
            capture_id: capture_id.clone(),
            warning: pageknot_model::CaptureWarning {
                code: "pageknot.resource.fixture".to_owned(),
                message: "fixture warning".to_owned(),
                frame_id: None,
                resource_id: Some(ResourceId::new(1)),
            },
        });
        journal.emit(CaptureEvent::CaptureCancelled { capture_id });

        let mut retained = Vec::new();
        while let Some(event) = events.next().await {
            retained.push(event);
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
    async fn detail_pressure_cannot_evict_lifecycle_or_terminal_events() {
        let journal = Arc::new(EventJournal::new());
        let capture_id = CaptureId::new();
        journal.emit(CaptureEvent::CaptureStarted {
            capture_id: capture_id.clone(),
        });
        for resource in 0..u32::try_from(EVENT_CAPACITY + 32).unwrap_or(u32::MAX) {
            journal.emit(CaptureEvent::ResourceDiscovered {
                capture_id: capture_id.clone(),
                resource_id: ResourceId::new(resource),
            });
        }
        journal.emit(CaptureEvent::CaptureCancelled { capture_id });
        let mut events = CaptureEvents::new(journal);

        assert!(matches!(
            events.next().await,
            Some(CaptureEvent::CaptureStarted { .. })
        ));
        let mut detail_count = 0_usize;
        let mut unexpected = None;
        let terminal_seen = loop {
            match events.next().await {
                Some(CaptureEvent::ResourceDiscovered { .. }) => detail_count += 1,
                Some(CaptureEvent::CaptureCancelled { .. }) => break true,
                Some(other) => {
                    unexpected = Some(other);
                    break false;
                }
                None => break false,
            }
        };

        assert_eq!(detail_count, EVENT_CAPACITY);
        assert!(
            unexpected.is_none(),
            "unexpected retained event: {unexpected:?}"
        );
        assert!(terminal_seen);
        assert!(events.next().await.is_none());
    }

    #[tokio::test]
    async fn subscriber_created_after_completion_observes_the_terminal_event() {
        let journal = Arc::new(EventJournal::new());
        let capture_id = CaptureId::new();
        journal.emit(CaptureEvent::CaptureStarted {
            capture_id: capture_id.clone(),
        });
        journal.emit(CaptureEvent::CaptureCancelled { capture_id });
        let mut events = CaptureEvents::new(journal);

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
}
