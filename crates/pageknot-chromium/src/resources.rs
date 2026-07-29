use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use pageknot_browser::{LoadedResource, ResourceObservationLimits};
use pageknot_model::{
    ErrorStage, PageKnotError, RedactionPolicy, ResourceRetrievalSource, Result, SourceSummary,
};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Notify};
use tokio::time::{Instant, timeout};
use tokio_util::sync::CancellationToken;
use url::Url;

use self::identity::{
    ObservedResourceState, ResponseSelection, observe_request, observe_request_id,
    observed_response, record_response, refresh_observed_request_identity, request_key,
    select_reusable_response,
};
use self::normalization::{fulfilled_response_headers, validate_intercepted_body};
use self::storage::{
    ObservedBody, append_observed_bytes_at, append_observed_data, begin_observed_stream,
    complete_observed_stream, mark_observed_body_too_large, mark_observed_body_unavailable,
    observed_body_error, update_observed_completion,
};
use self::stream::{
    capture_service_worker_body, capture_service_worker_buffered_data, service_worker_body_pending,
};
use self::tasks::OrderedBodyTasks;
use crate::CdpClient;
use crate::targets::SessionRegistry;

mod identity;
mod normalization;
mod storage;
mod stream;
mod tasks;
#[cfg(test)]
pub(crate) mod test_support;

pub(crate) use normalization::{RENDERED_RESPONSE_RESOURCE_TYPES, captures_rendered_response};
pub(crate) use stream::resource_body_stream;

const MAXIMUM_OBSERVED_RESPONSES: usize = 20_000;
const MAXIMUM_RESOURCE_BODY_TASKS: usize = 64;
const OBSERVED_RESPONSE_WAIT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub(crate) struct ObservedResources {
    state: Arc<Mutex<ObservedResourceState>>,
    changed: Arc<Notify>,
    cancellation: CancellationToken,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

#[derive(Clone, Debug)]
pub(crate) struct ObservedResourceRecorder {
    state: Arc<Mutex<ObservedResourceState>>,
    changed: Arc<Notify>,
}

pub(crate) struct InterceptedResponse<'a> {
    pub(crate) session_id: &'a str,
    pub(crate) request_id: &'a str,
    pub(crate) network_id: &'a str,
    pub(crate) parameters: &'a Value,
    pub(crate) cancellation: CancellationToken,
}

pub(crate) struct CapturedInterceptedResponse {
    pub(crate) response_headers: Vec<Value>,
    pub(crate) body: String,
}

