use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::tungstenite::Message;
use url::Url;

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug)]
pub(crate) struct TestCdpServer {
    endpoint: Url,
    outgoing: mpsc::UnboundedSender<Value>,
    commands: Arc<Mutex<Vec<Value>>>,
    task: Option<tokio::task::JoinHandle<TestResult>>,
}

impl TestCdpServer {
    pub(crate) async fn start(gated_method: &'static str) -> TestResult<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let endpoint = Url::parse(&format!("ws://{}", listener.local_addr()?))?;
        let (outgoing, mut outgoing_rx) = mpsc::unbounded_channel::<Value>();
        let commands = Arc::new(Mutex::new(Vec::new()));
        let task_commands = Arc::clone(&commands);
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            let mut socket = tokio_tungstenite::accept_async(stream).await?;
            loop {
                tokio::select! {
                    outgoing = outgoing_rx.recv() => {
                        let Some(outgoing) = outgoing else {
                            return Ok(());
                        };
                        socket
                            .send(Message::Text(outgoing.to_string().into()))
                            .await?;
                    }
                    incoming = socket.next() => {
                        let Some(incoming) = incoming else {
                            return Ok(());
                        };
                        let incoming = incoming?;
                        let Message::Text(text) = incoming else {
                            if matches!(incoming, Message::Close(_)) {
                                return Ok(());
                            }
                            continue;
                        };
                        let command: Value = serde_json::from_str(text.as_ref())?;
                        task_commands.lock().await.push(command.clone());
                        if command.get("method").and_then(Value::as_str) == Some(gated_method) {
                            continue;
                        }
                        let Some(id) = command.get("id").cloned() else {
                            continue;
                        };
                        socket
                            .send(Message::Text(
                                json!({"id": id, "result": {}}).to_string().into(),
                            ))
                            .await?;
                    }
                }
            }
        });
        Ok(Self {
            endpoint,
            outgoing,
            commands,
            task: Some(task),
        })
    }

    pub(crate) fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    pub(crate) fn send_event(&self, method: &str, params: Value, session_id: &str) -> TestResult {
        self.outgoing
            .send(json!({
                "method": method,
                "params": params,
                "sessionId": session_id,
            }))
            .map_err(|_| std::io::Error::other("test CDP server is closed"))?;
        Ok(())
    }

    pub(crate) async fn wait_for_method(&self, method: &str) -> TestResult {
        tokio::time::timeout(Duration::from_secs(2), async {
            while !self
                .commands
                .lock()
                .await
                .iter()
                .any(|command| command.get("method").and_then(Value::as_str) == Some(method))
            {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        Ok(())
    }

    pub(crate) async fn terminal_commands(&self, request_id: &str) -> Vec<Value> {
        self.commands
            .lock()
            .await
            .iter()
            .filter(|command| {
                matches!(
                    command.get("method").and_then(Value::as_str),
                    Some(
                        "Fetch.continueRequest"
                            | "Fetch.continueResponse"
                            | "Fetch.fulfillRequest"
                            | "Fetch.failRequest"
                    )
                ) && command.pointer("/params/requestId").and_then(Value::as_str)
                    == Some(request_id)
            })
            .cloned()
            .collect()
    }

    pub(crate) async fn close(mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ignored = task.await;
        }
    }
}

impl Drop for TestCdpServer {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
