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
    ObservedBody, append_observed_bytes_at, append_observed_data, append_observed_data_at,
    begin_observed_stream, complete_observed_stream, mark_observed_body_too_large,
    mark_observed_body_unavailable, observed_body_error, update_observed_completion,
};
use self::stream::capture_service_worker_body;
use crate::CdpClient;
use crate::targets::SessionRegistry;

mod identity;
mod normalization;
mod storage;
mod stream;

pub(crate) use normalization::{RENDERED_RESPONSE_RESOURCE_TYPES, captures_rendered_response};
pub(crate) use stream::resource_body_stream;

const MAXIMUM_OBSERVED_RESPONSES: usize = 20_000;
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
    pub(crate) response_code: u16,
    pub(crate) parameters: &'a Value,
    pub(crate) cancellation: CancellationToken,
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
            loop {
                let event = tokio::select! {
                    () = task_cancellation.cancelled() => return,
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
                        let stream_service_worker_body = event
                            .params
                            .pointer("/response/fromServiceWorker")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                            && captures_rendered_response(
                                event.params.get("type").and_then(Value::as_str),
                            )
                            && begin_observed_stream(&task_state, &key).await;
                        let buffered_data = if stream_service_worker_body {
                            client
                                .command(
                                    "Network.streamResourceContent",
                                    json!({"requestId": response.request_id.as_str()}),
                                    Some(session_id),
                                )
                                .await
                                .ok()
                                .and_then(|result| {
                                    result
                                        .get("bufferedData")
                                        .and_then(Value::as_str)
                                        .map(str::to_owned)
                                })
                        } else {
                            None
                        };
                        record_response(&task_state, key.clone(), response).await;
                        if stream_service_worker_body
                            && let Some(buffered_data) = buffered_data
                            && let Err(error) =
                                append_observed_data_at(&task_state, &key, &buffered_data, true)
                                    .await
                        {
                            task_state.lock().await.error = Some(error);
                            task_changed.notify_waiters();
                            return;
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
                        capture_service_worker_body(
                            &client,
                            &task_state,
                            session_id,
                            &event.params,
                        )
                        .await;
                        update_observed_completion(&task_state, session_id, &event.params, true)
                            .await;
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
}

impl ObservedResourceRecorder {
    pub(crate) async fn capture_intercepted_response(
        &self,
        client: &CdpClient,
        response: InterceptedResponse<'_>,
    ) -> Result<()> {
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
        let encoded = base64::engine::general_purpose::STANDARD.encode(&body);
        client
            .command(
                "Fetch.fulfillRequest",
                json!({
                    "requestId": response.request_id,
                    "responseCode": response.response_code,
                    "responseHeaders": fulfilled_response_headers(response.parameters),
                    "body": encoded,
                }),
                Some(response.session_id),
            )
            .await
            .map_err(|error| with_intercepted_resource_url(error, response.parameters))
            .map(|_| ())
    }

    pub(crate) async fn fail_intercepted_response(
        &self,
        client: &CdpClient,
        session_id: &str,
        request_id: &str,
        network_id: &str,
    ) {
        self.reject_stream(session_id, network_id).await;
        let _ignored = client
            .command(
                "Fetch.failRequest",
                json!({
                    "requestId": request_id,
                    "errorReason": "Failed",
                }),
                Some(session_id),
            )
            .await;
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
    use super::*;

    #[tokio::test]
    async fn enabled_notification_preserves_a_change_before_await() {
        let changed = Notify::new();
        let notified = changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        changed.notify_waiters();

        assert!(timeout(Duration::from_millis(20), notified).await.is_ok());
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
