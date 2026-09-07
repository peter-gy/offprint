use std::error::Error;

use clap::Parser as _;
use offprint::{
    ArtifactFormat, CaptureOutput, CaptureRequest, ConflictPolicy, ErrorStage, ExportResult,
    LazyLoadPolicy, Milliseconds, Offprint, OffprintError, PortablePath, ReadinessMode,
    VerificationReport,
};
use offprint_test_support::{FixtureResponse, FixtureServer};

use crate::command::{ColorOutput, OutputOptions};
use crate::output::exit_for_error;
use crate::{Cli, CommandExit, run_with_terminal_diagnostics};

use super::arguments::{apply_capture_arguments, parse_duration};
use super::capture::CapturePlan;
use super::{drive_capture, drive_verification, read_credentials, run};

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[test]
fn capture_wait_until_accepts_each_readiness_mode() -> TestResult {
    for (value, expected) in [
        ("render-idle", ReadinessMode::RenderIdle),
        ("network-idle", ReadinessMode::NetworkIdle),
        ("load", ReadinessMode::Load),
        ("dom-content-loaded", ReadinessMode::DomContentLoaded),
    ] {
        let cli = Cli::try_parse_from([
            "offprint",
            "capture",
            "https://example.com",
            "--wait-until",
            value,
            "--output",
            "capture.html",
        ])?;
        let crate::command::Command::Capture(arguments) = cli.command else {
            return Err(std::io::Error::other("fixture parsed the wrong command").into());
        };
        let mut request = CaptureRequest::builder("https://example.com")?.build()?;

        apply_capture_arguments(&mut request, &arguments)?;

        assert_eq!(request.readiness.mode, expected, "{value}");
        if expected != ReadinessMode::RenderIdle {
            assert_eq!(
                request.readiness.lazy_load,
                LazyLoadPolicy::Disabled,
                "{value}"
            );
        }
    }
    Ok(())
}

#[test]
fn capture_plan_selects_the_canonical_html_target() -> TestResult {
    for (arguments, expected) in [
        (
            vec![
                "offprint",
                "capture",
                "https://example.com",
                "--output",
                "-",
            ],
            "bytes",
        ),
        (
            vec![
                "offprint",
                "capture",
                "https://example.com",
                "--output",
                "capture.html",
            ],
            "capture.html",
        ),
    ] {
        let cli = Cli::try_parse_from(arguments)?;
        let crate::command::Command::Capture(arguments) = cli.command else {
            return Err(std::io::Error::other("fixture parsed the wrong command").into());
        };
        let plan = CapturePlan::from_arguments(&arguments)?;
        let mut request = CaptureRequest::builder("https://example.com")?.build()?;

        plan.apply_to_request(&mut request);

        match request.output {
            CaptureOutput::Memory { .. } => assert_eq!(expected, "bytes"),
            CaptureOutput::File { path, .. } => assert_eq!(path.as_str(), expected),
        }
    }
    Ok(())
}

#[test]
fn capture_duration_overflow_is_invalid_input() {
    let value = format!("{:.0}h", f64::MAX / 2.0);

    assert!(
        parse_duration(&value)
            .is_err_and(|error| { error.code.as_str() == "offprint.input.duration" })
    );
}

#[test]
fn capture_delay_composes_with_the_selected_readiness_mode() -> TestResult {
    let cli = Cli::try_parse_from([
        "offprint",
        "capture",
        "https://example.com",
        "--wait-until",
        "network-idle",
        "--delay",
        "1250ms",
        "--timeout",
        "30s",
        "--output",
        "capture.html",
    ])?;
    let crate::command::Command::Capture(arguments) = cli.command else {
        return Err(std::io::Error::other("fixture parsed the wrong command").into());
    };
    let mut request = CaptureRequest::builder("https://example.com")?.build()?;

    apply_capture_arguments(&mut request, &arguments)?;

    assert_eq!(request.readiness.mode, ReadinessMode::NetworkIdle);
    assert_eq!(request.readiness.delay.get(), 1_250);
    assert_eq!(request.limits.duration.get(), 30_000);
    Ok(())
}

