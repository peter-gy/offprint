use base64::Engine as _;
use bytes::Bytes;
use offprint_model::{ErrorStage, OffprintError, Result};
use serde_json::Value;
use tokio::sync::Mutex;

use super::identity::{ObservedResourceState, RequestKey};

#[derive(Clone, Debug)]
pub(super) enum ObservedBody {
    Streaming(Vec<u8>),
    Complete(Bytes),
    Unavailable,
    TooLarge { attempted: u64, limit: u64 },
}

impl ObservedBody {
    pub(super) fn len(&self) -> u64 {
        match self {
            Self::Streaming(bytes) => u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            Self::Complete(bytes) => u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            Self::Unavailable | Self::TooLarge { .. } => 0,
        }
    }
}

pub(super) async fn begin_observed_stream(
    state: &Mutex<ObservedResourceState>,
    key: &RequestKey,
) -> bool {
    let mut state = state.lock().await;
    if let Some(request) = state.requests.get_mut(key) {
        if request.stream_attempted
            || request
                .variant
                .as_ref()
                .is_none_or(|variant| !variant.reusable())
            || request
                .urls
                .last()
                .is_none_or(|url| !matches!(url.scheme(), "http" | "https"))
        {
            return false;
        }
        request.stream_attempted = true;
        request.stream_pending = true;
        return true;
    }
    let body_bytes = {
        let Some(record) = state.records.get(key) else {
            return false;
        };
        if record.stream_attempted
            || record
                .request_variant
                .as_ref()
                .is_none_or(|variant| !variant.reusable())
            || !matches!(record.url.scheme(), "http" | "https")
        {
            return false;
        }
        record.body.len()
    };
    state.body_bytes = state.body_bytes.saturating_sub(body_bytes);
    let Some(record) = state.records.get_mut(key) else {
        return false;
    };
    record.body = ObservedBody::Streaming(Vec::new());
    record.stream_attempted = true;
    record.stream_pending = true;
    true
}

pub(super) async fn append_observed_data(
    state: &Mutex<ObservedResourceState>,
    key: &RequestKey,
    encoded: &str,
) -> Result<()> {
    append_observed_data_at(state, key, encoded, false).await
}

pub(super) async fn append_observed_data_at(
    state: &Mutex<ObservedResourceState>,
    key: &RequestKey,
    encoded: &str,
    prepend: bool,
) -> Result<()> {
    let decoded_upper_bound = decoded_length(encoded);
    let available = available_append_bytes(state, key).await;
    let chunk = if decoded_upper_bound > available {
        None
    } else {
        Some(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|error| {
                    OffprintError::new(
                        "offprint.resource.decode",
                        ErrorStage::Resource,
                        format!("streamed response body contains invalid base64: {error}"),
                    )
                })?,
        )
    };
    let attempted_chunk = chunk.as_ref().map_or(decoded_upper_bound, |chunk| {
        u64::try_from(chunk.len()).unwrap_or(u64::MAX)
    });
    append_observed_chunk(state, key, chunk, attempted_chunk, prepend).await
}

fn decoded_length(encoded: &str) -> u64 {
    let padding = encoded
        .as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .take(2)
        .count();
    u64::try_from(base64::decoded_len_estimate(encoded.len()).saturating_sub(padding))
        .unwrap_or(u64::MAX)
}

pub(super) async fn append_observed_bytes_at(
    state: &Mutex<ObservedResourceState>,
    key: &RequestKey,
    bytes: Vec<u8>,
    prepend: bool,
) -> Result<()> {
    let attempted = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    append_observed_chunk(state, key, Some(bytes), attempted, prepend).await
}

pub(super) async fn available_append_bytes(
    state: &Mutex<ObservedResourceState>,
    key: &RequestKey,
) -> u64 {
    let state = state.lock().await;
    let current_body_bytes = observed_body_bytes(&state, key);
    let retained_other_bytes = state.body_bytes.saturating_sub(current_body_bytes);
    state
        .limits
        .maximum_resource_bytes
        .saturating_sub(current_body_bytes)
        .min(
            state
                .limits
                .maximum_total_resource_bytes
                .saturating_sub(retained_other_bytes),
        )
}

