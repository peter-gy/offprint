use std::collections::BTreeSet;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt as _, StreamExt as _};
use pageknot_browser::NetworkGuard;
use pageknot_model::{
    BrowserEnvironment, NetworkPolicy, NetworkRules, ReadinessMode, RequestHeader, SecretString,
};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, Semaphore};
use tokio_tungstenite::tungstenite::Message;
use url::Url;

use crate::CdpClient;
use crate::resources::captures_rendered_response;

use super::{
    ChromiumPage, continued_headers, is_long_lived_resource_type, is_long_lived_response,
    required_string, same_origin,
};

type AsyncTestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug)]
struct GatedCdpServer {
    endpoint: Url,
    gated_method: &'static str,
    entered: Arc<Semaphore>,
    release: Arc<Semaphore>,
    commands: Arc<Mutex<Vec<serde_json::Value>>>,
    task: tokio::task::JoinHandle<AsyncTestResult>,
}

impl GatedCdpServer {
    async fn start(gated_method: &'static str) -> AsyncTestResult<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let endpoint = Url::parse(&format!("ws://{}", listener.local_addr()?))?;
        let entered = Arc::new(Semaphore::new(0));
        let release = Arc::new(Semaphore::new(0));
        let commands = Arc::new(Mutex::new(Vec::new()));
        let task_entered = Arc::clone(&entered);
        let task_release = Arc::clone(&release);
        let task_commands = Arc::clone(&commands);
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
                let command: serde_json::Value = serde_json::from_str(text.as_ref())?;
                let Some(id) = command.get("id").cloned() else {
                    continue;
                };
                let Some(method) = command.get("method").and_then(serde_json::Value::as_str) else {
                    continue;
                };
                task_commands.lock().await.push(command.clone());
                if method == gated_method {
                    task_entered.add_permits(1);
                    if let Ok(permit) = task_release.acquire().await {
                        permit.forget();
                    }
                }
                let result =
                    match method {
                        "Target.createBrowserContext" => {
                            json!({"browserContextId": "test-context"})
                        }
                        "Target.createTarget" => json!({"targetId": "test-target"}),
                        "Target.attachToTarget" => json!({"sessionId": "test-session"}),
                        "Page.addScriptToEvaluateOnNewDocument" => {
                            json!({"identifier": "test-script"})
                        }
                        "Page.navigate" => {
                            json!({"frameId": "test-frame", "loaderId": "test-loader"})
                        }
                        "Runtime.evaluate"
                            if command["params"]["expression"].as_str().is_some_and(
                                |expression| expression.contains("document.readyState"),
                            ) =>
                        {
                            json!({"result": {"type": "boolean", "value": true}})
                        }
                        "Runtime.evaluate"
                            if command["params"]["expression"]
                                == r#"__pageknotCollector.call("freeze", [])"# =>
                        {
                            json!({"result": {"type": "boolean", "value": true}})
                        }
                        _ => json!({}),
                    };
                let response = json!({"id": id, "result": result}).to_string();
                socket.send(Message::Text(response.into())).await?;
            }
            Ok(())
        });
        Ok(Self {
            endpoint,
            gated_method,
            entered,
            release,
            commands,
            task,
        })
    }

    async fn wait_until_entered(&self) -> AsyncTestResult {
        tokio::time::timeout(Duration::from_secs(2), self.entered.acquire())
            .await
            .map_err(|error| {
                std::io::Error::other(format!(
                    "timed out waiting for {}: {error}",
                    self.gated_method
                ))
            })??
            .forget();
        Ok(())
    }

    fn resume(&self) {
        self.release.add_permits(1);
    }

    async fn wait_for_method(&self, method: &str) -> AsyncTestResult {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if self
                    .commands
                    .lock()
                    .await
                    .iter()
                    .any(|command| command["method"] == method)
                {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|error| {
            std::io::Error::other(format!("timed out waiting for {method}: {error}"))
        })?;
        Ok(())
    }

    async fn close(self) -> AsyncTestResult {
        self.task.abort();
        let _aborted = self.task.await;
        Ok(())
    }
}

