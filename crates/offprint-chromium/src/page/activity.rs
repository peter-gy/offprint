use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use offprint_model::{ErrorStage, OffprintError, Result};
use serde_json::Value;
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;

use crate::CdpClient;
use crate::targets::SessionRegistry;

#[derive(Debug)]
struct ActivityState {
    // Chrome preserves Network.RequestId when a request moves from a parent
    // target to an out-of-process frame or worker session.
    active: HashSet<String>,
    last_change: tokio::time::Instant,
    error: Option<OffprintError>,
}

#[derive(Debug)]
pub(super) struct NetworkActivity {
    state: Arc<Mutex<ActivityState>>,
    changed: Arc<Notify>,
    cancellation: CancellationToken,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl NetworkActivity {
    pub(super) fn start(client: CdpClient, sessions: SessionRegistry) -> Self {
        let state = Arc::new(Mutex::new(ActivityState {
            active: HashSet::new(),
            last_change: tokio::time::Instant::now(),
            error: None,
        }));
        let changed = Arc::new(Notify::new());
        let cancellation = CancellationToken::new();
        let task_state = Arc::clone(&state);
        let task_changed = Arc::clone(&changed);
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let mut events = client.subscribe();
            loop {
                let event = tokio::select! {
                    () = task_cancellation.cancelled() => return,
                    event = events.recv() => event,
                };
                let event = match event {
                    Ok(event) => event,
                    Err(error) => {
                        let mut state = task_state.lock().await;
                        state.error = Some(OffprintError::new(
                            "offprint.browser.cdp_event_lag",
                            ErrorStage::Readiness,
                            format!("network activity stream failed: {error}"),
                        ));
                        task_changed.notify_waiters();
                        return;
                    }
                };
                let Some(session_id) = event.session_id.as_deref() else {
                    continue;
                };
                if !sessions.read().await.contains(session_id) {
                    continue;
                }
                let mut changed_state = false;
                let mut state = task_state.lock().await;
                match event.method.as_ref() {
                    "Network.requestWillBeSent" => {
                        if tracks_readiness_request(&event.params)
                            && let Some(request_id) =
                                event.params.get("requestId").and_then(Value::as_str)
                        {
                            state.active.insert(request_id.to_owned());
                            changed_state = true;
                        }
                    }
                    "Network.loadingFinished" | "Network.loadingFailed" => {
                        if let Some(request_id) =
                            event.params.get("requestId").and_then(Value::as_str)
                            && state.active.remove(request_id)
                        {
                            changed_state = true;
                        }
                    }
                    _ => {}
                }
                if changed_state {
                    state.last_change = tokio::time::Instant::now();
                    drop(state);
                    task_changed.notify_waiters();
                }
            }
        });
        Self {
            state,
            changed,
            cancellation,
            task: Mutex::new(Some(task)),
        }
    }

    pub(super) async fn wait_for_quiet(&self, quiet: Duration) -> Result<u32> {
        loop {
            let notified = self.changed.notified();
            let (active, last_change, error) = {
                let state = self.state.lock().await;
                (state.active.len(), state.last_change, state.error.clone())
            };
            if let Some(error) = error {
                return Err(error);
            }
            let elapsed = last_change.elapsed();
            if elapsed >= quiet {
                return request_count(active);
            }
            tokio::select! {
                () = tokio::time::sleep(quiet - elapsed) => {}
                () = notified => {}
            }
        }
    }

    pub(super) async fn wait_for_idle(&self, quiet: Duration) -> Result<()> {
        loop {
            let notified = self.changed.notified();
            let (active, last_change, error) = {
                let state = self.state.lock().await;
                (state.active.len(), state.last_change, state.error.clone())
            };
            if let Some(error) = error {
                return Err(error);
            }
            if active > 0 {
                notified.await;
                continue;
            }
            let elapsed = last_change.elapsed();
            if elapsed >= quiet {
                return Ok(());
            }
            tokio::select! {
                () = tokio::time::sleep(quiet - elapsed) => {}
                () = notified => {}
            }
        }
    }

    pub(super) async fn in_flight(&self) -> Result<u32> {
        let state = self.state.lock().await;
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        request_count(state.active.len())
    }

    pub(super) async fn close(&self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.lock().await.take() {
            let _ignored = task.await;
        }
    }
}