#[tokio::test]
async fn completion_writes_script_data_to_stdout() -> TestResult {
    let cli = Cli::try_parse_from(["offprint", "completion", "bash"])?;
    let mut input = std::io::empty();
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

    assert_eq!(exit, CommandExit::Success);
    assert!(String::from_utf8_lossy(&output).contains("_offprint"));
    assert!(diagnostics.is_empty());
    Ok(())
}

#[tokio::test]
async fn missing_artifact_input_returns_a_runtime_status() -> TestResult {
    for command in ["inspect", "verify"] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("missing.html");
        let path = path
            .to_str()
            .ok_or_else(|| std::io::Error::other("artifact path is not UTF-8"))?;
        let mut arguments = vec!["offprint", "artifact", command, path];
        if command == "verify" {
            arguments.extend(["--verification", "static"]);
        }
        let cli = Cli::try_parse_from(arguments)?;
        let mut input = std::io::empty();
        let mut output = Vec::new();
        let mut diagnostics = Vec::new();

        let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

        assert_eq!(exit, CommandExit::RuntimeFailure, "{command}");
        assert!(output.is_empty(), "{command}");
        assert!(
            String::from_utf8_lossy(&diagnostics).contains("offprint.artifact.read"),
            "{command}: {}",
            String::from_utf8_lossy(&diagnostics)
        );
    }
    Ok(())
}

#[tokio::test]
async fn missing_exported_artifact_returns_a_runtime_status() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("missing.pdf");
    let path = path
        .to_str()
        .ok_or_else(|| std::io::Error::other("artifact path is not UTF-8"))?;
    let cli = Cli::try_parse_from(["offprint", "artifact", "verify", path, "--format", "pdf"])?;
    let mut input = std::io::empty();
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

    assert_eq!(exit, CommandExit::RuntimeFailure);
    assert!(output.is_empty());
    assert!(String::from_utf8_lossy(&diagnostics).contains("offprint.artifact.read"));
    Ok(())
}

#[tokio::test]
async fn malformed_inspection_input_returns_a_runtime_status() -> TestResult {
    let cli = Cli::try_parse_from(["offprint", "artifact", "inspect", "-"])?;
    let mut input = std::io::Cursor::new(b"<html><body>incomplete</body></html>".as_slice());
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

    assert_eq!(exit, CommandExit::RuntimeFailure);
    assert!(output.is_empty());
    assert!(String::from_utf8_lossy(&diagnostics).contains("offprint.verification.manifest"));
    Ok(())
}

#[tokio::test]
async fn quiet_inspect_keeps_command_data_on_stdout() -> TestResult {
    let manifest = serde_json::from_str::<offprint::ArtifactManifest>(include_str!(
        "../../../../schemas/examples/artifact-manifest.json"
    ))?;
    let document =
        offprint_document::Document::parse(b"<!doctype html><html><head></head><body></body>");
    let artifact = offprint_html::encode_html(&document, &manifest)?;
    let cli = Cli::try_parse_from(["offprint", "artifact", "inspect", "-", "--quiet"])?;
    let mut input = std::io::Cursor::new(artifact);
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

    assert_eq!(exit, CommandExit::Success);
    assert!(diagnostics.is_empty());
    let output = String::from_utf8(output)?;
    for field in [
        "format: Offprint HTML v2 (schema 2)",
        "source: https://example.com/",
        "captured: 2026-07-27T12:00:00+00:00",
        "generator: Offprint 0.1.0",
        "browser: Chrome 151.0.7922.47 (Managed, CDP 1.3)",
        "environment: 1440x900@1, en-US, UTC, Light, Reduce",
        "policy sha256:",
        "frames: 1",
        "resources: 0 discovered",
        "warnings: none",
        "structural repair: applied=false",
        "verification mode: Offline",
    ] {
        assert!(output.contains(field), "missing `{field}` in:\n{output}");
    }
    Ok(())
}