impl ObservedResources {
    pub(crate) fn start(
        client: CdpClient,
        sessions: SessionRegistry,
        limits: ResourceObservationLimits,
    ) -> Self {
        let state = Arc::new(Mutex::new(ObservedResourceState::new(limits)));
        let changed = Arc::new(Notify::new());
        let cancellation = CancellationToken::new();
        let task_state = Arc::clone(&state);
        let task_changed = Arc::clone(&changed);
        let task_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let mut events = client.subscribe();
            let mut body_tasks = OrderedBodyTasks::new(MAXIMUM_RESOURCE_BODY_TASKS);
            loop {
                let event = tokio::select! {
                    () = task_cancellation.cancelled() => {
                        body_tasks.abort_and_drain().await;
                        return;
                    }
                    completed = body_tasks.join_next(), if !body_tasks.is_empty() => {
                        if !handle_body_task_completion(
                            completed,
                            &task_state,
                            &task_changed,
                        )
                        .await
                        {
                            body_tasks.abort_and_drain().await;
                            return;
                        }
                        continue;
                    }
                    event = events.recv() => event,
                };
                let event = match event {
                    Ok(event) => event,
                    Err(error) => {
                        task_state.lock().await.error = Some(
                            PageKnotError::new(
                                "pageknot.browser.cdp_event_lag",
                                ErrorStage::Resource,
                                format!("observed resource event stream failed: {error}"),
                            )
                            .retryable(true),
                        );
                        task_changed.notify_waiters();
                        body_tasks.abort_and_drain().await;
                        return;
                    }
                };
                let Some(session_id) = event.session_id.as_deref() else {
                    continue;
                };
                if !sessions.read().await.contains(session_id) {
                    continue;
                }
                match event.method.as_ref() {
                    "Network.requestWillBeSent" => {
                        observe_request(&task_state, session_id, &event.params).await;
                    }
                    "Network.responseReceived" => {
                        let Some(response) = observed_response(&event.params, session_id) else {
                            continue;
                        };
                        let key = request_key(&response.session_id, &response.request_id);
                        let request_id = response.request_id.clone();
                        let stream_service_worker_body = event
                            .params
                            .pointer("/response/fromServiceWorker")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                            && captures_rendered_response(
                                event.params.get("type").and_then(Value::as_str),
                            );
                        record_response(&task_state, key.clone(), response).await;
                        if stream_service_worker_body {
                            if !drain_ready_body_tasks(&mut body_tasks, &task_state, &task_changed)
                                .await
                            {
                                body_tasks.abort_and_drain().await;
                                return;
                            }
                            if !body_tasks.has_capacity() {
                                mark_observed_body_unavailable(&task_state, &key).await;
                            } else if begin_observed_stream(&task_state, &key).await {
                                let body_client = client.clone();
                                let body_state = Arc::clone(&task_state);
                                let body_session_id = session_id.to_owned();
                                let body_cancellation = task_cancellation.clone();
                                let spawned = body_tasks.spawn(key.clone(), async move {
                                    capture_service_worker_buffered_data(
                                        &body_client,
                                        &body_state,
                                        &body_session_id,
                                        &request_id,
                                        body_cancellation,
                                    )
                                    .await
                                });
                                debug_assert!(spawned);
                            }
                        }
                    }
                    "Network.dataReceived" => {
                        let Some(request_id) =
                            event.params.get("requestId").and_then(Value::as_str)
                        else {
                            continue;
                        };
                        let Some(data) = event.params.get("data").and_then(Value::as_str) else {
                            continue;
                        };
                        let key = request_key(session_id, request_id);
                        if let Err(error) = append_observed_data(&task_state, &key, data).await {
                            task_state.lock().await.error = Some(error);
                            task_changed.notify_waiters();
                            return;
                        }
                    }
                    "Network.loadingFinished" => {
                        let Some(request_id) =
                            event.params.get("requestId").and_then(Value::as_str)
                        else {
                            continue;
                        };
                        let key = request_key(session_id, request_id);
                        if service_worker_body_pending(&task_state, &key).await {
                            if !drain_ready_body_tasks(&mut body_tasks, &task_state, &task_changed)
                                .await
                            {
                                body_tasks.abort_and_drain().await;
                                return;
                            }
                            if body_tasks.has_capacity() {
                                let body_client = client.clone();
                                let body_state = Arc::clone(&task_state);
                                let body_session_id = session_id.to_owned();
                                let body_parameters = event.params.clone();
                                let body_cancellation = task_cancellation.clone();
                                let spawned = body_tasks.spawn(key, async move {
                                    capture_service_worker_body(
                                        &body_client,
                                        &body_state,
                                        &body_session_id,
                                        &body_parameters,
                                        body_cancellation,
                                    )
                                    .await?;
                                    update_observed_completion(
                                        &body_state,
                                        &body_session_id,
                                        &body_parameters,
                                        true,
                                    )
                                    .await;
                                    Ok(())
                                });
                                debug_assert!(spawned);
                            } else {
                                mark_observed_body_unavailable(&task_state, &key).await;
                                update_observed_completion(
                                    &task_state,
                                    session_id,
                                    &event.params,
                                    true,
                                )
                                .await;
                            }
                        } else {
                            update_observed_completion(
                                &task_state,
                                session_id,
                                &event.params,
                                true,
                            )
                            .await;
                        }
                    }
                    "Network.loadingFailed" => {
                        update_observed_completion(&task_state, session_id, &event.params, false)
                            .await;
                    }
                    _ => {}
                }
                task_changed.notify_waiters();
            }
        });
        Self {
            state,
            changed,
            cancellation,
            task: Mutex::new(Some(task)),
        }
    }

    pub(crate) fn recorder(&self) -> ObservedResourceRecorder {
        ObservedResourceRecorder {
            state: Arc::clone(&self.state),
            changed: Arc::clone(&self.changed),
        }
    }

    pub(crate) async fn load(
        &self,
        session_id: &str,
        frame_id: &str,
        url: &Url,
        maximum_bytes: u64,
    ) -> Result<Option<LoadedResource>> {
        let body_limit = {
            let state = self.state.lock().await;
            maximum_bytes.min(state.limits.maximum_resource_bytes)
        };
        let deadline = Instant::now() + OBSERVED_RESPONSE_WAIT;
        let selection = loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let selection = {
                let state = self.state.lock().await;
                if let Some(error) = &state.error {
                    return Err(error.clone());
                }
                select_reusable_response(&state, session_id, frame_id, url)
            };
            if !matches!(selection, ResponseSelection::Pending) {
                break selection;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || timeout(remaining, notified).await.is_err() {
                return Err(with_resource_url(
                    PageKnotError::new(
                        "pageknot.resource.load",
                        ErrorStage::Resource,
                        "observed response did not settle before resource loading",
                    )
                    .retryable(true),
                    url,
                ));
            }
        };
        let response = match selection {
            ResponseSelection::Missing => return Ok(None),
            ResponseSelection::Pending => {
                return Err(with_resource_url(
                    PageKnotError::new(
                        "pageknot.resource.load",
                        ErrorStage::Resource,
                        "observed response remained pending after resource synchronization",
                    )
                    .retryable(true),
                    url,
                ));
            }
            ResponseSelection::Ambiguous => {
                return Err(with_resource_url(
                    PageKnotError::new(
                        "pageknot.resource.load",
                        ErrorStage::Resource,
                        "observed response identity is ambiguous for the rendered resource",
                    ),
                    url,
                ));
            }
            ResponseSelection::Unavailable => {
                return Err(with_resource_url(
                    PageKnotError::new(
                        "pageknot.resource.load",
                        ErrorStage::Resource,
                        "observed response bytes are unavailable for the rendered resource",
                    ),
                    url,
                ));
            }
            ResponseSelection::TooLarge { attempted, limit } => {
                return Err(with_resource_url(
                    PageKnotError::new(
                        "pageknot.resource.limit",
                        ErrorStage::Resource,
                        "observed response body exceeds the configured byte limit",
                    )
                    .with_detail("attempted", attempted)
                    .with_detail("limit", limit),
                    url,
                ));
            }
            ResponseSelection::Ready(response) => response,
        };
        let ObservedBody::Complete(bytes) = response.body else {
            return Err(PageKnotError::new(
                "pageknot.resource.load",
                ErrorStage::Resource,
                "observed response bytes changed state during resource loading",
            ));
        };
        let bytes_length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if bytes_length > body_limit {
            return Err(with_resource_url(
                PageKnotError::new(
                    "pageknot.resource.limit",
                    ErrorStage::Resource,
                    "observed response body exceeds the configured byte limit",
                )
                .with_detail("attempted", bytes_length)
                .with_detail("limit", body_limit),
                url,
            ));
        }
        let stream = futures_util::stream::once(async move { Ok(bytes) });
        Ok(Some(LoadedResource {
            final_url: response.url,
            redirects: response.request_urls.into_iter().skip(1).collect(),
            status: response.status,
            media_type: response.media_type,
            encoded_length: Some(bytes_length),
            body: Box::pin(stream),
            source: ResourceRetrievalSource::ObservedResponse,
        }))
    }

    pub(crate) async fn close(&self) {
        self.cancellation.cancel();
        if let Some(task) = self.task.lock().await.take() {
            let _ignored = task.await;
        }
    }

    #[cfg(test)]
    pub(crate) async fn has_request(&self, session_id: &str, request_id: &str) -> bool {
        self.state
            .lock()
            .await
            .requests
            .contains_key(&request_key(session_id, request_id))
    }
}

