use std::error::Error;
use std::sync::Arc;

use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::time::{Duration, timeout};
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use crate::CdpEvent;

use super::*;

type AsyncTestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

struct RecordingCdpServer {
    endpoint: Url,
    commands: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<AsyncTestResult>,
}

impl RecordingCdpServer {
    async fn start() -> AsyncTestResult<Self> {
        Self::start_with_failures(Vec::new()).await
    }

    async fn start_with_failures(
        failures: Vec<(&'static str, &'static str)>,
    ) -> AsyncTestResult<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let endpoint = Url::parse(&format!("ws://{}", listener.local_addr()?))?;
        let commands = Arc::new(Mutex::new(Vec::new()));
        let task_commands = Arc::clone(&commands);
        let mut failures = failures
            .into_iter()
            .map(|(method, session_id)| (method.to_owned(), session_id.to_owned()))
            .collect::<Vec<_>>();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await?;
            let mut socket = tokio_tungstenite::accept_async(stream).await?;
            while let Some(message) = socket.next().await {
                let message = message?;
                let Message::Text(text) = message else {
                    if matches!(message, Message::Close(_)) {
                        break;
                    }
                    continue;
                };
                let command: Value = serde_json::from_str(text.as_ref())?;
                let Some(id) = command.get("id").cloned() else {
                    continue;
                };
                let Some(method) = command
                    .get("method")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                else {
                    continue;
                };
                task_commands.lock().await.push(method.clone());
                let session_id = command
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let failure = failures
                    .iter()
                    .position(|(failure_method, failure_session)| {
                        failure_method == &method && failure_session == &session_id
                    });
                let response = if let Some(failure) = failure {
                    failures.remove(failure);
                    json!({
                        "id": id,
                        "error": {
                            "code": -32_000,
                            "message": "injected target policy failure"
                        }
                    })
                } else {
                    json!({"id": id, "result": {}})
                };
                socket
                    .send(Message::Text(response.to_string().into()))
                    .await?;
            }
            Ok(())
        });
        Ok(Self {
            endpoint,
            commands,
            task,
        })
    }

    async fn close(self) -> AsyncTestResult {
        self.task.abort();
        let _aborted = self.task.await;
        Ok(())
    }
}

#[tokio::test]
async fn policy_commits_after_every_existing_target_is_configured() -> AsyncTestResult {
    let server =
        RecordingCdpServer::start_with_failures(vec![("Fetch.enable", "session-1")]).await?;
    let client = CdpClient::connect(server.endpoint.clone()).await?;
    let manager = test_target_manager(client.clone());
    add_policy_targets(&manager).await;

    let first = manager.enable_fetch(false).await;

    assert_eq!(
        first.as_ref().err().map(|error| error.code.as_str()),
        Some("pageknot.browser.cdp_command")
    );
    assert!(manager.error().await.is_none());
    assert!(!manager.cancellation.is_cancelled());

    manager.enable_fetch(false).await?;

    assert_eq!(
        *server.commands.lock().await,
        ["Fetch.enable", "Fetch.enable", "Fetch.enable"]
    );
    manager.close().await;
    client.close().await?;
    server.close().await?;
    Ok(())
}

#[tokio::test]
async fn partial_policy_application_closes_manager_with_recorded_error() -> AsyncTestResult {
    let server =
        RecordingCdpServer::start_with_failures(vec![("Fetch.enable", "session-2")]).await?;
    let client = CdpClient::connect(server.endpoint.clone()).await?;
    let manager = test_target_manager(client.clone());
    add_policy_targets(&manager).await;

    let first = manager.enable_fetch(false).await;
    let second = manager.enable_fetch(false).await;
    let frames = manager.frames().await;

    for result in [&first, &second] {
        assert_eq!(
            result.as_ref().err().map(|error| error.code.as_str()),
            Some("pageknot.browser.cdp_command")
        );
    }
    assert_eq!(
        frames.as_ref().err().map(|error| error.code.as_str()),
        Some("pageknot.browser.cdp_command")
    );
    assert_eq!(
        manager
            .error()
            .await
            .as_ref()
            .map(|error| error.code.as_str()),
        Some("pageknot.browser.cdp_command")
    );
    assert_eq!(
        *server.commands.lock().await,
        ["Fetch.enable", "Fetch.enable"]
    );
    assert!(manager.cancellation.is_cancelled());

    client.close().await?;
    server.close().await?;
    Ok(())
}

fn test_target_manager(client: CdpClient) -> FrameTargetManager {
    FrameTargetManager::start_with_limits(
        client,
        "main".to_owned(),
        Arc::new(RwLock::new(HashSet::new())),
        false,
        TargetLimits::default(),
        true,
    )
}

async fn add_policy_targets(manager: &FrameTargetManager) {
    manager.state.lock().await.managed.extend([
        (
            "session-1".to_owned(),
            ManagedTarget {
                kind: TargetKind::ServiceWorker,
            },
        ),
        (
            "session-2".to_owned(),
            ManagedTarget {
                kind: TargetKind::ServiceWorker,
            },
        ),
    ]);
}

