use std::sync::Arc;

use offprint_browser::NetworkGuard;
use offprint_model::{ErrorStage, OffprintError, RequestHeader, Result};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::activity::is_long_lived_response;
use crate::CdpClient;
use crate::resources::ObservedResourceRecorder;
use crate::targets::SessionRegistry;

use self::paused::{InterceptionContext, InterceptionFailure, PausedRequest};

mod paused;

const MAXIMUM_INTERCEPTION_TASKS: usize = 256;

#[derive(Debug)]
struct InterceptionState {
    errors: Vec<(Option<String>, bool, OffprintError)>,
}

#[derive(Debug)]
pub(super) struct NetworkInterception {
    state: Arc<Mutex<InterceptionState>>,
    cancellation: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl NetworkInterception {
    pub(super) fn start(
        client: CdpClient,
        sessions: SessionRegistry,
        observed_resources: ObservedResourceRecorder,
        guard: NetworkGuard,
        headers: Vec<RequestHeader>,
        header_origin: Url,
    ) -> Self {
        let state = Arc::new(Mutex::new(InterceptionState { errors: Vec::new() }));
        let cancellation = CancellationToken::new();
        let task_state = Arc::clone(&state);
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let mut events = client.subscribe();
            let mut continuations = tokio::task::JoinSet::new();
            let context = InterceptionContext::new(
                client.clone(),
                observed_resources,
                guard,
                headers,
                header_origin,
                task_cancellation.clone(),
            );
            loop {
                let event = tokio::select! {
                    () = task_cancellation.cancelled() => {
                        drain_interception_tasks(&mut continuations, &task_state).await;
                        return;
                    }
                    completed = continuations.join_next(), if !continuations.is_empty() => {
                        record_interception_task_completion(&task_state, completed).await;
                        continue;
                    }
                    event = events.recv() => event,
                };
                let event = match event {
                    Ok(event) => event,
                    Err(error) => {
                        record_interception_error(
                            &task_state,
                            OffprintError::new(
                                "offprint.browser.cdp_event_lag",
                                ErrorStage::Navigation,
                                format!("network policy event stream failed: {error}"),
                            ),
                            None,
                            true,
                        )
                        .await;
                        task_cancellation.cancel();
                        drain_interception_tasks(&mut continuations, &task_state).await;
                        return;
                    }
                };
                let Some(session_id) = event.session_id.as_deref() else {
                    continue;
                };
                if !sessions.read().await.contains(session_id)
                    || event.method.as_ref() != "Fetch.requestPaused"
                {
                    continue;
                }
                let request = match PausedRequest::from_event(session_id, event.params.clone()) {
                    Ok(request) => request,
                    Err(error) => {
                        record_interception_error(&task_state, error, None, true).await;
                        task_cancellation.cancel();
                        drain_interception_tasks(&mut continuations, &task_state).await;
                        return;
                    }
                };
                drain_ready_interception_tasks(&mut continuations, &task_state).await;
                if continuations.len() >= MAXIMUM_INTERCEPTION_TASKS {
                    let error = OffprintError::new(
                        "offprint.navigation.request_limit",
                        ErrorStage::Navigation,
                        "concurrent intercepted requests exceed the supported limit",
                    )
                    .with_detail("limit", MAXIMUM_INTERCEPTION_TASKS);
                    record_interception_error(&task_state, error, None, false).await;
                    if let Err(error) = request.fail_overloaded(&client).await {
                        record_interception_error(&task_state, error, None, true).await;
                    }
                    continue;
                }
                let task_context = context.clone();
                continuations.spawn(request.run(task_context));
            }
        });
        Self {
            state,
            cancellation,
            task: Some(task),
        }
    }

    pub(super) async fn error(
        &self,
        frame_id: &str,
        use_single_document_error: bool,
    ) -> Option<OffprintError> {
        let state = self.state.lock().await;
        state
            .errors
            .iter()
            .find(|(owner, document, _)| {
                *document && owner.as_deref().is_none_or(|owner| owner == frame_id)
            })
            .map(|(_, _, error)| error.clone())
            .or_else(|| {
                use_single_document_error
                    .then_some(())
                    .and_then(|()| state.errors.last())
                    .map(|(_, _, error)| error.clone())
            })
    }

    pub(super) async fn close(mut self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            let _ignored = task.await;
        }
    }
}

impl Drop for NetworkInterception {
    fn drop(&mut self) {
        self.cancellation.cancel();
        let _detached = self.task.take();
    }
}

async fn drain_ready_interception_tasks(
    tasks: &mut tokio::task::JoinSet<Vec<InterceptionFailure>>,
    state: &Mutex<InterceptionState>,
) {
    while let Some(completed) = tasks.try_join_next() {
        record_interception_task_completion(state, Some(completed)).await;
    }
}

async fn drain_interception_tasks(
    tasks: &mut tokio::task::JoinSet<Vec<InterceptionFailure>>,
    state: &Mutex<InterceptionState>,
) {
    while let Some(completed) = tasks.join_next().await {
        record_interception_task_completion(state, Some(completed)).await;
    }
}

async fn record_interception_task_completion(
    state: &Mutex<InterceptionState>,
    completed: Option<std::result::Result<Vec<InterceptionFailure>, tokio::task::JoinError>>,
) {
    match completed {
        Some(Ok(failures)) => {
            for failure in failures {
                record_interception_error(
                    state,
                    failure.error,
                    failure.frame_id,
                    failure.document_request,
                )
                .await;
            }
        }
        Some(Err(error)) => {
            record_interception_error(
                state,
                OffprintError::new(
                    "offprint.browser.interception_task",
                    ErrorStage::Navigation,
                    format!("network interception task failed: {error}"),
                ),
                None,
                true,
            )
            .await;
        }
        None => {}
    }
}