async fn drain_ready_body_tasks(
    tasks: &mut OrderedBodyTasks,
    state: &Mutex<ObservedResourceState>,
    changed: &Notify,
) -> bool {
    while let Some(completed) = tasks.try_join_next() {
        if !handle_body_task_completion(Some(completed), state, changed).await {
            return false;
        }
    }
    true
}

async fn handle_body_task_completion(
    completed: Option<std::result::Result<pageknot_model::Result<()>, tokio::task::JoinError>>,
    state: &Mutex<ObservedResourceState>,
    changed: &Notify,
) -> bool {
    let error = match completed {
        Some(Ok(Ok(()))) => None,
        Some(Ok(Err(error))) => Some(error),
        Some(Err(error)) => Some(PageKnotError::new(
            "pageknot.browser.resource_task",
            ErrorStage::Resource,
            format!("resource body task failed: {error}"),
        )),
        None => None,
    };
    if let Some(error) = error {
        state.lock().await.error = Some(error);
        changed.notify_waiters();
        return false;
    }
    changed.notify_waiters();
    true
}

impl ObservedResourceRecorder {
    pub(crate) async fn capture_intercepted_response(
        &self,
        client: &CdpClient,
        response: InterceptedResponse<'_>,
    ) -> Result<CapturedInterceptedResponse> {
        let taken = client
            .command(
                "Fetch.takeResponseBodyAsStream",
                json!({"requestId": response.request_id}),
                Some(response.session_id),
            )
            .await
            .map_err(|error| with_intercepted_resource_url(error, response.parameters))?;
        let handle = taken
            .get("stream")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.resource.stream",
                    ErrorStage::Resource,
                    "intercepted response returned no body stream",
                )
            })?
            .to_owned();
        let maximum_resource_bytes = self.state.lock().await.limits.maximum_resource_bytes;
        let mut stream = resource_body_stream(
            client.clone(),
            response.session_id.to_owned(),
            handle,
            maximum_resource_bytes,
            response.cancellation,
        );
        let key = request_key(response.session_id, response.network_id);
        while let Some(chunk) = tokio_stream::StreamExt::next(&mut stream).await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    if error.code.as_str() == "pageknot.resource.limit"
                        && let (Some(attempted), Some(limit)) = (
                            error.details.get("attempted").and_then(Value::as_u64),
                            error.details.get("limit").and_then(Value::as_u64),
                        )
                    {
                        mark_observed_body_too_large(&self.state, &key, attempted, limit).await;
                    }
                    return Err(with_intercepted_resource_url(error, response.parameters));
                }
            };
            append_observed_bytes_at(&self.state, &key, chunk.to_vec(), false).await?;
            if let Some(error) = observed_body_error(&self.state, &key).await {
                return Err(with_intercepted_resource_url(error, response.parameters));
            }
        }
        let body = complete_observed_stream(&self.state, &key)
            .await
            .map_err(|error| with_intercepted_resource_url(error, response.parameters))?;
        validate_intercepted_body(response.parameters, body.len())
            .map_err(|error| with_intercepted_resource_url(error, response.parameters))?;
        Ok(CapturedInterceptedResponse {
            response_headers: fulfilled_response_headers(response.parameters),
            body: base64::engine::general_purpose::STANDARD.encode(&body),
        })
    }

    pub(crate) async fn reject_intercepted_response(&self, session_id: &str, network_id: &str) {
        self.reject_stream(session_id, network_id).await;
    }

    pub(crate) async fn begin_stream(
        &self,
        session_id: &str,
        request_id: &str,
        parameters: &Value,
    ) -> bool {
        let key = request_key(session_id, request_id);
        let known = {
            let state = self.state.lock().await;
            state.requests.contains_key(&key) || state.records.contains_key(&key)
        };
        if !known {
            observe_request_id(&self.state, session_id, request_id, parameters, false).await;
        }
        refresh_observed_request_identity(&self.state, &key, parameters).await;
        let started = begin_observed_stream(&self.state, &key).await;
        self.changed.notify_waiters();
        started
    }

    pub(crate) async fn reject_stream(&self, session_id: &str, request_id: &str) {
        mark_observed_body_unavailable(&self.state, &request_key(session_id, request_id)).await;
        self.changed.notify_waiters();
    }
}

