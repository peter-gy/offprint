use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use pageknot_model::{ErrorStage, PageKnotError, Result};
use serde_json::{Value, json};
use tokio::sync::{Semaphore, broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{connect_async_with_config, tungstenite};
use url::Url;

use crate::cdp::{CdpCommand, CdpEventMessage};

// `Network.getResponseBody` base64 expands the 64 MiB resource contract.
const MAX_MESSAGE_BYTES: usize = 96 * 1024 * 1024;
const MAX_EVENT_BYTES: usize = 1024 * 1024;
const MAX_EVENT_QUEUE_BYTES: usize = 128 * 1024 * 1024;
const COMMAND_CAPACITY: usize = 256;
const EVENT_CAPACITY: usize = MAX_EVENT_QUEUE_BYTES / MAX_EVENT_BYTES;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
// Each guarded connection holds one browser socket and one upstream socket.
// Share this limit across every context owned by the browser process.
const MAXIMUM_PROXY_CONNECTIONS: usize = 32;

#[derive(Clone, Debug)]
pub struct CdpEvent {
    pub method: Arc<str>,
    pub params: Arc<Value>,
    pub session_id: Option<Arc<str>>,
}

impl CdpEvent {
    pub(crate) fn decode<E>(&self) -> Result<Option<E>>
    where
        E: CdpEventMessage,
    {
        if self.method.as_ref() != E::METHOD {
            return Ok(None);
        }
        serde_json::from_value((*self.params).clone())
            .map(Some)
            .map_err(|error| {
                cdp_error(
                    "pageknot.browser.cdp_shape",
                    format!("failed to decode CDP event `{}`: {error}", E::METHOD),
                )
                .with_detail("cdpMethod", E::METHOD)
            })
    }
}

#[derive(Clone, Debug)]
pub struct CdpClient {
    inner: Arc<ClientInner>,
}

#[derive(Debug)]
struct ClientInner {
    next_id: AtomicU64,
    owned_browser: AtomicBool,
    outbound: mpsc::Sender<Outbound>,
    cancellations: mpsc::UnboundedSender<u64>,
    events: EventBus,
    proxy_connections: Arc<Semaphore>,
    task: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone, Debug)]
struct EventBus {
    all: broadcast::Sender<CdpEvent>,
    navigation: broadcast::Sender<CdpEvent>,
    offline: broadcast::Sender<CdpEvent>,
    targets: broadcast::Sender<CdpEvent>,
}

impl EventBus {
    fn new(capacity: usize) -> Self {
        let (all, _) = broadcast::channel(capacity);
        let (navigation, _) = broadcast::channel(capacity);
        let (offline, _) = broadcast::channel(capacity);
        let (targets, _) = broadcast::channel(capacity);
        Self {
            all,
            navigation,
            offline,
            targets,
        }
    }

    fn publish(&self, event: CdpEvent) {
        if navigation_event(&event) {
            let _ignored = self.navigation.send(event.clone());
        }
        if offline_event(&event) {
            let _ignored = self.offline.send(event.clone());
        }
        if target_event(&event) {
            let _ignored = self.targets.send(event.clone());
        }
        let _ignored = self.all.send(event);
    }
}