#[tokio::test]
async fn verify_human_output_names_the_artifact() -> TestResult {
    let manifest = serde_json::from_str::<offprint::ArtifactManifest>(include_str!(
        "../../../../schemas/examples/artifact-manifest.json"
    ))?;
    let document =
        offprint_document::Document::parse(b"<!doctype html><html><head></head><body></body>");
    let artifact = offprint_html::encode_html(&document, &manifest)?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("capture.html");
    std::fs::write(&path, artifact)?;
    let path = path
        .to_str()
        .ok_or_else(|| std::io::Error::other("artifact path is not UTF-8"))?;
    let cli = Cli::try_parse_from([
        "offprint",
        "artifact",
        "verify",
        path,
        "--verification",
        "static",
    ])?;
    let mut input = std::io::empty();
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

    assert_eq!(exit, CommandExit::Success);
    assert!(diagnostics.is_empty());
    let output = String::from_utf8(output)?;
    assert!(output.contains(&format!("artifact {path} (")));
    assert!(output.contains("with 0 network requests"));
    Ok(())
}

#[tokio::test]
async fn doctor_reports_browser_flags_as_effective_configuration() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("chromium");
    let path = path
        .to_str()
        .ok_or_else(|| std::io::Error::other("browser path is not UTF-8"))?;
    let cli = Cli::try_parse_from(["offprint", "doctor", "--browser-path", path, "--json"])?;
    let mut input = std::io::empty();
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

    assert!(matches!(
        exit,
        CommandExit::Success | CommandExit::RuntimeFailure
    ));
    let report = serde_json::from_slice::<serde_json::Value>(&output)?;
    let browser_path = report["configuration"]
        .as_array()
        .and_then(|values| values.iter().find(|value| value["field"] == "browser.path"))
        .ok_or_else(|| std::io::Error::other("doctor omitted browser.path"))?;
    assert_eq!(browser_path["value"], path);
    assert_eq!(browser_path["provenance"], "flag");
    assert_eq!(browser_path["redacted"], false);
    Ok(())
}

