use std::sync::Arc;

use pageknot_browser::NetworkGuard;
use pageknot_model::{ErrorStage, PageKnotError, RequestHeader, Result};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::activity::is_long_lived_response;
use crate::CdpClient;
use crate::resources::{InterceptedResponse, ObservedResourceRecorder, captures_rendered_response};
use crate::targets::SessionRegistry;

const MAXIMUM_INTERCEPTION_TASKS: usize = 256;
const MAXIMUM_RESPONSE_STREAMS: usize = 4;

#[derive(Debug)]
struct InterceptionState {
    errors: Vec<(Option<String>, bool, PageKnotError)>,
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
            let response_streams = Arc::new(Semaphore::new(MAXIMUM_RESPONSE_STREAMS));
            loop {
                let event = tokio::select! {
                    () = task_cancellation.cancelled() => {
                        continuations.abort_all();
                        while continuations.join_next().await.is_some() {}
                        return;
                    }
                    completed = continuations.join_next(), if !continuations.is_empty() => {
                        if let Some(Err(error)) = completed {
                            record_interception_error(
                                &task_state,
                                PageKnotError::new(
                                    "pageknot.browser.interception_task",
                                    ErrorStage::Navigation,
                                    format!("network interception task failed: {error}"),
                                ),
                                None,
                                true,
                            )
                            .await;
                        }
                        continue;
                    }
                    event = events.recv() => event,
                };
                let event = match event {
                    Ok(event) => event,
                    Err(error) => {
                        record_interception_error(
                            &task_state,
                            PageKnotError::new(
                                "pageknot.browser.cdp_event_lag",
                                ErrorStage::Navigation,
                                format!("network policy event stream failed: {error}"),
                            ),
                            None,
                            true,
                        )
                        .await;
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
                let Some(request_id) = event.params.get("requestId").and_then(Value::as_str) else {
                    record_interception_error(
                        &task_state,
                        PageKnotError::new(
                            "pageknot.browser.cdp_shape",
                            ErrorStage::Navigation,
                            "Fetch.requestPaused omitted requestId",
                        ),
                        None,
                        true,
                    )
                    .await;
                    return;
                };
                if event.params.get("responseStatusCode").is_some() {
                    let parameters = json!({"requestId": request_id});
                    let response_code = event
                        .params
                        .get("responseStatusCode")
                        .and_then(Value::as_u64)
                        .and_then(|value| u16::try_from(value).ok())
                        .unwrap_or(200);
                    let continue_without_body = !captures_rendered_response(
                        event.params.get("resourceType").and_then(Value::as_str),
                    ) || is_long_lived_response(&event.params)
                        || matches!(response_code, 204 | 205 | 304)
                        || (300..400).contains(&response_code);
                    if continue_without_body {
                        if let Err(error) = client
                            .command("Fetch.continueResponse", parameters, Some(session_id))
                            .await
                        {
                            record_interception_error(&task_state, error, None, true).await;
                            return;
                        }
                        continue;
                    }
                    if let Some(network_id) = event.params.get("networkId").and_then(Value::as_str)
                        && observed_resources
                            .begin_stream(session_id, network_id, &event.params)
                            .await
                    {
                        if continuations.len() >= MAXIMUM_INTERCEPTION_TASKS {
                            let error = PageKnotError::new(
                                "pageknot.navigation.request_limit",
                                ErrorStage::Navigation,
                                "concurrent intercepted requests exceed the supported limit",
                            )
                            .with_detail("limit", MAXIMUM_INTERCEPTION_TASKS);
                            record_interception_error(&task_state, error, None, false).await;
                            observed_resources
                                .fail_intercepted_response(
                                    &client, session_id, request_id, network_id,
                                )
                                .await;
                            continue;
                        }
                        let task_client = client.clone();
                        let task_observed_resources = observed_resources.clone();
                        let task_state = Arc::clone(&task_state);
                        let task_session_id = session_id.to_owned();
                        let task_network_id = network_id.to_owned();
                        let task_request_id = request_id.to_owned();
                        let task_parameters = event.params.clone();
                        let task_frame_id = event
                            .params
                            .get("frameId")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                        let task_cancellation = task_cancellation.clone();
                        let task_response_streams = Arc::clone(&response_streams);
                        continuations.spawn(async move {
                            let permit = task_response_streams.acquire_owned().await;
                            let Ok(_permit) = permit else {
                                let error = PageKnotError::new(
                                    "pageknot.browser.interception_closed",
                                    ErrorStage::Resource,
                                    "response interception closed before the body was captured",
                                );
                                task_observed_resources
                                    .fail_intercepted_response(
                                        &task_client,
                                        &task_session_id,
                                        &task_request_id,
                                        &task_network_id,
                                    )
                                    .await;
                                record_interception_error(&task_state, error, task_frame_id, false)
                                    .await;
                                return;
                            };
                            let captured = task_observed_resources
                                .capture_intercepted_response(
                                    &task_client,
                                    InterceptedResponse {
                                        session_id: &task_session_id,
                                        request_id: &task_request_id,
                                        network_id: &task_network_id,
                                        response_code,
                                        parameters: &task_parameters,
                                        cancellation: task_cancellation,
                                    },
                                )
                                .await;
                            if let Err(error) = captured {
                                task_observed_resources
                                    .fail_intercepted_response(
                                        &task_client,
                                        &task_session_id,
                                        &task_request_id,
                                        &task_network_id,
                                    )
                                    .await;
                                record_interception_error(&task_state, error, task_frame_id, false)
                                    .await;
                            }
                        });
                        continue;
                    }
                    if let Err(error) = client
                        .command("Fetch.continueResponse", parameters, Some(session_id))
                        .await
                    {
                        record_interception_error(&task_state, error, None, true).await;
                        return;
                    }
                    continue;
                }
                let destination = event
                    .params
                    .pointer("/request/url")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        PageKnotError::new(
                            "pageknot.browser.cdp_shape",
                            ErrorStage::Navigation,
                            "Fetch.requestPaused omitted the request URL",
                        )
                    })
                    .and_then(|value| {
                        Url::parse(value).map_err(|error| {
                            PageKnotError::new(
                                "pageknot.navigation.url",
                                ErrorStage::Navigation,
                                format!("Chromium requested an invalid URL: {error}"),
                            )
                        })
                    });
                let inject_headers = destination
                    .as_ref()
                    .is_ok_and(|destination| same_origin(destination, &header_origin));
                let decision = match &destination {
                    Ok(destination) => validate_guard_destination(&guard, destination).await,
                    Err(error) => Err(error.clone()),
                };
                let (method, parameters) = match decision {
                    Ok(()) => {
                        let mut parameters = json!({"requestId": request_id});
                        if inject_headers && !headers.is_empty() {
                            parameters["headers"] =
                                Value::Array(continued_headers(&event.params, &headers));
                        }
                        ("Fetch.continueRequest", parameters)
                    }
                    Err(error) => {
                        let document_request =
                            event.params.get("resourceType").and_then(Value::as_str)
                                == Some("Document")
                                || event
                                    .params
                                    .get("isNavigationRequest")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false);
                        let frame_id = event
                            .params
                            .get("frameId")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                        record_interception_error(&task_state, error, frame_id, document_request)
                            .await;
                        (
                            "Fetch.failRequest",
                            json!({
                                "requestId": request_id,
                                "errorReason": "BlockedByClient"
                            }),
                        )
                    }
                };
                if let Err(error) = client.command(method, parameters, Some(session_id)).await {
                    record_interception_error(&task_state, error, None, true).await;
                    return;
                }
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
    ) -> Option<PageKnotError> {
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
        if let Some(task) = self.task.take() {
            task.abort();
        }
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
    error: PageKnotError,
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
        PageKnotError::new(
            "pageknot.navigation.host",
            ErrorStage::Navigation,
            "network URL must contain a host",
        )
    })?;
    if let Ok(address) = host.parse::<std::net::IpAddr>() {
        return guard.validate_resolved(url, [address]);
    }
    let port = url.port_or_known_default().ok_or_else(|| {
        PageKnotError::new(
            "pageknot.navigation.port",
            ErrorStage::Navigation,
            "network URL has no usable port",
        )
    })?;
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| {
            PageKnotError::new(
                "pageknot.navigation.dns",
                ErrorStage::Navigation,
                format!("failed to resolve navigation host: {error}"),
            )
            .retryable(true)
        })?
        .map(|address| address.ip())
        .collect::<Vec<_>>();
    guard.validate_resolved(url, addresses)
}
