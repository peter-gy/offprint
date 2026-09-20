use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Weak};

use tokio::sync::Notify;
use tokio::sync::broadcast::error::{RecvError, SendError, TryRecvError};

use crate::CdpEvent;

#[derive(Clone, Debug)]
pub(crate) struct EventSender {
    shared: Arc<Subscribers>,
}

#[derive(Debug)]
struct Subscribers {
    state: Mutex<SubscriberState>,
    maximum_events: usize,
    maximum_bytes: usize,
}

#[derive(Debug, Default)]
struct SubscriberState {
    receivers: Vec<Weak<Inbox>>,
    closed: bool,
}

impl EventSender {
    pub(crate) fn new(maximum_events: usize, maximum_bytes: usize) -> Self {
        Self {
            shared: Arc::new(Subscribers {
                state: Mutex::new(SubscriberState::default()),
                maximum_events,
                maximum_bytes,
            }),
        }
    }

    pub(crate) fn subscribe(&self) -> CdpEventReceiver {
        let inbox = Arc::new(Inbox {
            state: Mutex::new(InboxState::default()),
            changed: Notify::new(),
        });
        let mut subscribers = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if subscribers.closed {
            inbox.close();
        } else {
            subscribers
                .receivers
                .retain(|receiver| receiver.strong_count() != 0);
            subscribers.receivers.push(Arc::downgrade(&inbox));
        }
        CdpEventReceiver { inbox }
    }

    pub(crate) fn receiver_count(&self) -> usize {
        self.shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .receivers
            .iter()
            .filter(|receiver| receiver.strong_count() != 0)
            .count()
    }

    pub(crate) fn send(
        &self,
        event: CdpEvent,
        encoded_bytes: usize,
    ) -> Result<usize, SendError<CdpEvent>> {
        let mut count = 0;
        let mut subscribers = self
            .shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if subscribers.closed {
            return Err(SendError(event));
        }
        subscribers.receivers.retain(|receiver| {
            let Some(inbox) = receiver.upgrade() else {
                return false;
            };
            inbox.push(
                &event,
                encoded_bytes,
                self.shared.maximum_events,
                self.shared.maximum_bytes,
            );
            count += 1;
            true
        });
        if count == 0 {
            Err(SendError(event))
        } else {
            Ok(count)
        }
    }

    pub(crate) fn close(&self) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .close();
    }
}

impl Drop for Subscribers {
    fn drop(&mut self) {
        self.state
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .close();
    }
}

impl SubscriberState {
    fn close(&mut self) {
        self.closed = true;
        for inbox in self
            .receivers
            .drain(..)
            .filter_map(|receiver| receiver.upgrade())
        {
            inbox.close();
        }
    }
}

#[derive(Debug)]
struct QueuedEvent {
    event: CdpEvent,
    encoded_bytes: usize,
}

#[derive(Debug, Default)]
struct InboxState {
    events: VecDeque<QueuedEvent>,
    encoded_bytes: usize,
    skipped: u64,
    closed: bool,
}

#[derive(Debug)]
struct Inbox {
    state: Mutex<InboxState>,
    changed: Notify,
}

impl Inbox {
    fn close(&self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .closed = true;
        self.changed.notify_one();
    }

    fn push(
        &self,
        event: &CdpEvent,
        encoded_bytes: usize,
        maximum_events: usize,
        maximum_bytes: usize,
    ) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while state.events.len() >= maximum_events
            || encoded_bytes > maximum_bytes.saturating_sub(state.encoded_bytes)
        {
            let Some(oldest) = state.events.pop_front() else {
                break;
            };
            state.encoded_bytes -= oldest.encoded_bytes;
            state.skipped = state.skipped.saturating_add(1);
        }
        if maximum_events == 0 || encoded_bytes > maximum_bytes {
            state.skipped = state.skipped.saturating_add(1);
        } else {
            state.encoded_bytes += encoded_bytes;
            state.events.push_back(QueuedEvent {
                event: event.clone(),
                encoded_bytes,
            });
        }
        drop(state);
        self.changed.notify_one();
    }

    fn try_recv(&self) -> Result<CdpEvent, TryRecvError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.skipped != 0 {
            return Err(TryRecvError::Lagged(std::mem::take(&mut state.skipped)));
        }
        if let Some(queued) = state.events.pop_front() {
            state.encoded_bytes -= queued.encoded_bytes;
            return Ok(queued.event);
        }
        Err(if state.closed {
            TryRecvError::Closed
        } else {
            TryRecvError::Empty
        })
    }
}

/// One ordered CDP subscription bounded by event count and encoded bytes.
pub struct CdpEventReceiver {
    inbox: Arc<Inbox>,
}

impl std::fmt::Debug for CdpEventReceiver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (events, bytes, skipped, closed) = {
            let state = self
                .inbox
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (
                state.events.len(),
                state.encoded_bytes,
                state.skipped,
                state.closed,
            )
        };
        formatter
            .debug_struct("CdpEventReceiver")
            .field("buffered_events", &events)
            .field("encoded_bytes", &bytes)
            .field("skipped", &skipped)
            .field("closed", &closed)
            .finish()
    }
}

