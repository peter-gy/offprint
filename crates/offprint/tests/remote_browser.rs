use offprint::{
    BrowserChannel, BrowserSource, CaptureArtifact, CaptureRequest, ErrorStage, NetworkPolicy,
    Offprint, OffprintError, Result, VerificationMode,
};
use offprint_chromium::{ChromiumDiscovery, ChromiumLaunchOptions, ChromiumProcess};
use offprint_test_support::{FixtureResponse, FixtureServer};

#[cfg(unix)]
#[tokio::test]
async fn doctor_rejects_a_discoverable_browser_that_cannot_launch() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().map_err(fixture_io_error)?;
    let executable = directory.path().join("fake-chromium");
    std::fs::write(
        &executable,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  echo \"Chromium 150.0.0.0\"\n  exit 0\nfi\nexit 23\n",
    )
    .map_err(fixture_io_error)?;
    let mut permissions = std::fs::metadata(&executable)
        .map_err(fixture_io_error)?
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&executable, permissions).map_err(fixture_io_error)?;
    let offprint = Offprint::builder()
        .browser_path(executable.to_string_lossy().into_owned())
        .build()?;

    let doctor = offprint.browsers().doctor().await;

    assert!(!doctor.ready);
    assert_eq!(
        doctor.selected.as_ref().map(|browser| browser.source),
        Some(BrowserSource::Explicit)
    );
    assert!(!doctor.collector.compatible);
    assert!(doctor.collector.peer_version.is_none());
    assert!(
        doctor
            .recovery
            .iter()
            .any(|action| action.code.starts_with("offprint.browser.launch"))
    );
    offprint.close().await
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn local_doctor_probes_launch_cdp_and_collector_readiness() -> Result<()> {
    let executable = ChromiumDiscovery::new()
        .discover()
        .await
        .selected
        .and_then(|browser| browser.executable_path)
        .ok_or_else(browser_unavailable)?;
    let offprint = Offprint::builder()
        .browser_path(executable.into_utf8_path_buf())
        .build()?;

    let doctor = offprint.browsers().doctor().await;

    assert!(doctor.ready);
    assert_eq!(
        doctor.selected.as_ref().map(|browser| browser.source),
        Some(BrowserSource::Explicit)
    );
    assert!(doctor.collector.compatible);
    assert_eq!(doctor.collector.peer_version.as_deref(), Some("1.5"));
    assert!(doctor.collector.missing_capabilities.is_empty());
    assert!(!doctor.collector.capabilities.is_empty());
    offprint.close().await
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn remote_doctor_reports_constraints_and_static_capture_keeps_process_ownership() -> Result<()>
{
    let server = FixtureServer::start().await?;
    let event_url = server.url("/events")?;
    let mut socket_url = server.url("/socket")?;
    socket_url.set_scheme("ws").map_err(|()| {
        OffprintError::new(
            "offprint.internal.fixture",
            ErrorStage::Internal,
            "fixture URL cannot use the WebSocket scheme",
        )
    })?;
    server
        .register(
            "/",
            FixtureResponse::html(format!(
                r#"<title>Remote capture</title><main id="result">starting</main>
                <script>
                const outcomes = {{}};
                const probe = (name, open) => {{
                    try {{
                        const transport = open();
                        transport.close?.();
                        outcomes[name] = "allowed";
                    }} catch (error) {{
                        outcomes[name] = error?.name ?? "error";
                    }}
                }};
                probe("webSocket", () => new WebSocket({}));
                probe("eventSource", () => new EventSource({}));
                probe("rtc", () => new RTCPeerConnection());
                document.getElementById("result").textContent =
                    `eventSource=${{outcomes.eventSource}};` +
                    `rtc=${{outcomes.rtc}};webSocket=${{outcomes.webSocket}}`;
                </script>"#,
                serde_json::to_string(socket_url.as_str()).map_err(|error| {
                    OffprintError::new(
                        "offprint.internal.fixture",
                        ErrorStage::Internal,
                        format!("failed to encode the WebSocket fixture URL: {error}"),
                    )
                })?,
                serde_json::to_string(event_url.as_str()).map_err(|error| {
                    OffprintError::new(
                        "offprint.internal.fixture",
                        ErrorStage::Internal,
                        format!("failed to encode the event stream fixture URL: {error}"),
                    )
                })?,
            )),
        )
        .await?;
    let fixture_url = server.url("/")?;

    let discovery = ChromiumDiscovery::new().discover().await;
    let executable = discovery
        .selected
        .and_then(|browser| browser.executable_path)
        .ok_or_else(browser_unavailable)?;
    let process = ChromiumProcess::launch(ChromiumLaunchOptions::new(executable)).await?;
    let remote_endpoint = process.endpoint().clone();
    let offprint = Offprint::builder().cdp_url(remote_endpoint).build()?;
    let doctor = offprint.browsers().doctor().await;

    assert!(!doctor.ready);
    assert_eq!(
        doctor.selected.as_ref().map(|browser| browser.source),
        Some(BrowserSource::Remote)
    );
    assert!(doctor.collector.compatible);
    assert_eq!(doctor.collector.peer_version.as_deref(), Some("1.5"));
    assert!(doctor.collector.missing_capabilities.is_empty());
    assert!(!doctor.collector.capabilities.is_empty());
    assert!(
        doctor
            .recovery
            .iter()
            .any(|action| action.code == "offprint.input.remote_network_policy")
    );
    assert!(
        doctor
            .recovery
            .iter()
            .any(|action| action.code == "offprint.browser.offline_verifier_unavailable")
    );

    let request = CaptureRequest::builder(fixture_url.as_str())?
        .network(NetworkPolicy::Unrestricted)
        .verification(VerificationMode::Static)
        .build()?;
    let result = offprint.captures().start(request).await?.result().await?;

    assert_eq!(result.verification.network_requests, 0);
    let CaptureArtifact::Bytes { content, .. } = result.artifact else {
        return Err(OffprintError::new(
            "offprint.internal.test",
            ErrorStage::Internal,
            "remote capture did not return the requested byte artifact",
        ));
    };
    assert!(
        String::from_utf8_lossy(&content)
            .contains("eventSource=SecurityError;rtc=SecurityError;webSocket=SecurityError")
    );

    offprint.close().await?;
    assert!(process.is_healthy().await);
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
async fn remote_doctor_reports_endpoint_failure_without_falling_back_locally() -> Result<()> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|error| {
        OffprintError::new(
            "offprint.internal.fixture",
            ErrorStage::Internal,
            format!("failed to reserve a closed test endpoint: {error}"),
        )
    })?;
    let port = listener.local_addr().map_err(|error| {
        OffprintError::new(
            "offprint.internal.fixture",
            ErrorStage::Internal,
            format!("failed to inspect the test endpoint: {error}"),
        )
    })?;
    drop(listener);
    let endpoint = url::Url::parse(&format!("http://127.0.0.1:{}?token=secret", port.port()))
        .map_err(|error| {
            OffprintError::new(
                "offprint.internal.fixture",
                ErrorStage::Internal,
                format!("failed to construct the test endpoint: {error}"),
            )
        })?;
    let cache = tempfile::tempdir().map_err(|error| {
        OffprintError::new(
            "offprint.internal.fixture",
            ErrorStage::Internal,
            format!("failed to create a test browser cache: {error}"),
        )
    })?;
    let offprint = Offprint::builder()
        .cdp_url(endpoint)
        .browser_channel(BrowserChannel::Managed)
        .cache_dir(cache.path().to_string_lossy().into_owned())
        .build()?;

    let doctor = offprint.browsers().doctor().await;

    assert!(!doctor.ready);
    assert!(doctor.selected.is_none());
    assert!(!doctor.collector.compatible);
    assert!(doctor.collector.peer_version.is_none());
    assert!(
        doctor
            .recovery
            .iter()
            .any(|action| action.code == "offprint.browser.cdp_connect")
    );
    assert!(
        doctor
            .recovery
            .iter()
            .all(|action| !action.description.contains("secret"))
    );
    offprint.close().await?;
    Ok(())
}

fn browser_unavailable() -> OffprintError {
    OffprintError::new(
        "offprint.browser.unavailable",
        ErrorStage::Browser,
        "the remote capture test requires a compatible local browser",
    )
}

#[cfg(unix)]
fn fixture_io_error(error: std::io::Error) -> OffprintError {
    OffprintError::new(
        "offprint.internal.fixture",
        ErrorStage::Internal,
        format!("failed to prepare browser fixture: {error}"),
    )
}