#[tokio::test]
async fn detached_iframe_becomes_a_typed_collection_failure() {
    let state = Mutex::new(TargetState {
        frames: BTreeMap::from([(
            "target-1".to_owned(),
            AttachedFrame {
                session_id: "session-1".to_owned(),
                target_id: "target-1".to_owned(),
                parent_session_id: "main".to_owned(),
                url: "https://frame.example/".to_owned(),
            },
        )]),
        managed: BTreeMap::from([(
            "session-1".to_owned(),
            ManagedTarget {
                kind: TargetKind::Document,
            },
        )]),
        error: None,
    });

    record_detached_target(&state, "session-1").await;

    let state = state.lock().await;
    assert!(state.frames.is_empty());
    assert_eq!(
        state.error.as_ref().map(|error| error.code.as_str()),
        Some("pageknot.frame.detached")
    );
    assert_eq!(
        state.error.as_ref().map(|error| error.retryable),
        Some(true)
    );
}

#[tokio::test]
async fn worker_detachment_does_not_poison_frame_collection() {
    let state = Mutex::new(TargetState {
        managed: BTreeMap::from([(
            "worker-session".to_owned(),
            ManagedTarget {
                kind: TargetKind::Worker,
            },
        )]),
        ..TargetState::default()
    });

    record_detached_target(&state, "worker-session").await;

    assert!(state.lock().await.error.is_none());
}

#[tokio::test]
async fn target_ingress_drains_unrelated_event_bursts_while_configuration_waits() -> AsyncTestResult
{
    let (event_tx, event_rx) = broadcast::channel(4);
    let (target_tx, mut target_rx) = mpsc::channel(1);
    let sessions = Arc::new(RwLock::new(HashSet::from(["main".to_owned()])));
    let state = Arc::new(Mutex::new(TargetState::default()));
    let cancellation = CancellationToken::new();
    let task = tokio::spawn(forward_target_events(
        event_rx,
        target_tx,
        sessions,
        Arc::clone(&state),
        cancellation.clone(),
    ));
    tokio::task::yield_now().await;

    assert!(
        event_tx
            .send(test_event("Target.attachedToTarget", "main"))
            .is_ok()
    );
    assert_eq!(
        target_rx.recv().await.map(|event| event.method),
        Some(Arc::from("Target.attachedToTarget"))
    );

    for index in 0..128 {
        assert!(
            event_tx
                .send(CdpEvent {
                    method: Arc::from("Network.dataReceived"),
                    params: Arc::new(json!({"requestId": index.to_string()})),
                    session_id: Some(Arc::from("main")),
                })
                .is_ok()
        );
        tokio::task::yield_now().await;
    }
    assert!(
        event_tx
            .send(test_event("Target.detachedFromTarget", "main"))
            .is_ok()
    );

    let event = timeout(Duration::from_secs(1), target_rx.recv())
        .await?
        .ok_or_else(|| std::io::Error::other("target event channel closed"))?;
    assert_eq!(event.method.as_ref(), "Target.detachedFromTarget");
    assert!(state.lock().await.error.is_none());

    cancellation.cancel();
    task.await?;
    Ok(())
}

fn test_event(method: &'static str, session_id: &'static str) -> CdpEvent {
    CdpEvent {
        method: Arc::from(method),
        params: Arc::new(json!({})),
        session_id: Some(Arc::from(session_id)),
    }
}

#[tokio::test]
async fn frame_admission_stops_before_the_bounded_maps_grow() -> Result<()> {
    let state = Mutex::new(TargetState::default());
    let limits = TargetLimits {
        maximum_frames: 1,
        maximum_targets: 2,
    };

    reserve_target(
        &state,
        "frame-session-1",
        "frame-target-1",
        "main",
        "https://frame.example/one",
        TargetKind::Document,
        limits,
    )
    .await?;
    let result = reserve_target(
        &state,
        "frame-session-2",
        "frame-target-2",
        "main",
        "https://frame.example/two",
        TargetKind::Document,
        limits,
    )
    .await;

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("pageknot.frame.limit")
    );
    let state = state.lock().await;
    assert_eq!(state.frames.len(), 1);
    assert_eq!(state.managed.len(), 1);
    assert!(!state.managed.contains_key("frame-session-2"));
    Ok(())
}

#[tokio::test]
async fn worker_admission_stops_at_the_managed_target_limit() -> Result<()> {
    let state = Mutex::new(TargetState::default());
    let limits = TargetLimits {
        maximum_frames: 4,
        maximum_targets: 1,
    };

    reserve_target(
        &state,
        "worker-session-1",
        "worker-target-1",
        "main",
        "",
        TargetKind::Worker,
        limits,
    )
    .await?;
    let result = reserve_target(
        &state,
        "worker-session-2",
        "worker-target-2",
        "main",
        "",
        TargetKind::Worker,
        limits,
    )
    .await;

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("pageknot.frame.limit")
    );
    assert_eq!(state.lock().await.managed.len(), 1);
    Ok(())
}

