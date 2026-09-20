use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Weak};

#[cfg(test)]
use tokio::sync::broadcast;

mod queue;
pub use queue::CdpEventReceiver;
use queue::EventSender;

use super::{CdpEvent, MAX_EVENT_QUEUE_BYTES, navigation_event, offline_event, target_event};

type Routes = Arc<Mutex<RouteState>>;

#[derive(Debug, Default)]
struct RouteState {
    sessions: BTreeMap<String, Weak<PageEvents>>,
    pages: Vec<Weak<PageEvents>>,
    closed: bool,
}

#[derive(Debug)]
struct EventChannels {
    navigation: EventSender,
    offline: EventSender,
    targets: EventSender,
    interception: EventSender,
    activity: EventSender,
    resources: EventSender,
}

impl EventChannels {
    fn close(&self) {
        for channel in [
            &self.navigation,
            &self.offline,
            &self.targets,
            &self.interception,
            &self.activity,
            &self.resources,
        ] {
            channel.close();
        }
    }

    #[cfg(test)]
    fn receiver_count(&self) -> usize {
        self.navigation.receiver_count()
            + self.offline.receiver_count()
            + self.targets.receiver_count()
            + self.interception.receiver_count()
            + self.activity.receiver_count()
            + self.resources.receiver_count()
    }
    fn new(capacity: usize) -> Self {
        Self {
            navigation: EventSender::new(capacity, MAX_EVENT_QUEUE_BYTES),
            offline: EventSender::new(capacity, MAX_EVENT_QUEUE_BYTES),
            targets: EventSender::new(capacity, MAX_EVENT_QUEUE_BYTES),
            interception: EventSender::new(capacity, MAX_EVENT_QUEUE_BYTES),
            activity: EventSender::new(capacity, MAX_EVENT_QUEUE_BYTES),
            resources: EventSender::new(capacity, MAX_EVENT_QUEUE_BYTES),
        }
    }

    fn publish(&self, event: CdpEvent, encoded_bytes: usize) {
        match event.method.as_ref() {
            "Network.requestWillBeSent" | "Network.loadingFinished" | "Network.loadingFailed" => {
                let _ignored = self.activity.send(event.clone(), encoded_bytes);
                let _ignored = self.resources.send(event.clone(), encoded_bytes);
            }
            "Network.responseReceived" => {
                let _ignored = self.resources.send(event.clone(), encoded_bytes);
            }
            "Network.dataReceived"
                if event
                    .params
                    .get("data")
                    .is_some_and(|data| data.is_string()) =>
            {
                let _ignored = self.resources.send(event.clone(), encoded_bytes);
            }
            _ => {}
        }
        if event.method.as_ref() == "Fetch.requestPaused" {
            let _ignored = self.interception.send(event.clone(), encoded_bytes);
        }
        if navigation_event(&event) {
            let _ignored = self.navigation.send(event.clone(), encoded_bytes);
        }
        if self.offline.receiver_count() != 0 && offline_event(&event) {
            let _ignored = self.offline.send(event.clone(), encoded_bytes);
        }
        if target_event(&event) {
            let _ignored = self.targets.send(event, encoded_bytes);
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct EventBus {
    pub(super) all: EventSender,
    routes: Routes,
    capacity: usize,
}

impl EventBus {
    #[cfg(test)]
    pub(super) fn receiver_count(&self) -> usize {
        let scopes = self
            .routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pages
            .iter()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        self.all.receiver_count()
            + scopes
                .iter()
                .map(|scope| scope.channels.receiver_count())
                .sum::<usize>()
    }
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            all: EventSender::new(capacity, MAX_EVENT_QUEUE_BYTES),
            routes: Arc::new(Mutex::new(RouteState::default())),
            capacity,
        }
    }

    pub(super) fn scope(&self, session: &str) -> Arc<PageEvents> {
        let scope = Arc::new(PageEvents {
            channels: EventChannels::new(self.capacity),
            routes: Arc::clone(&self.routes),
        });
        let mut routes = self
            .routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if routes.closed {
            scope.channels.close();
        } else {
            // Subscription lifetime outlives any individual session route.
            routes.pages.push(Arc::downgrade(&scope));
            routes
                .sessions
                .insert(session.to_owned(), Arc::downgrade(&scope));
        }
        drop(routes);
        scope
    }

    pub(super) fn publish(&self, event: CdpEvent, encoded_bytes: usize) {
        let scope = event.session_id.as_deref().and_then(|session| {
            self.routes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .sessions
                .get(session)
                .and_then(Weak::upgrade)
        });
        if let Some(scope) = scope {
            scope.channels.publish(event.clone(), encoded_bytes);
        }
        let _ignored = self.all.send(event, encoded_bytes);
    }

    pub(super) fn publisher(&self) -> EventPublisher {
        EventPublisher { bus: self.clone() }
    }

    fn close(&self) {
        let scopes = {
            let mut routes = self
                .routes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            routes.closed = true;
            routes.sessions.clear();
            std::mem::take(&mut routes.pages)
                .into_iter()
                .filter_map(|scope| scope.upgrade())
                .collect::<Vec<_>>()
        };
        self.all.close();
        for scope in scopes {
            scope.channels.close();
        }
    }
}

// The wire task owns publication. Dropping its future, even before its first
// poll, terminates subscriptions while clients may remain alive for diagnostics.
#[derive(Debug)]
pub(super) struct EventPublisher {
    pub(super) bus: EventBus,
}

impl Drop for EventPublisher {
    fn drop(&mut self) {
        self.bus.close();
    }
}

/// Page admission is checked once, when the transport publishes an event.
/// Detaching a session never retracts events already admitted to a subscriber.
#[derive(Debug)]
pub(crate) struct PageEvents {
    channels: EventChannels,
    routes: Routes,
}

impl PageEvents {
    pub(crate) fn register(self: &Arc<Self>, session: &str) {
        let mut routes = self
            .routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if routes.closed {
            self.channels.close();
        } else {
            routes
                .sessions
                .insert(session.to_owned(), Arc::downgrade(self));
        }
    }

