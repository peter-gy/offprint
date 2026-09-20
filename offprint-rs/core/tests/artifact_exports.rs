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

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn pdf_keeps_visual_figure_on_one_page() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><html><head><title>Figure pagination</title>
                <style>
                @page { size: A4; margin: 40px; }
                html, body { margin: 0; font: 16px/20px sans-serif; }
                .opening { height: 1000px; }
                .figure-row { height: 160px; }
                .figure-row::before { content: "Figure top"; display: block; height: 20px; }
                .figure-row::after { content: "Figure bottom"; display: block; height: 20px; }
                .figure-row p, .figure-row picture, .figure-row img {
                    display: block;
                    height: 120px;
                    margin: 0;
                    width: 120px;
                }
                </style></head><body>
                <div class="opening">Opening page</div>
                <div class="figure-row"><p><picture><img alt="Blue square"
                    src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='120' height='120'%3E%3Cpath fill='%230078d4' d='M0 0h120v120H0z'/%3E%3C/svg%3E">
                </picture></p></div>
                <p>Following explanation</p>
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
                base_name: "figure-pagination".to_owned(),
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
    assert!(!first.contains("Figure top"));
    assert!(second.contains("Figure top"));
    assert!(second.contains("Figure bottom"));
    assert!(second.contains("Following explanation"));
    offprint.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn pdf_scales_oversized_figure_to_one_page() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><html><head><title>Oversized figure</title>
                <style>
                @page { size: A4; margin: 40px; }
                html, body, h1, p, figure { margin: 0; font: 16px/20px sans-serif; }
                img { display: block; width: 400px; height: 1600px; }
                </style></head><body>
                <h1>Oversized figure</h1>
                <figure><img alt="Tall chart"
                    src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='400' height='1600'%3E%3Cpath fill='%230078d4' d='M0 0h400v1600H0z'/%3E%3C/svg%3E">
                </figure>
                <p>Following explanation</p>
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
                base_name: "oversized-figure".to_owned(),
                formats: vec![FormatSpec::Pdf(PdfOptions {
                    landscape: false,
                    prefer_css_page_size: true,
                })],
                conflict: ConflictPolicy::Fail,
            },
        )
        .await?;
    let document = lopdf::Document::load(exported.artifacts[0].path.as_std_path())?;
    let pages = document.get_pages().keys().copied().collect::<Vec<_>>();
    let text = document.extract_text(&pages)?;
    assert!(text.contains("Following explanation"));
    for (name, page_style, expected_width, expected_height, page_margin) in [
        (
            "large-margins",
            "size: A4; margin: 120px",
            595.0,
            842.0,
            90.0,
        ),
        (
            "landscape",
            "size: A4 landscape; margin: 80px",
            842.0,
            595.0,
            60.0,
        ),
        (
            "small-page",
            "size: 300px 400px; margin: 40px",
            225.0,
            300.0,
            30.0,
        ),
    ] {
        let html = format!(
            r#"<!doctype html><style>
            @page {{ {page_style}; }}
            html, body {{ margin: 0; font: 16px/20px sans-serif; }}
            figure {{ margin: 0; padding: 12px; border: 2px solid; }}
            svg {{ display: block; width: 400px; height: 1600px; max-width: 100%; }}
            p {{ margin: 0; }}
            </style><figure><svg xmlns="http://www.w3.org/2000/svg" width="400" height="1600" viewBox="0 0 400 1600">
            <rect width="400" height="1600" fill="lightblue"/>
            <a href="https://example.com/media-start"><text x="10" y="30" font-size="24">Media start</text></a>
            <a href="https://example.com/media-end"><text x="10" y="1580" font-size="24">Media end</text></a>
            </svg><figcaption><a href="https://example.com/caption-start">Caption first line</a><br>
            <a href="https://example.com/caption-end">Caption last line</a></figcaption></figure>
            <p>Following paragraph</p>"#
        );
        server
            .register(&format!("/{name}"), FixtureResponse::html(html))
            .await?;
        let captured = offprint
            .capture(server.url(&format!("/{name}"))?.as_str())?
            .bytes(1024 * 1024)
            .await?;
        let exported = offprint
            .artifacts()
            .export_capture(
                &captured,
                ExportRequest {
                    schema_version: offprint::PUBLIC_SCHEMA_VERSION,
                    output_directory: PortablePath::from_path_buf(directory.path().join(name))?,
                    base_name: name.to_owned(),
                    formats: vec![FormatSpec::Pdf(PdfOptions {
                        landscape: false,
                        prefer_css_page_size: true,
                    })],
                    conflict: ConflictPolicy::Fail,
                },
            )
            .await?;
        let document = lopdf::Document::load(exported.artifacts[0].path.as_std_path())?;
        let first_page = document.get_pages()[&1];
        let bounds = document
            .get_object(first_page)?
            .as_dict()?
            .get(b"MediaBox")?
            .as_array()?;
        assert!(
            (f64::from(bounds[2].as_float()?) - expected_width).abs() < 1.0,
            "{name}"
        );
        assert!(
            (f64::from(bounds[3].as_float()?) - expected_height).abs() < 1.0,
            "{name}"
        );
        let first = document.extract_text(&[1])?;
        for marker in [
            "Media start",
            "Media end",
            "Caption first line",
            "Caption last line",
        ] {
            assert!(
                first.contains(marker),
                "{name}: {marker} missing from figure page: {first}"
            );
        }
        let annotations = document
            .get_object(first_page)?
            .as_dict()?
            .get(b"Annots")?
            .as_array()?;
        assert_eq!(annotations.len(), 4, "{name}");
        for annotation in annotations {
            let (_, annotation) = document.dereference(annotation)?;
            let rect = annotation.as_dict()?.get(b"Rect")?.as_array()?;
            let coordinates = rect
                .iter()
                .map(lopdf::Object::as_float)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            assert!(
                f64::from(coordinates[0]) >= page_margin - 1.0,
                "{name}: {coordinates:?}"
            );
            assert!(
                f64::from(coordinates[1]) >= page_margin - 1.0,
                "{name}: {coordinates:?}"
            );
            assert!(
                f64::from(coordinates[2]) <= expected_width - page_margin + 1.0,
                "{name}: {coordinates:?}"
            );
            assert!(
                f64::from(coordinates[3]) <= expected_height - page_margin + 1.0,
                "{name}: {coordinates:?}"
            );
        }
        assert_eq!(document.get_pages().len(), 2, "{name}");
        assert!(
            document.extract_text(&[2])?.contains("Following paragraph"),
            "{name}"
        );
    }
    offprint.close().await?;
    verify_pdf_layout_invariance(&server).await?;
    server.close().await;
    Ok(())
}

