use std::error::Error;
use std::time::Duration;

use offprint::{
    ArtifactFormat, ArtifactSource, CaptureArtifact, CaptureReceipt, ConflictPolicy, ContentDigest,
    ExportRequest, FormatSpec, MarkdownOptions, Offprint, PdfOptions, PortablePath,
};
use offprint_chromium::{ChromiumDiscovery, ChromiumLaunchOptions, ChromiumProcess};
use offprint_test_support::{FixtureResponse, FixtureServer};
use url::Url;

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

fn capture_with_bytes(
    content: Vec<u8>,
    verification: offprint::VerificationReport,
) -> TestResult<CaptureReceipt> {
    let bytes = u64::try_from(content.len())?;
    let digest = ContentDigest::sha256(&content);
    let mut capture: CaptureReceipt = serde_json::from_str(include_str!(
        "../../../schemas/examples/capture-receipt.json"
    ))?;
    capture.artifact = CaptureArtifact::Bytes {
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
    let output_path = directory.path().join("formats");
    let output = PortablePath::from_path_buf(output_path.clone())?;
    let offprint = Offprint::builder().build()?;

    let result = offprint
        .artifacts()
        .export(
            ArtifactSource::Bytes(b"not an Offprint artifact".to_vec()),
            ExportRequest {
                schema_version: offprint_model::PUBLIC_SCHEMA_VERSION,
                output_directory: output,
                base_name: "capture".to_owned(),
                formats: vec![FormatSpec::Zip],
                conflict: ConflictPolicy::Replace,
            },
        )
        .await;

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.verification.manifest")
    );
    assert!(!output_path.exists());
    offprint.close().await?;
    Ok(())
}

#[tokio::test]
async fn export_capture_revalidates_html_before_reusing_offline_evidence() -> TestResult {
    let manifest: offprint::ArtifactManifest = serde_json::from_str(include_str!(
        "../../../schemas/examples/artifact-manifest.json"
    ))?;
    let transformed = offprint_transform::transform_document(
        b"<!doctype html><html><body><main>capture</main></body></html>",
    )?;
    let encoded = offprint_transform::encode_artifact(&transformed, manifest)?;
    let html = String::from_utf8(encoded.bytes)?
        .replacen(
            "</body>",
            "<script>globalThis.injected = true</script></body>",
            1,
        )
        .into_bytes();
    let bytes = u64::try_from(html.len())?;
    let digest = ContentDigest::sha256(&html);
    let fixture: CaptureReceipt = serde_json::from_str(include_str!(
        "../../../schemas/examples/capture-receipt.json"
    ))?;
    let mut verification = fixture.verification;
    verification.artifact_sha256 = digest;
    verification.bytes = bytes;
    let capture = capture_with_bytes(html, verification)?;
    let directory = tempfile::tempdir()?;
    let output_path = directory.path().join("formats");
    let offprint = Offprint::builder().build()?;

    let result = offprint
        .artifacts()
        .export_capture(
            &capture,
            ExportRequest {
                schema_version: offprint_model::PUBLIC_SCHEMA_VERSION,
                output_directory: PortablePath::from_path_buf(output_path.clone())?,
                base_name: "capture".to_owned(),
                formats: vec![FormatSpec::Markdown(MarkdownOptions::default())],
                conflict: ConflictPolicy::Replace,
            },
        )
        .await;

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.verification.active_content")
    );
    assert!(!output_path.exists());
    offprint.close().await?;
    Ok(())
}