#[test]
fn header_object_from_stdin_maps_to_redacted_credentials() -> TestResult {
    let cli = Cli::try_parse_from([
        "offprint",
        "capture",
        "https://example.com",
        "--headers",
        "-",
        "--output",
        "capture.html",
    ])?;
    let crate::command::Command::Capture(arguments) = cli.command else {
        return Err(std::io::Error::other("fixture parsed the wrong command").into());
    };
    let mut input = std::io::Cursor::new(br#"{"Authorization":"Bearer secret"}"#);
    let credentials = read_credentials(
        arguments.headers.as_deref(),
        arguments.cookies.as_deref(),
        &mut input,
    )?;

    assert_eq!(
        credentials
            .headers
            .first()
            .map(|header| header.value.expose_secret()),
        Some("Bearer secret")
    );
    assert!(!format!("{credentials:?}").contains("Bearer secret"));
    Ok(())
}

#[tokio::test]
async fn artifact_stdout_rejects_credential_stdin_before_browser_work() -> TestResult {
    let cli = Cli::try_parse_from([
        "offprint",
        "capture",
        "https://example.com",
        "--headers",
        "-",
        "--output",
        "-",
    ])?;
    let mut input = std::io::Cursor::new(br#"{"Authorization":"secret"}"#);
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

    assert_eq!(exit, CommandExit::InvalidInput);
    assert!(output.is_empty());
    assert!(!String::from_utf8_lossy(&diagnostics).contains("secret"));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn capture_staging_failure_returns_runtime_status_with_sanitized_diagnostics() -> TestResult {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir()?;
    let output_directory = directory.path().join("output");
    std::fs::create_dir(&output_directory)?;
    std::fs::set_permissions(&output_directory, std::fs::Permissions::from_mode(0o500))?;
    let destination = output_directory.join("capture.html");
    let diagnostics_directory = directory.path().join("diagnostics");
    let destination = destination
        .to_str()
        .ok_or_else(|| std::io::Error::other("artifact path is not UTF-8"))?;
    let diagnostics_path = diagnostics_directory
        .to_str()
        .ok_or_else(|| std::io::Error::other("diagnostic path is not UTF-8"))?;
    let cli = Cli::try_parse_from([
        "offprint",
        "capture",
        "https://example.com",
        "--output",
        destination,
        "--verification",
        "offline",
        "--diagnostics",
        diagnostics_path,
        "--json",
        "--color",
        "never",
    ])?;
    let mut input = std::io::empty();
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;
    std::fs::set_permissions(&output_directory, std::fs::Permissions::from_mode(0o700))?;

    assert_eq!(exit, CommandExit::RuntimeFailure);
    assert!(output.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&diagnostics)?;
    assert_eq!(error["code"], "offprint.output.staging");
    assert!(error["diagnosticsPath"].is_string());
    let files =
        std::fs::read_dir(&diagnostics_directory)?.collect::<std::result::Result<Vec<_>, _>>()?;
    assert_eq!(files.len(), 1);
    let bundle: serde_json::Value = serde_json::from_slice(&std::fs::read(files[0].path())?)?;
    assert_eq!(bundle["status"], "failed");
    assert_eq!(bundle["request"]["verification"], "offline");
    assert_eq!(bundle["failure"]["code"], "offprint.output.staging");
    assert_eq!(bundle["failure"]["stage"], "encoding");
    assert!(
        bundle["failure"]["retryable"]
            .as_bool()
            .is_some_and(|retryable| retryable)
    );
    assert_eq!(
        bundle["failure"]["chain"][0]["code"],
        "offprint.output.staging"
    );
    Ok(())
}

#[tokio::test]
async fn forced_color_marks_error_labels_even_when_stderr_is_not_a_terminal() -> TestResult {
    let cli = Cli::try_parse_from([
        "offprint",
        "capture",
        "https://example.com",
        "--viewport",
        "invalid",
        "--color",
        "always",
        "--output",
        "capture.html",
    ])?;
    let mut input = std::io::empty();
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit =
        run_with_terminal_diagnostics(cli, &mut input, &mut output, &mut diagnostics, false).await;

    assert_eq!(exit, CommandExit::InvalidInput);
    assert!(String::from_utf8_lossy(&diagnostics).contains("\u{1b}[31merror\u{1b}[0m:"));
    Ok(())
}

#[tokio::test]
async fn disabled_color_keeps_terminal_diagnostics_stable() -> TestResult {
    let cli = Cli::try_parse_from([
        "offprint",
        "capture",
        "https://example.com",
        "--viewport",
        "invalid",
        "--color",
        "never",
        "--output",
        "capture.html",
    ])?;
    let mut input = std::io::empty();
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit =
        run_with_terminal_diagnostics(cli, &mut input, &mut output, &mut diagnostics, true).await;

    assert_eq!(exit, CommandExit::InvalidInput);
    assert!(!diagnostics.contains(&b'\x1b'));
    assert!(String::from_utf8_lossy(&diagnostics).starts_with("error: "));
    Ok(())
}

#[test]
fn interrupted_errors_map_to_the_shell_interrupt_status() {
    let error = OffprintError::new(
        "offprint.runtime.interrupted",
        ErrorStage::Shutdown,
        "capture was interrupted",
    );

    assert_eq!(exit_for_error(&error, true), CommandExit::Interrupted);
}

#[tokio::test]
async fn verification_interrupt_wins_over_a_pending_operation() {
    let verification = std::future::pending::<offprint::Result<VerificationReport>>();
    let result = drive_verification(verification, None, async { Ok(()) }).await;

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.runtime.interrupted")
    );
}

#[tokio::test]
async fn verification_deadline_returns_a_retryable_timeout() {
    let verification = std::future::pending::<offprint::Result<VerificationReport>>();
    let signal = std::future::pending::<std::io::Result<()>>();
    let result = drive_verification(
        verification,
        Some(std::time::Duration::from_millis(1)),
        signal,
    )
    .await;

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.runtime.timeout")
    );
    assert!(result.is_err_and(|error| error.retryable));
}

#[tokio::test]
async fn verify_rejects_an_invalid_deadline() -> TestResult {
    let cli = Cli::try_parse_from([
        "offprint",
        "artifact",
        "verify",
        "missing-artifact.html",
        "--verification",
        "static",
        "--timeout",
        "later",
    ])?;
    let mut input = std::io::empty();
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

    assert_eq!(exit, CommandExit::InvalidInput);
    assert!(String::from_utf8_lossy(&diagnostics).contains("offprint.input.duration"));
    Ok(())
}

#[tokio::test]
async fn static_verify_rejects_local_browser_selection() -> TestResult {
    let cli = Cli::try_parse_from([
        "offprint",
        "artifact",
        "verify",
        "missing-artifact.html",
        "--verification",
        "static",
        "--browser-path",
        "/browser",
    ])?;
    let mut input = std::io::empty();
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

    assert_eq!(exit, CommandExit::InvalidInput);
    let diagnostics = String::from_utf8(diagnostics)?;
    assert!(diagnostics.contains("offprint.input.browser_selection"));
    Ok(())
}

#[tokio::test]
async fn static_verify_ignores_a_configured_remote_browser() -> TestResult {
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        r#"
        [browser]
        cdp_url = "http://127.0.0.1:9222"
        "#,
    )?;
    let cli = Cli::try_parse_from([
        "offprint",
        "artifact",
        "verify",
        "missing-artifact.html",
        "--verification",
        "static",
        "--config",
        config
            .to_str()
            .ok_or_else(|| std::io::Error::other("configuration path is not UTF-8"))?,
    ])?;
    let mut input = std::io::empty();
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

    assert_eq!(exit, CommandExit::RuntimeFailure);
    let diagnostics = String::from_utf8(diagnostics)?;
    assert!(diagnostics.contains("offprint.artifact.read"));
    assert!(!diagnostics.contains("offprint.input.browser_selection"));
    Ok(())
}