pub(super) async fn append_observed_chunk(
    state: &Mutex<ObservedResourceState>,
    key: &RequestKey,
    chunk: Option<Vec<u8>>,
    attempted_chunk: u64,
    prepend: bool,
) -> Result<()> {
    let mut state = state.lock().await;
    let current_body_bytes = observed_body_bytes(&state, key);
    let attempted = current_body_bytes.saturating_add(attempted_chunk);
    let total_attempted = state
        .body_bytes
        .saturating_sub(current_body_bytes)
        .saturating_add(attempted);
    let exceeded = if attempted > state.limits.maximum_resource_bytes {
        Some((attempted, state.limits.maximum_resource_bytes))
    } else if total_attempted > state.limits.maximum_total_resource_bytes {
        Some((total_attempted, state.limits.maximum_total_resource_bytes))
    } else {
        None
    };
    if let Some((attempted, limit)) = exceeded {
        state.body_bytes = state.body_bytes.saturating_sub(current_body_bytes);
        if let Some(record) = state.records.get_mut(key) {
            record.body = ObservedBody::TooLarge { attempted, limit };
        } else if let Some(request) = state.requests.get_mut(key) {
            request.body = ObservedBody::TooLarge { attempted, limit };
        }
        return Ok(());
    }
    let Some(chunk) = chunk else {
        return Ok(());
    };
    let append = |body: &mut Vec<u8>| {
        if prepend {
            let mut combined = Vec::with_capacity(body.len().saturating_add(chunk.len()));
            combined.extend_from_slice(&chunk);
            combined.extend_from_slice(body);
            *body = combined;
        } else {
            body.extend_from_slice(&chunk);
        }
    };
    let appended = if let Some(record) = state.records.get_mut(key) {
        match &mut record.body {
            ObservedBody::Streaming(body) => {
                append(body);
                true
            }
            ObservedBody::Complete(body) if prepend => {
                let mut combined = Vec::with_capacity(body.len().saturating_add(chunk.len()));
                combined.extend_from_slice(&chunk);
                combined.extend_from_slice(body);
                *body = Bytes::from(combined);
                true
            }
            _ => false,
        }
    } else if let Some(request) = state.requests.get_mut(key)
        && let ObservedBody::Streaming(body) = &mut request.body
    {
        append(body);
        true
    } else {
        false
    };
    if appended {
        state.body_bytes = total_attempted;
    }
    Ok(())
}

pub(super) async fn complete_observed_stream(
    state: &Mutex<ObservedResourceState>,
    key: &RequestKey,
) -> Result<Bytes> {
    let mut state = state.lock().await;
    if let Some(record) = state.records.get_mut(key) {
        let bytes = complete_body(&mut record.body)?;
        record.stream_pending = false;
        return Ok(bytes);
    }
    if let Some(request) = state.requests.get_mut(key) {
        let bytes = complete_body(&mut request.body)?;
        request.stream_pending = false;
        return Ok(bytes);
    }
    Err(OffprintError::new(
        "offprint.resource.load",
        ErrorStage::Resource,
        "observed response identity was lost while buffering its body",
    ))
}

pub(super) async fn observed_body_error(
    state: &Mutex<ObservedResourceState>,
    key: &RequestKey,
) -> Option<OffprintError> {
    let state = state.lock().await;
    let body = state
        .records
        .get(key)
        .map(|record| &record.body)
        .or_else(|| state.requests.get(key).map(|request| &request.body))?;
    match body {
        ObservedBody::TooLarge { attempted, limit } => {
            Some(resource_limit_error(*attempted, *limit))
        }
        ObservedBody::Streaming(_) | ObservedBody::Complete(_) | ObservedBody::Unavailable => None,
    }
}

pub(super) async fn mark_observed_body_too_large(
    state: &Mutex<ObservedResourceState>,
    key: &RequestKey,
    attempted: u64,
    limit: u64,
) {
    let mut state = state.lock().await;
    let body_bytes = observed_body_bytes(&state, key);
    state.body_bytes = state.body_bytes.saturating_sub(body_bytes);
    if let Some(record) = state.records.get_mut(key) {
        record.body = ObservedBody::TooLarge { attempted, limit };
        record.stream_pending = false;
    } else if let Some(request) = state.requests.get_mut(key) {
        request.body = ObservedBody::TooLarge { attempted, limit };
        request.stream_pending = false;
    }
}

