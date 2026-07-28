use std::collections::{BTreeMap, BTreeSet, VecDeque};

use pageknot_browser::ResourceObservationLimits;
use pageknot_model::PageKnotError;
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use tokio::sync::Mutex;
use url::Url;

use super::storage::ObservedBody;

pub(super) type RequestKey = (String, String);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct RequestVariant {
    pub(super) method: String,
    pub(super) has_body: bool,
    pub(super) headers_digest: [u8; 32],
}

impl RequestVariant {
    pub(super) fn reusable(&self) -> bool {
        self.method.eq_ignore_ascii_case("GET") && !self.has_body
    }
}

#[derive(Clone, Debug)]
pub(super) struct ObservedResponse {
    pub(super) sequence: u64,
    pub(super) session_id: String,
    pub(super) request_id: String,
    pub(super) frame_id: Option<String>,
    pub(super) url: Url,
    pub(super) request_urls: Vec<Url>,
    pub(super) request_variant: Option<RequestVariant>,
    pub(super) status: u16,
    pub(super) media_type: Option<String>,
    pub(super) from_service_worker: bool,
    pub(super) body: ObservedBody,
    pub(super) stream_attempted: bool,
    pub(super) stream_pending: bool,
    pub(super) finished: bool,
    pub(super) failed: bool,
}

#[derive(Clone, Debug)]
pub(super) struct ObservedRequest {
    pub(super) sequence: u64,
    pub(super) frame_id: Option<String>,
    pub(super) urls: Vec<Url>,
    pub(super) variant: Option<RequestVariant>,
    pub(super) body: ObservedBody,
    pub(super) stream_attempted: bool,
    pub(super) stream_pending: bool,
    pub(super) network_observed: bool,
}

#[derive(Debug)]
pub(super) struct ObservedResourceState {
    pub(super) limits: ResourceObservationLimits,
    pub(super) sequence: u64,
    pub(super) body_bytes: u64,
    pub(super) requests: BTreeMap<RequestKey, ObservedRequest>,
    pub(super) request_order: VecDeque<(RequestKey, u64)>,
    pub(super) records: BTreeMap<RequestKey, ObservedResponse>,
    pub(super) order: VecDeque<(RequestKey, u64)>,
    pub(super) error: Option<PageKnotError>,
}

impl ObservedResourceState {
    pub(super) fn new(limits: ResourceObservationLimits) -> Self {
        Self {
            limits,
            sequence: 0,
            body_bytes: 0,
            requests: BTreeMap::new(),
            request_order: VecDeque::new(),
            records: BTreeMap::new(),
            order: VecDeque::new(),
            error: None,
        }
    }
}

impl Default for ObservedResourceState {
    fn default() -> Self {
        Self::new(ResourceObservationLimits::default())
    }
}

#[derive(Clone, Debug)]
pub(super) enum ResponseSelection {
    Missing,
    Pending,
    Ready(Box<ObservedResponse>),
    Ambiguous,
    Unavailable,
    TooLarge { attempted: u64, limit: u64 },
}