#[tokio::test]
async fn export_capture_requires_matching_offline_evidence() -> TestResult {
    let manifest: offprint::ArtifactManifest = serde_json::from_str(include_str!(
        "../../../schemas/examples/artifact-manifest.json"
    ))?;
    let transformed = offprint_transform::transform_document(
        b"<!doctype html><html><body><main>capture</main></body></html>",
    )?;
    let encoded = offprint_transform::encode_artifact(&transformed, manifest)?;
    let mut offline = encoded.verification.clone();
    offline.mode = offprint::VerificationMode::Offline;
    let mut mismatched = offline.clone();
    mismatched.artifact_sha256 = ContentDigest::sha256(b"different artifact");
    let directory = tempfile::tempdir()?;
    let offprint = Offprint::builder().build()?;

    for (name, verification) in [
        ("static", encoded.verification.clone()),
        ("mismatched", mismatched),
    ] {
        let output_path = directory.path().join(name);
        let capture = capture_with_bytes(encoded.bytes.clone(), verification)?;
        let result = offprint
            .artifacts()
            .export_capture(
                &capture,
                ExportRequest {
                    schema_version: offprint_model::PUBLIC_SCHEMA_VERSION,
                    output_directory: PortablePath::from_path_buf(output_path.clone())?,
                    base_name: "capture".to_owned(),
                    formats: vec![FormatSpec::Zip],
                    conflict: ConflictPolicy::Replace,
                },
            )
            .await;

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.verification.record"),
            "{name}"
        );
        assert!(!output_path.exists(), "{name}");
    }

    offprint.close().await?;
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
                r##"<!doctype html><html lang="en-GB"><head><title>Artifact formats</title>
                <meta name="author" content="Ada Lovelace">
                <meta name="description" content="Portable artifact representations">
                <meta name="keywords" content="capture, metadata, accessibility">
                <meta property="og:site_name" content="Offprint fixtures">
                <meta property="article:published_time" content="2026-07-29">
                <style>body{font-family:system-ui}h1{color:rgb(20,40,80)}</style></head>
                <body><article><h1>Artifact formats</h1><p>portable output</p>
                <a href="/details">Details</a>
                <a href="#section">Jump to section</a>
                <h2 id="section">Section destination</h2>
                <a href="#literal%">Percent section</a>
                <h2 id="literal%">Percent destination</h2>
                <img src="/pixel.svg" alt="blue pixel"></article></body></html>"##,
            ),
        )
        .await?;
    let directory = tempfile::tempdir()?;
    let output = PortablePath::from_path_buf(directory.path().join("formats"))?;
    let offprint = Offprint::builder().build()?;
    let captured = offprint
        .capture(server.url("/")?.as_str())?
        .bytes(64 * 1024 * 1024)
        .await?;
    let exported = offprint
        .artifacts()
        .export_capture(
            &captured,
            ExportRequest {
                schema_version: offprint_model::PUBLIC_SCHEMA_VERSION,
                output_directory: output,
                base_name: "artifact-formats".to_owned(),
                formats: vec![
                    FormatSpec::Pdf(PdfOptions::default()),
                    FormatSpec::Markdown(MarkdownOptions::default()),
                    FormatSpec::Zip,
                    FormatSpec::SelfExtractingHtml,
                    FormatSpec::Mhtml,
                ],
                conflict: ConflictPolicy::Fail,
            },
        )
        .await?;

    assert_eq!(exported.artifacts.len(), 5);
    assert_eq!(exported.capture_policy_sha256, {
        let offprint::CaptureArtifact::Bytes { content, .. } = &captured.artifact else {
            return Err("missing source bytes".into());
        };
        offprint_html::inspect_html(content)?.capture_policy_sha256
    });
    assert_eq!(exported.resources, captured.resources);
    for format in &exported.artifacts {
        let verified = offprint
            .artifacts()
            .verify_format(format.path.clone(), format.format)
            .await?;
        assert_eq!(verified.sha256, format.sha256);
    }

    let pdf = exported
        .artifacts
        .iter()
        .find(|format| format.format == ArtifactFormat::Pdf)
        .map(|format| format.path.as_ref() as &std::path::Path)
        .ok_or("missing PDF format")?;
    verify_pdf_semantics(pdf, exported.source_artifact_sha256)?;

    let self_extracting = exported
        .artifacts
        .iter()
        .find(|format| format.format == ArtifactFormat::SelfExtractingHtml)
        .map(|format| format.path.as_ref() as &std::path::Path)
        .ok_or("missing self-extracting format")?;
    let observation = verify_browser_file(self_extracting).await?;
    assert!(observation.attempted_urls.is_empty(), "{observation:?}");
    assert!(observation.page_errors.is_empty(), "{observation:?}");
    assert!(observation.stable, "{observation:?}");

    let mhtml = exported
        .artifacts
        .iter()
        .find(|format| format.format == ArtifactFormat::Mhtml)
        .map(|format| format.path.as_ref() as &std::path::Path)
        .ok_or("missing MHTML format")?;
    verify_mhtml_browser_import(mhtml).await?;

    offprint.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn pdf_keeps_heading_with_following_content() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><html><head><title>Pagination</title>
                <style>
                @page { size: 400px 600px; margin: 40px; }
                html, body { margin: 0; font: 16px/20px sans-serif; }
                h2, p { margin: 0; font: 16px/20px sans-serif; }
                </style></head><body>
                <div style="height:480px">Opening page</div>
                <h2>Section heading</h2>
                <p>Following explanation<br>Continues here<br>And ends here</p>
                </body></html>"#,
            ),
        )
        .await?;
    let directory = tempfile::tempdir()?;
    let offprint = Offprint::new()?;
    let captured = offprint
        .capture(server.url("/")?.as_str())?
        .bytes(1024 * 1024)
        .await?;
    let exported = offprint
        .artifacts()
        .export_capture(
            &captured,
            ExportRequest {
                schema_version: offprint::PUBLIC_SCHEMA_VERSION,
                output_directory: PortablePath::from_path_buf(directory.path().join("pdf"))?,
                base_name: "pagination".to_owned(),
                formats: vec![FormatSpec::Pdf(PdfOptions {
                    landscape: false,
                    prefer_css_page_size: true,
                })],
                conflict: ConflictPolicy::Fail,
            },
        )
        .await?;
    let document = lopdf::Document::load(exported.artifacts[0].path.as_std_path())?;
    assert_eq!(document.get_pages().len(), 2);
    let first = document.extract_text(&[1])?;
    let second = document.extract_text(&[2])?;
    assert!(first.contains("Opening page"));
    assert!(!first.contains("Section heading"));
    assert!(second.contains("Section heading"));
    assert!(second.contains("Following explanation"));
    offprint.close().await?;
    server.close().await;
    Ok(())
}

