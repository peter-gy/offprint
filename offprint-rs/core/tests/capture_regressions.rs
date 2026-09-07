use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use chrono::{DateTime, Utc};
use data_url::DataUrl;
use offprint::{
    BrowserEnvironment, CaptureArtifact, CaptureOutput, CaptureRequest, Clock, NetworkPolicy,
    Offprint, ReadinessMode, ResourceOutcome, ResourceRetrievalSource,
};
use offprint_chromium::{ChromiumDiscovery, ChromiumLaunchOptions, ChromiumProcess};
use offprint_test_support::{FixtureResponse, FixtureServer};

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug)]
struct FixedClock(DateTime<Utc>);

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn tainted_canvas_uses_a_clipped_browser_fallback() -> TestResult {
    let page_server = FixtureServer::start().await?;
    let image_server = FixtureServer::start().await?;
    image_server
        .register(
            "/pixel.svg",
            response(
                "image/svg+xml",
                br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="rgb(11,99,211)"/></svg>"#,
            ),
        )
        .await?;
    let image_url = image_server.url("/pixel.svg")?;
    page_server
        .register(
            "/",
            FixtureResponse::html(format!(
                r#"<!doctype html><title>Tainted canvas</title>
                <canvas id="plot" width="16" height="16"></canvas>
                <script>
                const image = new Image();
                image.onload = () => {{
                  document.getElementById("plot").getContext("2d").drawImage(image, 0, 0);
                  document.body.dataset.ready = "true";
                }};
                image.src = {image_url:?};
                </script>"#
            )),
        )
        .await?;
    let mut request = CaptureRequest::builder(page_server.url("/")?.as_str())?.build()?;
    request.network = NetworkPolicy::Unrestricted;
    request.output = CaptureOutput::memory(4 * 1024 * 1024);
    let offprint = Offprint::builder().build()?;

    let result = offprint.captures().start(request).await?.result().await?;
    let content = artifact_bytes(&result)?;
    let manifest = offprint_html::inspect_html(content)?;

    assert_eq!(result.resources.discovered, 1);
    assert!(result.warnings.iter().all(|warning| {
        !matches!(
            warning.code.as_str(),
            "offprint.canvas.capture_failed" | "offprint.canvas.capture_unavailable"
        )
    }));
    assert!(matches!(
        manifest.resource_records[0].outcome,
        ResourceOutcome::Embedded {
            ref media_type,
            ..
        } if media_type == "image/png"
    ));
    assert_eq!(
        manifest.resource_records[0]
            .provenance
            .as_ref()
            .map(|provenance| provenance.source),
        Some(ResourceRetrievalSource::InlineData)
    );
    let html = String::from_utf8_lossy(content);
    assert!(html.contains("data-offprint-canvas"), "{html}");
    assert!(html.contains("data:image/png;base64,"), "{html}");

    offprint.close().await?;
    page_server.close().await;
    image_server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn webgl_canvas_preserves_composited_pixels() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>WebGL canvas</title>
                <canvas id="plot" width="64" height="64" style="display:block"></canvas>
                <script>
                const gl = document.getElementById("plot").getContext("webgl");
                gl.clearColor(0.1, 0.6, 0.3, 1);
                gl.clear(gl.COLOR_BUFFER_BIT);
                </script>"#,
            ),
        )
        .await?;
    let offprint = Offprint::builder().build()?;

    let result = offprint
        .capture(server.url("/")?.as_str())?
        .bytes(4 * 1024 * 1024)
        .await?;
    let content = artifact_bytes(&result)?;
    let pixel = captured_png_center(content)?;

    assert_green_pixel(pixel);
    offprint.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn inline_webgl_fallback_identity_ignores_matching_page_text() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/child",
            FixtureResponse::html(
                r#"<!doctype html><title>Inline fallback child</title>
                <p>data-offprint-visual-fallback="0"</p>
                <canvas id="plot" width="64" height="64" style="display:block"></canvas>
                <script>
                const gl = document.getElementById("plot").getContext("webgl");
                gl.clearColor(0.1, 0.6, 0.3, 1);
                gl.clear(gl.COLOR_BUFFER_BIT);
                </script>"#,
            ),
        )
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Inline fallback parent</title>
                <iframe src="/child" width="96" height="128"></iframe>"#,
            ),
        )
        .await?;
    let offprint = Offprint::builder().build()?;

    let result = offprint
        .capture(server.url("/")?.as_str())?
        .bytes(4 * 1024 * 1024)
        .await?;
    let pixel = captured_png_center(artifact_bytes(&result)?)?;

    assert_green_pixel(pixel);
    offprint.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn oopif_webgl_canvas_preserves_composited_pixels() -> TestResult {
    let parent_server = FixtureServer::start().await?;
    let child_server = FixtureServer::start().await?;
    child_server
        .register(
            "/plot",
            FixtureResponse::html(
                r#"<!doctype html><title>Child WebGL canvas</title>
                <style>
                html, body { margin: 0 }
                #scroll { width: 96px; height: 96px; overflow: auto }
                #spacer { height: 400px }
                </style>
                <div id="scroll"><div id="spacer"></div><div id="host"></div></div>
                <script>
                const root = document.getElementById("host").attachShadow({mode: "closed"});
                root.innerHTML =
                  '<canvas id="plot" width="64" height="64" style="display:block"></canvas>';
                const gl = root.getElementById("plot").getContext("webgl");
                gl.clearColor(0.1, 0.6, 0.3, 1);
                gl.clear(gl.COLOR_BUFFER_BIT);
                </script>"#,
            ),
        )
        .await?;
    let mut child_url = child_server.url("/plot")?;
    child_url.set_host(Some("localhost"))?;
    parent_server
        .register(
            "/",
            FixtureResponse::html(format!(
                r#"<!doctype html><title>Parent WebGL frame</title>
                <iframe src="{child_url}" width="96" height="96"></iframe>"#
            )),
        )
        .await?;
    let mut request = CaptureRequest::builder(parent_server.url("/")?.as_str())?.build()?;
    request.network = NetworkPolicy::Unrestricted;
    request.output = CaptureOutput::memory(4 * 1024 * 1024);
    let offprint = Offprint::builder().build()?;

    let result = offprint.captures().start(request).await?.result().await?;

    assert!(
        result.warnings.iter().all(|warning| {
            !matches!(
                warning.code.as_str(),
                "offprint.canvas.capture_failed" | "offprint.canvas.capture_unavailable"
            )
        }),
        "{:?}",
        result.warnings
    );
    let pixel = captured_png_center(artifact_bytes(&result)?)?;
    assert_green_pixel(pixel);
    offprint.close().await?;
    parent_server.close().await;
    child_server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn malformed_browser_dom_reopens_with_the_observed_tree() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Structural repair</title>
                <main id="mount"></main>
                <script>
                const outer = document.createElement("p");
                outer.id = "outer";
                const inner = document.createElement("div");
                inner.id = "inner";
                inner.textContent = "browser-owned nesting";
                outer.append(inner);
                document.getElementById("mount").append(outer);
                </script>"#,
            ),
        )
        .await?;
    let offprint = Offprint::builder().build()?;
    let result = offprint
        .capture(server.url("/")?.as_str())?
        .bytes(4 * 1024 * 1024)
        .await?;
    let content = artifact_bytes(&result)?.to_vec();
    let manifest = offprint_html::inspect_html(&content)?;

    assert!(manifest.structural_repair.applied);
    let html = String::from_utf8_lossy(&content);
    assert!(html.contains("offprint-repair-data"), "{html}");
    assert!(html.contains("offprint-repair-script"), "{html}");
    offprint.close().await?;

    let encoded = base64::engine::general_purpose::STANDARD.encode(&content);
    let artifact_url = url::Url::parse(&format!("data:text/html;base64,{encoded}"))?;
    let executable = ChromiumDiscovery::new()
        .discover()
        .await
        .selected
        .and_then(|browser| browser.executable_path)
        .ok_or("no compatible Chromium browser was discovered")?;
    let process = ChromiumProcess::launch(ChromiumLaunchOptions::new(executable)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &artifact_url,
        ReadinessMode::Load,
        0,
        Duration::from_secs(10),
    )
    .await?;
    let repaired = page
        .evaluate(
            "document.querySelector('#outer > #inner') !== null && !document.querySelector('[data-offprint-node]')",
        )
        .await?;

    assert_eq!(repaired.as_bool(), Some(true));
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn detached_oopif_returns_a_typed_frame_outcome() -> TestResult {
    let parent_server = FixtureServer::start().await?;
    let child_server = FixtureServer::start().await?;
    child_server
        .register(
            "/child",
            FixtureResponse::html("<main>short-lived cross-site frame</main>"),
        )
        .await?;
    let mut child_url = child_server.url("/child")?;
    child_url.set_host(Some("localhost"))?;
    parent_server
        .register(
            "/",
            FixtureResponse::html(format!(
                r#"<title>Detached frame</title>
                <iframe id="child" src="{child_url}"></iframe>
                <script>
                document.getElementById("child").addEventListener("load", (event) => {{
                  const frame = event.currentTarget;
                  setTimeout(() => frame.remove(), 50);
                }});
                </script>"#
            )),
        )
        .await?;
    let mut request = CaptureRequest::builder(parent_server.url("/")?.as_str())?.build()?;
    request.network = NetworkPolicy::Unrestricted;
    request.output = CaptureOutput::memory(2 * 1024 * 1024);
    let offprint = Offprint::builder().build()?;

    let Err(error) = offprint.captures().start(request).await?.result().await else {
        return Err(
            std::io::Error::other("a detached attached frame completed successfully").into(),
        );
    };

    assert_eq!(error.code.as_str(), "offprint.frame.detached");
    offprint.close().await?;
    parent_server.close().await;
    child_server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn mixed_inline_and_oopif_branches_share_the_frame_budget() -> TestResult {
    let parent_server = FixtureServer::start().await?;
    let child_server = FixtureServer::start().await?;
    parent_server
        .register(
            "/inline",
            FixtureResponse::html("<main>parent inline frame</main>"),
        )
        .await?;
    child_server
        .register(
            "/nested",
            FixtureResponse::html("<main>oopif inline frame</main>"),
        )
        .await?;
    child_server
        .register(
            "/child",
            FixtureResponse::html(
                r#"<main>cross-site frame</main><iframe src="/nested"></iframe>"#,
            ),
        )
        .await?;
    let mut child_url = child_server.url("/child")?;
    child_url.set_host(Some("localhost"))?;
    parent_server
        .register(
            "/",
            FixtureResponse::html(format!(
                r#"<!doctype html><title>Mixed frame budget</title>
                <iframe src="/inline"></iframe>
                <iframe src="{child_url}"></iframe>"#
            )),
        )
        .await?;
    let mut request = CaptureRequest::builder(parent_server.url("/")?.as_str())?.build()?;
    request.network = NetworkPolicy::Unrestricted;
    request.limits.frames = 3;
    request.output = CaptureOutput::memory(2 * 1024 * 1024);
    let offprint = Offprint::builder().build()?;

    let result = offprint.captures().start(request).await?.result().await;

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.frame.limit")
    );
    offprint.close().await?;
    parent_server.close().await;
    child_server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn nested_oopif_replaces_its_owner_inside_the_inline_frame() -> TestResult {
    let parent_server = FixtureServer::start().await?;
    let child_server = FixtureServer::start().await?;
    child_server
        .register(
            "/target",
            FixtureResponse::html("<main>nested cross-site target</main>"),
        )
        .await?;
    let mut target_url = child_server.url("/target")?;
    target_url.set_host(Some("localhost"))?;
    parent_server
        .register(
            "/outer",
            FixtureResponse::html(format!(
                r#"<!doctype html><title>Outer frame</title>
                <main>outer frame shell</main>
                <iframe srcdoc="<p>nested decoy</p>"></iframe>
                <iframe src="{target_url}"></iframe>
                <script>
                  const fail = () => {{ throw new Error("page prototype poisoned"); }};
                  Document.prototype.querySelectorAll = fail;
                  Array.prototype.push = fail;
                  Array.prototype.reverse = fail;
                </script>"#
            )),
        )
        .await?;
    parent_server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Nested OOPIF</title>
                <iframe srcdoc="<p>root decoy</p>"></iframe>
                <iframe src="/outer"></iframe>"#,
            ),
        )
        .await?;
    let mut request = CaptureRequest::builder(parent_server.url("/")?.as_str())?.build()?;
    request.network = NetworkPolicy::Unrestricted;
    request.output = CaptureOutput::memory(4 * 1024 * 1024);
    let mut shallow_request = request.clone();
    shallow_request.limits.frame_depth = 1;
    let offprint = Offprint::builder().build()?;

    let result = offprint.captures().start(request).await?.result().await?;
    let content = artifact_bytes(&result)?;
    let document = offprint_document::Document::parse(content);
    let outer = document
        .inline_frames()
        .into_iter()
        .find(|frame| frame.html.contains("outer frame shell"))
        .ok_or("captured outer frame is missing")?;
    let nested_frames = offprint_document::Document::parse(outer.html.as_bytes()).inline_frames();

    assert_eq!(offprint_html::inspect_html(content)?.frames, 5);
    assert!(String::from_utf8_lossy(content).contains("root decoy"));
    assert!(
        nested_frames
            .iter()
            .any(|frame| { frame.captured && frame.html.contains("nested cross-site target") })
    );
    assert!(
        nested_frames
            .iter()
            .any(|frame| frame.html.contains("nested decoy"))
    );
    let shallow = offprint
        .captures()
        .start(shallow_request)
        .await?
        .result()
        .await;
    assert_eq!(
        shallow.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.frame.depth")
    );
    offprint.close().await?;
    parent_server.close().await;
    child_server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn request_variants_keep_distinct_resource_bytes() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register_handler("/variant.svg", |request| {
            let color = match request.query.as_deref() {
                Some("variant=blue") => "blue",
                Some("variant=green") => "green",
                _ => "red",
            };
            response(
                "image/svg+xml",
                format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4" fill="{color}"/></svg>"#
                ),
            )
            .with_header("Cache-Control", "public, max-age=3600")
            .with_header("Vary", "X-Fixture-Variant")
            .into()
        })
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Cache variants</title>
                <img src="/variant.svg?variant=blue">
                <img src="/variant.svg?variant=green">"#,
            ),
        )
        .await?;
    let offprint = Offprint::builder().build()?;
    let result = offprint
        .capture(server.url("/")?.as_str())?
        .bytes(4 * 1024 * 1024)
        .await?;
    let manifest = offprint_html::inspect_html(artifact_bytes(&result)?)?;
    let digests = manifest
        .resource_records
        .iter()
        .filter_map(|record| match record.outcome {
            ResourceOutcome::Embedded { digest, .. } => Some(digest),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(manifest.resource_records.len(), 2);
    assert_eq!(digests.len(), 2);
    offprint.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn redirect_address_policy_rejects_a_disallowed_host() -> TestResult {
    let server = FixtureServer::start().await?;
    let redirect = format!("http://localhost:{}/private", server.address().port());
    server
        .register(
            "/redirect",
            FixtureResponse::text(Vec::new())
                .with_status(302)
                .with_header("Location", redirect),
        )
        .await?;
    server
        .register("/private", FixtureResponse::html("<h1>private</h1>"))
        .await?;
    let offprint = Offprint::builder().build()?;

    let Err(error) = offprint
        .capture(server.url("/redirect")?.as_str())?
        .bytes(1024 * 1024)
        .await
    else {
        return Err(std::io::Error::other("a public-to-private redirect was accepted").into());
    };

    assert_eq!(error.code.as_str(), "offprint.navigation.address_blocked");
    offprint.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn fixed_capture_inputs_serialize_identical_artifacts() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                "<!doctype html><html><head><title>Stable</title></head><body><p>same</p></body></html>",
            ),
        )
        .await?;
    let timestamp = DateTime::parse_from_rfc3339("2026-07-27T12:34:56Z")?.with_timezone(&Utc);
    let offprint = Offprint::builder()
        .clock(Arc::new(FixedClock(timestamp)))
        .build()?;
    let url = server.url("/")?;

    let first = offprint
        .capture(url.as_str())?
        .bytes(2 * 1024 * 1024)
        .await?;
    let second = offprint
        .capture(url.as_str())?
        .bytes(2 * 1024 * 1024)
        .await?;

    assert_eq!(artifact_bytes(&first)?, artifact_bytes(&second)?);
    offprint.close().await?;
    server.close().await;
    Ok(())
}

