use std::error::Error;
use std::time::Duration;

use pageknot::{
    ArtifactExportRequest, ArtifactInput, ArtifactResult, ArtifactVariant, ArtifactVariantKind,
    CaptureResult, ConflictPolicy, ContentDigest, MarkdownOptions, PageKnot, PdfOptions,
    PortablePath,
};
use pageknot_chromium::{ChromiumDiscovery, ChromiumLaunchOptions, ChromiumProcess};
use pageknot_test_support::{FixtureResponse, FixtureServer};
use url::Url;

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

fn capture_with_bytes(
    content: Vec<u8>,
    verification: pageknot::VerificationResult,
) -> TestResult<CaptureResult> {
    let bytes = u64::try_from(content.len())?;
    let digest = ContentDigest::sha256(&content);
    let mut capture: CaptureResult = serde_json::from_str(include_str!(
        "../../../schemas/examples/capture-result.json"
    ))?;
    capture.artifact = ArtifactResult::Bytes {
        content,
        bytes,
        sha256: digest,
    };
    capture.verification = verification;
    Ok(capture)
}

#[tokio::test]
async fn invalid_source_does_not_create_the_export_destination() -> TestResult {
    let directory = tempfile::tempdir()?;
    let output_path = directory.path().join("variants");
    let output = PortablePath::from_path_buf(output_path.clone())?;
    let pageknot = PageKnot::builder().build()?;

    let result = pageknot
        .artifacts()
        .export(
            ArtifactInput::Bytes(b"not a PageKnot artifact".to_vec()),
            ArtifactExportRequest {
                output_directory: output,
                base_name: "capture".to_owned(),
                variants: vec![ArtifactVariant::Zip],
                conflict: ConflictPolicy::Replace,
            },
        )
        .await;

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("pageknot.verification.manifest")
    );
    assert!(!output_path.exists());
    pageknot.close().await?;
    Ok(())
}

#[tokio::test]
async fn export_capture_revalidates_html_before_reusing_offline_evidence() -> TestResult {
    let manifest: pageknot::ArtifactManifest = serde_json::from_str(include_str!(
        "../../../schemas/examples/artifact-manifest.json"
    ))?;
    let transformed = pageknot_transform::transform_document(
        b"<!doctype html><html><body><main>capture</main></body></html>",
    )?;
    let encoded = pageknot_transform::encode_artifact(&transformed, manifest)?;
    let html = String::from_utf8(encoded.bytes)?
        .replacen(
            "</body>",
            "<script>globalThis.injected = true</script></body>",
            1,
        )
        .into_bytes();
    let bytes = u64::try_from(html.len())?;
    let digest = ContentDigest::sha256(&html);
    let fixture: CaptureResult = serde_json::from_str(include_str!(
        "../../../schemas/examples/capture-result.json"
    ))?;
    let mut verification = fixture.verification;
    verification.artifact_sha256 = digest;
    verification.bytes = bytes;
    let capture = capture_with_bytes(html, verification)?;
    let directory = tempfile::tempdir()?;
    let output_path = directory.path().join("variants");
    let pageknot = PageKnot::builder().build()?;

    let result = pageknot
        .artifacts()
        .export_capture(
            &capture,
            ArtifactExportRequest {
                output_directory: PortablePath::from_path_buf(output_path.clone())?,
                base_name: "capture".to_owned(),
                variants: vec![ArtifactVariant::Markdown(MarkdownOptions::default())],
                conflict: ConflictPolicy::Replace,
            },
        )
        .await;

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("pageknot.verification.active_content")
    );
    assert!(!output_path.exists());
    pageknot.close().await?;
    Ok(())
}

