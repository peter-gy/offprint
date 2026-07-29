use std::error::Error;
use std::time::Duration;

use base64::Engine as _;
use image::ImageReader;
use pageknot::{
    ArtifactResult, ArtifactSpec, BrowserCookie, BrowserEnvironment, CapturePolicy, CaptureRequest,
    CaptureResult, CaptureScope, CaptureStatus, CaptureTerminalStatus, ConflictPolicy,
    ContentDigest, Milliseconds, MissingResourcePolicy, NetworkPolicy, OptimizationPolicy,
    PageKnot, PortablePath, ReadinessMode, RequestHeader, SecretString,
};
use pageknot_chromium::{ChromiumDiscovery, ChromiumLaunchOptions, ChromiumProcess};
use pageknot_test_support::{FixtureResponse, FixtureRoute, FixtureServer};
use url::Url;

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug)]
struct CapturedArtifact {
    result: CaptureResult,
    content: Vec<u8>,
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn static_article_with_font_round_trips() -> TestResult {
    let server = FixtureServer::start().await?;
    register_svg(&server, "/article-small.svg", "rgb(40, 90, 150)").await?;
    register_svg(&server, "/article-large.svg", "rgb(20, 130, 90)").await?;
    let font = base64::engine::general_purpose::STANDARD.decode(TEST_FONT_WOFF2_BASE64)?;
    server
        .register("/fixture.woff2", response("font/woff2", font))
        .await?;
    server
        .register(
            "/article.css",
            response(
                "text/css",
                b"@font-face{font-family:Fixture;src:url('/fixture.woff2') format('woff2')}article{font-family:Fixture,serif}",
            ),
        )
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><html><head><title>Static article</title>
                <link rel="stylesheet" href="/article.css"></head><body>
                <article><h1>(</h1><p>captured article</p>
                <img alt="responsive article image" src="/article-small.svg"
                  srcset="/article-small.svg 400w, /article-large.svg 1200w"
                  sizes="100vw"></article>
                <output id="font-state"></output>
                <script>
                  document.fonts.load("24px Fixture", "(").then((fonts) => {
                    document.getElementById("font-state").textContent =
                      `font faces ${fonts.length}`;
                  });
                </script></body></html>"#,
            ),
        )
        .await?;
    let pageknot = PageKnot::builder().build()?;

    let captured = capture_bytes(&pageknot, server.url("/")?, NetworkPolicy::Standard).await?;

    assert_success(&captured);
    let html = String::from_utf8_lossy(&captured.content);
    assert!(html.contains("captured article"), "{html}");
    assert!(html.contains("font faces 1"), "{html}");
    let manifest = pageknot_html::inspect_html(&captured.content)?;
    assert!(manifest.resource_records.iter().any(|record| {
        matches!(
            &record.outcome,
            pageknot::ResourceOutcome::Embedded { media_type, .. }
                if media_type == "font/woff2"
        )
    }));
    assert!(
        server
            .requests()
            .await
            .iter()
            .any(|request| request.path == "/article-large.svg")
    );

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn external_stylesheet_resources_resolve_from_stylesheet_url() -> TestResult {
    let server = FixtureServer::start().await?;
    let font = base64::engine::general_purpose::STANDARD.decode(TEST_FONT_WOFF2_BASE64)?;
    server
        .register("/styles/fixture.woff2", response("font/woff2", font))
        .await?;
    register_svg(&server, "/styles/background.svg", "rgb(30, 120, 180)").await?;
    server
        .register(
            "/styles/theme.css",
            response(
                "text/css",
                br#"@font-face{font-family:Nested;src:url("fixture.woff2") format("woff2")}
                article{font-family:Nested,serif;background-image:url("background.svg")}"#,
            ),
        )
        .await?;
    server
        .register(
            "/pages/index.html",
            FixtureResponse::html(
                r#"<!doctype html><html><head><title>Stylesheet base</title>
                <link rel="stylesheet" href="/styles/theme.css"></head>
                <body><article>stylesheet-relative resources</article></body></html>"#,
            ),
        )
        .await?;
    let pageknot = PageKnot::builder().build()?;

    let captured = capture_bytes(
        &pageknot,
        server.url("/pages/index.html")?,
        NetworkPolicy::Standard,
    )
    .await?;

    assert_success(&captured);
    assert_eq!(captured.result.resources.failed, 0);
    let manifest = pageknot_html::inspect_html(&captured.content)?;
    for suffix in ["/styles/fixture.woff2", "/styles/background.svg"] {
        assert!(manifest.resource_records.iter().any(|record| {
            record.requested_url.as_str().ends_with(suffix)
                && matches!(record.outcome, pageknot::ResourceOutcome::Embedded { .. })
        }));
    }

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn repeated_missing_resource_loads_stop_after_the_first_batch() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/missing.svg",
            response("image/svg+xml", Vec::new()).with_status(404),
        )
        .await?;
    let images = (0..24)
        .map(|index| format!(r#"<img src="/missing.svg" alt="missing {index}">"#))
        .collect::<String>();
    server
        .register(
            "/",
            FixtureResponse::html(format!(
                "<!doctype html><title>Repeated missing resource</title>{images}"
            )),
        )
        .await?;
    let pageknot = PageKnot::builder().build()?;

    let captured = capture_bytes(&pageknot, server.url("/")?, NetworkPolicy::Standard).await?;

    assert_success(&captured);
    assert_eq!(captured.result.resources.discovered, 24);
    assert_eq!(captured.result.resources.failed, 24);
    let loads = server
        .requests()
        .await
        .iter()
        .filter(|request| request.path == "/missing.svg")
        .count();
    assert!(
        loads <= 9,
        "missing resource was requested {loads} times across an eight-resource batch"
    );

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn adopted_stylesheet_reuses_the_document_font_face() -> TestResult {
    let server = FixtureServer::start().await?;
    let font = base64::engine::general_purpose::STANDARD.decode(TEST_FONT_WOFF2_BASE64)?;
    server
        .register("/styles/fixture.woff2", response("font/woff2", font))
        .await?;
    server
        .register(
            "/styles/fonts.css",
            response(
                "text/css",
                br#"@font-face{font-family:Nested;src:url("fixture.woff2") format("woff2")}
                .outer{font-family:Nested,serif}"#,
            ),
        )
        .await?;
    server
        .register(
            "/pages/adopted.html",
            FixtureResponse::html(
                r#"<!doctype html><html><head><title>Adopted font face</title>
                <link rel="stylesheet" href="/styles/fonts.css"></head>
                <body><span class="outer">(</span><section id="host"></section>
                <script>
                  const source = [...document.styleSheets[0].cssRules]
                    .find((rule) => rule.type === CSSRule.FONT_FACE_RULE);
                  const sheet = new CSSStyleSheet();
                  sheet.replaceSync(`${source.cssText}
                    :host { font-family: Nested, serif; }`);
                  const root = document.getElementById("host").attachShadow({mode: "open"});
                  root.adoptedStyleSheets = [sheet];
                  root.innerHTML = "<strong>shadow font content</strong>";
                </script></body></html>"#,
            ),
        )
        .await?;
    let pageknot = PageKnot::builder().build()?;

    let captured = capture_bytes(
        &pageknot,
        server.url("/pages/adopted.html")?,
        NetworkPolicy::Standard,
    )
    .await?;

    assert_success(&captured);
    assert_eq!(captured.result.resources.discovered, 1);
    assert_eq!(captured.result.resources.failed, 0);
    let html = String::from_utf8_lossy(&captured.content);
    assert!(html.contains("shadow font content"), "{html}");
    assert!(html.contains("data-pageknot-adopted"), "{html}");

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn adopted_stylesheets_preserve_their_cascade_order() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><html><head><title>Adopted cascade</title>
                <style>#light { color: rgb(10, 20, 30) }</style></head>
                <body><span id="light">light tree</span>
                <style>#light { color: rgb(40, 50, 60) }</style>
                <section id="host"></section>
                <script>
                  const first = new CSSStyleSheet();
                  first.replaceSync(`
                    #light { color: rgb(70, 80, 90) }
                    #shadow { color: rgb(70, 80, 90) }
                  `);
                  const second = new CSSStyleSheet();
                  second.replaceSync(`
                    #light { color: rgb(100, 110, 120) }
                    #shadow { color: rgb(100, 110, 120) }
                  `);
                  document.adoptedStyleSheets = [first, second];
                  const root = document.getElementById("host").attachShadow({mode: "open"});
                  root.innerHTML =
                    '<style>#shadow { color: rgb(10, 20, 30) }</style>' +
                    '<span id="shadow">shadow tree</span>';
                  root.adoptedStyleSheets = [first, second];
                </script></body></html>"#,
            ),
        )
        .await?;
    let pageknot = PageKnot::builder().build()?;

    let captured = capture_bytes(&pageknot, server.url("/")?, NetworkPolicy::Standard).await?;
    let directory = tempfile::tempdir()?;
    let artifact = directory.path().join("adopted-cascade.html");
    std::fs::write(&artifact, &captured.content)?;
    let colors = evaluate_file(
        &artifact,
        r#"({
          light: getComputedStyle(document.getElementById("light")).color,
          shadow: getComputedStyle(
            document.getElementById("host").shadowRoot.getElementById("shadow")
          ).color
        })"#,
    )
    .await?;

