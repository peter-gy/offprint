use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use pageknot_test_support::{FixtureResponse, FixtureServer};
use serde_json::Value;
use tempfile::TempDir;
use tokio::process::Command;

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test]
#[ignore = "requires PAGEKNOT_RELEASE_BINARY and a compatible browser"]
async fn release_binary_captures_and_verifies_offline() -> TestResult {
    let binary = std::env::var_os("PAGEKNOT_RELEASE_BINARY")
        .map(PathBuf::from)
        .ok_or("PAGEKNOT_RELEASE_BINARY is unset")?;
    let metadata = std::fs::symlink_metadata(&binary)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("release binary must be a directly addressed regular file".into());
    }

    let browser_path = std::env::var_os("PAGEKNOT_RELEASE_BROWSER_PATH").map(PathBuf::from);
    if browser_path.is_none() {
        let install = run(
            &binary,
            &["browser", "install", "--json"],
            Duration::from_secs(300),
        )
        .await?;
        require_success("browser install", &install)?;
        let installed: Value = serde_json::from_slice(&install.stdout)?;
        if installed.get("action").and_then(Value::as_str) != Some("install")
            || installed.pointer("/browser/source").and_then(Value::as_str) != Some("managed")
            || installed.get("revision").and_then(Value::as_str).is_none()
        {
            return Err("browser install returned an invalid operation record".into());
        }
    } else if !browser_path.as_deref().is_some_and(Path::is_file) {
        return Err("PAGEKNOT_RELEASE_BROWSER_PATH must name a browser executable".into());
    }

    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r##"<!doctype html><html><head><title>Release smoke</title></head>
                <body><h1 id="state">pending</h1><script>
                document.querySelector("#state").textContent = "captured";
                </script></body></html>"##,
            ),
        )
        .await?;
    let temporary = TempDir::new()?;
    let artifact = temporary.path().join("release-smoke.html");
    let source = server.url("/")?;
    let mut capture_arguments = vec![
        "capture".to_owned(),
        source.to_string(),
        "--output".to_owned(),
        artifact.to_string_lossy().into_owned(),
        "--json".to_owned(),
    ];
    append_browser_path(&mut capture_arguments, browser_path.as_deref());
    let capture = run_owned(&binary, capture_arguments, Duration::from_secs(180)).await?;
    require_success("capture", &capture)?;
    let captured: Value = serde_json::from_slice(&capture.stdout)?;
    if captured.get("status").and_then(Value::as_str) != Some("succeeded")
        || captured
            .pointer("/verification/passed")
            .and_then(Value::as_bool)
            != Some(true)
        || captured
            .pointer("/verification/networkRequests")
            .and_then(Value::as_u64)
            != Some(0)
    {
        return Err("capture returned an invalid success record".into());
    }
    let html = std::fs::read_to_string(&artifact)?;
    if !html.contains("captured") {
        return Err("release artifact omitted rendered fixture state".into());
    }

    let mut verify_arguments = vec![
        "verify".to_owned(),
        artifact.to_string_lossy().into_owned(),
        "--level".to_owned(),
        "offline".to_owned(),
        "--json".to_owned(),
    ];
    append_browser_path(&mut verify_arguments, browser_path.as_deref());
    let verify = run_owned(&binary, verify_arguments, Duration::from_secs(180)).await?;
    require_success("offline verify", &verify)?;
    let verified: Value = serde_json::from_slice(&verify.stdout)?;
    if verified.get("passed").and_then(Value::as_bool) != Some(true)
        || verified.get("networkRequests").and_then(Value::as_u64) != Some(0)
    {
        return Err("offline verify returned an invalid success record".into());
    }
    server.close().await;
    Ok(())
}

fn append_browser_path(arguments: &mut Vec<String>, browser_path: Option<&Path>) {
    if let Some(path) = browser_path {
        arguments.push("--browser-path".to_owned());
        arguments.push(path.to_string_lossy().into_owned());
    }
}

async fn run(binary: &Path, arguments: &[&str], timeout: Duration) -> TestResult<Output> {
    run_owned(
        binary,
        arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect(),
        timeout,
    )
    .await
}

async fn run_owned(binary: &Path, arguments: Vec<String>, timeout: Duration) -> TestResult<Output> {
    let mut command = Command::new(binary);
    command.kill_on_drop(true).args(arguments);
    tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "release command exceeded its time budget",
            )
        })?
        .map_err(Into::into)
}

fn require_success(operation: &str, output: &Output) -> TestResult {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.chars().take(4_096).collect::<String>();
    Err(format!("{operation} exited with {}: {stderr}", output.status).into())
}