pub(crate) fn with_resource_url(mut error: PageKnotError, url: &Url) -> PageKnotError {
    let summary = SourceSummary::new(url, url, &RedactionPolicy::default());
    redact_url_from_error(&mut error, url.as_str(), summary.requested_url.as_str());
    error
        .with_detail("url", summary.requested_url.as_str().to_owned())
        .with_detail("urlSha256", summary.requested_url_sha256.to_string())
}

fn with_intercepted_resource_url(error: PageKnotError, parameters: &Value) -> PageKnotError {
    let url = parameters
        .pointer("/request/url")
        .and_then(Value::as_str)
        .and_then(|url| Url::parse(url).ok());
    if let Some(url) = url {
        with_resource_url(error, &url)
    } else {
        error
    }
}

fn redact_url_from_error(error: &mut PageKnotError, raw: &str, redacted: &str) {
    error.message = error.message.replace(raw, redacted);
    for value in error.details.values_mut() {
        redact_url_from_value(value, raw, redacted);
    }
    if let Some(source) = error.source.as_deref_mut() {
        redact_url_from_error(source, raw, redacted);
    }
}

fn redact_url_from_value(value: &mut Value, raw: &str, redacted: &str) {
    match value {
        Value::String(text) => *text = text.replace(raw, redacted),
        Value::Array(values) => {
            for value in values {
                redact_url_from_value(value, raw, redacted);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                redact_url_from_value(value, raw, redacted);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

impl Drop for ObservedResources {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Ok(mut task) = self.task.try_lock()
            && let Some(task) = task.take()
        {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::Arc;

    use pageknot_browser::ResourceObservationLimits;
    use serde_json::json;
    use tokio::sync::RwLock;

    use super::*;
    use crate::resources::test_support::TestCdpServer;

    #[tokio::test]
    async fn enabled_notification_preserves_a_change_before_await() {
        let changed = Notify::new();
        let notified = changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        changed.notify_waiters();

        assert!(timeout(Duration::from_millis(20), notified).await.is_ok());
    }

    #[tokio::test]
    async fn slow_body_commands_do_not_block_later_network_events()
    -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let server = TestCdpServer::start("Network.streamResourceContent").await?;
        let client = CdpClient::connect(server.endpoint().clone()).await?;
        let sessions = Arc::new(RwLock::new(HashSet::from(["session".to_owned()])));
        let observed = ObservedResources::start(
            client.clone(),
            sessions,
            ResourceObservationLimits::default(),
        );
        wait_for_receivers(&client, 1).await?;

        server.send_event(
            "Network.requestWillBeSent",
            json!({
                "requestId": "slow",
                "frameId": "frame",
                "request": {
                    "url": "https://example.test/slow.svg",
                    "method": "GET",
                    "headers": {}
                }
            }),
            "session",
        )?;
        wait_for_request(&observed, "slow").await?;
        server.send_event(
            "Network.responseReceived",
            json!({
                "requestId": "slow",
                "frameId": "frame",
                "type": "Image",
                "response": {
                    "url": "https://example.test/slow.svg",
                    "status": 200,
                    "mimeType": "image/svg+xml",
                    "fromServiceWorker": true
                }
            }),
            "session",
        )?;
        server
            .wait_for_method("Network.streamResourceContent")
            .await?;

        server.send_event(
            "Network.requestWillBeSent",
            json!({
                "requestId": "later",
                "frameId": "frame",
                "request": {
                    "url": "https://example.test/later.svg",
                    "method": "GET",
                    "headers": {}
                }
            }),
            "session",
        )?;

        wait_for_request(&observed, "later").await?;
        observed.close().await;
        client.close().await?;
        server.close().await;
        Ok(())
    }

    async fn wait_for_receivers(
        client: &CdpClient,
        expected: usize,
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        timeout(Duration::from_secs(2), async {
            while client.event_receiver_count() < expected {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        Ok(())
    }

    async fn wait_for_request(
        observed: &ObservedResources,
        request_id: &str,
    ) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
        timeout(Duration::from_secs(2), async {
            while !observed.has_request("session", request_id).await {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        Ok(())
    }

    #[test]
    fn resource_error_url_details_redact_credentials_and_keep_a_digest()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let url = Url::parse(
            "https://user:userinfo-secret@example.test/a?X-Amz-Signature=signed-secret&view=full",
        )?;
        let error = with_resource_url(
            PageKnotError::new(
                "pageknot.resource.load",
                ErrorStage::Resource,
                format!("resource load failed for {}", url.as_str()),
            )
            .with_detail("upstream", url.as_str()),
            &url,
        );
        let encoded = serde_json::to_string(&error)?;

        assert!(!encoded.contains("userinfo-secret"));
        assert!(!encoded.contains("signed-secret"));
        assert!(encoded.contains("view=full"));
        assert_eq!(
            error.details.get("urlSha256"),
            Some(&serde_json::json!(
                pageknot_model::ContentDigest::sha256(url.as_str()).to_string()
            ))
        );
        Ok(())
    }
}