#[tokio::test]
async fn export_capture_requires_matching_offline_evidence() -> TestResult {
    let manifest: pageknot::ArtifactManifest = serde_json::from_str(include_str!(
        "../../../schemas/examples/artifact-manifest.json"
    ))?;
    let transformed = pageknot_transform::transform_document(
        b"<!doctype html><html><body><main>capture</main></body></html>",
    )?;
    let encoded = pageknot_transform::encode_artifact(&transformed, manifest)?;
    let mut offline = encoded.verification.clone();
    offline.level = pageknot::VerificationPolicy::Offline;
    let mut mismatched = offline.clone();
    mismatched.artifact_sha256 = ContentDigest::sha256(b"different artifact");
    let directory = tempfile::tempdir()?;
    let pageknot = PageKnot::builder().build()?;

    for (name, verification) in [
        ("static", encoded.verification.clone()),
        ("mismatched", mismatched),
    ] {
        let output_path = directory.path().join(name);
        let capture = capture_with_bytes(encoded.bytes.clone(), verification)?;
        let result = pageknot
            .artifacts()
            .export_capture(
                &capture,
                ArtifactExportRequest {
                    output_directory: PortablePath::from_path_buf(output_path.clone())?,
                    base_name: "capture".to_owned(),
                    variants: vec![ArtifactVariant::Zip],
                    conflict: ConflictPolicy::Replace,
                },
            )
            .await;

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.verification.record"),
            "{name}"
        );
        assert!(!output_path.exists(), "{name}");
    }

    pageknot.close().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn every_artifact_variant_encodes_and_verifies() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/pixel.svg",
            FixtureResponse {
                status: 200,
                content_type: "image/svg+xml".to_owned(),
                headers: Default::default(),
                body: br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16">
                    <rect width="16" height="16" fill="rgb(30, 100, 180)"/>
                </svg>"#
                    .to_vec(),
            },
        )
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><html><head><title>Artifact variants</title>
                <style>body{font-family:system-ui}h1{color:rgb(20,40,80)}</style></head>
                <body><article><h1>Artifact variants</h1><p>portable output</p>
                <a href="/details">Details</a>
                <img src="/pixel.svg" alt="blue pixel"></article></body></html>"#,
            ),
        )
        .await?;
    let directory = tempfile::tempdir()?;
    let output = PortablePath::from_path_buf(directory.path().join("variants"))?;
    let pageknot = PageKnot::builder().build()?;
    let captured = pageknot
        .capture(server.url("/")?.as_str())?
        .to_bytes(64 * 1024 * 1024)
        .await?;
    let exported = pageknot
        .artifacts()
        .export_capture(
            &captured,
            ArtifactExportRequest {
                output_directory: output,
                base_name: "artifact-variants".to_owned(),
                variants: vec![
                    ArtifactVariant::Pdf(PdfOptions::default()),
                    ArtifactVariant::Markdown(MarkdownOptions::default()),
                    ArtifactVariant::Zip,
                    ArtifactVariant::SelfExtracting,
                    ArtifactVariant::Mhtml,
                ],
                conflict: ConflictPolicy::Fail,
            },
        )
        .await?;

    assert_eq!(exported.variants.len(), 5);
    assert_eq!(exported.policy_sha256, {
        let pageknot::ArtifactResult::Bytes { content, .. } = &captured.artifact else {
            return Err("missing source bytes".into());
        };
        pageknot_html::inspect_html(content)?.policy_sha256
    });
    assert_eq!(exported.resources, captured.resources);
    for variant in &exported.variants {
        assert!(variant.verification.passed, "{variant:?}");
        let verified = pageknot
            .artifacts()
            .verify_variant(variant.path.clone(), variant.kind)
            .await?;
        assert!(verified.passed, "{verified:?}");
        assert_eq!(verified.sha256, variant.sha256);
    }

    let self_extracting = exported
        .variants
        .iter()
        .find(|variant| variant.kind == ArtifactVariantKind::SelfExtracting)
        .map(|variant| variant.path.as_ref() as &std::path::Path)
        .ok_or("missing self-extracting variant")?;
    let observation = verify_browser_file(self_extracting).await?;
    assert!(observation.attempted_urls.is_empty(), "{observation:?}");
    assert!(observation.page_errors.is_empty(), "{observation:?}");
    assert!(observation.stable, "{observation:?}");

    let mhtml = exported
        .variants
        .iter()
        .find(|variant| variant.kind == ArtifactVariantKind::Mhtml)
        .map(|variant| variant.path.as_ref() as &std::path::Path)
        .ok_or("missing MHTML variant")?;
    verify_mhtml_browser_import(mhtml).await?;

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

async fn verify_browser_file(
    path: &std::path::Path,
) -> TestResult<pageknot_browser::OfflineBrowserObservation> {
    let executable = ChromiumDiscovery::new()
        .discover()
        .await
        .selected
        .and_then(|browser| browser.executable_path)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no browser"))?;
    let process = ChromiumProcess::launch(ChromiumLaunchOptions::new(executable)).await?;
    let page = process
        .new_page(&pageknot::BrowserEnvironment::default())
        .await?;
    let url = Url::from_file_path(path)
        .map_err(|()| std::io::Error::other("variant path is not a file URL"))?;
    let observation = page
        .verify_offline_url(&url, Duration::from_secs(20))
        .await?;
    page.close().await?;
    process.close().await?;
    Ok(observation)
}

async fn verify_mhtml_browser_import(path: &std::path::Path) -> TestResult {
    let executable = ChromiumDiscovery::new()
        .discover()
        .await
        .selected
        .and_then(|browser| browser.executable_path)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no browser"))?;
    let process = ChromiumProcess::launch(ChromiumLaunchOptions::new(executable)).await?;
    let page = process
        .new_page(&pageknot::BrowserEnvironment::default())
        .await?;
    let url = Url::from_file_path(path)
        .map_err(|()| std::io::Error::other("MHTML path is not a file URL"))?;
    let result = async {
        let observation = page
            .verify_offline_url(&url, Duration::from_secs(20))
            .await?;
        assert!(observation.attempted_urls.is_empty(), "{observation:?}");
        assert!(observation.page_errors.is_empty(), "{observation:?}");
        assert!(observation.stable, "{observation:?}");
        let document = page
            .evaluate("document.documentElement.outerHTML")
            .await?
            .as_str()
            .ok_or_else(|| std::io::Error::other("MHTML import returned no document"))?
            .to_owned();
        assert!(
            document.contains("<h1>Artifact variants</h1>"),
            "{document}"
        );
        assert!(document.contains("portable output"), "{document}");
        TestResult::Ok(())
    }
    .await;
    let page_close = page.close().await;
    let process_close = process.close().await;
    result?;
    page_close?;
    process_close?;
    Ok(())
}
