use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{Sink, SinkExt, Stream, StreamExt};
use offprint_model::{ErrorStage, OffprintError, Result};
use serde::Deserialize;
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

#[derive(Clone, Copy, Debug)]
struct WireDeadlines {
    send: Duration,
    close: Duration,
}

const WIRE_DEADLINES: WireDeadlines = WireDeadlines {
    send: COMMAND_TIMEOUT,
    close: CLOSE_TIMEOUT,
};

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
                    "offprint.browser.cdp_shape",
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
    task: Mutex<Option<JoinHandle<Result<()>>>>,
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
                "offprint.browser.cdp_connect",
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
                "offprint.browser.cdp_connect",
                format!("failed to connect to the browser endpoint: {reason}"),
            )
            .retryable(true)
        })?;

        let (outbound_tx, outbound_rx) = mpsc::channel(COMMAND_CAPACITY);
        let (cancellation_tx, cancellation_rx) = mpsc::unbounded_channel();
        let events = EventBus::new(EVENT_CAPACITY);
        let actor_events = events.clone();
        let task = tokio::spawn(run_transport(
            socket,
            outbound_rx,
            cancellation_rx,
            actor_events,
            WIRE_DEADLINES,
        ));

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
                "offprint.browser.cdp_encode",
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
                "offprint.browser.cdp_timeout",
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
                "offprint.browser.cdp_encode",
                format!("failed to encode CDP command `{}`: {error}", C::METHOD),
            )
            .with_detail("cdpMethod", C::METHOD)
        })?;
        let response = self
            .command_with_timeout(C::METHOD, params, session_id, deadline)
            .await?;
        serde_json::from_value(response).map_err(|error| {
            cdp_error(
                "offprint.browser.cdp_shape",
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
                "offprint.browser.cdp_timeout",
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
                    "offprint.browser.cdp_close_timeout",
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
                    Ok(result) => result.map_err(cdp_task_error)?,
                    Err(_) => {
                        task.abort();
                        let _ignored = task.await;
                        Err(cdp_error(
                            "offprint.browser.cdp_close_timeout",
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

async fn run_transport<W>(
    socket: W,
    mut outbound: mpsc::Receiver<Outbound>,
    mut cancellations: mpsc::UnboundedReceiver<u64>,
    events: EventBus,
    deadlines: WireDeadlines,
) -> Result<()>
where
    W: Sink<Message, Error = tungstenite::Error>
        + Stream<Item = std::result::Result<Message, tungstenite::Error>>
        + Unpin,
{
    let (mut writer, mut reader) = socket.split();
    let mut pending = BTreeMap::<u64, PendingCommand>::new();
    let outcome = loop {
        tokio::select! {
            Some(id) = cancellations.recv() => {
                pending.remove(&id);
            }
            outbound_message = outbound.recv() => {
                let Some(outbound_message) = outbound_message else {
                    break close_wire(&mut writer, deadlines.close).await;
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
                                        "offprint.browser.cdp_encode",
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
                        if let Err(error) =
                            send_wire(&mut writer, Message::Text(serialized.into()), deadlines.send)
                                .await
                        {
                            break Err(error);
                        }
                    }
                    Outbound::CommandNoWait { payload, sent } => {
                        if sent.is_closed() {
                            continue;
                        }
                        if let Err(error) =
                            send_wire(&mut writer, Message::Text(payload.into()), deadlines.send)
                                .await
                        {
                            let _ignored = sent.send(Err(error.clone()));
                            break Err(error);
                        }
                        let _ignored = sent.send(Ok(()));
                    }
                    Outbound::Close => {
                        break close_wire(&mut writer, deadlines.close).await;
                    }
                }
            }
            inbound = reader.next() => {
                match inbound {
                    Some(Ok(message)) => {
                        if let Some(error) = route_message(message, &mut pending, &events) {
                            break Err(error);
                        }
                    }
                    Some(Err(error)) => break Err(websocket_error(error)),
                    None => break Err(cdp_closed()),
                }
            }
        }
    };

    let terminal_error = outcome.as_ref().err().cloned().unwrap_or_else(cdp_closed);
    for (id, command) in pending {
        let error = command_context(terminal_error.clone(), id, &command.method);
        let _ignored = command.response.send(Err(error));
    }
    outcome
}

async fn send_wire<W>(writer: &mut W, message: Message, deadline: Duration) -> Result<()>
where
    W: Sink<Message, Error = tungstenite::Error> + Unpin,
{
    timeout(deadline, writer.send(message))
        .await
        .map_err(|_| {
            cdp_error(
                "offprint.browser.cdp_transport",
                "CDP WebSocket send exceeded its deadline",
            )
            .retryable(true)
        })?
        .map_err(websocket_error)
}

async fn close_wire<W>(writer: &mut W, deadline: Duration) -> Result<()>
where
    W: Sink<Message, Error = tungstenite::Error> + Unpin,
{
    timeout(deadline, writer.close())
        .await
        .map_err(|_| {
            cdp_error(
                "offprint.browser.cdp_close_timeout",
                "CDP WebSocket close exceeded its deadline",
            )
            .retryable(true)
        })?
        .map_err(websocket_error)
}

fn route_message(
    message: Message,
    pending: &mut BTreeMap<u64, PendingCommand>,
    events: &EventBus,
) -> Option<OffprintError> {
    let text = match message {
        Message::Text(text) => text,
        Message::Binary(bytes) => match String::from_utf8(bytes.to_vec()) {
            Ok(text) => text.into(),
            Err(error) => {
                return Some(cdp_error(
                    "offprint.browser.cdp_decode",
                    format!("browser sent non-UTF-8 CDP data: {error}"),
                ));
            }
        },
        Message::Close(_) => return Some(cdp_closed()),
        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => return None,
    };
    if text.len() > MAX_EVENT_BYTES {
        return route_oversized_message(&text, pending, events);
    }
    route_materialized_message(&text, pending, events)
}

fn route_oversized_message(
    text: &str,
    pending: &mut BTreeMap<u64, PendingCommand>,
    events: &EventBus,
) -> Option<OffprintError> {
    let envelope: WireEnvelope<'_> = match serde_json::from_str(text) {
        Ok(envelope) => envelope,
        Err(error) => return Some(cdp_decode_error(error)),
    };
    if envelope.id.is_some() {
        return route_materialized_message(text, pending, events);
    }
    let method = envelope.method.as_deref()?;
    if oversized_inline_resource_event(method, text) {
        return None;
    }
    Some(
        cdp_error(
            "offprint.browser.cdp_event_limit",
            "browser event exceeds the CDP event byte limit",
        )
        .with_detail("attempted", text.len())
        .with_detail("cdpEventMethod", diagnostic_method(method))
        .with_detail("limit", MAX_EVENT_BYTES),
    )
}

fn route_materialized_message(
    text: &str,
    pending: &mut BTreeMap<u64, PendingCommand>,
    events: &EventBus,
) -> Option<OffprintError> {
    let message: Value = match serde_json::from_str(text) {
        Ok(message) => message,
        Err(error) => return Some(cdp_decode_error(error)),
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

#[derive(Debug, Deserialize)]
struct WireEnvelope<'a> {
    id: Option<u64>,
    #[serde(borrow)]
    method: Option<Cow<'a, str>>,
}

#[derive(Debug, Default, Deserialize)]
struct InlineResourceEnvelope<'a> {
    #[serde(borrow, default)]
    params: InlineResourceParams<'a>,
}

#[derive(Debug, Default, Deserialize)]
struct InlineResourceParams<'a> {
    #[serde(borrow, default)]
    request: InlineResourceUrl<'a>,
    #[serde(borrow, default)]
    response: InlineResourceUrl<'a>,
}

#[derive(Debug, Default, Deserialize)]
struct InlineResourceUrl<'a> {
    #[serde(borrow, default)]
    url: Option<Cow<'a, str>>,
}

fn oversized_inline_resource_event(method: &str, text: &str) -> bool {
    let envelope: InlineResourceEnvelope<'_> = match serde_json::from_str(text) {
        Ok(envelope) => envelope,
        Err(_) => return false,
    };
    let url = match method {
        "Network.requestWillBeSent" => envelope.params.request.url,
        "Network.responseReceived" => envelope.params.response.url,
        _ => None,
    };
    url.is_some_and(|url| url.starts_with("data:"))
}

fn remote_error(error: &Value, request_id: u64, method: &str) -> OffprintError {
    let mut result = command_context(
        cdp_error(
            "offprint.browser.cdp_command",
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

fn command_context(error: OffprintError, request_id: u64, method: &str) -> OffprintError {
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

fn websocket_error(error: tungstenite::Error) -> OffprintError {
    cdp_error(
        "offprint.browser.cdp_transport",
        format!("CDP WebSocket transport failed: {error}"),
    )
    .retryable(true)
}

fn cdp_task_error(error: tokio::task::JoinError) -> OffprintError {
    cdp_error(
        "offprint.browser.cdp_task",
        format!("CDP transport task failed: {error}"),
    )
}

fn cdp_decode_error(error: serde_json::Error) -> OffprintError {
    cdp_error(
        "offprint.browser.cdp_decode",
        format!("browser sent malformed CDP JSON: {error}"),
    )
}

fn cdp_closed() -> OffprintError {
    cdp_error(
        "offprint.browser.cdp_closed",
        "the CDP connection is closed",
    )
    .retryable(true)
}

fn cdp_error(code: &'static str, message: impl Into<String>) -> OffprintError {
    OffprintError::new(code, ErrorStage::Browser, message)
}

#[cfg(test)]
mod tests;