    assert_success(&captured);
    assert_eq!(
        colors,
        serde_json::json!({
            "light": "rgb(100, 110, 120)",
            "shadow": "rgb(100, 110, 120)"
        })
    );
    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn browser_state_fixture_round_trips() -> TestResult {
    let server = FixtureServer::start().await?;
    register_svg(&server, "/small.svg", "rgb(170, 20, 20)").await?;
    register_svg(&server, "/large.svg", "rgb(20, 120, 210)").await?;
    register_svg(&server, "/lazy.svg", "rgb(40, 160, 80)").await?;
    register_svg(&server, "/poster.svg", "rgb(90, 50, 180)").await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r##"<!doctype html>
                <html>
                <head><title>Browser state matrix</title></head>
                <body>
                  <output id="script-state"></output>
                  <output id="mutation-state">pending</output>
                  <section id="open-host"></section>
                  <section id="closed-host"></section>
                  <input id="text" value="before">
                  <input id="check" type="checkbox">
                  <textarea id="notes">before</textarea>
                  <select id="choice"><option>first</option><option>second</option></select>
                  <details id="disclosure"><summary>State</summary><p>open body</p></details>
                  <picture>
                    <img id="responsive" alt="responsive" src="/small.svg"
                      srcset="/small.svg 400w, /large.svg 1200w" sizes="100vw">
                  </picture>
                  <div style="height: 1400px"></div>
                  <img id="lazy" alt="lazy">
                  <img id="inline-data" alt="inline data"
                    src="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='2' height='2'%3E%3C/svg%3E">
                  <img id="blob-image" alt="blob">
                  <canvas id="canvas-2d" width="12" height="12"></canvas>
                  <canvas id="canvas-webgl" width="12" height="12"></canvas>
                  <video id="poster-video" poster="/poster.svg" width="12" height="12"></video>
                  <script>
                    document.getElementById("script-state").textContent = "script rendered";
                    setTimeout(() => {
                      document.getElementById("mutation-state").textContent = "mutation one";
                    }, 30);
                    setTimeout(() => {
                      document.getElementById("mutation-state").textContent = "mutation settled";
                    }, 60);

                    const openRoot = document.getElementById("open-host").attachShadow({mode: "open"});
                    openRoot.innerHTML = "<strong id='open-state'>open shadow</strong>";
                    const sheet = new CSSStyleSheet();
                    sheet.replaceSync("#open-state { color: rgb(17, 34, 51) }");
                    openRoot.adoptedStyleSheets = [sheet];

                    const closedRoot = document.getElementById("closed-host").attachShadow({mode: "closed"});
                    closedRoot.innerHTML = "<strong>closed shadow</strong>";

                    document.getElementById("text").value = "after";
                    document.getElementById("check").checked = true;
                    document.getElementById("notes").value = "current notes";
                    document.getElementById("choice").selectedIndex = 1;
                    document.getElementById("disclosure").open = true;

                    const drawing = document.getElementById("canvas-2d").getContext("2d");
                    drawing.fillStyle = "rgb(200, 30, 40)";
                    drawing.fillRect(0, 0, 12, 12);

                    const gl = document.getElementById("canvas-webgl").getContext("webgl");
                    if (gl) {
                      gl.clearColor(0.1, 0.6, 0.3, 1);
                      gl.clear(gl.COLOR_BUFFER_BIT);
                    }

                    const blob = new Blob([
                      "<svg xmlns='http://www.w3.org/2000/svg' width='3' height='3'><rect width='3' height='3' fill='orange'/></svg>"
                    ], {type: "image/svg+xml"});
                    document.getElementById("blob-image").src = URL.createObjectURL(blob);

                    const lazy = document.getElementById("lazy");
                    new IntersectionObserver((entries, observer) => {
                      if (entries.some((entry) => entry.isIntersecting)) {
                        lazy.src = "/lazy.svg";
                        observer.disconnect();
                      }
                    }).observe(lazy);
                  </script>
                </body>
                </html>"##,
            ),
        )
        .await?;
    let pageknot = PageKnot::builder().build()?;

    let captured = capture_bytes(&pageknot, server.url("/")?, NetworkPolicy::Standard).await?;

    assert_success(&captured);
    let html = String::from_utf8_lossy(&captured.content);
    assert!(html.contains("script rendered"), "{html}");
    assert!(html.contains("mutation settled"), "{html}");
    assert!(
        html.matches("shadowrootmode=\"open\"").count() >= 2,
        "{html}"
    );
    assert!(
        html.contains("data-pageknot-shadow-mode=\"closed\""),
        "{html}"
    );
    assert!(!html.contains("shadowrootmode=\"closed\""), "{html}");
    assert!(html.contains("data-pageknot-adopted"), "{html}");
    assert!(html.contains("rgb(17, 34, 51)"), "{html}");
    assert!(html.contains("value=\"after\""), "{html}");
    assert!(html.contains("checked=\"\""), "{html}");
    assert!(html.contains("current notes"), "{html}");
    assert!(html.contains("selected=\"\""), "{html}");
    assert!(html.contains("open=\"\""), "{html}");
    assert!(html.matches("data-pageknot-canvas").count() >= 2, "{html}");
    assert!(html.contains("data-pageknot-media-poster"), "{html}");
    assert!(html.contains("data:image/svg+xml;base64,"), "{html}");
    assert!(
        server
            .requests()
            .await
            .iter()
            .any(|request| request.path == "/large.svg")
    );
    assert!(
        server
            .requests()
            .await
            .iter()
            .any(|request| request.path == "/lazy.svg")
    );

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn rendered_state_reopens_across_light_and_shadow_trees() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><html><head><title>Captured view state</title>
                <style>
                  body { margin: 0; height: 1800px }
                  #scroller { width: 120px; height: 80px; overflow: scroll }
                  #content { width: 500px; height: 500px }
                  #animated { width: 20px; height: 20px; animation: move 1s linear both }
                  #transitioned {
                    width: 20px;
                    height: 20px;
                    transform: translateX(0);
                    transition: transform 2s linear;
                  }
                  @keyframes move {
                    from { transform: translateX(0) }
                    to { transform: translateX(120px) }
                  }
                </style></head><body>
                <div id="scroller"><div id="content"></div></div>
                <div id="animated"></div>
                <div id="transitioned"></div>
                <section id="open-host"></section>
                <section id="closed-host"></section>
                <script>
                  scrollTo(0, 240);
                  const scroller = document.getElementById("scroller");
                  scroller.scrollLeft = 30;
                  scroller.scrollTop = 70;
                  const animation = document.getElementById("animated").getAnimations()[0];
                  animation.pause();
                  animation.currentTime = 500;
                  const transitioned = document.getElementById("transitioned");
                  transitioned.getBoundingClientRect();
                  transitioned.style.transform = "translateX(120px)";
                  const transition = transitioned.getAnimations()[0];
                  transition.pause();
                  transition.currentTime = 1000;
                  const attachStatefulShadow = (host, mode) => {
                    const root = host.attachShadow({mode});
                    root.innerHTML = `<style>
                      #shadow-scroll { width: 100px; height: 60px; overflow: scroll }
                      #shadow-content { width: 400px; height: 300px }
                      #shadow-animated {
                        width: 20px;
                        height: 20px;
                        animation: shadow-move 2s linear both
                      }
                      @keyframes shadow-move {
                        from { transform: translateX(0) }
                        to { transform: translateX(120px) }
                      }
                    </style>
                    <div id="shadow-scroll"><div id="shadow-content"></div></div>
                    <div id="shadow-animated"></div>
                    <table id="shadow-table"></table>`;
                    const shadowScroller = root.getElementById("shadow-scroll");
                    shadowScroller.scrollLeft = 25;
                    shadowScroller.scrollTop = 45;
                    const shadowAnimation =
                      root.getElementById("shadow-animated").getAnimations()[0];
                    shadowAnimation.pause();
                    shadowAnimation.currentTime = 1000;
                    const repaired = document.createElement("div");
                    repaired.id = `${mode}-repaired`;
                    root.getElementById("shadow-table").append(repaired);
                  };
                  attachStatefulShadow(document.getElementById("open-host"), "open");
                  attachStatefulShadow(document.getElementById("closed-host"), "closed");
                </script></body></html>"#,
            ),
        )
        .await?;
    let pageknot = PageKnot::builder().build()?;

    let captured = capture_bytes(&pageknot, server.url("/")?, NetworkPolicy::Standard).await?;

    assert_success(&captured);
    let manifest = pageknot_html::inspect_html(&captured.content)?;
    assert_eq!(manifest.view_state.scroll_x, "0");
    assert_eq!(manifest.view_state.scroll_y, "240");
    let html = String::from_utf8_lossy(&captured.content);
    assert!(html.contains("data-pageknot-scroll-left=\"30\""), "{html}");
    assert!(html.contains("data-pageknot-scroll-top=\"70\""), "{html}");
    assert!(
        html.contains("data-pageknot-shadow-mode=\"closed\""),
        "{html}"
    );

    let directory = tempfile::tempdir()?;
    let path = directory.path().join("state.html");
    std::fs::write(&path, &captured.content)?;
    let state = evaluate_file(
        &path,
        r#"(async () => {
          await new Promise((resolve) =>
            requestAnimationFrame(() => requestAnimationFrame(resolve))
          );
          const scroller = document.getElementById("scroller");
          const animated = document.getElementById("animated");
          const transitioned = document.getElementById("transitioned");
          const openRoot = document.getElementById("open-host").shadowRoot;
          const closedRoot = document.getElementById("closed-host").shadowRoot;
          const shadowState = (root, mode) => ({
            present: Boolean(root),
            nestedLeft: root?.getElementById("shadow-scroll").scrollLeft,
            nestedTop: root?.getElementById("shadow-scroll").scrollTop,
            animatedLeft:
              root?.getElementById("shadow-animated").getBoundingClientRect().left,
            repairedParent:
              root?.getElementById(`${mode}-repaired`).parentElement.localName
          });
          return {
            scrollX,
            scrollY,
            nestedLeft: scroller.scrollLeft,
            nestedTop: scroller.scrollTop,
            animatedLeft: animated.getBoundingClientRect().left,
            animationName: getComputedStyle(animated).animationName,
            transitionedLeft: transitioned.getBoundingClientRect().left,
            open: shadowState(openRoot, "open"),
            closed: shadowState(closedRoot, "closed")
          };
        })()"#,
    )
    .await?;

    assert_eq!(
        state.get("scrollX").and_then(serde_json::Value::as_f64),
        Some(0.0)
    );
    assert_eq!(
        state.get("scrollY").and_then(serde_json::Value::as_f64),
        Some(240.0)
    );
    assert_eq!(
        state.get("nestedLeft").and_then(serde_json::Value::as_f64),
        Some(30.0)
    );
    assert_eq!(
        state.get("nestedTop").and_then(serde_json::Value::as_f64),
        Some(70.0)
    );
    let animated_left = state
        .get("animatedLeft")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or_default();
    assert!(
        (animated_left - 60.0).abs() <= 0.5,
        "animation reopened at x={animated_left}: {state}"
    );
    assert_eq!(
        state
            .get("animationName")
            .and_then(serde_json::Value::as_str),
        Some("none")
    );
    assert_eq!(
        state
            .get("transitionedLeft")
            .and_then(serde_json::Value::as_f64),
        Some(60.0)
    );
    for mode in ["open", "closed"] {
        let shadow = state
            .get(mode)
            .and_then(serde_json::Value::as_object)
            .ok_or("missing reopened shadow state")?;
        assert_eq!(
            shadow.get("present").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            shadow.get("nestedLeft").and_then(serde_json::Value::as_f64),
            Some(25.0)
        );
        assert_eq!(
            shadow.get("nestedTop").and_then(serde_json::Value::as_f64),
            Some(45.0)
        );
        assert_eq!(
            shadow
                .get("animatedLeft")
                .and_then(serde_json::Value::as_f64),
            Some(60.0)
        );
        assert_eq!(
            shadow
                .get("repairedParent")
                .and_then(serde_json::Value::as_str),
            Some("table")
        );
    }

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn resource_graph_fixture_round_trips() -> TestResult {
    let server = FixtureServer::start().await?;
    register_svg(&server, "/background.svg", "rgb(30, 80, 160)").await?;
    register_svg(&server, "/nested.svg", "rgb(180, 100, 20)").await?;
    server
        .register(
            "/root.css",
            response(
                "text/css",
                b"@import url('/nested.css'); article { background-image: url('/background.svg'); }",
            ),
        )
        .await?;
    server
        .register(
            "/nested.css",
            response(
                "text/css",
                b"@import url('/cycle.css'); article { border-color: rgb(10, 20, 30); }",
            ),
        )
        .await?;
    server
        .register(
            "/cycle.css",
            response(
                "text/css",
                b"@import url('/nested.css'); article { color: rgb(40, 50, 60); }",
            ),
        )
        .await?;
    server
        .register(
            "/graph.svg",
            response(
                "image/svg+xml",
                br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
                    <image href="/nested.svg" width="8" height="8"/>
                </svg>"#,
            ),
        )
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><html><head><title>Resources</title>
                <link rel="stylesheet" href="/root.css"></head>
                <body><article>resource graph</article>
                <img id="svg-graph" src="/graph.svg#root"></body></html>"#,
            ),
        )
        .await?;
    let pageknot = PageKnot::builder().build()?;

    let captured = capture_bytes(&pageknot, server.url("/")?, NetworkPolicy::Standard).await?;

    assert_success(&captured);
    assert!(captured.result.resources.failed >= 1);
    assert!(captured.result.resources.is_complete());
    let html = String::from_utf8_lossy(&captured.content);
    assert!(html.contains("data:image/svg+xml;base64,"), "{html}");
    let manifest = pageknot_html::inspect_html(&captured.content)?;
    assert!(manifest.resource_records.iter().any(|record| {
        record.requested_url.as_str().ends_with("/nested.svg")
            && matches!(record.outcome, pageknot::ResourceOutcome::Embedded { .. })
    }));

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn permanent_connections_do_not_block_render_idle() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register_route(
            "/events",
            FixtureRoute::event_stream(vec!["ready".to_owned()], Duration::ZERO),
        )
        .await?;
    server
        .register_route(
            "/socket",
            FixtureRoute::websocket(vec!["ready".to_owned()], Duration::ZERO)?,
        )
        .await?;
    server
        .register_route(
            "/slow-request",
            FixtureRoute::delayed(
                FixtureResponse::text("late response"),
                Duration::from_secs(60),
            ),
        )
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Permanent activity</title>
                <output id="state">starting</output>
                <script>
                  const output = document.getElementById("state");
                  const source = new EventSource("/events");
                  const socket = new WebSocket(`ws://${location.host}/socket`);
                  addEventListener("load", () => {
                    fetch("/slow-request").catch(() => {});
                  }, {once: true});
                  let eventsReady = false;
                  let socketReady = false;
                  const update = () => {
                    if (eventsReady && socketReady) output.textContent = "connections ready";
                  };
                  source.onmessage = () => { eventsReady = true; update(); };
                  socket.onmessage = () => { socketReady = true; update(); };
                </script>"#,
            ),
        )
        .await?;
    let pageknot = PageKnot::builder().build()?;

    let captured = tokio::time::timeout(
        Duration::from_secs(15),
        capture_bytes(&pageknot, server.url("/")?, NetworkPolicy::Standard),
    )
    .await??;

    assert_success(&captured);
    assert!(
        String::from_utf8_lossy(&captured.content).contains("connections ready"),
        "{}",
        String::from_utf8_lossy(&captured.content)
    );

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn frame_fixture_round_trips() -> TestResult {
    let parent = FixtureServer::start().await?;
    let cross_origin = FixtureServer::start().await?;
    register_svg(&parent, "/same.svg", "rgb(30, 130, 200)").await?;
    register_svg(&parent, "/srcdoc.svg", "rgb(60, 160, 90)").await?;
    register_svg(&parent, "/sandbox.svg", "rgb(190, 100, 30)").await?;
    register_svg(&cross_origin, "/cross.svg", "rgb(100, 50, 180)").await?;
    parent
        .register(
            "/same",
            FixtureResponse::html(
                r#"<title>Same frame</title><main id="same-state">same frame
                <img src="/same.svg"></main><script>
                document.getElementById("same-state").dataset.rendered = "true";
                </script>"#,
            ),
        )
        .await?;
    parent
        .register(
            "/sandbox",
            FixtureResponse::html(
                r#"<title>Sandbox frame</title><main>sandbox frame
                <img src="/sandbox.svg"></main>"#,
            ),
        )
        .await?;
    cross_origin
        .register(
            "/cross",
            FixtureResponse::html(
                r#"<title>Cross frame</title><main>cross frame
                <img src="/cross.svg"></main>"#,
            ),
        )
        .await?;
    let cross_url = cross_origin.url_with_host("/cross", "localhost")?;
    parent
        .register(
            "/",
            FixtureResponse::html(format!(
                r#"<!doctype html><title>Frame matrix</title>
                <iframe id="same" src="/same"></iframe>
                <iframe id="srcdoc" srcdoc="<main>srcdoc frame<img src='/srcdoc.svg'></main>"></iframe>
                <iframe id="sandbox" sandbox src="/sandbox"></iframe>
                <iframe id="cross" src="{cross_url}"></iframe>"#
            )),
        )
        .await?;
    let pageknot = PageKnot::builder().build()?;

    let captured = capture_bytes(&pageknot, parent.url("/")?, NetworkPolicy::Unrestricted).await?;

    assert_success(&captured);
    let manifest = pageknot_html::inspect_html(&captured.content)?;
    assert_eq!(manifest.frames, 5);
    let html = String::from_utf8_lossy(&captured.content);
    for state in ["same frame", "srcdoc frame", "sandbox frame", "cross frame"] {
        assert!(html.contains(state), "missing {state}: {html}");
    }
    assert_eq!(html.matches("data-pageknot-frame-id=").count(), 4);
    assert!(captured.result.warnings.iter().all(|warning| {
        !matches!(
            warning.code.as_str(),
            "pageknot.frame.cross_origin" | "pageknot.frame.collection"
        )
    }));

    pageknot.close().await?;
    parent.close().await;
    cross_origin.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn request_policy_fixture_round_trips() -> TestResult {
    let server = FixtureServer::start().await?;
    let redirect_location = server.url("/final")?.to_string();
    server
        .register(
            "/start",
            FixtureResponse::text(Vec::new())
                .with_status(302)
                .with_header("Location", redirect_location),
        )
        .await?;
    server
        .register(
            "/final",
            FixtureResponse::html(
                r#"<!doctype html><title>Request policy</title>
                <img src="/protected.svg">
                <img src="/referrer.svg">
                <img src="/compressed.svg">"#,
            )
            .with_header("Content-Security-Policy", "default-src 'self'"),
        )
        .await?;
    server
        .register_handler("/protected.svg", |request| {
            let authenticated = request
                .headers
                .get("authorization")
                .is_some_and(|value| value == "Bearer fixture-secret")
                && request
                    .headers
                    .get("cookie")
                    .is_some_and(|value| value.contains("pageknot_session=cookie-secret"));
            if authenticated {
                response("image/svg+xml", svg("rgb(20, 140, 80)")).into()
            } else {
                FixtureResponse::text("credentials missing")
                    .with_status(401)
                    .into()
            }
        })
        .await?;
    server
        .register_handler("/referrer.svg", |request| {
            let color = if request
                .headers
                .get("referer")
                .is_some_and(|value| value.ends_with("/final"))
            {
                "rgb(30, 90, 180)"
            } else {
                "rgb(220, 40, 40)"
            };
            response("image/svg+xml", svg(color)).into()
        })
        .await?;
    server
        .register(
            "/compressed.svg",
            response("image/svg+xml", svg("rgb(100, 60, 170)")).gzip()?,
        )
        .await?;
    let mut request = CaptureRequest::builder(server.url("/start")?.as_str())?.build()?;
    request.artifact = ArtifactSpec::html_bytes(8 * 1024 * 1024);
    request.credentials.headers.push(RequestHeader {
        name: "Authorization".to_owned(),
        value: SecretString::new("Bearer fixture-secret"),
    });
    request.credentials.cookies.push(BrowserCookie {
        name: "pageknot_session".to_owned(),
        value: SecretString::new("cookie-secret"),
        url: Some(server.url("/")?),
        domain: None,
        path: Some("/".to_owned()),
        secure: None,
        http_only: Some(true),
        same_site: None,
        expires: None,
    });
    let pageknot = PageKnot::builder().build()?;

    let result = pageknot.captures().start(request).await?.wait().await?;
    let content = artifact_bytes(&result)?;
    let captured = CapturedArtifact { result, content };

    assert_success(&captured);
    assert!(
        captured
            .result
            .source
            .final_url
            .as_str()
            .ends_with("/final")
    );
    assert_eq!(captured.result.resources.discovered, 3);
    assert_eq!(captured.result.resources.embedded, 3);
    assert_eq!(captured.result.resources.failed, 0);
    let manifest = pageknot_html::inspect_html(&captured.content)?;
    assert_eq!(manifest.resources.embedded, 3);
    assert!(manifest.resource_records.iter().all(|record| {
        matches!(record.outcome, pageknot::ResourceOutcome::Embedded { .. })
            && record.provenance.as_ref().is_some_and(|provenance| {
                provenance.source == pageknot::ResourceRetrievalSource::ObservedResponse
            })
    }));
    let requests = server.requests().await;
    assert!(requests.iter().any(|request| {
        request.path == "/protected.svg"
            && request
                .headers
                .get("authorization")
                .is_some_and(|value| value == "Bearer fixture-secret")
    }));
    assert!(requests.iter().any(|request| {
        request.path == "/referrer.svg"
            && request
                .headers
                .get("referer")
                .is_some_and(|value| value.ends_with("/final"))
    }));

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn service_worker_response_round_trips() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/worker.js",
            response(
                "application/javascript",
                br#"self.addEventListener("install", (event) => {
                    self.skipWaiting();
                });
                self.addEventListener("activate", (event) => {
                    event.waitUntil(self.clients.claim());
                });
                self.addEventListener("fetch", (event) => {
                    if (new URL(event.request.url).pathname === "/worker-image.svg") {
                        event.respondWith(new Response(
                            "<svg xmlns='http://www.w3.org/2000/svg' width='8' height='8'><rect width='8' height='8' fill='rgb(15, 145, 95)'/></svg>",
                            {headers: {"Content-Type": "image/svg+xml"}}
                        ));
                    }
                });"#,
            ),
        )
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Service worker</title>
                <output id="state">waiting</output><img id="worker-image">
                <script>
                (async () => {
                  await navigator.serviceWorker.register("/worker.js");
                  await navigator.serviceWorker.ready;
                  if (!navigator.serviceWorker.controller) {
                    await new Promise((resolve) =>
                      navigator.serviceWorker.addEventListener("controllerchange", resolve, {once: true})
                    );
                  }
                  const image = document.getElementById("worker-image");
                  await new Promise((resolve, reject) => {
                    image.onload = resolve;
                    image.onerror = reject;
                    image.src = "/worker-image.svg";
                  });
                  document.getElementById("state").textContent = "worker response ready";
                })();
                </script>"#,
            ),
        )
        .await?;
    let pageknot = PageKnot::builder().build()?;

    let captured = capture_bytes(&pageknot, server.url("/")?, NetworkPolicy::Standard).await?;

    assert_success(&captured);
    assert!(
        String::from_utf8_lossy(&captured.content).contains("worker response ready"),
        "{}",
        String::from_utf8_lossy(&captured.content)
    );
    assert_eq!(captured.result.resources.embedded, 1);
    assert_eq!(captured.result.resources.failed, 0);
    let requests = server.requests().await;
    assert!(requests.iter().any(|request| request.path == "/worker.js"));
    assert!(
        requests
            .iter()
            .all(|request| request.path != "/worker-image.svg")
    );

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn partial_resource_has_a_typed_outcome() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register_route(
            "/partial.svg",
            FixtureRoute::partial(
                response("image/svg+xml", b"<svg xmlns='http://www.w3.org/2000/svg'>"),
                512,
            )?,
        )
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html("<title>Partial resource</title><img src='/partial.svg'>"),
        )
        .await?;
    let pageknot = PageKnot::builder().build()?;

    let captured = capture_bytes(&pageknot, server.url("/")?, NetworkPolicy::Standard).await?;

    assert_success(&captured);
    assert_eq!(captured.result.resources.discovered, 1);
    assert_eq!(captured.result.resources.failed, 1);
    let manifest = pageknot_html::inspect_html(&captured.content)?;
    assert!(matches!(
        manifest.resource_records[0].outcome,
        pageknot::ResourceOutcome::Failed { .. }
    ));

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn oversized_resource_fails_before_artifact_commit() -> TestResult {
    let server = FixtureServer::start().await?;
    let payload = format!(
        "<svg xmlns='http://www.w3.org/2000/svg'><metadata>{}</metadata></svg>",
        "x".repeat(4096)
    );
    server
        .register("/large.svg", response("image/svg+xml", payload))
        .await?;
    let mut signed_url = server.url("/large.svg")?;
    signed_url.set_query(Some(
        "X-Amz-Credential=signed-credential&X-Amz-Signature=signed-secret&view=full",
    ));
    let mut userinfo_url = server.url("/large.svg")?;
    userinfo_url
        .set_username("resource-user")
        .map_err(|()| std::io::Error::other("fixture URL rejected its username"))?;
    userinfo_url
        .set_password(Some("userinfo-secret"))
        .map_err(|()| std::io::Error::other("fixture URL rejected its password"))?;
    server
        .register(
            "/signed",
            FixtureResponse::html(format!(
                "<title>Oversized</title><img src='{}'>",
                signed_url.as_str()
            )),
        )
        .await?;
    server
        .register(
            "/userinfo",
            FixtureResponse::html(format!(
                "<title>Oversized</title><img src='{}'>",
                userinfo_url.as_str()
            )),
        )
        .await?;
    let pageknot = PageKnot::builder().build()?;

    for (root, resource_url, expected_code) in [
        ("/signed", &signed_url, "pageknot.resource.limit"),
        ("/userinfo", &userinfo_url, "pageknot.resource.load"),
    ] {
        let mut request = CaptureRequest::builder(server.url(root)?.as_str())?.build()?;
        request.artifact = ArtifactSpec::html_bytes(1024 * 1024);
        request.capture.missing_resources = MissingResourcePolicy::Fail;
        request.limits.resource_bytes = 1024;
        request.limits.total_resource_bytes = 1024;
        let error = match pageknot.captures().start(request).await?.wait().await {
            Ok(result) => {
                return Err(std::io::Error::other(format!(
                    "oversized fixture unexpectedly succeeded: {result:?}"
                ))
                .into());
            }
            Err(error) => error,
        };
        let serialized = serde_json::to_string(&error)?;

        assert_eq!(error.code.as_str(), expected_code, "{root}: {error:?}");
        for secret in ["signed-credential", "signed-secret", "userinfo-secret"] {
            assert!(!serialized.contains(secret), "{serialized}");
        }
        let redacted_url = error
            .details
            .get("url")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| std::io::Error::other("resource error omitted its redacted URL"))?;
        assert!(redacted_url.contains("%3Credacted%3E"), "{redacted_url}");
        assert_eq!(
            error.details.get("urlSha256"),
            Some(&serde_json::json!(
                ContentDigest::sha256(resource_url.as_str()).to_string()
            ))
        );
    }

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn navigation_deadline_returns_a_typed_timeout() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register_route(
            "/slow",
            FixtureRoute::delayed(
                FixtureResponse::html("<title>Too late</title>"),
                Duration::from_secs(2),
            ),
        )
        .await?;
    let mut request = CaptureRequest::builder(server.url("/slow")?.as_str())?.build()?;
    request.limits.duration = Milliseconds::new(300);
    request.artifact = ArtifactSpec::html_bytes(1024 * 1024);
    let pageknot = PageKnot::builder().build()?;

    let error = match pageknot.captures().start(request).await?.wait().await {
        Ok(result) => {
            return Err(std::io::Error::other(format!(
                "slow navigation unexpectedly succeeded: {result:?}"
            ))
            .into());
        }
        Err(error) => error,
    };

    assert_eq!(error.code.as_str(), "pageknot.runtime.timeout");
    assert!(error.retryable);

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn network_idle_uses_the_total_capture_deadline() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register_route(
            "/slow-asset",
            FixtureRoute::delayed(FixtureResponse::text("late asset"), Duration::from_secs(2)),
        )
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Network idle deadline</title>
                <img src="/slow-asset" alt="">"#,
            ),
        )
        .await?;
    let mut request = CaptureRequest::builder(server.url("/")?.as_str())?.build()?;
    request.readiness.mode = ReadinessMode::NetworkIdle;
    request.readiness.network_quiet = Milliseconds::new(50);
    request.limits.duration = Milliseconds::new(500);
    request.artifact = ArtifactSpec::html_bytes(1024 * 1024);
    let pageknot = PageKnot::builder().build()?;

    let error = match pageknot.captures().start(request).await?.wait().await {
        Ok(result) => {
            return Err(std::io::Error::other(format!(
                "network-idle capture unexpectedly succeeded: {result:?}"
            ))
            .into());
        }
        Err(error) => error,
    };

    assert_eq!(error.code.as_str(), "pageknot.runtime.timeout");
    assert!(error.retryable);

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
async fn explicit_output_conflict_preserves_the_destination() -> TestResult {
    let directory = tempfile::tempdir()?;
    let destination = directory.path().join("capture.html");
    std::fs::write(&destination, b"existing artifact")?;
    let mut request = CaptureRequest::builder("https://example.com")?.build()?;
    request.artifact = ArtifactSpec::html_file(PortablePath::from_path_buf(destination.clone())?);
    let ArtifactSpec::Html(artifact) = &mut request.artifact;
    artifact.conflict = ConflictPolicy::Fail;
    let pageknot = PageKnot::builder().build()?;

    let Err(error) = pageknot.captures().start(request).await?.wait().await else {
        return Err(std::io::Error::other(
            "the fail conflict policy replaced an existing destination",
        )
        .into());
    };

    assert_eq!(error.code.as_str(), "pageknot.output.exists");
    assert_eq!(std::fs::read(&destination)?, b"existing artifact");
    pageknot.close().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn atomic_replacement_commits_the_verified_artifact() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html("<!doctype html><title>Replacement</title><h1>new artifact</h1>"),
        )
        .await?;
    let directory = tempfile::tempdir()?;
    let destination = directory.path().join("capture.html");
    std::fs::write(&destination, b"existing artifact")?;
    let mut request = CaptureRequest::builder(server.url("/")?.as_str())?.build()?;
    request.artifact = ArtifactSpec::html_file(PortablePath::from_path_buf(destination.clone())?);
    let pageknot = PageKnot::builder().build()?;

    let result = pageknot.captures().start(request).await?.wait().await?;

    assert_success_result(&result);
    let content = std::fs::read(&destination)?;
    assert!(String::from_utf8_lossy(&content).contains("new artifact"));
    assert!(pageknot_html::verify_static(&content)?.passed);
    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn cancellation_rolls_back_every_pipeline_stage() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html("<!doctype html><title>Cancellation</title><h1>ready</h1>"),
        )
        .await?;
    let directory = tempfile::tempdir()?;
    let destination = directory.path().join("capture.html");
    let pageknot = PageKnot::builder().build()?;
    for stage in [
        CaptureStatus::Validating,
        CaptureStatus::WaitingForBrowser,
        CaptureStatus::Navigating,
        CaptureStatus::Settling,
        CaptureStatus::Collecting,
        CaptureStatus::ResolvingResources,
        CaptureStatus::Transforming,
        CaptureStatus::Encoding,
        CaptureStatus::Verifying,
        CaptureStatus::Committing,
    ] {
        std::fs::write(&destination, b"existing artifact")?;
        let mut request = CaptureRequest::builder(server.url("/")?.as_str())?.build()?;
        request.artifact =
            ArtifactSpec::html_file(PortablePath::from_path_buf(destination.clone())?);
        let ArtifactSpec::Html(artifact) = &mut request.artifact;
        artifact.conflict = ConflictPolicy::Replace;
        let job = pageknot.captures().start(request).await?;

        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let current = job.status();
                if current == stage {
                    job.cancel();
                    break;
                }
                assert!(
                    !current.is_terminal(),
                    "capture reached {current:?} before cancellation at {stage:?}"
                );
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let Err(error) = job.wait().await else {
            return Err(std::io::Error::other(format!(
                "capture succeeded after cancellation at {stage:?}"
            ))
            .into());
        };

        assert_eq!(
            error.code.as_str(),
            "pageknot.runtime.cancelled",
            "{stage:?}"
        );
        assert_eq!(
            std::fs::read(&destination)?,
            b"existing artifact",
            "{stage:?}"
        );
        let remaining =
            std::fs::read_dir(directory.path())?.collect::<std::io::Result<Vec<_>>>()?;
        assert_eq!(remaining.len(), 1, "{stage:?}");
        assert_eq!(remaining[0].path(), destination, "{stage:?}");
    }
    pageknot.close().await?;
    server.close().await;
    Ok(())
}