fn request_count(active: usize) -> Result<u32> {
    u32::try_from(active).map_err(|error| {
        OffprintError::new(
            "offprint.readiness.request_count",
            ErrorStage::Readiness,
            format!("in-flight request count exceeds the supported range: {error}"),
        )
    })
}

impl Drop for NetworkActivity {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Ok(mut task) = self.task.try_lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}

pub(super) fn is_long_lived_resource_type(resource_type: Option<&str>) -> bool {
    matches!(resource_type, Some("WebSocket" | "EventSource"))
}

fn tracks_readiness_request(parameters: &Value) -> bool {
    let resource_type = parameters.get("type").and_then(Value::as_str);
    if is_long_lived_resource_type(resource_type) {
        return false;
    }
    !parameters
        .pointer("/request/url")
        .and_then(Value::as_str)
        .and_then(|value| url::Url::parse(value).ok())
        .is_some_and(|url| matches!(url.scheme(), "blob" | "data"))
}

pub(super) fn is_long_lived_response(parameters: &Value) -> bool {
    is_long_lived_resource_type(parameters.get("resourceType").and_then(Value::as_str))
        || parameters
            .get("responseHeaders")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|header| {
                header
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| name.eq_ignore_ascii_case("content-type"))
                    && header
                        .get("value")
                        .and_then(Value::as_str)
                        .and_then(|value| value.split(';').next())
                        .is_some_and(|media_type| {
                            media_type.trim().eq_ignore_ascii_case("text/event-stream")
                        })
            })
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::error::Error;
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::{Mutex, Notify};
    use tokio_util::sync::CancellationToken;

    use super::{ActivityState, NetworkActivity, tracks_readiness_request};

    #[test]
    fn readiness_request_filter_tracks_transferable_urls() {
        for url in [
            "https://example.test/asset.js",
            "http://example.test/asset.js",
            "file:///tmp/asset.js",
        ] {
            assert!(tracks_readiness_request(&serde_json::json!({
                "request": {"url": url},
                "type": "Script"
            })));
        }
        for url in [
            "blob:https://example.test/worker",
            "data:text/javascript,postMessage('ready')",
        ] {
            assert!(!tracks_readiness_request(&serde_json::json!({
                "request": {"url": url},
                "type": "Script"
            })));
        }
        assert!(!tracks_readiness_request(&serde_json::json!({
            "request": {"url": "https://example.test/events"},
            "type": "EventSource"
        })));
    }

    #[tokio::test]
    async fn quiet_window_completes_with_a_stable_in_flight_request()
    -> Result<(), Box<dyn Error + Send + Sync>> {
        let activity = NetworkActivity {
            state: Arc::new(Mutex::new(ActivityState {
                active: HashSet::from(["pending-request".to_owned()]),
                last_change: tokio::time::Instant::now(),
                error: None,
            })),
            changed: Arc::new(Notify::new()),
            cancellation: CancellationToken::new(),
            task: Mutex::new(None),
        };

        let active = tokio::time::timeout(
            Duration::from_millis(100),
            activity.wait_for_quiet(Duration::from_millis(10)),
        )
        .await??;

        assert_eq!(active, 1);
        Ok(())
    }

    #[tokio::test]
    async fn idle_window_waits_for_in_flight_requests_to_finish()
    -> Result<(), Box<dyn Error + Send + Sync>> {
        let state = Arc::new(Mutex::new(ActivityState {
            active: HashSet::from(["pending-request".to_owned()]),
            last_change: tokio::time::Instant::now(),
            error: None,
        }));
        let changed = Arc::new(Notify::new());
        let activity = NetworkActivity {
            state: Arc::clone(&state),
            changed: Arc::clone(&changed),
            cancellation: CancellationToken::new(),
            task: Mutex::new(None),
        };
        let waiter =
            tokio::spawn(async move { activity.wait_for_idle(Duration::from_millis(10)).await });

        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!waiter.is_finished());
        {
            let mut state = state.lock().await;
            state.active.clear();
            state.last_change = tokio::time::Instant::now();
        }
        changed.notify_waiters();

        tokio::time::timeout(Duration::from_millis(100), waiter).await???;
        Ok(())
    }
}
