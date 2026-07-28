use base64::Engine as _;
use bytes::Bytes;
use pageknot_browser::BodyStream;
use pageknot_model::{ErrorStage, PageKnotError};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

use super::identity::{ObservedResourceState, request_key};
use super::storage::{
    ObservedBody, append_observed_bytes_at, append_observed_chunk, append_observed_data,
    available_append_bytes, mark_observed_body_unavailable,
};
use crate::CdpClient;

const RESOURCE_READ_BYTES: u64 = 64 * 1024;

pub(crate) fn resource_body_stream(
    client: CdpClient,
    session_id: String,
    handle: String,
    maximum_bytes: u64,
    cancellation: CancellationToken,
) -> BodyStream {
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    tokio::spawn(async move {
        let mut received = 0_u64;
        loop {
            let response = tokio::select! {
                () = cancellation.cancelled() => {
                    let error = PageKnotError::new(
                        "pageknot.resource.stream",
                        ErrorStage::Resource,
                        "browser resource stream was cancelled",
                    );
                    let _ignored = sender.send(Err(error)).await;
                    break;
                }
                response = client.command(
                    "IO.read",
                    json!({"handle": handle.as_str(), "size": RESOURCE_READ_BYTES}),
                    Some(&session_id),
                ) => response,
            };
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    let _ignored = sender.send(Err(error)).await;
                    break;
                }
            };
            let data = response.get("data").and_then(Value::as_str).unwrap_or("");
            let chunk = if response
                .get("base64Encoded")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                match base64::engine::general_purpose::STANDARD.decode(data) {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        let error = PageKnotError::new(
                            "pageknot.resource.decode",
                            ErrorStage::Resource,
                            format!("browser resource stream contains invalid base64: {error}"),
                        );
                        let _ignored = sender.send(Err(error)).await;
                        break;
                    }
                }
            } else {
                data.as_bytes().to_vec()
            };
            received = match received.checked_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX)) {
                Some(received) => received,
                None => {
                    let error = PageKnotError::new(
                        "pageknot.resource.limit",
                        ErrorStage::Resource,
                        "resource body byte count overflowed",
                    );
                    let _ignored = sender.send(Err(error)).await;
                    break;
                }
            };
            if received > maximum_bytes {
                let error = PageKnotError::new(
                    "pageknot.resource.limit",
                    ErrorStage::Resource,
                    "resource body exceeds the configured byte limit",
                )
                .with_detail("attempted", received)
                .with_detail("limit", maximum_bytes);
                let _ignored = sender.send(Err(error)).await;
                break;
            }
            if !chunk.is_empty() {
                let sent = tokio::select! {
                    () = cancellation.cancelled() => false,
                    result = sender.send(Ok(Bytes::from(chunk))) => result.is_ok(),
                };
                if !sent {
                    break;
                }
            }
            if response
                .get("eof")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                break;
            }
        }
        let _ignored = client
            .command(
                "IO.close",
                json!({"handle": handle.as_str()}),
                Some(&session_id),
            )
            .await;
    });
    Box::pin(ReceiverStream::new(receiver))
}

pub(super) async fn capture_service_worker_body(
    client: &CdpClient,
    state: &Mutex<ObservedResourceState>,
    session_id: &str,
    parameters: &Value,
) {
    let Some(request_id) = parameters.get("requestId").and_then(Value::as_str) else {
        return;
    };
    let key = request_key(session_id, request_id);
    let needs_body = {
        let state = state.lock().await;
        state.records.get(&key).is_some_and(|response| {
            response.from_service_worker
                && response.stream_attempted
                && matches!(&response.body, ObservedBody::Streaming(body) if body.is_empty())
        })
    };
    if !needs_body {
        return;
    }
    let declared_bytes = parameters
        .get("encodedDataLength")
        .and_then(Value::as_f64)
        .filter(|length| length.is_finite() && *length >= 0.0)
        .map(|length| length as u64);
    let available = available_append_bytes(state, &key).await;
    if declared_bytes.is_some_and(|length| length > available) {
        let _ignored =
            append_observed_chunk(state, &key, None, declared_bytes.unwrap_or(u64::MAX), false)
                .await;
        return;
    }
    let response = client
        .command(
            "Network.getResponseBody",
            json!({"requestId": request_id}),
            Some(session_id),
        )
        .await;
    let Some(response) = response.ok() else {
        mark_observed_body_unavailable(state, &key).await;
        return;
    };
    let Some(body) = response.get("body").and_then(Value::as_str) else {
        mark_observed_body_unavailable(state, &key).await;
        return;
    };
    let appended = if response
        .get("base64Encoded")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        append_observed_data(state, &key, body).await
    } else {
        append_observed_bytes_at(state, &key, body.as_bytes().to_vec(), false).await
    };
    if let Err(error) = appended {
        state.lock().await.error = Some(error);
    }
}