async fn verify_pdf_layout_invariance(server: &FixtureServer) -> TestResult {
    let executable = ChromiumDiscovery::new()
        .discover()
        .await
        .selected
        .and_then(|browser| browser.executable_path)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no browser"))?;
    let process = ChromiumProcess::launch(ChromiumLaunchOptions::new(executable)).await?;
    let image = r#"<img id="image" src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='24' height='48'%3E%3Crect width='24' height='48' fill='blue'/%3E%3C/svg%3E">"#;
    let metrics = r#"Array.from(document.querySelectorAll('[id]'), element => {
        const box = element.getBoundingClientRect();
        return { id: element.id, x: box.x, y: box.y, width: box.width, height: box.height,
            boxSizing: getComputedStyle(element).boxSizing };
    })"#;
    let result = async {
        for (name, css, body) in [
            ("content-box-caption-minimum", "figure{width:300px;padding:12px;border:2px solid}img{display:block}figcaption{min-height:80px}", image.to_owned()),
            ("intrinsic-inline", "figure{width:300px}", image.to_owned()),
            ("authored-stretch", "figure{width:300px}img{display:block;width:48px;height:48px}", image.to_owned()),
            ("grid-picture", "figure{width:300px}picture{display:grid;grid-template-columns:100px 100px}img{display:block}", format!("<picture id=picture>{image}</picture>")),
            ("relative-cap", "figure{width:300px}img{display:block;max-height:50vh;height:1600px;width:24px}", image.to_owned()),
        ] {
            let route = format!("/invariance-{name}");
            server.register(&route, FixtureResponse::html(format!(
                "<!doctype html><style>body{{margin:0;font:16px/20px sans-serif}}figure{{margin:0}}{css}</style><figure id=figure>{body}<figcaption id=caption>Caption</figcaption></figure>"
            ))).await?;
            let page = process.new_page(&offprint::BrowserEnvironment::default()).await?;
            let viewport = serde_json::json!({
                "width": 800, "height": 900, "deviceScaleFactor": 1, "mobile": false,
            });
            process.client().command("Emulation.setDeviceMetricsOverride", viewport.clone(), Some(page.session_id())).await?;
            let url = server.url(&route)?;
            page.navigate(&url, offprint::ReadinessMode::Load, 20, Duration::from_secs(10)).await?;
            page.evaluate("Promise.all(Array.from(document.images, image => image.decode()))").await?;
            let before = page.evaluate(metrics).await?;
            let printed = page.print_to_pdf(&url, false, true, 1024 * 1024).await?;
            assert!(!printed.is_empty(), "{name}");
            // Printing resolves viewport lengths against the page area. Re-enter
            // the same screen viewport before comparing source-layout geometry.
            process.client().command("Emulation.clearDeviceMetricsOverride", serde_json::json!({}), Some(page.session_id())).await?;
            process.client().command("Emulation.setDeviceMetricsOverride", viewport, Some(page.session_id())).await?;
            assert_eq!(page.evaluate(metrics).await?, before, "{name}");
            if name == "content-box-caption-minimum" {
                assert_eq!(page.evaluate("getComputedStyle(document.querySelector('figcaption')).minHeight").await?.as_str(), Some("80px"));
            }
            if name == "grid-picture" {
                assert_eq!(page.evaluate("getComputedStyle(document.querySelector('picture')).display").await?.as_str(), Some("grid"));
            }
            if name == "authored-stretch" {
                assert_eq!(page.evaluate("getComputedStyle(document.querySelector('img')).objectFit").await?.as_str(), Some("fill"));
            }
            if name == "relative-cap" {
                process.client().command("Emulation.setDeviceMetricsOverride", serde_json::json!({
                    "width": 800, "height": 400, "deviceScaleFactor": 1, "mobile": false,
                }), Some(page.session_id())).await?;
                assert_eq!(page.evaluate("getComputedStyle(document.querySelector('img')).maxHeight").await?.as_str(), Some("200px"));
            }
            page.close().await?;
        }
        TestResult::Ok(())
    }.await;
    let close = process.close().await;
    result?;
    close?;
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