pub(super) async fn mark_observed_body_unavailable(
    state: &Mutex<ObservedResourceState>,
    key: &RequestKey,
) {
    let mut state = state.lock().await;
    let body_bytes = state
        .records
        .get(key)
        .map(|record| record.body.len())
        .or_else(|| state.requests.get(key).map(|request| request.body.len()))
        .unwrap_or(0);
    state.body_bytes = state.body_bytes.saturating_sub(body_bytes);
    if let Some(record) = state.records.get_mut(key) {
        if !matches!(record.body, ObservedBody::TooLarge { .. }) {
            record.body = ObservedBody::Unavailable;
        }
        record.stream_pending = false;
    } else if let Some(request) = state.requests.get_mut(key) {
        if !matches!(request.body, ObservedBody::TooLarge { .. }) {
            request.body = ObservedBody::Unavailable;
        }
        request.stream_pending = false;
    }
}

fn observed_body_bytes(state: &ObservedResourceState, key: &RequestKey) -> u64 {
    state
        .records
        .get(key)
        .map(|record| record.body.len())
        .or_else(|| state.requests.get(key).map(|request| request.body.len()))
        .unwrap_or(0)
}

fn complete_body(body: &mut ObservedBody) -> Result<Bytes> {
    match body {
        ObservedBody::Streaming(buffer) => {
            let bytes = Bytes::from(std::mem::take(buffer));
            *body = ObservedBody::Complete(bytes.clone());
            Ok(bytes)
        }
        ObservedBody::Complete(bytes) => Ok(bytes.clone()),
        ObservedBody::TooLarge { attempted, limit } => {
            Err(resource_limit_error(*attempted, *limit))
        }
        ObservedBody::Unavailable => Err(OffprintError::new(
            "offprint.resource.load",
            ErrorStage::Resource,
            "observed response bytes are unavailable",
        )),
    }
}

fn resource_limit_error(attempted: u64, limit: u64) -> OffprintError {
    OffprintError::new(
        "offprint.resource.limit",
        ErrorStage::Resource,
        "observed response body exceeds the configured byte limit",
    )
    .with_detail("attempted", attempted)
    .with_detail("limit", limit)
}