async fn capture_bytes(
    pageknot: &PageKnot,
    url: Url,
    network: NetworkPolicy,
) -> TestResult<CapturedArtifact> {
    let mut request = CaptureRequest::builder(url.as_str())?.build()?;
    request.network = network;
    request.artifact = ArtifactSpec::html_bytes(16 * 1024 * 1024);
    let result = pageknot.captures().start(request).await?.wait().await?;
    let content = artifact_bytes(&result)?;
    Ok(CapturedArtifact { result, content })
}

async fn policy_fixture() -> TestResult<(FixtureServer, Url)> {
    let server = FixtureServer::start().await?;
    register_svg(&server, "/selected.svg", "rgb(30, 110, 190)").await?;
    register_svg(&server, "/alternative.svg", "rgb(190, 80, 30)").await?;
    let fixture = r#"<!doctype html><html><head><title>Policy evidence</title>
                <style>
                  body { margin: 0; background: white }
                  .used { color: rgb(12, 34, 56); padding: 20px }
                  .unused { background-image: url("/unused.svg") }
                  @font-face { font-family: UsedFace; src: url("data:font/woff2;base64,d09GMgAB") }
                  @font-face { font-family: UnusedFace; src: url("/unused.woff2") }
                  .used { font-family: system-ui, sans-serif }
                </style></head><body>
                <main>
                  <article id="selected" class="used capture-target">
                    <h1>selected policy evidence</h1>
                    <picture>
                      <source srcset="/alternative.svg 2x">
                      <img src="/selected.svg" srcset="/selected.svg 1x, /alternative.svg 2x"
                        width="64" height="64" alt="selected">
                    </picture>
                  </article>
                  <p id="unselected">unselected branch</p>
                  <article class="capture-target">second selector match</article>
                  <p id="hidden" style="display:none">hidden branch</p>
                </main>
                <script>
                  const range = document.createRange();
                  range.selectNodeContents(document.getElementById("selected"));
                  const selection = getSelection();
                  selection.removeAllRanges();
                  selection.addRange(range);
                </script></body></html>"#;
    server.register("/", FixtureResponse::html(fixture)).await?;
    let url = server.url("/")?;
    Ok((server, url))
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn visual_optimizers_reduce_resources_and_preserve_rendering() -> TestResult {
    let (server, url) = policy_fixture().await?;
    let pageknot = PageKnot::builder().build()?;
    let baseline = capture_bytes_with_policy(&pageknot, url.clone(), CapturePolicy::default())
        .await
        .map_err(|error| std::io::Error::other(format!("baseline capture failed: {error:?}")))?;
    let optimized_policy = CapturePolicy {
        optimizations: OptimizationPolicy {
            remove_unused_css: true,
            remove_unused_fonts: true,
            remove_hidden_elements: true,
        },
        ..CapturePolicy::default()
    };
    let optimized = capture_bytes_with_policy(&pageknot, url, optimized_policy)
        .await
        .map_err(|error| std::io::Error::other(format!("optimized capture failed: {error:?}")))?;

    assert_success(&baseline);
    assert_success(&optimized);
    assert!(
        optimized.result.resources.discovered < baseline.result.resources.discovered,
        "baseline: {:?}, optimized: {:?}",
        baseline.result.resources,
        optimized.result.resources
    );
    assert!(optimized.content.len() < baseline.content.len());

    let directory = tempfile::tempdir()?;
    let baseline_path = directory.path().join("baseline.html");
    let optimized_path = directory.path().join("optimized.html");
    std::fs::write(&baseline_path, &baseline.content)?;
    std::fs::write(&optimized_path, &optimized.content)?;
    let (baseline_screenshot, optimized_screenshot) =
        screenshot_pair(&baseline_path, &optimized_path).await?;
    let similarity = pixel_similarity(&baseline_screenshot, &optimized_screenshot)?;
    assert!(
        similarity >= 0.999,
        "optimizer screenshot similarity was {similarity:.6}"
    );

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn selection_and_selector_capture_the_first_rendered_target() -> TestResult {
    let (server, url) = policy_fixture().await?;
    let pageknot = PageKnot::builder().build()?;
    let selection = capture_bytes_with_policy(
        &pageknot,
        url.clone(),
        CapturePolicy {
            scope: CaptureScope::Selection,
            ..CapturePolicy::default()
        },
    )
    .await?;
    let selector = capture_bytes_with_policy(
        &pageknot,
        url,
        CapturePolicy {
            selector: Some(".capture-target".to_owned()),
            ..CapturePolicy::default()
        },
    )
    .await?;

    assert_success(&selection);
    assert_success(&selector);
    for scoped in [&selection, &selector] {
        let html = String::from_utf8_lossy(&scoped.content);
        assert!(html.contains("selected policy evidence"), "{html}");
        assert!(!html.contains("unselected branch"), "{html}");
        assert!(!html.contains("second selector match"), "{html}");
    }

    pageknot.close().await?;
    server.close().await;
    Ok(())
}

async fn capture_bytes_with_policy(
    pageknot: &PageKnot,
    url: Url,
    policy: CapturePolicy,
) -> TestResult<CapturedArtifact> {
    let mut request = CaptureRequest::builder(url.as_str())?.build()?;
    request.capture = policy;
    request.network = NetworkPolicy::Standard;
    request.artifact = ArtifactSpec::html_bytes(16 * 1024 * 1024);
    let result = pageknot.captures().start(request).await?.wait().await?;
    let content = artifact_bytes(&result)?;
    Ok(CapturedArtifact { result, content })
}

async fn screenshot_pair(
    first: &std::path::Path,
    second: &std::path::Path,
) -> TestResult<(Vec<u8>, Vec<u8>)> {
    let executable = ChromiumDiscovery::new()
        .discover()
        .await
        .selected
        .and_then(|browser| browser.executable_path)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no browser"))?;
    let process = ChromiumProcess::launch(ChromiumLaunchOptions::new(executable)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    let screenshots: TestResult<(Vec<u8>, Vec<u8>)> = async {
        let first_url = Url::from_file_path(first)
            .map_err(|()| std::io::Error::other("artifact path is not a file URL"))?;
        page.navigate(&first_url, ReadinessMode::Load, 0, Duration::from_secs(10))
            .await?;
        let first = page.capture_page_screenshot(16 * 1024 * 1024).await?;
        let second_url = Url::from_file_path(second)
            .map_err(|()| std::io::Error::other("artifact path is not a file URL"))?;
        page.navigate(&second_url, ReadinessMode::Load, 0, Duration::from_secs(10))
            .await?;
        let second = page.capture_page_screenshot(16 * 1024 * 1024).await?;
        Ok((first, second))
    }
    .await;
    let page_close = page.close().await;
    let process_close = process.close().await;
    page_close?;
    process_close?;
    screenshots
}

async fn evaluate_file(path: &std::path::Path, expression: &str) -> TestResult<serde_json::Value> {
    let executable = ChromiumDiscovery::new()
        .discover()
        .await
        .selected
        .and_then(|browser| browser.executable_path)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no browser"))?;
    let process = ChromiumProcess::launch(ChromiumLaunchOptions::new(executable)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    let url = Url::from_file_path(path)
        .map_err(|()| std::io::Error::other("artifact path is not a file URL"))?;
    page.navigate(&url, ReadinessMode::Load, 0, Duration::from_secs(10))
        .await?;
    let value = page.evaluate(expression).await?;
    page.close().await?;
    process.close().await?;
    Ok(value)
}

fn pixel_similarity(left: &[u8], right: &[u8]) -> TestResult<f64> {
    let left = ImageReader::new(std::io::Cursor::new(left))
        .with_guessed_format()?
        .decode()?
        .to_rgb8();
    let right = ImageReader::new(std::io::Cursor::new(right))
        .with_guessed_format()?
        .decode()?
        .to_rgb8();
    if left.dimensions() != right.dimensions() {
        return Ok(0.0);
    }
    let difference = left
        .as_raw()
        .iter()
        .zip(right.as_raw())
        .map(|(left, right)| u64::from(left.abs_diff(*right)))
        .sum::<u64>();
    let maximum = 255_u64.saturating_mul(u64::try_from(left.as_raw().len()).unwrap_or(u64::MAX));
    Ok(if maximum == 0 {
        1.0
    } else {
        1.0 - difference as f64 / maximum as f64
    })
}

fn artifact_bytes(result: &CaptureResult) -> TestResult<Vec<u8>> {
    match &result.artifact {
        ArtifactResult::Bytes { content, .. } => Ok(content.clone()),
        ArtifactResult::File { .. } => {
            Err(std::io::Error::other("fixture capture returned a file artifact").into())
        }
    }
}

fn assert_success(captured: &CapturedArtifact) {
    assert_success_result(&captured.result);
}

fn assert_success_result(result: &CaptureResult) {
    assert_eq!(
        result.status,
        CaptureTerminalStatus::Succeeded,
        "{:?}",
        result
    );
    assert!(result.verification.passed);
    assert_eq!(result.verification.network_requests, 0);
    assert!(result.resources.is_complete());
}

async fn register_svg(server: &FixtureServer, path: &str, color: &str) -> TestResult {
    server
        .register(
            path,
            response(
                "image/svg+xml",
                format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="{color}"/></svg>"#
                ),
            ),
        )
        .await?;
    Ok(())
}

fn svg(color: &str) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="{color}"/></svg>"#
    )
}

fn response(content_type: &str, body: impl Into<Vec<u8>>) -> FixtureResponse {
    FixtureResponse {
        status: 200,
        content_type: content_type.to_owned(),
        headers: Default::default(),
        body: body.into(),
    }
}

const TEST_FONT_WOFF2_BASE64: &str = "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";
