use offprint::{
    CaptureRequest, DiagnosticsPolicy, ErrorStage, MissingResourcePolicy, Offprint, OffprintError,
    PortablePath, Result,
};
use offprint_test_support::{FixtureResponse, FixtureServer};

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn failed_capture_commits_a_sanitized_diagnostic_bundle() -> Result<()> {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                "<title>Diagnostic failure</title><img src=\"/missing-image.png?token=image-secret\">",
            ),
        )
        .await?;
    let source = server.url("/")?;
    let root = tempfile::tempdir().map_err(|error| {
        OffprintError::new(
            "offprint.internal.fixture",
            ErrorStage::Internal,
            format!("failed to create diagnostic fixture directory: {error}"),
        )
    })?;
    let directory = root.path().join("bundles");
    let mut request = CaptureRequest::builder(source.as_str())?.build()?;
    request.content.missing_resources = MissingResourcePolicy::Fail;
    request.diagnostics = DiagnosticsPolicy {
        directory: Some(PortablePath::from_path_buf(directory)?),
        screenshots: false,
    };
    let offprint = Offprint::builder().build()?;
    let result = offprint.captures().start(request).await?.result().await;
    let error = result.err().ok_or_else(|| {
        OffprintError::new(
            "offprint.internal.fixture",
            ErrorStage::Internal,
            "diagnostic failure fixture unexpectedly succeeded",
        )
    })?;
    let diagnostics_path = error.diagnostics_path.ok_or_else(|| {
        OffprintError::new(
            "offprint.internal.fixture",
            ErrorStage::Internal,
            "failed capture did not name its diagnostic bundle",
        )
    })?;
    let bundle = std::fs::read_to_string(diagnostics_path.as_utf8_path()).map_err(|error| {
        OffprintError::new(
            "offprint.internal.fixture",
            ErrorStage::Internal,
            format!("failed to read capture diagnostic bundle: {error}"),
        )
    })?;

    assert!(bundle.contains("\"status\": \"failed\""), "{bundle}");
    assert!(bundle.contains("capture.started"), "{bundle}");
    assert!(bundle.contains("resource.discovered"), "{bundle}");
    assert!(!bundle.contains("image-secret"), "{bundle}");

    offprint.close().await?;
    server.close().await;
    Ok(())
}