#[tokio::test]
async fn owned_browser_commands_reject_a_configured_remote() -> TestResult {
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        r#"
        [browser]
        cdp_url = "http://127.0.0.1:9222"
        "#,
    )?;
    let config = config
        .to_str()
        .ok_or_else(|| std::io::Error::other("configuration path is not UTF-8"))?
        .to_owned();
    let missing_artifact = directory
        .path()
        .join("missing.html")
        .to_str()
        .ok_or_else(|| std::io::Error::other("artifact path is not UTF-8"))?
        .to_owned();
    let export_directory = directory
        .path()
        .join("export")
        .to_str()
        .ok_or_else(|| std::io::Error::other("export path is not UTF-8"))?
        .to_owned();
    let crawl_directory = directory
        .path()
        .join("crawl")
        .to_str()
        .ok_or_else(|| std::io::Error::other("crawl path is not UTF-8"))?
        .to_owned();
    let commands = [
        vec![
            "offprint".to_owned(),
            "artifact".to_owned(),
            "export".to_owned(),
            missing_artifact.clone(),
            "--output".to_owned(),
            export_directory,
            "--format".to_owned(),
            "pdf".to_owned(),
            "--config".to_owned(),
            config.clone(),
        ],
        vec![
            "offprint".to_owned(),
            "artifact".to_owned(),
            "verify".to_owned(),
            missing_artifact,
            "--verification".to_owned(),
            "offline".to_owned(),
            "--config".to_owned(),
            config.clone(),
        ],
        vec![
            "offprint".to_owned(),
            "crawl".to_owned(),
            "https://example.com".to_owned(),
            "--output".to_owned(),
            crawl_directory,
            "--config".to_owned(),
            config,
        ],
    ];

    for arguments in commands {
        let command = arguments[1].clone();
        let cli = Cli::try_parse_from(arguments)?;
        let mut input = std::io::empty();
        let mut output = Vec::new();
        let mut diagnostics = Vec::new();

        let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

        assert_eq!(exit, CommandExit::InvalidInput, "{command}");
        assert!(output.is_empty(), "{command}");
        let diagnostics = String::from_utf8(diagnostics)?;
        assert!(
            diagnostics.contains("offprint.input.browser_selection"),
            "{command}: {diagnostics}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn local_browser_flags_override_a_configured_remote() -> TestResult {
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("config.toml");
    std::fs::write(
        &config,
        r#"
        [browser]
        cdp_url = "http://127.0.0.1:9222"
        "#,
    )?;
    let config = config
        .to_str()
        .ok_or_else(|| std::io::Error::other("configuration path is not UTF-8"))?
        .to_owned();
    let missing_artifact = directory
        .path()
        .join("missing.html")
        .to_str()
        .ok_or_else(|| std::io::Error::other("artifact path is not UTF-8"))?
        .to_owned();
    let export_directory = directory
        .path()
        .join("export")
        .to_str()
        .ok_or_else(|| std::io::Error::other("export path is not UTF-8"))?
        .to_owned();
    let crawl_directory = directory
        .path()
        .join("crawl")
        .to_str()
        .ok_or_else(|| std::io::Error::other("crawl path is not UTF-8"))?
        .to_owned();
    let commands = [
        (
            vec![
                "offprint".to_owned(),
                "artifact".to_owned(),
                "export".to_owned(),
                missing_artifact.clone(),
                "--output".to_owned(),
                export_directory,
                "--format".to_owned(),
                "pdf".to_owned(),
                "--config".to_owned(),
                config.clone(),
                "--browser-path".to_owned(),
                "/local-browser".to_owned(),
            ],
            CommandExit::RuntimeFailure,
            "offprint.artifact.read",
        ),
        (
            vec![
                "offprint".to_owned(),
                "artifact".to_owned(),
                "verify".to_owned(),
                missing_artifact,
                "--verification".to_owned(),
                "offline".to_owned(),
                "--config".to_owned(),
                config.clone(),
                "--browser-path".to_owned(),
                "/local-browser".to_owned(),
            ],
            CommandExit::RuntimeFailure,
            "offprint.artifact.read",
        ),
        (
            vec![
                "offprint".to_owned(),
                "crawl".to_owned(),
                "invalid-url".to_owned(),
                "--output".to_owned(),
                crawl_directory,
                "--config".to_owned(),
                config,
                "--browser-path".to_owned(),
                "/local-browser".to_owned(),
            ],
            CommandExit::InvalidInput,
            "offprint.input.url",
        ),
    ];

    for (arguments, expected_exit, expected_code) in commands {
        let command = arguments[1].clone();
        let cli = Cli::try_parse_from(arguments)?;
        let mut input = std::io::empty();
        let mut output = Vec::new();
        let mut diagnostics = Vec::new();

        let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

        assert_eq!(exit, expected_exit, "{command}");
        assert!(output.is_empty(), "{command}");
        let diagnostics = String::from_utf8(diagnostics)?;
        assert!(
            diagnostics.contains(expected_code),
            "{command}: {diagnostics}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn verify_failure_writes_sanitized_diagnostics() -> TestResult {
    let directory = tempfile::tempdir()?;
    let directory_path = directory
        .path()
        .to_str()
        .ok_or_else(|| std::io::Error::other("diagnostic path is not UTF-8"))?;
    let cli = Cli::try_parse_from([
        "offprint",
        "artifact",
        "verify",
        "-",
        "--verification",
        "static",
        "--diagnostics",
        directory_path,
    ])?;
    let mut input =
        std::io::Cursor::new(b"<html><body>verification-secret</body></html>".as_slice());
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

    assert_eq!(exit, CommandExit::VerificationFailure);
    assert!(output.is_empty());
    let files = std::fs::read_dir(directory.path())?.collect::<std::result::Result<Vec<_>, _>>()?;
    assert_eq!(files.len(), 1);
    let bundle = std::fs::read_to_string(files[0].path())?;
    assert!(!bundle.contains("verification-secret"));
    let payload: serde_json::Value = serde_json::from_str(&bundle)?;
    assert_eq!(payload["operation"], "verify");
    assert_eq!(payload["verificationMode"], "static");
    assert_eq!(payload["failure"]["code"], "offprint.verification.manifest");
    assert!(String::from_utf8_lossy(&diagnostics).contains("verification.diagnostics.json"));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn group_readable_credential_file_is_rejected_before_browser_work() -> TestResult {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;

    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(br#"{"Authorization":"secret"}"#)?;
    std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o640))?;
    let path = file
        .path()
        .to_str()
        .ok_or_else(|| std::io::Error::other("credential path is not UTF-8"))?;
    let cli = Cli::try_parse_from([
        "offprint",
        "capture",
        "https://example.com",
        "--headers",
        path,
        "--output",
        "capture.html",
    ])?;
    let mut input = std::io::empty();
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

    assert_eq!(exit, CommandExit::InvalidInput);
    assert!(!String::from_utf8_lossy(&diagnostics).contains("secret"));
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn capture_commits_and_reports_the_destination() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html("<title>CLI fixture</title><h1>ready</h1>"),
        )
        .await?;
    let url = server.url("/")?;
    let directory = tempfile::tempdir()?;
    let destination = directory.path().join("capture.html");
    std::fs::write(&destination, b"existing output")?;
    let destination = destination
        .to_str()
        .ok_or_else(|| std::io::Error::other("artifact path is not UTF-8"))?;
    let cli = Cli::try_parse_from([
        "offprint",
        "capture",
        url.as_str(),
        "--output",
        destination,
        "--on-exists",
        "replace",
        "--quiet",
    ])?;
    let mut input = std::io::empty();
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();

    let exit = run(cli, &mut input, &mut output, &mut diagnostics).await;

    assert_eq!(exit, CommandExit::Success);
    assert_eq!(String::from_utf8_lossy(&output), format!("{destination}\n"));
    let verification = offprint_html::verify_static(&std::fs::read(destination)?)?;
    assert_eq!(verification.mode, offprint::VerificationMode::Static);
    assert!(diagnostics.is_empty());
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn artifact_export_writes_and_verifies_pdf_output() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html("<title>CLI PDF fixture</title><main>ready</main>"),
        )
        .await?;
    let directory = tempfile::tempdir()?;
    let capture = directory.path().join("capture.html");
    let export_directory = directory.path().join("formats");
    std::fs::create_dir(&export_directory)?;
    let pdf = export_directory.join("capture.pdf");
    std::fs::write(&pdf, b"existing output")?;

    let capture_cli = Cli::try_parse_from([
        "offprint",
        "capture",
        server.url("/")?.as_str(),
        "--output",
        capture
            .to_str()
            .ok_or_else(|| std::io::Error::other("capture path is not UTF-8"))?,
        "--quiet",
    ])?;
    let export_cli = Cli::try_parse_from([
        "offprint",
        "artifact",
        "export",
        capture
            .to_str()
            .ok_or_else(|| std::io::Error::other("capture path is not UTF-8"))?,
        "--output",
        export_directory
            .to_str()
            .ok_or_else(|| std::io::Error::other("export path is not UTF-8"))?,
        "--format",
        "pdf",
        "--on-exists",
        "replace",
        "--quiet",
    ])?;
    let mut input = std::io::empty();
    let mut capture_output = Vec::new();
    let mut diagnostics = Vec::new();
    assert_eq!(
        run(
            capture_cli,
            &mut input,
            &mut capture_output,
            &mut diagnostics,
        )
        .await,
        CommandExit::Success
    );
    let mut export_output = Vec::new();
    assert_eq!(
        run(export_cli, &mut input, &mut export_output, &mut diagnostics,).await,
        CommandExit::Success
    );
    let offprint = Offprint::new()?;
    offprint
        .artifacts()
        .verify_format(PortablePath::from_path_buf(pdf)?, ArtifactFormat::Pdf)
        .await?;
    offprint.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn artifact_export_json_returns_the_committed_artifact() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html("<title>CLI PDF JSON fixture</title><main>ready</main>"),
        )
        .await?;
    let directory = tempfile::tempdir()?;
    let capture = directory.path().join("capture.html");
    let export_directory = directory.path().join("formats");
    let capture_path = capture
        .to_str()
        .ok_or_else(|| std::io::Error::other("capture path is not UTF-8"))?;
    let export_path = export_directory
        .to_str()
        .ok_or_else(|| std::io::Error::other("export path is not UTF-8"))?;
    let capture_cli = Cli::try_parse_from([
        "offprint",
        "capture",
        server.url("/")?.as_str(),
        "--output",
        capture_path,
        "--quiet",
    ])?;
    let export_cli = Cli::try_parse_from([
        "offprint",
        "artifact",
        "export",
        capture_path,
        "--output",
        export_path,
        "--format",
        "pdf",
        "--json",
    ])?;
    let mut input = std::io::empty();
    let mut output = Vec::new();
    let mut diagnostics = Vec::new();
    assert_eq!(
        run(capture_cli, &mut input, &mut output, &mut diagnostics).await,
        CommandExit::Success
    );
    output.clear();
    assert_eq!(
        run(export_cli, &mut input, &mut output, &mut diagnostics).await,
        CommandExit::Success
    );
    let result: ExportResult = serde_json::from_slice(&output)?;
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(result.artifacts[0].format, ArtifactFormat::Pdf);
    assert_eq!(
        result.artifacts[0].entrypoint,
        PortablePath::from_path_buf(export_directory.join("capture.pdf"))?
    );
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn interrupted_capture_preserves_the_existing_destination_and_redacts_progress() -> TestResult
{
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html("<title>Interrupt fixture</title><h1>ready</h1>"),
        )
        .await?;
    let mut url = server.url("/")?;
    url.query_pairs_mut()
        .append_pair("token", "cli-interrupt-secret")
        .append_pair("view", "full");
    let directory = tempfile::tempdir()?;
    let destination = directory.path().join("capture.html");
    std::fs::write(&destination, b"existing artifact")?;
    let destination_text = destination
        .to_str()
        .ok_or_else(|| std::io::Error::other("artifact path is not UTF-8"))?;
    let mut request = CaptureRequest::builder(url.as_str())?
        .output(CaptureOutput::file(destination_text.into()).with_conflict(ConflictPolicy::Replace))
        .build()?;
    request.readiness.delay = Milliseconds::new(30_000);
    request.readiness.lazy_load = LazyLoadPolicy::Disabled;
    let offprint = Offprint::builder().build()?;
    let job = offprint.captures().start(request).await?;
    let interrupt = async {
        loop {
            if !server.requests().await.is_empty() {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    };
    let options = OutputOptions {
        json: false,
        quiet: false,
        color: ColorOutput::Never,
    };
    let mut diagnostics = Vec::new();

    let result = drive_capture(&job, &options, &mut diagnostics, false, interrupt).await;

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.runtime.interrupted")
    );
    assert_eq!(std::fs::read(&destination)?, b"existing artifact");
    let diagnostics = String::from_utf8_lossy(&diagnostics);
    assert!(diagnostics.contains("interrupt: cancelling capture"));
    assert!(!diagnostics.contains("cli-interrupt-secret"));
    offprint.close().await?;
    server.close().await;
    Ok(())
}