impl Drop for ClientInner {
    fn drop(&mut self) {
        let task = match self.task.get_mut() {
            Ok(task) => task.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(task) = task {
            task.abort();
        }
    }
}

#[derive(Debug)]
enum Outbound {
    Command {
        id: u64,
        method: String,
        params: Value,
        session_id: Option<String>,
        response: oneshot::Sender<Result<Value>>,
    },
    CommandNoWait {
        payload: String,
        sent: oneshot::Sender<Result<()>>,
    },
    Close,
}

#[derive(Debug)]
struct PendingCommand {
    method: String,
    response: oneshot::Sender<Result<Value>>,
}

#[derive(Debug)]
struct CancelOnDrop {
    id: u64,
    sender: mpsc::UnboundedSender<u64>,
    armed: bool,
}

impl CancelOnDrop {
    fn new(id: u64, sender: mpsc::UnboundedSender<u64>) -> Self {
        Self {
            id,
            sender,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            let _ignored = self.sender.send(self.id);
        }
    }
}

impl CdpClient {
    pub async fn connect(endpoint: Url) -> Result<Self> {
        let mut config = WebSocketConfig::default();
        config.max_message_size = Some(MAX_MESSAGE_BYTES);
        config.max_frame_size = Some(MAX_MESSAGE_BYTES);
        let (socket, _) = timeout(
            CONNECT_TIMEOUT,
            connect_async_with_config(endpoint.as_str(), Some(config), false),
        )
        .await
        .map_err(|_| {
            cdp_error(
                "pageknot.browser.cdp_connect",
                "browser endpoint connection exceeded its deadline",
            )
            .retryable(true)
        })?
        .map_err(|error| {
            let reason = match &error {
                tungstenite::Error::Io(error) => {
                    format!("I/O error ({:?})", error.kind())
                }
                tungstenite::Error::Tls(_) => "TLS handshake failed".to_owned(),
                tungstenite::Error::Http(response) => {
                    format!("HTTP {}", response.status().as_u16())
                }
                tungstenite::Error::Url(_) => "endpoint URL was rejected".to_owned(),
                tungstenite::Error::Protocol(_) => {
                    "WebSocket handshake violated the protocol".to_owned()
                }
                _ => "WebSocket handshake failed".to_owned(),
            };
            cdp_error(
                "pageknot.browser.cdp_connect",
                format!("failed to connect to the browser endpoint: {reason}"),
            )
            .retryable(true)
        })?;

        let (outbound_tx, outbound_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (cancellation_tx, cancellation_rx) = mpsc::unbounded_channel();
        let events = EventBus::new(EVENT_CAPACITY);
        let actor_events = events.clone();
        let task = tokio::spawn(async move {
            run_transport(socket, outbound_rx, cancellation_rx, actor_events).await;
        });

        Ok(Self {
            inner: Arc::new(ClientInner {
                next_id: AtomicU64::new(1),
                owned_browser: AtomicBool::new(false),
                outbound: outbound_tx,
                cancellations: cancellation_tx,
                events,
                proxy_connections: Arc::new(Semaphore::new(MAXIMUM_PROXY_CONNECTIONS)),
                task: Mutex::new(Some(task)),
            }),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CdpEvent> {
        self.inner.events.all.subscribe()
    }

    pub(crate) fn subscribe_navigation(&self) -> broadcast::Receiver<CdpEvent> {
        self.inner.events.navigation.subscribe()
    }

    pub(crate) fn subscribe_offline(&self) -> broadcast::Receiver<CdpEvent> {
        self.inner.events.offline.subscribe()
    }

    pub(crate) fn subscribe_targets(&self) -> broadcast::Receiver<CdpEvent> {
        self.inner.events.targets.subscribe()
    }

    #[cfg(test)]
    pub(crate) fn event_receiver_count(&self) -> usize {
        self.inner.events.all.receiver_count()
            + self.inner.events.navigation.receiver_count()
            + self.inner.events.offline.receiver_count()
            + self.inner.events.targets.receiver_count()
    }

    pub(crate) fn mark_owned_browser(&self) {
        self.inner.owned_browser.store(true, Ordering::Release);
    }

    #[must_use]
    pub(crate) fn is_owned_browser(&self) -> bool {
        self.inner.owned_browser.load(Ordering::Acquire)
    }

    pub(crate) fn proxy_connection_budget(&self) -> Arc<Semaphore> {
        Arc::clone(&self.inner.proxy_connections)
    }

    pub async fn command(
        &self,
        method: impl Into<String>,
        params: Value,
        session_id: Option<&str>,
    ) -> Result<Value> {
        self.command_with_timeout(method, params, session_id, COMMAND_TIMEOUT)
            .await
    }

    pub(crate) async fn command_no_wait(
        &self,
        method: impl Into<String>,
        params: Value,
        session_id: Option<&str>,
    ) -> Result<()> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let method = method.into();
        let diagnostic_method = diagnostic_method(&method);
        let mut payload = json!({
            "id": id,
            "method": method,
            "params": params,
        });
        if let Some(session_id) = session_id {
            payload["sessionId"] = Value::String(session_id.to_owned());
        }
        let payload = serde_json::to_string(&payload).map_err(|error| {
            cdp_error(
                "pageknot.browser.cdp_encode",
                format!("failed to encode CDP command `{diagnostic_method}`: {error}"),
            )
            .with_detail("cdpMethod", diagnostic_method.clone())
            .with_detail("cdpRequestId", id)
        })?;
        let (sent_tx, sent_rx) = oneshot::channel();
        timeout(COMMAND_TIMEOUT, async {
            self.inner
                .outbound
                .send(Outbound::CommandNoWait {
                    payload,
                    sent: sent_tx,
                })
                .await
                .map_err(|_| command_context(cdp_closed(), id, &method))?;
            sent_rx
                .await
                .map_err(|_| command_context(cdp_closed(), id, &method))?
                .map_err(|error| command_context(error, id, &method))
        })
        .await
        .map_err(|_| {
            cdp_error(
                "pageknot.browser.cdp_timeout",
                format!("CDP command `{diagnostic_method}` could not be sent before its deadline"),
            )
            .retryable(true)
            .with_detail("cdpMethod", diagnostic_method)
            .with_detail("cdpRequestId", id)
        })?
    }

    pub(crate) async fn execute<C>(
        &self,
        params: C::Params,
        session_id: Option<&str>,
    ) -> Result<C::Response>
    where
        C: CdpCommand,
    {
        self.execute_with_timeout::<C>(params, session_id, COMMAND_TIMEOUT)
            .await
    }

    pub(crate) async fn execute_with_timeout<C>(
        &self,
        params: C::Params,
        session_id: Option<&str>,
        deadline: Duration,
    ) -> Result<C::Response>
    where
        C: CdpCommand,
    {
        let params = serde_json::to_value(params).map_err(|error| {
            cdp_error(
                "pageknot.browser.cdp_encode",
                format!("failed to encode CDP command `{}`: {error}", C::METHOD),
            )
            .with_detail("cdpMethod", C::METHOD)
        })?;
        let response = self
            .command_with_timeout(C::METHOD, params, session_id, deadline)
            .await?;
        serde_json::from_value(response).map_err(|error| {
            cdp_error(
                "pageknot.browser.cdp_shape",
                format!("failed to decode CDP response `{}`: {error}", C::METHOD),
            )
            .with_detail("cdpMethod", C::METHOD)
        })
    }

    pub async fn command_with_timeout(
        &self,
        method: impl Into<String>,
        params: Value,
        session_id: Option<&str>,
        deadline: Duration,
    ) -> Result<Value> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let method = method.into();
        let (response_tx, response_rx) = oneshot::channel();
        let diagnostic_method = diagnostic_method(&method);
        timeout(deadline, async {
            self.inner
                .outbound
                .send(Outbound::Command {
                    id,
                    method: method.clone(),
                    params,
                    session_id: session_id.map(str::to_owned),
                    response: response_tx,
                })
                .await
                .map_err(|_| command_context(cdp_closed(), id, &method))?;
            let mut cancellation = CancelOnDrop::new(id, self.inner.cancellations.clone());
            let response = response_rx
                .await
                .map_err(|_| command_context(cdp_closed(), id, &method))?;
            cancellation.disarm();
            response.map_err(|error| command_context(error, id, &method))
        })
        .await
        .map_err(|_| {
            cdp_error(
                "pageknot.browser.cdp_timeout",
                format!("CDP command `{diagnostic_method}` exceeded its deadline"),
            )
            .retryable(true)
            .with_detail("cdpMethod", diagnostic_method)
            .with_detail("cdpRequestId", id)
        })?
    }

    pub async fn close(&self) -> Result<()> {
        self.close_with_timeout(CLOSE_TIMEOUT).await
    }

    async fn close_with_timeout(&self, deadline: Duration) -> Result<()> {
        let mut send_timed_out = false;
        let send_result = match timeout(deadline, self.inner.outbound.send(Outbound::Close)).await {
            Ok(result) => result.map_err(|_| cdp_closed()),
            Err(_) => {
                send_timed_out = true;
                Err(cdp_error(
                    "pageknot.browser.cdp_close_timeout",
                    "CDP transport shutdown exceeded its deadline",
                )
                .retryable(true))
            }
        };

        let task = match self.inner.task.lock() {
            Ok(mut task) => task.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        let task_result = if let Some(mut task) = task {
            if send_timed_out {
                task.abort();
                let _ignored = task.await;
                Ok(())
            } else {
                match timeout(deadline, &mut task).await {
                    Ok(result) => result.map_err(|error| {
                        cdp_error(
                            "pageknot.browser.cdp_task",
                            format!("CDP transport task failed: {error}"),
                        )
                    }),
                    Err(_) => {
                        task.abort();
                        let _ignored = task.await;
                        Err(cdp_error(
                            "pageknot.browser.cdp_close_timeout",
                            "CDP transport task did not stop",
                        )
                        .retryable(true))
                    }
                }
            }
        } else {
            Ok(())
        };
        send_result?;
        task_result
    }
}

async fn run_transport<S>(
    socket: tokio_tungstenite::WebSocketStream<S>,
    mut outbound: mpsc::Receiver<Outbound>,
    mut cancellations: mpsc::UnboundedReceiver<u64>,
    events: EventBus,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut writer, mut reader) = socket.split();
    let mut pending = BTreeMap::<u64, PendingCommand>::new();
    let terminal_error = loop {
        tokio::select! {
            Some(id) = cancellations.recv() => {
                pending.remove(&id);
            }
            outbound_message = outbound.recv() => {
                let Some(outbound_message) = outbound_message else {
                    let _ignored = writer.close().await;
                    break cdp_closed();
                };
                match outbound_message {
                    Outbound::Command {
                        id,
                        method,
                        params,
                        session_id,
                        response,
                    } => {
                        if response.is_closed() {
                            continue;
                        }
                        let mut payload = json!({
                            "id": id,
                            "method": method,
                            "params": params,
                        });
                        if let Some(session_id) = session_id {
                            payload["sessionId"] = Value::String(session_id);
                        }
                        let serialized = match serde_json::to_string(&payload) {
                            Ok(serialized) => serialized,
                            Err(error) => {
                                let error = command_context(
                                    cdp_error(
                                        "pageknot.browser.cdp_encode",
                                        format!("failed to encode a CDP command: {error}"),
                                    ),
                                    id,
                                    &method,
                                );
                                let _ignored = response.send(Err(error));
                                continue;
                            }
                        };
                        pending.insert(id, PendingCommand { method, response });
                        if let Err(error) = writer.send(Message::Text(serialized.into())).await {
                            break websocket_error(error);
                        }
                    }
                    Outbound::CommandNoWait { payload, sent } => {
                        if sent.is_closed() {
                            continue;
                        }
                        if let Err(error) = writer.send(Message::Text(payload.into())).await {
                            let error = websocket_error(error);
                            let _ignored = sent.send(Err(error.clone()));
                            break error;
                        }
                        let _ignored = sent.send(Ok(()));
                    }
                    Outbound::Close => {
                        let _ignored = writer.close().await;
                        break cdp_closed();
                    }
                }
            }
            inbound = reader.next() => {
                match inbound {
                    Some(Ok(message)) => {
                        if let Some(error) = route_message(message, &mut pending, &events) {
                            break error;
                        }
                    }
                    Some(Err(error)) => break websocket_error(error),
                    None => break cdp_closed(),
                }
            }
        }
    };

    for (id, command) in pending {
        let error = command_context(terminal_error.clone(), id, &command.method);
        let _ignored = command.response.send(Err(error));
    }
}

fn route_message(
    message: Message,
    pending: &mut BTreeMap<u64, PendingCommand>,
    events: &EventBus,
) -> Option<PageKnotError> {
    let text = match message {
        Message::Text(text) => text,
        Message::Binary(bytes) => match String::from_utf8(bytes.to_vec()) {
            Ok(text) => text.into(),
            Err(error) => {
                return Some(cdp_error(
                    "pageknot.browser.cdp_decode",
                    format!("browser sent non-UTF-8 CDP data: {error}"),
                ));
            }
        },
        Message::Close(_) => return Some(cdp_closed()),
        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => return None,
    };
    let message: Value = match serde_json::from_str(&text) {
        Ok(message) => message,
        Err(error) => {
            return Some(cdp_error(
                "pageknot.browser.cdp_decode",
                format!("browser sent malformed CDP JSON: {error}"),
            ));
        }
    };
    if let Some(id) = message.get("id").and_then(Value::as_u64) {
        if let Some(command) = pending.remove(&id) {
            let result = if let Some(error) = message.get("error") {
                Err(remote_error(error, id, &command.method))
            } else {
                Ok(message.get("result").cloned().unwrap_or(Value::Null))
            };
            let _ignored = command.response.send(result);
        }
        return None;
    }
    if let Some(method) = message.get("method").and_then(Value::as_str) {
        if text.len() > MAX_EVENT_BYTES {
            if oversized_inline_resource_event(method, &message) {
                return None;
            }
            return Some(
                cdp_error(
                    "pageknot.browser.cdp_event_limit",
                    "browser event exceeds the CDP event byte limit",
                )
                .with_detail("attempted", text.len())
                .with_detail("cdpEventMethod", diagnostic_method(method))
                .with_detail("limit", MAX_EVENT_BYTES),
            );
        }
        let event = CdpEvent {
            method: Arc::from(method),
            params: Arc::new(message.get("params").cloned().unwrap_or(Value::Null)),
            session_id: message
                .get("sessionId")
                .and_then(Value::as_str)
                .map(Arc::from),
        };
        events.publish(event);
    }
    None
}

fn navigation_event(event: &CdpEvent) -> bool {
    match event.method.as_ref() {
        "Page.domContentEventFired" | "Page.loadEventFired" | "Inspector.targetCrashed" => true,
        "Network.requestWillBeSent" | "Network.loadingFailed" => {
            event.params.get("type").and_then(Value::as_str) == Some("Document")
        }
        _ => false,
    }
}

fn offline_event(event: &CdpEvent) -> bool {
    match event.method.as_ref() {
        "Network.requestWillBeSent" => event
            .params
            .pointer("/request/url")
            .and_then(Value::as_str)
            .and_then(|url| Url::parse(url).ok())
            .is_some_and(|url| matches!(url.scheme(), "http" | "https" | "ws" | "wss")),
        "Runtime.exceptionThrown" | "Log.entryAdded" => true,
        _ => false,
    }
}

fn target_event(event: &CdpEvent) -> bool {
    matches!(
        event.method.as_ref(),
        "Target.attachedToTarget" | "Target.detachedFromTarget"
    )
}

fn oversized_inline_resource_event(method: &str, message: &Value) -> bool {
    let url = match method {
        "Network.requestWillBeSent" => message.pointer("/params/request/url"),
        "Network.responseReceived" => message.pointer("/params/response/url"),
        _ => None,
    };
    url.and_then(Value::as_str)
        .is_some_and(|url| url.starts_with("data:"))
}

fn remote_error(error: &Value, request_id: u64, method: &str) -> PageKnotError {
    let mut result = command_context(
        cdp_error(
            "pageknot.browser.cdp_command",
            "browser rejected the CDP command",
        ),
        request_id,
        method,
    );
    if let Some(code) = error.get("code").and_then(Value::as_i64) {
        result = result.with_detail("cdpCode", code);
    }
    if error.get("message").is_some() {
        result = result.with_detail("cdpMessageRedacted", true);
    }
    if error.get("data").is_some() {
        result = result.with_detail("cdpDataRedacted", true);
    }
    result
}

fn command_context(error: PageKnotError, request_id: u64, method: &str) -> PageKnotError {
    error
        .with_detail("cdpMethod", diagnostic_method(method))
        .with_detail("cdpRequestId", request_id)
}

fn diagnostic_method(method: &str) -> String {
    if !method.is_empty()
        && method.len() <= 128
        && method
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_'))
    {
        method.to_owned()
    } else {
        "<redacted>".to_owned()
    }
}

fn websocket_error(error: tungstenite::Error) -> PageKnotError {
    cdp_error(
        "pageknot.browser.cdp_transport",
        format!("CDP WebSocket transport failed: {error}"),
    )
    .retryable(true)
}

fn cdp_closed() -> PageKnotError {
    cdp_error(
        "pageknot.browser.cdp_closed",
        "the CDP connection is closed",
    )
    .retryable(true)
}

fn cdp_error(code: &'static str, message: impl Into<String>) -> PageKnotError {
    PageKnotError::new(code, ErrorStage::Browser, message)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::error::Error;
    use std::future::pending;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use pageknot_model::PageKnotError;
    use serde_json::{Value, json};
    use tokio::sync::{Semaphore, mpsc, oneshot};
    use tokio::task::JoinHandle;
    use tokio_tungstenite::tungstenite::Message;

    use super::{CdpClient, ClientInner, EventBus, Outbound, PendingCommand, route_message};

    type TestResult<T = ()> = std::result::Result<T, Box<dyn Error>>;

    #[test]
    fn response_routes_to_the_matching_command() {
        let (sender, receiver) = oneshot::channel();
        let mut pending = BTreeMap::from([(
            7,
            PendingCommand {
                method: "Runtime.evaluate".to_owned(),
                response: sender,
            },
        )]);
        let events = EventBus::new(1);

        let terminal = route_message(
            Message::Text(r#"{"id":7,"result":{"value":42}}"#.into()),
            &mut pending,
            &events,
        );
        let response = receiver.blocking_recv();

        assert!(terminal.is_none());
        assert_eq!(
            response.ok().and_then(Result::ok),
            Some(json!({"value": 42}))
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn remote_error_messages_cannot_publish_urls_or_header_secrets() -> TestResult {
        let error = route_remote_error(
            41,
            "Page.navigate",
            json!({
                "code": -32_000,
                "message": concat!(
                    "navigation rejected https://user:userinfo-secret@example.test/path",
                    "?X-Amz-Signature=signed-secret ",
                    "Authorization: Bearer authorization-secret ",
                    "Cookie: session=cookie-secret"
                )
            }),
        )?;
        let encoded = serde_json::to_string(&error)?;

        for secret in [
            "userinfo-secret",
            "signed-secret",
            "authorization-secret",
            "cookie-secret",
        ] {
            assert!(!encoded.contains(secret));
        }
        assert_eq!(error.message, "browser rejected the CDP command");
        assert_eq!(error.details.get("cdpCode"), Some(&json!(-32_000)));
        assert_eq!(
            error.details.get("cdpMethod"),
            Some(&json!("Page.navigate"))
        );
        assert_eq!(error.details.get("cdpRequestId"), Some(&json!(41)));
        assert_eq!(error.details.get("cdpMessageRedacted"), Some(&json!(true)));
        Ok(())
    }

    #[test]
    fn remote_error_data_is_suppressed_while_correlation_is_retained() -> TestResult {
        let error = route_remote_error(
            73,
            "Network.setCookies",
            json!({
                "code": -32_001,
                "message": "invalid cookie input",
                "data": {
                    "Authorization": "Bearer data-authorization-secret",
                    "Cookie": "session=data-cookie-secret",
                    "nested": [
                        {
                            "url": concat!(
                                "https://data-user:data-userinfo-secret@example.test/",
                                "?token=data-token-secret"
                            )
                        }
                    ]
                }
            }),
        )?;
        let encoded = serde_json::to_string(&error)?;

        for secret in [
            "data-authorization-secret",
            "data-cookie-secret",
            "data-userinfo-secret",
            "data-token-secret",
        ] {
            assert!(!encoded.contains(secret));
        }
        assert_eq!(
            error.details.get("cdpMethod"),
            Some(&json!("Network.setCookies"))
        );
        assert_eq!(error.details.get("cdpRequestId"), Some(&json!(73)));
        assert_eq!(error.details.get("cdpDataRedacted"), Some(&json!(true)));
        assert!(!error.details.contains_key("cdpData"));
        Ok(())
    }

    #[test]
    fn events_keep_the_flat_session_identifier() {
        let mut pending = BTreeMap::new();
        let events = EventBus::new(1);
        let mut receiver = events.all.subscribe();

        let terminal = route_message(
            Message::Text(
                r#"{"method":"Page.loadEventFired","params":{"timestamp":1},"sessionId":"s1"}"#
                    .into(),
            ),
            &mut pending,
            &events,
        );
        let event = receiver.try_recv().ok();

        assert!(terminal.is_none());
        assert_eq!(
            event.as_ref().map(|value| value.method.as_ref()),
            Some("Page.loadEventFired")
        );
        assert_eq!(
            event.and_then(|value| value.session_id).as_deref(),
            Some("s1")
        );
    }

    #[test]
    fn navigation_stream_ignores_subresource_event_bursts() -> TestResult {
        let mut pending = BTreeMap::new();
        let events = EventBus::new(1);
        let mut navigation = events.navigation.subscribe();
        for index in 0..512 {
            let message = serde_json::to_string(&json!({
                "method": "Network.requestWillBeSent",
                "params": {
                    "requestId": format!("image-{index}"),
                    "type": "Image",
                    "request": {"url": format!("data:image/svg+xml,{index}")},
                },
                "sessionId": "s1",
            }))?;
            assert!(route_message(Message::Text(message.into()), &mut pending, &events).is_none());
        }
        let terminal = route_message(
            Message::Text(
                r#"{"method":"Page.loadEventFired","params":{"timestamp":1},"sessionId":"s1"}"#
                    .into(),
            ),
            &mut pending,
            &events,
        );
        let event = navigation.try_recv()?;

        assert!(terminal.is_none());
        assert_eq!(event.method.as_ref(), "Page.loadEventFired");
        Ok(())
    }

    #[test]
    fn offline_stream_ignores_inline_resource_event_bursts() -> TestResult {
        let mut pending = BTreeMap::new();
        let events = EventBus::new(1);
        let mut offline = events.offline.subscribe();
        for index in 0..512 {
            let message = serde_json::to_string(&json!({
                "method": "Network.requestWillBeSent",
                "params": {
                    "requestId": format!("image-{index}"),
                    "type": "Image",
                    "request": {"url": format!("data:image/svg+xml,{index}")},
                },
                "sessionId": "s1",
            }))?;
            assert!(route_message(Message::Text(message.into()), &mut pending, &events).is_none());
        }
        let terminal = route_message(
            Message::Text(
                r#"{"method":"Network.requestWillBeSent","params":{"request":{"url":"https://example.com/pixel"}},"sessionId":"s1"}"#
                    .into(),
            ),
            &mut pending,
            &events,
        );
        let event = offline.try_recv()?;

        assert!(terminal.is_none());
        assert_eq!(
            event.params.pointer("/request/url").and_then(Value::as_str),
            Some("https://example.com/pixel")
        );
        Ok(())
    }

    #[test]
    fn event_clones_share_the_parsed_parameter_storage() {
        let mut pending = BTreeMap::new();
        let events = EventBus::new(1);
        let mut receiver = events.all.subscribe();
        let terminal = route_message(
            Message::Text(r#"{"method":"Runtime.consoleAPICalled","params":{"value":42}}"#.into()),
            &mut pending,
            &events,
        );
        let event = receiver.try_recv().ok();
        let cloned = event.clone();

        assert!(terminal.is_none());
        assert!(
            event
                .as_ref()
                .zip(cloned.as_ref())
                .is_some_and(|(event, cloned)| Arc::ptr_eq(&event.params, &cloned.params))
        );
    }

    #[test]
    fn oversized_events_close_the_transport_before_broadcast() {
        let mut pending = BTreeMap::new();
        let events = EventBus::new(1);
        let mut receiver = events.all.subscribe();
        let payload = "x".repeat(super::MAX_EVENT_BYTES);
        let terminal = route_message(
            Message::Text(
                format!(
                    r#"{{"method":"Runtime.consoleAPICalled","params":{{"value":"{payload}"}}}}"#
                )
                .into(),
            ),
            &mut pending,
            &events,
        );

        assert_eq!(
            terminal.as_ref().map(|error| error.code.as_str()),
            Some("pageknot.browser.cdp_event_limit")
        );
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn oversized_inline_resource_events_do_not_close_the_transport() -> TestResult {
        let mut pending = BTreeMap::new();
        let events = EventBus::new(1);
        let mut receiver = events.all.subscribe();
        let payload = "x".repeat(super::MAX_EVENT_BYTES);
        let message = serde_json::to_string(&json!({
            "method": "Network.requestWillBeSent",
            "params": {
                "requestId": "inline",
                "type": "Image",
                "request": {
                    "url": format!("data:image/svg+xml,{payload}"),
                },
            },
        }))?;
        let terminal = route_message(Message::Text(message.into()), &mut pending, &events);

        assert!(terminal.is_none());
        assert!(receiver.try_recv().is_err());
        Ok(())
    }

    #[tokio::test]
    async fn command_deadline_includes_outbound_queue_wait() -> TestResult {
        let (outbound, mut outbound_receiver) = mpsc::channel(1);
        assert!(outbound.try_send(Outbound::Close).is_ok());
        let (client, _cancellations) = test_client(outbound, None);

        let result = tokio::time::timeout(
            Duration::from_secs(1),
            client.command_with_timeout(
                "Runtime.evaluate",
                json!({}),
                None,
                Duration::from_millis(20),
            ),
        )
        .await?;

        assert_eq!(
            result.as_ref().err().map(|error| error.code.as_str()),
            Some("pageknot.browser.cdp_timeout")
        );
        assert!(matches!(outbound_receiver.try_recv(), Ok(Outbound::Close)));
        Ok(())
    }

    #[tokio::test]
    async fn close_send_timeout_aborts_and_awaits_the_transport_task() {
        let (task, task_dropped) = spawn_pending_task().await;
        let (outbound, _outbound_receiver) = mpsc::channel(1);
        assert!(outbound.try_send(Outbound::Close).is_ok());
        let (client, _cancellations) = test_client(outbound, Some(task));

        let result = client.close_with_timeout(Duration::from_millis(20)).await;

        assert_eq!(
            result.as_ref().err().map(|error| error.code.as_str()),
            Some("pageknot.browser.cdp_close_timeout")
        );
        assert!(task_dropped.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn close_aborts_and_awaits_a_task_that_does_not_finish_shutdown() {
        let (task, task_dropped) = spawn_pending_task().await;
        let (outbound, _outbound_receiver) = mpsc::channel(1);
        let (client, _cancellations) = test_client(outbound, Some(task));

        let result = client.close_with_timeout(Duration::from_millis(20)).await;

        assert_eq!(
            result.as_ref().err().map(|error| error.code.as_str()),
            Some("pageknot.browser.cdp_close_timeout")
        );
        assert!(task_dropped.load(Ordering::Acquire));
    }

    fn route_remote_error(id: u64, method: &str, remote_error: Value) -> TestResult<PageKnotError> {
        let (sender, receiver) = oneshot::channel();
        let mut pending = BTreeMap::from([(
            id,
            PendingCommand {
                method: method.to_owned(),
                response: sender,
            },
        )]);
        let events = EventBus::new(1);
        let message = serde_json::to_string(&json!({
            "id": id,
            "error": remote_error,
        }))?;
        let terminal = route_message(Message::Text(message.into()), &mut pending, &events);
        if let Some(error) = terminal {
            return Err(std::io::Error::other(format!(
                "remote error closed the transport: {error}"
            ))
            .into());
        }
        match receiver.blocking_recv()? {
            Err(error) => Ok(error),
            Ok(_) => {
                Err(std::io::Error::other("remote error produced a successful response").into())
            }
        }
    }

    fn test_client(
        outbound: mpsc::Sender<Outbound>,
        task: Option<JoinHandle<()>>,
    ) -> (CdpClient, mpsc::UnboundedReceiver<u64>) {
        let (cancellations, cancellation_receiver) = mpsc::unbounded_channel();
        let events = EventBus::new(1);
        let client = CdpClient {
            inner: Arc::new(ClientInner {
                next_id: AtomicU64::new(1),
                owned_browser: AtomicBool::new(false),
                outbound,
                cancellations,
                events,
                proxy_connections: Arc::new(Semaphore::new(1)),
                task: Mutex::new(task),
            }),
        };
        (client, cancellation_receiver)
    }

    async fn spawn_pending_task() -> (JoinHandle<()>, Arc<AtomicBool>) {
        let task_dropped = Arc::new(AtomicBool::new(false));
        let task_drop_signal = Arc::clone(&task_dropped);
        let (task_started, task_started_receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            let _drop_signal = DropSignal(task_drop_signal);
            let _ignored = task_started.send(());
            pending::<()>().await;
        });
        assert!(task_started_receiver.await.is_ok());
        (task, task_dropped)
    }

    struct DropSignal(Arc<AtomicBool>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
}