pub(super) fn continued_headers(parameters: &Value, scoped: &[RequestHeader]) -> Vec<Value> {
    let mut headers = parameters
        .pointer("/request/headers")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|headers| headers.iter())
        .filter(|(name, _)| {
            !scoped
                .iter()
                .any(|header| header.name.eq_ignore_ascii_case(name))
        })
        .filter_map(|(name, value)| {
            value.as_str().map(|value| {
                json!({
                    "name": name,
                    "value": value,
                })
            })
        })
        .collect::<Vec<_>>();
    headers.extend(scoped.iter().map(|header| {
        json!({
            "name": header.name.as_str(),
            "value": header.value.expose_secret(),
        })
    }));
    headers
}

pub(super) fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left
            .host_str()
            .zip(right.host_str())
            .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right))
        && left.port_or_known_default() == right.port_or_known_default()
}

pub(super) fn cookie_domain_matches(domain: &str, initial_host: Option<&str>) -> bool {
    let domain = domain.trim_start_matches('.');
    initial_host.is_some_and(|host| {
        host.eq_ignore_ascii_case(domain)
            || host
                .strip_suffix(domain)
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

async fn record_interception_error(
    state: &Mutex<InterceptionState>,
    error: OffprintError,
    frame_id: Option<String>,
    document_request: bool,
) {
    let mut state = state.lock().await;
    if !state.errors.iter().any(|(owner, document, _)| {
        owner.as_deref() == frame_id.as_deref() && *document == document_request
    }) {
        state.errors.push((frame_id, document_request, error));
    }
}

pub(super) async fn validate_guard_destination(guard: &NetworkGuard, url: &Url) -> Result<()> {
    guard.validate_url(url)?;
    let host = url.host_str().ok_or_else(|| {
        OffprintError::new(
            "offprint.navigation.host",
            ErrorStage::Navigation,
            "network URL must contain a host",
        )
    })?;
    if let Ok(address) = host.parse::<std::net::IpAddr>() {
        return guard.validate_resolved(url, [address]);
    }
    let port = url.port_or_known_default().ok_or_else(|| {
        OffprintError::new(
            "offprint.navigation.port",
            ErrorStage::Navigation,
            "network URL has no usable port",
        )
    })?;
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| {
            OffprintError::new(
                "offprint.navigation.dns",
                ErrorStage::Navigation,
                format!("failed to resolve navigation host: {error}"),
            )
            .retryable(true)
        })?
        .map(|address| address.ip())
        .collect::<Vec<_>>();
    guard.validate_resolved(url, addresses)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::error::Error;
    use std::sync::Arc;
    use std::time::Duration;

    use offprint_browser::{NetworkGuard, ResourceObservationLimits};
    use offprint_model::NetworkPolicy;
    use serde_json::json;
    use tokio::sync::RwLock;
    use tokio::time::timeout;

    use super::*;
    use crate::resources::ObservedResources;
    use crate::resources::test_support::TestCdpServer;

    type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

    #[tokio::test]
    async fn cancellation_resolves_each_paused_response_once() -> TestResult {
        let server = TestCdpServer::start("Fetch.takeResponseBodyAsStream").await?;
        let client = CdpClient::connect(server.endpoint().clone()).await?;
        let sessions = Arc::new(RwLock::new(HashSet::from(["session".to_owned()])));
        let observed = ObservedResources::start(
            client.clone(),
            Arc::clone(&sessions),
            ResourceObservationLimits::default(),
        );
        let origin = Url::parse("https://example.test/")?;
        let interception = NetworkInterception::start(
            client.clone(),
            sessions,
            observed.recorder(),
            NetworkGuard::new(NetworkPolicy::Unrestricted, &origin)?,
            Vec::new(),
            origin,
        );
        wait_for_receivers(&client, 2).await?;

        server.send_event(
            "Network.requestWillBeSent",
            json!({
                "requestId": "network",
                "frameId": "frame",
                "request": {
                    "url": "https://example.test/image.svg",
                    "method": "GET",
                    "headers": {}
                }
            }),
            "session",
        )?;
        timeout(Duration::from_secs(2), async {
            while !observed.has_request("session", "network").await {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        server.send_event(
            "Fetch.requestPaused",
            json!({
                "requestId": "paused",
                "networkId": "network",
                "frameId": "frame",
                "resourceType": "Image",
                "responseStatusCode": 200,
                "responseHeaders": [
                    {"name": "Content-Type", "value": "image/svg+xml"}
                ],
                "request": {
                    "url": "https://example.test/image.svg",
                    "method": "GET",
                    "headers": {}
                }
            }),
            "session",
        )?;
        server
            .wait_for_method("Fetch.takeResponseBodyAsStream")
            .await?;

        interception.close().await;

        timeout(Duration::from_secs(2), async {
            while server.terminal_commands("paused").await.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let terminal = server.terminal_commands("paused").await;
        assert_eq!(terminal.len(), 1);
        assert_eq!(terminal[0]["method"], "Fetch.failRequest");

        observed.close().await;
        client.close().await?;
        server.close().await;
        Ok(())
    }

    async fn wait_for_receivers(client: &CdpClient, expected: usize) -> TestResult {
        timeout(Duration::from_secs(2), async {
            while client.event_receiver_count() < expected {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        Ok(())
    }
}
