use std::error::Error;
use std::time::Duration;

use pageknot::{
    ArtifactExportRequest, ArtifactInput, ArtifactVariant, ArtifactVariantKind, ConflictPolicy,
    MarkdownOptions, PageKnot, PdfOptions, PortablePath,
};
use pageknot_chromium::{ChromiumDiscovery, ChromiumLaunchOptions, ChromiumProcess};
use pageknot_test_support::{FixtureResponse, FixtureServer};
use url::Url;

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

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
                <img src="/pixel.svg" alt="blue pixel"></article></body></html>"#,
            ),
        )
        .await?;
    let directory = tempfile::tempdir()?;
    let source = PortablePath::from_path_buf(directory.path().join("source.html"))?;
    let output = PortablePath::from_path_buf(directory.path().join("variants"))?;
    let pageknot = PageKnot::builder().build()?;
    let captured = pageknot
        .capture(server.url("/")?.as_str())?
        .save(source.clone())
        .await?;
    let exported = pageknot
        .artifacts()
        .export(
            ArtifactInput::File(source),
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
        let bytes = std::fs::read(captured.artifact.path().ok_or("missing source path")?)?;
        pageknot_html::inspect_html(&bytes)?.policy_sha256
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

trait ArtifactResultPath {
    fn path(&self) -> Option<&PortablePath>;
}

impl ArtifactResultPath for pageknot::ArtifactResult {
    fn path(&self) -> Option<&PortablePath> {
        match self {
            Self::File { path, .. } => Some(path),
            Self::Bytes { .. } => None,
        }
    }
}