#[tokio::test]
async fn cancelled_page_acquisition_disposes_contexts_and_actor_tasks() -> AsyncTestResult {
    for gated_method in [
        "Target.createTarget",
        "Target.attachToTarget",
        "Page.enable",
    ] {
        let server = GatedCdpServer::start(gated_method).await?;
        let client = CdpClient::connect(server.endpoint.clone()).await?;
        let task_client = client.clone();
        let acquisition = tokio::spawn(async move {
            ChromiumPage::create(task_client, &BrowserEnvironment::default()).await
        });
        server.wait_until_entered().await?;
        if gated_method == "Page.enable" {
            tokio::time::timeout(Duration::from_secs(2), async {
                while client.event_receiver_count() < 3 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .map_err(|error| {
                std::io::Error::other(format!(
                    "actor tasks did not start before {gated_method}: {error}"
                ))
            })?;
        }

        acquisition.abort();
        let _aborted = acquisition.await;
        server.resume();
        server
            .wait_for_method("Target.disposeBrowserContext")
            .await?;
        tokio::time::timeout(Duration::from_secs(2), async {
            while client.event_receiver_count() != 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|error| {
            std::io::Error::other(format!(
                "actor tasks survived cancellation at {gated_method}: {error}"
            ))
        })?;
        client.close().await?;
        server.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn context_downloads_are_denied_before_the_first_target() -> AsyncTestResult {
    let server = GatedCdpServer::start("Target.createTarget").await?;
    let client = CdpClient::connect(server.endpoint.clone()).await?;
    let task_client = client.clone();
    let acquisition = tokio::spawn(async move {
        ChromiumPage::create(task_client, &BrowserEnvironment::default()).await
    });
    server.wait_until_entered().await?;

    let commands = server.commands.lock().await.clone();
    assert_eq!(
        commands
            .iter()
            .take(3)
            .filter_map(|command| command["method"].as_str())
            .collect::<Vec<_>>(),
        [
            "Target.createBrowserContext",
            "Browser.setDownloadBehavior",
            "Target.createTarget",
        ]
    );
    assert_eq!(
        commands[1]["params"],
        json!({
            "behavior": "deny",
            "browserContextId": "test-context",
            "eventsEnabled": true
        })
    );

    acquisition.abort();
    let _aborted = acquisition.await;
    server.resume();
    server
        .wait_for_method("Target.disposeBrowserContext")
        .await?;
    client.close().await?;
    server.close().await?;
    Ok(())
}

#[tokio::test]
async fn freeze_uses_the_document_start_collector() -> AsyncTestResult {
    let server = GatedCdpServer::start("unused").await?;
    let client = CdpClient::connect(server.endpoint.clone()).await?;
    let page = ChromiumPage::create(client.clone(), &BrowserEnvironment::default()).await?;

    page.freeze_rendering().await?;

    let commands = server.commands.lock().await;
    let freeze = commands.iter().find(|command| {
        command["method"] == "Runtime.evaluate"
            && command["params"]["expression"] == r#"__pageknotCollector.call("freeze", [])"#
    });
    assert!(freeze.is_some());
    drop(commands);

    page.close().await?;
    client.close().await?;
    server.close().await?;
    Ok(())
}

#[tokio::test]
async fn navigation_accepts_an_already_complete_document_when_the_load_event_is_missed()
-> AsyncTestResult {
    let server = GatedCdpServer::start("unused").await?;
    let client = CdpClient::connect(server.endpoint.clone()).await?;
    let page = ChromiumPage::create(client.clone(), &BrowserEnvironment::default()).await?;
    let url = Url::parse("https://example.com/")?;

    let navigation = page
        .navigate(&url, ReadinessMode::Load, 20, Duration::from_millis(500))
        .await?;

    assert_eq!(navigation.final_url, url);
    page.close().await?;
    client.close().await?;
    server.close().await?;
    Ok(())
}

#[tokio::test]
async fn remote_pages_accept_only_unrestricted_network_policy() -> AsyncTestResult {
    let server = GatedCdpServer::start("unused").await?;
    let client = CdpClient::connect(server.endpoint.clone()).await?;
    let page = ChromiumPage::create(client.clone(), &BrowserEnvironment::default()).await?;
    let initial_url = Url::parse("https://example.com/")?;
    let policies = [
        NetworkPolicy::Standard,
        NetworkPolicy::Server,
        NetworkPolicy::Custom(NetworkRules {
            allowed_hosts: BTreeSet::new(),
            allowed_cidrs: BTreeSet::new(),
            allow_loopback: false,
            allow_private: false,
            allow_link_local: false,
        }),
    ];

    for policy in policies {
        let result = page
            .enable_network_guard(
                NetworkGuard::new(policy, &initial_url)?,
                Vec::new(),
                initial_url.clone(),
            )
            .await;
        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.input.remote_network_policy")
        );
    }
    page.enable_network_guard(
        NetworkGuard::new(NetworkPolicy::Unrestricted, &initial_url)?,
        Vec::new(),
        initial_url,
    )
    .await?;
    let offline = page
        .verify_offline_url(
            &Url::parse("file:///tmp/pageknot-test.html")?,
            Duration::from_secs(1),
        )
        .await;
    assert_eq!(
        offline
            .err()
            .map(|error| error.code.as_str().to_owned())
            .as_deref(),
        Some("pageknot.browser.offline_verifier_unavailable")
    );

    page.close().await?;
    client.close().await?;
    server.close().await?;
    Ok(())
}

#[test]
fn required_string_rejects_a_changed_protocol_shape() {
    let result = required_string(&json!({"target": 7}), "targetId", "create target");

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("pageknot.browser.cdp_shape")
    );
}

#[test]
fn scoped_headers_replace_matching_browser_headers() {
    let parameters = json!({
        "request": {
            "headers": {
                "Accept": "text/html",
                "Authorization": "old"
            }
        }
    });
    let headers = continued_headers(
        &parameters,
        &[RequestHeader {
            name: "authorization".to_owned(),
            value: SecretString::new("new"),
        }],
    );

    assert_eq!(headers.len(), 2);
    assert!(
        headers
            .iter()
            .any(|header| { header["name"] == "authorization" && header["value"] == "new" })
    );
    assert!(
        headers
            .iter()
            .any(|header| { header["name"] == "Accept" && header["value"] == "text/html" })
    );
}

#[test]
fn header_scope_uses_scheme_host_and_effective_port() {
    let origin = Url::parse("https://example.com/path").ok();
    let same = Url::parse("https://EXAMPLE.com:443/resource").ok();
    let other = Url::parse("https://example.com:444/resource").ok();

    assert!(
        origin
            .as_ref()
            .zip(same.as_ref())
            .is_some_and(|(origin, same)| same_origin(origin, same))
    );
    assert!(
        origin
            .as_ref()
            .zip(other.as_ref())
            .is_some_and(|(origin, other)| !same_origin(origin, other))
    );
}

#[test]
fn response_capture_ignores_long_lived_connection_types() {
    assert!(is_long_lived_resource_type(Some("EventSource")));
    assert!(is_long_lived_resource_type(Some("WebSocket")));
    assert!(!is_long_lived_resource_type(Some("Image")));
    assert!(captures_rendered_response(Some("Image")));
    assert!(captures_rendered_response(Some("Stylesheet")));
    assert!(!captures_rendered_response(Some("Script")));
    assert!(!captures_rendered_response(Some("XHR")));
    assert!(is_long_lived_response(&json!({
        "resourceType": "XHR",
        "responseHeaders": [{
            "name": "Content-Type",
            "value": "text/event-stream; charset=utf-8"
        }]
    })));
}