pub(super) async fn update_observed_completion(
    state: &Mutex<ObservedResourceState>,
    session_id: &str,
    parameters: &Value,
    finished: bool,
) {
    let Some(request_id) = parameters.get("requestId").and_then(Value::as_str) else {
        return;
    };
    let mut state = state.lock().await;
    let key = (session_id.to_owned(), request_id.to_owned());
    if !state.records.contains_key(&key) {
        if !finished {
            let body_bytes = state
                .requests
                .get(&key)
                .map_or(0, |request| request.body.len());
            state.body_bytes = state.body_bytes.saturating_sub(body_bytes);
            if let Some(request) = state.requests.get_mut(&key) {
                if !matches!(request.body, ObservedBody::TooLarge { .. }) {
                    request.body = ObservedBody::Unavailable;
                }
                request.stream_pending = false;
            }
        }
        return;
    }
    let Some(response) = state.records.get_mut(&key) else {
        return;
    };
    response.finished = finished;
    response.failed = !finished;
    if finished {
        response.stream_pending = false;
        if response.stream_attempted
            && let ObservedBody::Streaming(body) = &mut response.body
        {
            response.body = ObservedBody::Complete(Bytes::from(std::mem::take(body)));
        } else if !response.stream_attempted {
            response.body = ObservedBody::Unavailable;
        }
    } else {
        let body_bytes = response.body.len();
        response.body = ObservedBody::Unavailable;
        response.stream_pending = false;
        state.body_bytes = state.body_bytes.saturating_sub(body_bytes);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::error::Error;
    use std::sync::Arc;

    use offprint_browser::ResourceObservationLimits;
    use serde_json::json;
    use tokio::sync::Barrier;
    use url::Url;

    use super::*;
    use crate::resources::identity::{
        ObservedRequest, ObservedResponse, RequestVariant, select_reusable_response,
    };

    type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

    fn streaming_request(url: Url) -> ObservedRequest {
        ObservedRequest {
            sequence: 1,
            frame_id: Some("frame".to_owned()),
            urls: vec![url],
            variant: Some(RequestVariant {
                method: "GET".to_owned(),
                has_body: false,
                headers_digest: [0; 32],
            }),
            body: ObservedBody::Streaming(Vec::new()),
            stream_attempted: true,
            stream_pending: true,
            network_observed: true,
        }
    }

    fn completed_response(
        sequence: u64,
        request_id: &str,
        url: &Url,
        body: &'static [u8],
    ) -> ObservedResponse {
        ObservedResponse {
            sequence,
            session_id: "session".to_owned(),
            request_id: request_id.to_owned(),
            frame_id: Some("frame".to_owned()),
            url: url.clone(),
            request_urls: vec![url.clone()],
            request_variant: Some(RequestVariant {
                method: "GET".to_owned(),
                has_body: false,
                headers_digest: [1; 32],
            }),
            status: 200,
            media_type: Some("image/svg+xml".to_owned()),
            from_service_worker: false,
            body: ObservedBody::Complete(Bytes::from_static(body)),
            stream_attempted: true,
            stream_pending: false,
            finished: true,
            failed: false,
        }
    }

    #[tokio::test]
    async fn finished_response_without_captured_bytes_is_unavailable() -> TestResult {
        let url = Url::parse("https://example.test/image.svg")?;
        let key = ("session".to_owned(), "request".to_owned());
        let mut response = completed_response(1, "request", &url, b"");
        response.body = ObservedBody::Streaming(Vec::new());
        response.stream_attempted = false;
        response.finished = false;
        let state = Mutex::new(ObservedResourceState {
            records: BTreeMap::from([(key, response)]),
            ..ObservedResourceState::default()
        });

        update_observed_completion(&state, "session", &json!({"requestId": "request"}), true).await;

        let state = state.lock().await;
        assert!(matches!(
            select_reusable_response(&state, "session", "frame", &url),
            super::super::identity::ResponseSelection::Unavailable
        ));
        Ok(())
    }

    #[tokio::test]
    async fn finished_streamed_response_is_reusable() -> TestResult {
        let url = Url::parse("https://example.test/image.svg")?;
        let key = ("session".to_owned(), "request".to_owned());
        let mut response = completed_response(1, "request", &url, b"");
        response.body = ObservedBody::Streaming(b"<svg/>".to_vec());
        response.stream_attempted = true;
        response.stream_pending = true;
        response.finished = false;
        let state = Mutex::new(ObservedResourceState {
            records: BTreeMap::from([(key, response)]),
            ..ObservedResourceState::default()
        });

        update_observed_completion(&state, "session", &json!({"requestId": "request"}), true).await;

        let state = state.lock().await;
        assert!(matches!(
            select_reusable_response(&state, "session", "frame", &url),
            super::super::identity::ResponseSelection::Ready(_)
        ));
        Ok(())
    }

    #[tokio::test]
    async fn failed_request_without_a_response_record_becomes_unavailable() -> TestResult {
        let url = Url::parse("https://example.test/image.svg")?;
        let key = ("session".to_owned(), "request".to_owned());
        let state = Mutex::new(ObservedResourceState {
            requests: BTreeMap::from([(key, streaming_request(url.clone()))]),
            ..ObservedResourceState::default()
        });

        update_observed_completion(&state, "session", &json!({"requestId": "request"}), false)
            .await;

        let state = state.lock().await;
        assert!(matches!(
            select_reusable_response(&state, "session", "frame", &url),
            super::super::identity::ResponseSelection::Unavailable
        ));
        Ok(())
    }

    #[tokio::test]
    async fn streamed_body_limit_is_applied_before_accumulation() -> TestResult {
        let key = ("session".to_owned(), "request".to_owned());
        let state = Mutex::new(ObservedResourceState {
            limits: ResourceObservationLimits {
                maximum_resource_bytes: 4,
                maximum_total_resource_bytes: 10,
            },
            requests: BTreeMap::from([(
                key.clone(),
                streaming_request(Url::parse("https://example.test/image.svg")?),
            )]),
            ..ObservedResourceState::default()
        });
        let encoded = base64::engine::general_purpose::STANDARD.encode(b"12345");

        append_observed_data(&state, &key, &encoded).await?;

        let state = state.lock().await;
        assert_eq!(state.body_bytes, 0);
        assert!(state.requests.get(&key).is_some_and(|request| {
            matches!(
                request.body,
                ObservedBody::TooLarge {
                    attempted: 5,
                    limit: 4
                }
            )
        }));
        Ok(())
    }

    #[tokio::test]
    async fn total_limit_accounts_for_concurrent_response_bodies() -> TestResult {
        let first = ("session".to_owned(), "first".to_owned());
        let second = ("session".to_owned(), "second".to_owned());
        let state = Mutex::new(ObservedResourceState {
            limits: ResourceObservationLimits {
                maximum_resource_bytes: 6,
                maximum_total_resource_bytes: 7,
            },
            requests: BTreeMap::from([
                (
                    first.clone(),
                    streaming_request(Url::parse("https://example.test/first.svg")?),
                ),
                (
                    second.clone(),
                    streaming_request(Url::parse("https://example.test/second.svg")?),
                ),
            ]),
            ..ObservedResourceState::default()
        });

        append_observed_bytes_at(&state, &first, b"1234".to_vec(), false).await?;
        append_observed_bytes_at(&state, &second, b"5678".to_vec(), false).await?;

        let state = state.lock().await;
        assert_eq!(state.body_bytes, 4);
        assert!(state.requests.get(&first).is_some_and(|request| {
            matches!(&request.body, ObservedBody::Streaming(body) if body == b"1234")
        }));
        assert!(state.requests.get(&second).is_some_and(|request| {
            matches!(
                request.body,
                ObservedBody::TooLarge {
                    attempted: 8,
                    limit: 7
                }
            )
        }));
        Ok(())
    }

    #[tokio::test]
    async fn simultaneous_appends_reserve_total_bytes_atomically() -> TestResult {
        let first = ("session".to_owned(), "first".to_owned());
        let second = ("session".to_owned(), "second".to_owned());
        let state = Arc::new(Mutex::new(ObservedResourceState {
            limits: ResourceObservationLimits {
                maximum_resource_bytes: 4,
                maximum_total_resource_bytes: 4,
            },
            requests: BTreeMap::from([
                (
                    first.clone(),
                    streaming_request(Url::parse("https://example.test/first.svg")?),
                ),
                (
                    second.clone(),
                    streaming_request(Url::parse("https://example.test/second.svg")?),
                ),
            ]),
            ..ObservedResourceState::default()
        }));
        let barrier = Arc::new(Barrier::new(3));
        let first_state = Arc::clone(&state);
        let first_barrier = Arc::clone(&barrier);
        let first_append = tokio::spawn(async move {
            first_barrier.wait().await;
            append_observed_bytes_at(&first_state, &first, b"1234".to_vec(), false).await
        });
        let second_state = Arc::clone(&state);
        let second_barrier = Arc::clone(&barrier);
        let second_append = tokio::spawn(async move {
            second_barrier.wait().await;
            append_observed_bytes_at(&second_state, &second, b"5678".to_vec(), false).await
        });

        barrier.wait().await;
        first_append.await??;
        second_append.await??;

        let state = state.lock().await;
        assert_eq!(state.body_bytes, 4);
        let retained = state
            .requests
            .values()
            .filter(
                |request| matches!(&request.body, ObservedBody::Streaming(body) if body.len() == 4),
            )
            .count();
        let rejected = state
            .requests
            .values()
            .filter(|request| {
                matches!(
                    request.body,
                    ObservedBody::TooLarge {
                        attempted: 8,
                        limit: 4
                    }
                )
            })
            .count();
        assert_eq!((retained, rejected), (1, 1));
        Ok(())
    }

    #[tokio::test]
    async fn configured_resource_limit_above_the_default_is_not_clamped() -> TestResult {
        const CONFIGURED_LIMIT: u64 = 65 * 1024 * 1024;
        const ATTEMPTED: u64 = 64 * 1024 * 1024 + 1;

        let key = ("session".to_owned(), "request".to_owned());
        let state = Mutex::new(ObservedResourceState {
            limits: ResourceObservationLimits {
                maximum_resource_bytes: CONFIGURED_LIMIT,
                maximum_total_resource_bytes: CONFIGURED_LIMIT,
            },
            requests: BTreeMap::from([(
                key.clone(),
                streaming_request(Url::parse("https://example.test/image.svg")?),
            )]),
            ..ObservedResourceState::default()
        });

        append_observed_chunk(&state, &key, None, ATTEMPTED, false).await?;

        let state = state.lock().await;
        assert!(
            state
                .requests
                .get(&key)
                .is_some_and(|request| { matches!(request.body, ObservedBody::Streaming(_)) })
        );
        Ok(())
    }
}