#[test]
fn bounded_frame_snapshot_checks_before_cloning_records() -> Result<()> {
    let state = TargetState {
        frames: BTreeMap::from([(
            "target-1".to_owned(),
            AttachedFrame {
                session_id: "session-1".to_owned(),
                target_id: "target-1".to_owned(),
                parent_session_id: "main".to_owned(),
                url: "https://frame.example/".to_owned(),
            },
        )]),
        ..TargetState::default()
    };

    assert_eq!(
        clone_frames_bounded(&state, 0)
            .err()
            .map(|error| error.code.as_str().to_owned())
            .as_deref(),
        Some("pageknot.frame.limit")
    );
    assert_eq!(clone_frames_bounded(&state, 1)?.len(), 1);
    Ok(())
}

#[test]
fn auto_attach_pauses_every_network_capable_child_target() {
    let parameters = auto_attach_parameters();
    let types = parameters["filter"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["type"].as_str())
        .collect::<HashSet<_>>();

    assert_eq!(
        types,
        HashSet::from([
            "page",
            "iframe",
            "worker",
            "service_worker",
            "shared_worker"
        ])
    );
    assert_eq!(parameters["waitForDebuggerOnStart"], true);
}

#[test]
fn default_fetch_filters_pause_document_requests() {
    let parameters = fetch_enable_parameters(false);
    let request_patterns = parameters["patterns"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|pattern| pattern["requestStage"] == "Request")
        .collect::<Vec<_>>();

    assert_eq!(request_patterns.len(), 2);
    assert!(
        request_patterns
            .iter()
            .all(|pattern| pattern["resourceType"] == "Document")
    );
}

#[test]
fn scoped_headers_pause_subresource_requests() {
    let parameters = fetch_enable_parameters(true);
    let request_patterns = parameters["patterns"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|pattern| pattern["requestStage"] == "Request")
        .collect::<Vec<_>>();

    assert_eq!(request_patterns.len(), 2);
    assert!(
        request_patterns
            .iter()
            .all(|pattern| pattern.get("resourceType").is_none())
    );
}

#[test]
fn response_filters_capture_rendered_resource_types() {
    let parameters = fetch_enable_parameters(false);
    let response_types = parameters["patterns"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|pattern| pattern["requestStage"] == "Response")
        .filter_map(|pattern| pattern["resourceType"].as_str())
        .collect::<HashSet<_>>();

    assert_eq!(
        response_types,
        HashSet::from(["Stylesheet", "Image", "Media", "Font", "Other"])
    );
}

#[tokio::test]
async fn dedicated_workers_skip_unsupported_fetch_interception_before_resume() -> AsyncTestResult {
    let server = RecordingCdpServer::start().await?;
    let client = CdpClient::connect(server.endpoint.clone()).await?;
    configure_target_session(
        &client,
        "child-session",
        TargetKind::Worker,
        TargetPolicy {
            fetch_enabled: true,
            intercept_subresource_requests: false,
            network_blocked: true,
        },
        false,
        true,
    )
    .await?;
    resume_target(&client, "child-session").await?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while server.commands.lock().await.len() < 6 {
            tokio::task::yield_now().await;
        }
    })
    .await?;

    assert_eq!(
        *server.commands.lock().await,
        [
            "Runtime.enable",
            "Network.enable",
            "Runtime.evaluate",
            "Target.setAutoAttach",
            "Network.setBlockedURLs",
            "Runtime.runIfWaitingForDebugger",
        ]
    );
    client.close().await?;
    server.close().await?;
    Ok(())
}

#[tokio::test]
async fn service_workers_resume_after_supported_fetch_setup() -> AsyncTestResult {
    let server = RecordingCdpServer::start().await?;
    let client = CdpClient::connect(server.endpoint.clone()).await?;
    configure_target_session(
        &client,
        "service-worker-session",
        TargetKind::ServiceWorker,
        TargetPolicy {
            fetch_enabled: true,
            intercept_subresource_requests: false,
            network_blocked: true,
        },
        false,
        true,
    )
    .await?;
    resume_target(&client, "service-worker-session").await?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while server.commands.lock().await.len() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await?;

    assert_eq!(
        *server.commands.lock().await,
        ["Fetch.enable", "Runtime.runIfWaitingForDebugger"]
    );
    client.close().await?;
    server.close().await?;
    Ok(())
}

#[tokio::test]
async fn verifier_document_targets_install_containment_without_collector() -> AsyncTestResult {
    let server = RecordingCdpServer::start().await?;
    let client = CdpClient::connect(server.endpoint.clone()).await?;
    configure_target_session(
        &client,
        "frame-session",
        TargetKind::Document,
        TargetPolicy::default(),
        false,
        false,
    )
    .await?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while server.commands.lock().await.len() < 8 {
            tokio::task::yield_now().await;
        }
    })
    .await?;

    assert_eq!(
        *server.commands.lock().await,
        [
            "Page.enable",
            "Runtime.enable",
            "DOM.enable",
            "Log.enable",
            "Network.enable",
            "Page.setLifecycleEventsEnabled",
            "Page.addScriptToEvaluateOnNewDocument",
            "Target.setAutoAttach",
        ]
    );
    client.close().await?;
    server.close().await?;
    Ok(())
}