impl CdpEventReceiver {
    pub(crate) fn pending_len(&self) -> usize {
        let state = self
            .inbox
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.events.len() + usize::from(state.skipped != 0)
    }

    /// Waits for an event, reported overflow, or publisher shutdown.
    pub async fn recv(&mut self) -> Result<CdpEvent, RecvError> {
        loop {
            let changed = self.inbox.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            match self.inbox.try_recv() {
                Ok(event) => return Ok(event),
                Err(TryRecvError::Lagged(skipped)) => return Err(RecvError::Lagged(skipped)),
                Err(TryRecvError::Closed) => return Err(RecvError::Closed),
                Err(TryRecvError::Empty) => changed.await,
            }
        }
    }

    /// Reads an available event or reports overflow, closure, or an empty queue.
    pub fn try_recv(&mut self) -> Result<CdpEvent, TryRecvError> {
        self.inbox.try_recv()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(index: usize) -> CdpEvent {
        CdpEvent {
            method: "Network.loadingFinished".into(),
            params: Arc::new(json!({"index":index})),
            session_id: None,
        }
    }

    #[test]
    fn byte_pressure_releases_oldest_events_and_reports_loss()
    -> Result<(), Box<dyn std::error::Error>> {
        let sender = EventSender::new(8, 5);
        let mut receiver = sender.subscribe();
        for index in 0..3 {
            sender.send(event(index), 3)?;
        }
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Lagged(2))));
        assert_eq!(receiver.try_recv()?.params["index"], 2);
        assert_eq!(
            receiver
                .inbox
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .encoded_bytes,
            0
        );
        sender.send(event(3), usize::MAX)?;
        assert_eq!(receiver.pending_len(), 1);
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Lagged(1))));
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
        Ok(())
    }

    #[test]
    fn count_pressure_is_local_to_the_slow_subscriber() -> Result<(), Box<dyn std::error::Error>> {
        let sender = EventSender::new(2, 128);
        let mut slow = sender.subscribe();
        let mut fast = sender.subscribe();
        for index in 0..4 {
            sender.send(event(index), 1)?;
            assert_eq!(fast.try_recv()?.params["index"], index);
        }
        assert!(matches!(slow.try_recv(), Err(TryRecvError::Lagged(2))));
        assert_eq!(slow.try_recv()?.params["index"], 2);
        assert_eq!(slow.try_recv()?.params["index"], 3);
        Ok(())
    }

    #[test]
    fn subscriber_drop_releases_buffered_payloads() -> Result<(), Box<dyn std::error::Error>> {
        let sender = EventSender::new(4, 128);
        let receiver = sender.subscribe();
        let event = event(0);
        let payload = Arc::downgrade(&event.params);
        sender.send(event, 16)?;
        assert!(payload.upgrade().is_some());
        drop(receiver);
        assert!(payload.upgrade().is_none());
        assert_eq!(sender.receiver_count(), 0);
        Ok(())
    }

    #[test]
    fn receiver_diagnostics_report_counts_and_redact_payloads()
    -> Result<(), Box<dyn std::error::Error>> {
        let sender = EventSender::new(4, 128);
        let receiver = sender.subscribe();
        let mut message = event(0);
        message.params = Arc::new(json!({"header":"secret-fixture-value"}));
        sender.send(message, 64)?;
        let diagnostic = format!("{receiver:?}");
        assert!(diagnostic.contains("buffered_events: 1"));
        assert!(diagnostic.contains("encoded_bytes: 64"));
        assert!(!diagnostic.contains("secret-fixture-value"));
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_receive_preserves_events_and_final_close()
    -> Result<(), Box<dyn std::error::Error>> {
        let sender = EventSender::new(4, 128);
        let mut receiver = sender.subscribe();
        {
            let receiving = receiver.recv();
            tokio::pin!(receiving);
            assert!(futures_util::poll!(&mut receiving).is_pending());
        }
        sender.send(event(0), 16)?;
        drop(sender);
        assert_eq!(receiver.recv().await?.params["index"], 0);
        assert!(matches!(receiver.recv().await, Err(RecvError::Closed)));
        Ok(())
    }

    #[tokio::test]
    async fn waiting_receiver_wakes_for_publication_and_shutdown()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let sender = EventSender::new(4, 128);
        let mut receiver = sender.subscribe();
        let task = tokio::spawn(async move {
            assert_eq!(receiver.recv().await?.params["index"], 7);
            assert!(matches!(receiver.recv().await, Err(RecvError::Closed)));
            Ok::<_, RecvError>(())
        });
        tokio::task::yield_now().await;
        sender.send(event(7), 16)?;
        drop(sender);
        tokio::time::timeout(std::time::Duration::from_secs(1), task).await???;
        Ok(())
    }
}