pub(super) fn select_reusable_response(
    state: &ObservedResourceState,
    session_id: &str,
    frame_id: &str,
    url: &Url,
) -> ResponseSelection {
    let requested = network_url_identity(url);
    let matching_requests = state
        .requests
        .iter()
        .filter(|((owner, _), request)| {
            owner == session_id
                && request
                    .frame_id
                    .as_deref()
                    .is_none_or(|owner| owner == frame_id)
                && request
                    .urls
                    .iter()
                    .any(|url| network_url_identity(url) == requested)
        })
        .map(|(_, request)| request)
        .collect::<Vec<_>>();
    let candidates = state
        .records
        .values()
        .filter(|response| {
            response.session_id == session_id
                && response
                    .frame_id
                    .as_deref()
                    .is_none_or(|owner| owner == frame_id)
                && (network_url_identity(&response.url) == requested
                    || response
                        .request_urls
                        .iter()
                        .any(|url| network_url_identity(url) == requested))
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        if let Some((attempted, limit)) = matching_requests.iter().find_map(|request| match request
            .body
        {
            ObservedBody::TooLarge { attempted, limit } => Some((attempted, limit)),
            _ => None,
        }) {
            return ResponseSelection::TooLarge { attempted, limit };
        }
        if matching_requests
            .iter()
            .any(|request| matches!(request.body, ObservedBody::Unavailable))
        {
            return ResponseSelection::Unavailable;
        }
        return if matching_requests.is_empty() {
            ResponseSelection::Missing
        } else {
            ResponseSelection::Pending
        };
    }
    if !matching_requests.is_empty()
        || candidates
            .iter()
            .any(|response| response.stream_pending || !response.finished)
    {
        return ResponseSelection::Pending;
    }
    let variants = candidates
        .iter()
        .filter_map(|response| response.request_variant.as_ref())
        .collect::<BTreeSet<_>>();
    if variants.len() != 1
        || candidates.iter().any(|response| {
            response
                .request_variant
                .as_ref()
                .is_none_or(|variant| !variant.reusable())
        })
    {
        return ResponseSelection::Ambiguous;
    }
    let ready = candidates
        .iter()
        .filter(|response| response.finished && !response.failed)
        .filter(|response| matches!(response.body, ObservedBody::Complete(_)))
        .copied()
        .collect::<Vec<_>>();
    let Some(latest) = ready.into_iter().max_by_key(|response| response.sequence) else {
        if let Some((attempted, limit)) =
            candidates.iter().find_map(|response| match response.body {
                ObservedBody::TooLarge { attempted, limit } => Some((attempted, limit)),
                _ => None,
            })
        {
            return ResponseSelection::TooLarge { attempted, limit };
        }
        return ResponseSelection::Unavailable;
    };
    if candidates.iter().any(|response| {
        response.finished && !response.failed && !responses_are_equivalent(response, latest)
    }) {
        return ResponseSelection::Ambiguous;
    }
    ResponseSelection::Ready(Box::new(latest.clone()))
}

pub(super) fn observed_response(parameters: &Value, session_id: &str) -> Option<ObservedResponse> {
    let request_id = parameters.get("requestId")?.as_str()?.to_owned();
    let response = parameters.get("response")?;
    let url = Url::parse(response.get("url")?.as_str()?).ok()?;
    let status = response
        .get("status")
        .and_then(Value::as_f64)
        .and_then(|status| {
            if (0.0..=f64::from(u16::MAX)).contains(&status) {
                Some(status as u16)
            } else {
                None
            }
        })
        .unwrap_or(200);
    let media_type = response
        .get("mimeType")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let from_service_worker = response
        .get("fromServiceWorker")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some(ObservedResponse {
        sequence: 0,
        session_id: session_id.to_owned(),
        request_id,
        frame_id: parameters
            .get("frameId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        url,
        request_urls: Vec::new(),
        request_variant: None,
        status,
        media_type,
        from_service_worker,
        body: ObservedBody::Streaming(Vec::new()),
        stream_attempted: false,
        stream_pending: false,
        finished: false,
        failed: false,
    })
}

pub(super) async fn record_response(
    state: &Mutex<ObservedResourceState>,
    key: RequestKey,
    mut response: ObservedResponse,
) {
    let mut state = state.lock().await;
    if let Some(request) = state.requests.remove(&key) {
        response.request_urls = request.urls;
        response.request_variant = request.variant;
        response.body = request.body;
        response.stream_attempted = request.stream_attempted;
        response.stream_pending = request.stream_pending;
    } else {
        response.request_urls = vec![response.url.clone()];
        response.body = ObservedBody::Unavailable;
        response.stream_attempted = false;
        response.stream_pending = false;
    }
    state.sequence = state.sequence.saturating_add(1);
    response.sequence = state.sequence;
    let sequence = response.sequence;
    if let Some(previous) = state.records.insert(key.clone(), response) {
        state.body_bytes = state.body_bytes.saturating_sub(previous.body.len());
    }
    state.order.push_back((key, sequence));
    prune_observed_responses(&mut state);
}

pub(super) async fn observe_request(
    state: &Mutex<ObservedResourceState>,
    session_id: &str,
    parameters: &Value,
) {
    let Some(request_id) = parameters.get("requestId").and_then(Value::as_str) else {
        return;
    };
    observe_request_id(state, session_id, request_id, parameters, true).await;
}

pub(super) async fn observe_request_id(
    state: &Mutex<ObservedResourceState>,
    session_id: &str,
    request_id: &str,
    parameters: &Value,
    network_observed: bool,
) {
    let Some(url) = parameters
        .pointer("/request/url")
        .and_then(Value::as_str)
        .and_then(|url| Url::parse(url).ok())
    else {
        return;
    };
    let variant = request_variant(parameters);
    let frame_id = parameters
        .get("frameId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let key = request_key(session_id, request_id);
    let mut state = state.lock().await;
    let existed = state.requests.contains_key(&key);
    if !existed {
        state.sequence = state.sequence.saturating_add(1);
        let sequence = state.sequence;
        state.requests.insert(
            key.clone(),
            ObservedRequest {
                sequence,
                frame_id: frame_id.clone(),
                urls: Vec::new(),
                variant: None,
                body: ObservedBody::Streaming(Vec::new()),
                stream_attempted: false,
                stream_pending: false,
                network_observed,
            },
        );
        state.request_order.push_back((key.clone(), sequence));
    }
    let reset = existed
        && network_observed
        && state
            .requests
            .get(&key)
            .is_some_and(|request| request.network_observed);
    if reset {
        let body_bytes = state
            .requests
            .get(&key)
            .map_or(0, |request| request.body.len());
        state.body_bytes = state.body_bytes.saturating_sub(body_bytes);
        if let Some(request) = state.requests.get_mut(&key) {
            request.body = ObservedBody::Streaming(Vec::new());
            request.stream_attempted = false;
            request.stream_pending = false;
        }
    }
    let Some(request) = state.requests.get_mut(&key) else {
        return;
    };
    request.network_observed |= network_observed;
    if request.frame_id.is_none() {
        request.frame_id = frame_id;
    }
    request.variant = variant;
    if request
        .urls
        .last()
        .is_none_or(|previous| network_url_identity(previous) != network_url_identity(&url))
    {
        request.urls.push(url);
    }
    prune_observed_requests(&mut state);
}

pub(super) async fn refresh_observed_request_identity(
    state: &Mutex<ObservedResourceState>,
    key: &RequestKey,
    parameters: &Value,
) {
    let variant = request_variant(parameters);
    let url = parameters
        .pointer("/request/url")
        .and_then(Value::as_str)
        .and_then(|value| Url::parse(value).ok());
    let frame_id = parameters
        .get("frameId")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let mut state = state.lock().await;
    if let Some(request) = state.requests.get_mut(key) {
        if variant.is_some() {
            request.variant = variant;
        }
        if request.frame_id.is_none() {
            request.frame_id = frame_id;
        }
        if let Some(url) = url
            && request
                .urls
                .last()
                .is_none_or(|previous| network_url_identity(previous) != network_url_identity(&url))
        {
            request.urls.push(url);
        }
        return;
    }
    if let Some(response) = state.records.get_mut(key) {
        if variant.is_some() {
            response.request_variant = variant;
        }
        if response.frame_id.is_none() {
            response.frame_id = frame_id;
        }
        if let Some(url) = url
            && response
                .request_urls
                .last()
                .is_none_or(|previous| network_url_identity(previous) != network_url_identity(&url))
        {
            response.request_urls.push(url);
        }
    }
}

pub(super) fn request_key(session_id: &str, request_id: &str) -> RequestKey {
    (session_id.to_owned(), request_id.to_owned())
}

fn request_variant(parameters: &Value) -> Option<RequestVariant> {
    let request = parameters.get("request")?;
    let method = request.get("method")?.as_str()?.to_owned();
    let has_body = request
        .get("hasPostData")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || request.get("postData").is_some();
    let mut headers = request
        .get("headers")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|headers| headers.iter())
        .filter_map(|(name, value)| {
            value
                .as_str()
                .map(|value| (name.to_ascii_lowercase(), value.to_owned()))
        })
        .collect::<Vec<_>>();
    headers.sort();
    let mut digest = Sha256::new();
    for (name, value) in headers {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(value.as_bytes());
        digest.update([0xff]);
    }
    Some(RequestVariant {
        method,
        has_body,
        headers_digest: digest.finalize().into(),
    })
}

fn responses_are_equivalent(left: &ObservedResponse, right: &ObservedResponse) -> bool {
    left.status == right.status
        && left.media_type == right.media_type
        && match (&left.body, &right.body) {
            (ObservedBody::Complete(left), ObservedBody::Complete(right)) => left == right,
            _ => false,
        }
}

fn network_url_identity(url: &Url) -> String {
    let mut identity = url.clone();
    identity.set_fragment(None);
    identity.to_string()
}

fn prune_observed_responses(state: &mut ObservedResourceState) {
    while state.records.len() > super::MAXIMUM_OBSERVED_RESPONSES {
        let Some((key, sequence)) = state.order.pop_front() else {
            return;
        };
        if state
            .records
            .get(&key)
            .is_some_and(|record| record.sequence == sequence)
            && let Some(record) = state.records.remove(&key)
        {
            state.body_bytes = state.body_bytes.saturating_sub(record.body.len());
        }
    }
}

fn prune_observed_requests(state: &mut ObservedResourceState) {
    prune_observed_requests_to(state, super::MAXIMUM_OBSERVED_RESPONSES);
}

fn prune_observed_requests_to(state: &mut ObservedResourceState, maximum: usize) {
    while state.requests.len() > maximum {
        let Some((key, sequence)) = state.request_order.pop_front() else {
            return;
        };
        if state
            .requests
            .get(&key)
            .is_some_and(|request| request.sequence == sequence)
            && let Some(request) = state.requests.remove(&key)
        {
            state.body_bytes = state.body_bytes.saturating_sub(request.body.len());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::VecDeque;
    use std::error::Error;

    use bytes::Bytes;
    use serde_json::json;

    use super::*;

    type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

    fn completed_response(
        sequence: u64,
        request_id: &str,
        url: &Url,
        headers_digest: [u8; 32],
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
                headers_digest,
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

    #[test]
    fn request_variant_distinguishes_headers_and_bodies() {
        let first = request_variant(&json!({
            "request": {
                "url": "https://example.test/image",
                "method": "GET",
                "headers": {
                    "Accept": "image/avif",
                    "Authorization": "Bearer first",
                    "Cookie": "variant=first"
                }
            }
        }));
        let second = request_variant(&json!({
            "request": {
                "url": "https://example.test/image",
                "method": "GET",
                "headers": {
                    "Accept": "image/png",
                    "Authorization": "Bearer second",
                    "Cookie": "variant=second"
                }
            }
        }));
        let post = request_variant(&json!({
            "request": {
                "url": "https://example.test/image",
                "method": "POST",
                "hasPostData": true,
                "headers": {}
            }
        }));

        assert_ne!(first, second);
        assert!(post.is_some_and(|variant| !variant.reusable()));
    }

    #[test]
    fn same_url_with_distinct_request_variants_is_ambiguous() -> TestResult {
        let url = Url::parse("https://example.test/image.svg")?;
        let state = ObservedResourceState {
            records: BTreeMap::from([
                (
                    request_key("session", "first"),
                    completed_response(1, "first", &url, [1; 32], b"<svg/>"),
                ),
                (
                    request_key("session", "second"),
                    completed_response(2, "second", &url, [2; 32], b"<svg/>"),
                ),
            ]),
            ..ObservedResourceState::default()
        };

        assert!(matches!(
            select_reusable_response(&state, "session", "frame", &url),
            ResponseSelection::Ambiguous
        ));
        Ok(())
    }

    #[test]
    fn equivalent_observations_for_one_variant_are_reusable() -> TestResult {
        let url = Url::parse("https://example.test/image.svg")?;
        let state = ObservedResourceState {
            records: BTreeMap::from([
                (
                    request_key("session", "first"),
                    completed_response(1, "first", &url, [1; 32], b"<svg/>"),
                ),
                (
                    request_key("session", "second"),
                    completed_response(2, "second", &url, [1; 32], b"<svg/>"),
                ),
            ]),
            ..ObservedResourceState::default()
        };

        assert!(matches!(
            select_reusable_response(&state, "session", "frame", &url),
            ResponseSelection::Ready(_)
        ));
        Ok(())
    }

    #[test]
    fn observed_response_starts_without_reusable_bytes() {
        let parameters = json!({
            "requestId": "1",
            "frameId": "frame",
            "response": {
                "url": "https://example.test/style.css",
                "status": 200,
                "mimeType": "text/css",
                "headers": {"Content-Encoding": "gzip"}
            }
        });

        assert!(
            observed_response(&parameters, "session").is_some_and(|response| {
                !response.stream_attempted && matches!(response.body, ObservedBody::Streaming(_))
            })
        );
    }

    #[test]
    fn rejected_oversized_request_is_terminal_without_a_response_record() -> TestResult {
        let url = Url::parse("https://example.test/image.svg")?;
        let state = ObservedResourceState {
            requests: BTreeMap::from([(
                request_key("session", "request"),
                ObservedRequest {
                    sequence: 1,
                    frame_id: Some("frame".to_owned()),
                    urls: vec![url.clone()],
                    variant: None,
                    body: ObservedBody::TooLarge {
                        attempted: 513,
                        limit: 512,
                    },
                    stream_attempted: true,
                    stream_pending: false,
                    network_observed: true,
                },
            )]),
            ..ObservedResourceState::default()
        };

        assert!(matches!(
            select_reusable_response(&state, "session", "frame", &url),
            ResponseSelection::TooLarge {
                attempted: 513,
                limit: 512
            }
        ));
        Ok(())
    }

    #[test]
    fn pending_resource_requests_are_pruned_by_observation_order() {
        let first = ("session".to_owned(), "z-request".to_owned());
        let second = ("session".to_owned(), "a-request".to_owned());
        let third = ("session".to_owned(), "m-request".to_owned());
        let request = |sequence| ObservedRequest {
            sequence,
            frame_id: None,
            urls: Vec::new(),
            variant: None,
            body: ObservedBody::Streaming(Vec::new()),
            stream_attempted: false,
            stream_pending: false,
            network_observed: true,
        };
        let mut state = ObservedResourceState {
            sequence: 3,
            requests: BTreeMap::from([
                (first.clone(), request(1)),
                (second.clone(), request(2)),
                (third.clone(), request(3)),
            ]),
            request_order: VecDeque::from([
                (first.clone(), 1),
                (second.clone(), 2),
                (third.clone(), 3),
            ]),
            ..ObservedResourceState::default()
        };

        prune_observed_requests_to(&mut state, 2);

        assert!(!state.requests.contains_key(&first));
        assert!(state.requests.contains_key(&second));
        assert!(state.requests.contains_key(&third));
    }
}