fn verify_pdf_semantics(path: &std::path::Path, source_digest: ContentDigest) -> TestResult {
    let bytes = std::fs::read(path)?;
    let metadata = lopdf::Document::load_metadata_mem(&bytes)?;
    assert_eq!(metadata.title.as_deref(), Some("Artifact formats"));
    assert_eq!(metadata.author.as_deref(), Some("Ada Lovelace"));
    assert_eq!(
        metadata.subject.as_deref(),
        Some("Portable artifact representations")
    );
    assert_eq!(
        metadata.keywords.as_deref(),
        Some("capture, metadata, accessibility")
    );
    assert!(
        metadata
            .creator
            .as_deref()
            .is_some_and(|creator| creator.starts_with("Offprint "))
    );
    assert!(metadata.creation_date.is_some());

    let document = lopdf::Document::load_mem(&bytes)?;
    let page_numbers = document.get_pages().keys().copied().collect::<Vec<_>>();
    let text = document.extract_text_with_limit(&page_numbers, 8 * 1024 * 1024)?;
    assert!(!text.trim().is_empty());
    let catalog = document.catalog()?;
    let annotations = document.get_pages().values().try_fold(
        Vec::new(),
        |mut annotations, page| -> TestResult<Vec<lopdf::Dictionary>> {
            let page = document.get_object(*page)?.as_dict()?;
            if let Ok(entries) = page.get(b"Annots").and_then(lopdf::Object::as_array) {
                for entry in entries {
                    let (_, annotation) = document.dereference(entry)?;
                    annotations.push(annotation.as_dict()?.clone());
                }
            }
            Ok(annotations)
        },
    )?;
    assert_eq!(
        annotations
            .iter()
            .filter(|annotation| annotation.has(b"Dest"))
            .count(),
        2
    );
    assert!(annotations.iter().any(|annotation| {
        annotation
            .get(b"A")
            .and_then(lopdf::Object::as_dict)
            .and_then(|action| action.get(b"URI"))
            .and_then(lopdf::decode_text_string)
            .is_ok_and(|uri| uri.starts_with("http://127.0.0.1:") && uri.ends_with("/details"))
    }));
    assert!(catalog.get(b"StructTreeRoot").is_ok());
    assert!(catalog.get(b"Outlines").is_ok());
    assert_eq!(
        catalog
            .get(b"Lang")
            .and_then(lopdf::decode_text_string)?
            .as_str(),
        "en-GB"
    );
    let metadata_stream = catalog
        .get(b"Metadata")
        .and_then(lopdf::Object::as_reference)
        .and_then(|id| document.get_object(id))
        .and_then(lopdf::Object::as_stream)?
        .get_plain_content()?;
    let xmp = String::from_utf8(metadata_stream)?;
    assert!(xmp.contains("Portable artifact representations"));
    assert!(xmp.contains("Ada Lovelace"));
    assert!(xmp.contains("<offprint:SourceFinalURL>"));
    assert!(xmp.contains("<offprint:HasTextStructure>true</offprint:HasTextStructure>"));
    assert!(xmp.contains(&source_digest.to_hex()));
    Ok(())
}

async fn verify_browser_file(
    path: &std::path::Path,
) -> TestResult<offprint_browser::OfflineBrowserObservation> {
    let executable = ChromiumDiscovery::new()
        .discover()
        .await
        .selected
        .and_then(|browser| browser.executable_path)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no browser"))?;
    let process = ChromiumProcess::launch(ChromiumLaunchOptions::new(executable)).await?;
    let page = process
        .new_page(&offprint::BrowserEnvironment::default())
        .await?;
    let url = Url::from_file_path(path)
        .map_err(|()| std::io::Error::other("format path is not a file URL"))?;
    let observation = page
        .verify_offline_url(
            &url,
            Duration::from_secs(20),
            offprint_browser::RenderingMedia::Screen,
        )
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
        .new_page(&offprint::BrowserEnvironment::default())
        .await?;
    let url = Url::from_file_path(path)
        .map_err(|()| std::io::Error::other("MHTML path is not a file URL"))?;
    let result = async {
        let observation = page
            .verify_offline_url(
                &url,
                Duration::from_secs(20),
                offprint_browser::RenderingMedia::Screen,
            )
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
        assert!(document.contains("<h1>Artifact formats</h1>"), "{document}");
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