fn artifact_bytes(result: &offprint::CaptureReceipt) -> TestResult<&[u8]> {
    match &result.artifact {
        CaptureArtifact::Bytes { content, .. } => Ok(content),
        CaptureArtifact::File { .. } => {
            Err(std::io::Error::other("capture returned an unexpected file artifact").into())
        }
    }
}

fn captured_png_center(content: &[u8]) -> TestResult<[u8; 4]> {
    let base_url = url::Url::parse("https://artifact.invalid/")?;
    let mut documents = vec![offprint_document::Document::parse(content)];
    while let Some(document) = documents.pop() {
        let resources = offprint_document::discover_document_resources(&document, &base_url)?;
        if let Some(image_url) = resources
            .resources()
            .iter()
            .find(|resource| resource.resolved_url.as_str().starts_with("data:image/png"))
            .map(|resource| resource.resolved_url.as_str())
        {
            let (bytes, _) = DataUrl::process(image_url)?.decode_to_vec()?;
            let pixels = image::load_from_memory(&bytes)?.to_rgba8();
            return Ok(pixels.get_pixel(pixels.width() / 2, pixels.height() / 2).0);
        }
        documents.extend(
            document
                .inline_frames()
                .into_iter()
                .map(|frame| offprint_document::Document::parse(frame.html.as_bytes())),
        );
    }
    Err(std::io::Error::other("captured WebGL pixels are absent").into())
}

fn assert_green_pixel(pixel: [u8; 4]) {
    assert!(
        pixel[1] > pixel[0].saturating_add(50)
            && pixel[1] > pixel[2].saturating_add(50)
            && pixel[3] == 255,
        "captured WebGL center pixel was {pixel:?}"
    );
}

fn response(content_type: &str, body: impl Into<Vec<u8>>) -> FixtureResponse {
    FixtureResponse {
        status: 200,
        content_type: content_type.to_owned(),
        headers: Default::default(),
        body: body.into(),
    }
}