    pub(crate) fn unregister(&self, session: &str) {
        let mut routes = self
            .routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if routes
            .sessions
            .get(session)
            .is_some_and(|scope| std::ptr::eq(scope.as_ptr(), self))
        {
            routes.sessions.remove(session);
        }
    }

    pub(crate) fn navigation(&self) -> CdpEventReceiver {
        self.channels.navigation.subscribe()
    }
    pub(crate) fn offline(&self) -> CdpEventReceiver {
        self.channels.offline.subscribe()
    }
    pub(crate) fn targets(&self) -> CdpEventReceiver {
        self.channels.targets.subscribe()
    }
    pub(crate) fn interception(&self) -> CdpEventReceiver {
        self.channels.interception.subscribe()
    }
    pub(crate) fn activity(&self) -> CdpEventReceiver {
        self.channels.activity.subscribe()
    }
    pub(crate) fn resources(&self) -> CdpEventReceiver {
        self.channels.resources.subscribe()
    }
}

impl Drop for PageEvents {
    fn drop(&mut self) {
        let mut routes = self
            .routes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        routes
            .sessions
            .retain(|_, scope| !std::ptr::eq(scope.as_ptr(), self));
        routes
            .pages
            .retain(|scope| !std::ptr::eq(scope.as_ptr(), self));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn event(session: &str) -> CdpEvent {
        CdpEvent {
            method: "Page.loadEventFired".into(),
            params: Arc::new(json!({"timestamp":1})),
            session_id: Some(session.into()),
        }
    }

    #[test]
    fn target_events_remain_ordered_while_other_observations_burst()
    -> Result<(), Box<dyn std::error::Error>> {
        let bus = EventBus::new(2);
        let scope = bus.scope("main");
        let mut targets = scope.targets();
        let mut attached = event("main");
        attached.method = "Target.attachedToTarget".into();
        bus.publish(attached, 64);
        for _ in 0..512 {
            bus.publish(event("main"), 64);
        }
        let mut detached = event("main");
        detached.method = "Target.detachedFromTarget".into();
        bus.publish(detached, 64);
        assert_eq!(
            targets.try_recv()?.method.as_ref(),
            "Target.attachedToTarget"
        );
        assert_eq!(
            targets.try_recv()?.method.as_ref(),
            "Target.detachedFromTarget"
        );
        Ok(())
    }

    #[test]
    fn detach_preserves_admitted_completions_and_offline_evidence()
    -> Result<(), Box<dyn std::error::Error>> {
        let bus = EventBus::new(4);
        let scope = bus.scope("main");
        scope.register("worker");
        let mut activity = scope.activity();
        let mut resources = scope.resources();
        let mut offline = scope.offline();
        let mut request = event("worker");
        request.method = "Network.requestWillBeSent".into();
        request.params = Arc::new(json!({"request":{"url":"https://example.test/worker"}}));
        bus.publish(request, 64);
        let mut completed = event("worker");
        completed.method = "Network.loadingFinished".into();
        bus.publish(completed.clone(), 64);
        scope.unregister("worker");
        bus.publish(completed, 64);
        for receiver in [&mut activity, &mut resources] {
            assert_eq!(
                receiver.try_recv()?.method.as_ref(),
                "Network.requestWillBeSent"
            );
            assert_eq!(
                receiver.try_recv()?.method.as_ref(),
                "Network.loadingFinished"
            );
            assert!(matches!(
                receiver.try_recv(),
                Err(broadcast::error::TryRecvError::Empty)
            ));
        }
        assert_eq!(offline.try_recv()?.session_id.as_deref(), Some("worker"));
        Ok(())
    }

    #[test]
    fn publisher_termination_closes_retained_and_future_subscriptions()
    -> Result<(), Box<dyn std::error::Error>> {
        let bus = EventBus::new(4);
        let scope = bus.scope("main");
        let mut browser = bus.all.subscribe();
        let mut page = scope.navigation();
        let publisher = bus.publisher();
        bus.publish(event("main"), 64);
        scope.unregister("main");
        drop(publisher);
        for receiver in [&mut browser, &mut page] {
            assert_eq!(receiver.try_recv()?.method.as_ref(), "Page.loadEventFired");
            assert!(matches!(
                receiver.try_recv(),
                Err(broadcast::error::TryRecvError::Closed)
            ));
        }
        assert!(matches!(
            scope.navigation().try_recv(),
            Err(broadcast::error::TryRecvError::Closed)
        ));
        assert!(matches!(
            bus.scope("late").activity().try_recv(),
            Err(broadcast::error::TryRecvError::Closed)
        ));
        assert!(matches!(
            bus.all.subscribe().try_recv(),
            Err(broadcast::error::TryRecvError::Closed)
        ));
        Ok(())
    }

    #[test]
    fn another_capture_cannot_displace_queued_events() -> Result<(), Box<dyn std::error::Error>> {
        let bus = EventBus::new(1);
        let first = bus.scope("first");
        let second = bus.scope("second");
        let mut events = first.navigation();
        let mut noisy = second.navigation();
        bus.publish(event("first"), 64);
        for _ in 0..512 {
            bus.publish(event("second"), 64);
        }
        assert_eq!(events.try_recv()?.session_id.as_deref(), Some("first"));
        assert!(matches!(
            events.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        assert!(matches!(
            noisy.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(_))
        ));
        Ok(())
    }

    #[test]
    fn page_observations_cannot_displace_intercepted_requests()
    -> Result<(), Box<dyn std::error::Error>> {
        let bus = EventBus::new(1);
        let scope = bus.scope("page");
        let mut requests = scope.channels.interception.subscribe();
        let mut paused = event("page");
        paused.method = "Fetch.requestPaused".into();
        bus.publish(paused, 64);
        for _ in 0..512 {
            bus.publish(event("page"), 64);
        }
        assert_eq!(requests.try_recv()?.method.as_ref(), "Fetch.requestPaused");
        assert!(matches!(
            requests.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        Ok(())
    }

    #[test]
    fn unrelated_page_events_cannot_displace_network_observations()
    -> Result<(), Box<dyn std::error::Error>> {
        let bus = EventBus::new(1);
        let scope = bus.scope("page");
        let mut activity = scope.channels.activity.subscribe();
        let mut resources = scope.channels.resources.subscribe();
        let mut request = event("page");
        request.method = "Network.requestWillBeSent".into();
        bus.publish(request, 64);
        for _ in 0..512 {
            let mut progress = event("page");
            progress.method = "Network.dataReceived".into();
            bus.publish(progress, 64);
            bus.publish(event("page"), 64);
        }
        assert_eq!(
            activity.try_recv()?.method.as_ref(),
            "Network.requestWillBeSent"
        );
        assert_eq!(
            resources.try_recv()?.method.as_ref(),
            "Network.requestWillBeSent"
        );
        let mut data = event("page");
        data.method = "Network.dataReceived".into();
        data.params = Arc::new(json!({"data":"YQ=="}));
        bus.publish(data, 64);
        assert_eq!(
            resources.try_recv()?.method.as_ref(),
            "Network.dataReceived"
        );
        assert!(matches!(
            activity.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        Ok(())
    }

    #[test]
    fn admitted_children_share_owner_events_and_release_their_routes()
    -> Result<(), Box<dyn std::error::Error>> {
        let bus = EventBus::new(4);
        let scope = bus.scope("main");
        let mut events = scope.channels.navigation.subscribe();
        scope.register("child");
        bus.publish(event("child"), 64);
        assert_eq!(events.try_recv()?.session_id.as_deref(), Some("child"));
        scope.unregister("child");
        bus.publish(event("child"), 64);
        assert!(matches!(
            events.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        scope.register("other-child");
        drop(scope);
        assert!(
            bus.routes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .sessions
                .is_empty()
        );
        assert!(matches!(
            events.try_recv(),
            Err(broadcast::error::TryRecvError::Closed)
        ));
        Ok(())
    }
}
