use std::collections::BTreeMap;
use std::error::Error;
use std::future::pending;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use futures_util::{Sink, Stream};
use offprint_model::OffprintError;
use serde_json::{Value, json};
use tokio::sync::{Semaphore, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::{self, Message};

use super::{
    CdpClient, ClientInner, EventBus, Outbound, PendingCommand, WireDeadlines, route_message,
    run_transport,
};

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
            r#"{"method":"Page.loadEventFired","params":{"timestamp":1},"sessionId":"s1"}"#.into(),
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
            r#"{"method":"Page.loadEventFired","params":{"timestamp":1},"sessionId":"s1"}"#.into(),
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
            format!(r#"{{"method":"Runtime.consoleAPICalled","params":{{"value":"{payload}"}}}}"#)
                .into(),
        ),
        &mut pending,
        &events,
    );

    assert_eq!(
        terminal.as_ref().map(|error| error.code.as_str()),
        Some("offprint.browser.cdp_event_limit")
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

#[test]
fn oversized_responses_still_route_to_pending_commands() -> TestResult {
    let (sender, receiver) = oneshot::channel();
    let mut pending = BTreeMap::from([(
        19,
        PendingCommand {
            method: "Network.getResponseBody".to_owned(),
            response: sender,
        },
    )]);
    let events = EventBus::new(1);
    let body = "x".repeat(super::MAX_EVENT_BYTES);
    let message = serde_json::to_string(&json!({
        "id": 19,
        "result": {"body": body},
    }))?;

    let terminal = route_message(Message::Text(message.into()), &mut pending, &events);
    let response = receiver.blocking_recv()??;

    assert!(terminal.is_none());
    assert_eq!(
        response.get("body").and_then(Value::as_str).map(str::len),
        Some(super::MAX_EVENT_BYTES)
    );
    Ok(())
}

#[tokio::test]
async fn actor_send_timeout_fails_the_pending_command() -> TestResult {
    let (outbound, outbound_receiver) = mpsc::channel(1);
    let (_cancellations, cancellation_receiver) = mpsc::unbounded_channel();
    let events = EventBus::new(1);
    let task = tokio::spawn(run_transport(
        PendingWire,
        outbound_receiver,
        cancellation_receiver,
        events,
        test_wire_deadlines(),
    ));
    let (response, response_receiver) = oneshot::channel();
    assert!(
        outbound
            .send(Outbound::Command {
                id: 23,
                method: "Runtime.evaluate".to_owned(),
                params: json!({}),
                session_id: None,
                response,
            })
            .await
            .is_ok()
    );

    let response = tokio::time::timeout(Duration::from_secs(1), response_receiver).await??;
    let outcome = tokio::time::timeout(Duration::from_secs(1), task).await??;

    assert_eq!(
        response.as_ref().err().map(|error| error.code.as_str()),
        Some("offprint.browser.cdp_transport")
    );
    assert_eq!(
        outcome.as_ref().err().map(|error| error.code.as_str()),
        Some("offprint.browser.cdp_transport")
    );
    Ok(())
}

#[tokio::test]
async fn actor_close_timeout_terminates_the_transport() -> TestResult {
    let (outbound, outbound_receiver) = mpsc::channel(1);
    let (_cancellations, cancellation_receiver) = mpsc::unbounded_channel();
    let events = EventBus::new(1);
    let task = tokio::spawn(run_transport(
        PendingWire,
        outbound_receiver,
        cancellation_receiver,
        events,
        test_wire_deadlines(),
    ));
    assert!(outbound.send(Outbound::Close).await.is_ok());

    let outcome = tokio::time::timeout(Duration::from_secs(1), task).await??;

    assert_eq!(
        outcome.as_ref().err().map(|error| error.code.as_str()),
        Some("offprint.browser.cdp_close_timeout")
    );
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
        Some("offprint.browser.cdp_timeout")
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
        Some("offprint.browser.cdp_close_timeout")
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
        Some("offprint.browser.cdp_close_timeout")
    );
    assert!(task_dropped.load(Ordering::Acquire));
}

fn route_remote_error(id: u64, method: &str, remote_error: Value) -> TestResult<OffprintError> {
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
        return Err(
            std::io::Error::other(format!("remote error closed the transport: {error}")).into(),
        );
    }
    match receiver.blocking_recv()? {
        Err(error) => Ok(error),
        Ok(_) => Err(std::io::Error::other("remote error produced a successful response").into()),
    }
}

fn test_client(
    outbound: mpsc::Sender<Outbound>,
    task: Option<JoinHandle<offprint_model::Result<()>>>,
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

async fn spawn_pending_task() -> (JoinHandle<offprint_model::Result<()>>, Arc<AtomicBool>) {
    let task_dropped = Arc::new(AtomicBool::new(false));
    let task_drop_signal = Arc::clone(&task_dropped);
    let (task_started, task_started_receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let _drop_signal = DropSignal(task_drop_signal);
        let _ignored = task_started.send(());
        pending::<offprint_model::Result<()>>().await
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

fn test_wire_deadlines() -> WireDeadlines {
    WireDeadlines {
        send: Duration::from_millis(20),
        close: Duration::from_millis(20),
    }
}

#[derive(Debug)]
struct PendingWire;

impl Sink<Message> for PendingWire {
    type Error = tungstenite::Error;

    fn poll_ready(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Pending
    }

    fn start_send(self: Pin<&mut Self>, _message: Message) -> std::result::Result<(), Self::Error> {
        Ok(())
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Pending
    }

    fn poll_close(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Pending
    }
}

impl Stream for PendingWire {
    type Item = std::result::Result<Message, tungstenite::Error>;

    fn poll_next(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Pending
    }
}
