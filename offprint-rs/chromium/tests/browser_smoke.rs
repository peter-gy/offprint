use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io::Write as _;
use std::time::Duration;

use base64::Engine as _;
use futures_util::StreamExt as _;
use offprint_browser::{FrameObservation, NetworkGuard, PageSession};
use offprint_chromium::{
    ChromiumDiscovery, ChromiumLaunchOptions, ChromiumPage, ChromiumProcess, ObservationLimits,
    ResourceObservationLimits,
};
use offprint_model::{
    BrowserEnvironment, CaptureId, CaptureScope, ContentPolicy, FrameId, NetworkPolicy,
    NetworkRules, OptimizationPolicy, ReadinessMode, ReadinessPolicy, ResourceRetrievalSource,
};
use offprint_test_support::{FixtureResponse, FixtureRoute, FixtureServer};
use serde_json::Value;

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

const fn collector_limits(
    maximum_chunk_bytes: u64,
    maximum_payload_bytes: u64,
    maximum_nodes: u64,
) -> ObservationLimits {
    ObservationLimits {
        frame_depth: 0,
        maximum_chunk_bytes,
        maximum_frame_depth: 64,
        maximum_frames: 256,
        maximum_nodes,
        maximum_payload_bytes,
    }
}

async fn collect_page_observation(
    page: &ChromiumPage,
    capture_id: &CaptureId,
    limits: ObservationLimits,
    content: &ContentPolicy,
) -> offprint_model::Result<FrameObservation> {
    Ok(page
        .observe_frame(
            page.session_id(),
            FrameId::new(1),
            capture_id,
            limits,
            content,
        )
        .await?
        .observation)
}

async fn collect_frame_observation(
    page: &ChromiumPage,
    session_id: &str,
    frame_id: FrameId,
    capture_id: &CaptureId,
    limits: ObservationLimits,
    content: &ContentPolicy,
) -> offprint_model::Result<FrameObservation> {
    Ok(page
        .observe_frame(session_id, frame_id, capture_id, limits, content)
        .await?
        .observation)
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn local_browser_navigates_collects_and_closes() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Offprint fixture</title>
                <article id="ready"><h1>ready</h1><input id="name" value="before"></article>
                <script>
                document.getElementById("name").value = "after";
                document.getElementById("ready").attachShadow({mode: "closed"}).innerHTML =
                    "<strong>shadow</strong>";
                </script>"#,
            ),
        )
        .await?;
    let executable = local_executable().await?;
    let process = ChromiumProcess::launch(ChromiumLaunchOptions::new(executable)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;

    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        20,
        Duration::from_secs(10),
    )
    .await?;
    assert_eq!(
        page.evaluate("document.title").await?.as_str(),
        Some("Offprint fixture")
    );
    let observation = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &ContentPolicy::default(),
    )
    .await?;
    assert_eq!(observation.title, "Offprint fixture");
    assert!(observation.html.contains("value=\"after\""));
    assert!(observation.html.contains("shadow"));

    page.close().await?;
    let second = process.new_page(&BrowserEnvironment::default()).await?;
    second.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn collector_serializes_svg_styles_inside_shadow_roots() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Shadow SVG style</title>
                <main id="host"></main>
                <script>
                  document.getElementById("host").attachShadow({mode: "open"}).innerHTML = `
                    <svg viewBox="0 0 10 10">
                      <style>circle { fill: rgb(0, 128, 128); }</style>
                      <circle cx="5" cy="5" r="4"></circle>
                    </svg>
                  `;
                </script>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;

    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        0,
        Duration::from_secs(10),
    )
    .await?;
    let observation = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &ContentPolicy::default(),
    )
    .await?;

    assert!(observation.html.contains("<circle"));
    assert!(observation.html.contains("rgb(0, 128, 128)"));
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn module_graph_reaches_load_with_default_network_guard() -> TestResult {
    let server = FixtureServer::start().await?;
    let mut imports = String::new();
    for index in 0..64 {
        imports.push_str(&format!("import '/module-{index}.js';\n"));
        server
            .register(
                format!("/module-{index}.js"),
                FixtureResponse {
                    status: 200,
                    content_type: "text/javascript; charset=utf-8".to_owned(),
                    headers: BTreeMap::new(),
                    body: format!("export const value = {index};").into_bytes(),
                },
            )
            .await?;
    }
    imports.push_str("document.body.dataset.modules = 'loaded';");
    server
        .register(
            "/entry.js",
            FixtureResponse {
                status: 200,
                content_type: "text/javascript; charset=utf-8".to_owned(),
                headers: BTreeMap::new(),
                body: imports.into_bytes(),
            },
        )
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Module graph</title>
                <script type="module" src="/entry.js"></script>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    let url = server.url("/")?;
    page.enable_network_guard(
        NetworkGuard::new(NetworkPolicy::Standard, &url)?,
        Vec::new(),
        url.clone(),
    )
    .await?;

    page.navigate(&url, ReadinessMode::Load, 0, Duration::from_secs(10))
        .await?;

    assert_eq!(
        page.evaluate("document.body.dataset.modules")
            .await?
            .as_str(),
        Some("loaded")
    );
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn network_idle_waits_for_finite_asset_requests() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register_route(
            "/slow.svg",
            FixtureRoute::delayed(
                FixtureResponse {
                    status: 200,
                    content_type: "image/svg+xml".to_owned(),
                    headers: BTreeMap::new(),
                    body: br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="2">
                        <rect width="2" height="2"/>
                    </svg>"#
                        .to_vec(),
                },
                Duration::from_millis(750),
            ),
        )
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Network idle</title>
                <img src="/slow.svg" alt="">"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    let url = server.url("/")?;

    page.navigate(&url, ReadinessMode::NetworkIdle, 0, Duration::from_secs(10))
        .await?;
    let readiness = page
        .settle(&ReadinessPolicy {
            mode: ReadinessMode::NetworkIdle,
            network_quiet: offprint_model::Milliseconds::new(50),
            ..ReadinessPolicy::default()
        })
        .await?;

    assert_eq!(readiness.reason, "network-idle");
    assert_eq!(readiness.in_flight_requests, 0);
    assert_eq!(
        page.evaluate("document.querySelector('img').naturalWidth > 0")
            .await?
            .as_bool(),
        Some(true)
    );
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn network_idle_completes_after_cross_site_request_changes_sessions() -> TestResult {
    let parent_server = FixtureServer::start().await?;
    let child_server = FixtureServer::start().await?;
    child_server
        .register(
            "/child",
            FixtureResponse::html(
                r#"<!doctype html><title>Child frame</title><main>frame ready</main>"#,
            ),
        )
        .await?;
    let child_url = child_server.url_with_host("/child", "localhost")?;
    parent_server
        .register(
            "/",
            FixtureResponse::html(format!(
                r#"<!doctype html><title>Cross-site idle</title>
                <iframe src="{child_url}"></iframe>"#
            )),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    let parent_url = parent_server.url("/")?;
    page.enable_network_guard(
        NetworkGuard::new(
            NetworkPolicy::Custom(NetworkRules {
                allowed_hosts: BTreeSet::new(),
                allowed_cidrs: BTreeSet::new(),
                allow_loopback: true,
                allow_private: false,
                allow_link_local: false,
            }),
            &parent_url,
        )?,
        Vec::new(),
        parent_url.clone(),
    )
    .await?;

    page.navigate(&parent_url, ReadinessMode::Load, 0, Duration::from_secs(10))
        .await?;
    assert_eq!(page.attached_frames().await?.len(), 1);
    let readiness = tokio::time::timeout(
        Duration::from_secs(2),
        page.settle(&ReadinessPolicy {
            mode: ReadinessMode::NetworkIdle,
            network_quiet: offprint_model::Milliseconds::new(50),
            ..ReadinessPolicy::default()
        }),
    )
    .await;

    page.close().await?;
    process.close().await?;
    parent_server.close().await;
    child_server.close().await;
    let readiness = readiness??;
    assert_eq!(readiness.reason, "network-idle");
    assert_eq!(readiness.in_flight_requests, 0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn network_idle_completes_while_a_blob_worker_remains_active() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Blob worker</title>
                <script>
                  const source = `
                    postMessage("ready");
                    setInterval(() => {}, 1000);
                  `;
                  const url = URL.createObjectURL(
                    new Blob([source], {type: "text/javascript"})
                  );
                  const worker = new Worker(url);
                  window.worker = worker;
                  window.workerReady = new Promise((resolve, reject) => {
                    worker.onmessage = () => {
                      document.body.dataset.worker = "ready";
                      resolve(true);
                    };
                    worker.onerror = reject;
                  });
                </script>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    let url = server.url("/")?;

    page.navigate(&url, ReadinessMode::NetworkIdle, 0, Duration::from_secs(10))
        .await?;
    assert_eq!(
        page.evaluate("window.workerReady").await?.as_bool(),
        Some(true)
    );
    let readiness = tokio::time::timeout(
        Duration::from_secs(2),
        page.settle(&ReadinessPolicy {
            mode: ReadinessMode::NetworkIdle,
            network_quiet: offprint_model::Milliseconds::new(50),
            ..ReadinessPolicy::default()
        }),
    )
    .await??;

    assert_eq!(readiness.reason, "network-idle");
    assert_eq!(readiness.in_flight_requests, 0);
    assert_eq!(
        page.evaluate("document.body.dataset.worker")
            .await?
            .as_str(),
        Some("ready")
    );
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn collector_uses_document_start_primordials_after_page_poisoning() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Hostile fixture</title>
                <main id="ready">
                  <input id="name" value="before">
                  <p>selected content</p>
                </main>
                <script>
                  document.getElementById("name").value = "after";
                  document.getElementById("ready").attachShadow({mode: "closed"}).innerHTML =
                    "<strong>closed shadow survives</strong>";
                </script>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        0,
        Duration::from_secs(10),
    )
    .await?;
    page.evaluate(
        r#"(() => {
          const range = document.createRange();
          range.selectNodeContents(document.getElementById("ready"));
          const selection = getSelection();
          selection.removeAllRanges();
          selection.addRange(range);
          const fail = (name) => function () {
            throw new Error(`page poisoned ${name}`);
          };
          const failGetter = (name) => ({
            configurable: true,
            get: fail(name),
          });
          JSON.stringify = fail("JSON.stringify");
          Object.defineProperty(globalThis, "TextEncoder", {
            configurable: true,
            value: class {
              constructor() {
                throw new Error("page poisoned TextEncoder");
              }
            },
          });
          for (const name of ["get", "set", "delete"]) {
            Object.defineProperty(Map.prototype, name, {
              configurable: true,
              value: fail(`Map.prototype.${name}`),
            });
          }
          for (const name of [
            "cloneNode",
            "appendChild",
            "insertBefore",
            "removeChild",
            "replaceChild",
            "contains",
          ]) {
            Object.defineProperty(Node.prototype, name, {
              configurable: true,
              value: fail(`Node.prototype.${name}`),
            });
          }
          for (const name of [
            "nodeType",
            "firstChild",
            "nextSibling",
            "lastChild",
            "previousSibling",
            "parentNode",
            "ownerDocument",
            "nodeValue",
            "childNodes",
            "textContent",
            "baseURI",
            "namespaceURI",
            "localName",
          ]) {
            Object.defineProperty(Node.prototype, name, failGetter(`Node.prototype.${name}`));
          }
          for (const name of [
            "querySelector",
            "querySelectorAll",
            "setAttribute",
            "getAttribute",
            "hasAttribute",
            "removeAttribute",
            "toggleAttribute",
            "getBoundingClientRect",
          ]) {
            Object.defineProperty(Element.prototype, name, {
              configurable: true,
              value: fail(`Element.prototype.${name}`),
            });
          }
          for (const name of ["attributes", "children", "shadowRoot"]) {
            Object.defineProperty(
              Element.prototype,
              name,
              failGetter(`Element.prototype.${name}`),
            );
          }
          for (const name of [
            "documentElement",
            "doctype",
            "title",
            "characterSet",
            "defaultView",
            "body",
            "scrollingElement",
            "styleSheets",
            "adoptedStyleSheets",
          ]) {
            Object.defineProperty(
              Document.prototype,
              name,
              failGetter(`Document.prototype.${name}`),
            );
          }
          for (const name of [
            "querySelector",
            "querySelectorAll",
            "createElement",
            "createDocumentFragment",
            "getSelection",
            "createTreeWalker",
          ]) {
            Object.defineProperty(Document.prototype, name, {
              configurable: true,
              value: fail(`Document.prototype.${name}`),
            });
          }
          try {
            globalThis.__offprintCollector = {
              call: fail("collector replacement"),
            };
          } catch {}
          globalThis.globalThis = {
            __offprintCollector: {
              call: fail("globalThis collector replacement"),
            },
          };
          Object.defineProperty(Array.prototype, Symbol.iterator, {
            configurable: true,
            value: fail("Array.prototype[Symbol.iterator]"),
          });
          return true;
        })()"#,
    )
    .await?;

    let observation = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &ContentPolicy {
            scope: CaptureScope::Selection,
            ..ContentPolicy::default()
        },
    )
    .await?;

    assert_eq!(observation.title, "Hostile fixture");
    assert_eq!(observation.selection.ranges, 1);
    assert!(observation.html.contains("value=\"after\""));
    assert!(observation.html.contains("selected content"));
    assert!(observation.html.contains("closed shadow survives"));
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn collector_reads_page_controlled_shadow_mode_once() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Shadow options</title>
                <main id="host"></main>
                <script>
                  const host = document.getElementById("host");
                  let modeReads = 0;
                  const init = {};
                  Object.defineProperty(init, "mode", {
                    get() {
                      modeReads += 1;
                      if (modeReads > 1) throw new Error("shadow mode read twice");
                      return "closed";
                    }
                  });
                  host.attachShadow(init).innerHTML = "<strong>closed shadow</strong>";
                  host.setAttribute("data-mode-reads", String(modeReads));
                </script>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        0,
        Duration::from_secs(10),
    )
    .await?;

    let observation = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &ContentPolicy::default(),
    )
    .await?;

    assert!(observation.html.contains(r#"data-mode-reads="1""#));
    assert!(observation.html.contains("closed shadow"));
    assert!(
        observation
            .html
            .contains(r#"data-offprint-shadow-mode="closed""#)
    );
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn structural_repair_uses_captured_attribute_writes() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Repair setter</title>
                <main id="mount"></main>
                <script>
                  const outer = document.createElement("p");
                  const inner = document.createElement("div");
                  inner.textContent = "browser-owned nesting";
                  outer.append(inner);
                  document.getElementById("mount").append(outer);
                  const descriptor =
                    Object.getOwnPropertyDescriptor(HTMLScriptElement.prototype, "type");
                  Object.defineProperty(HTMLScriptElement.prototype, "type", {
                    configurable: true,
                    get: descriptor?.get,
                    set() {
                      throw new Error("page poisoned HTMLScriptElement.type");
                    }
                  });
                  document.getElementById("mount")
                    .setAttribute("data-type-setter", "poisoned");
                </script>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        0,
        Duration::from_secs(10),
    )
    .await?;

    let observation = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &ContentPolicy::default(),
    )
    .await?;

    assert!(observation.html.contains("browser-owned nesting"));
    assert!(observation.html.contains(r#"data-type-setter="poisoned""#));
    assert!(observation.html.contains("offprint-repair-data"));
    assert!(
        observation
            .html
            .contains("application/vnd.offprint.repair+json")
    );
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn freeze_orchestration_uses_document_start_primordials() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Freeze primordials</title>
                <main>stable content</main>
                <script>
                  const root = document.documentElement;
                  const fail = (name) => function () {
                    throw new Error(`page poisoned ${name}`);
                  };
                  Object.defineProperty(Document.prototype, "querySelector", {
                    configurable: true,
                    value: fail("Document.prototype.querySelector"),
                  });
                  Object.defineProperty(Document.prototype, "createElement", {
                    configurable: true,
                    value: fail("Document.prototype.createElement"),
                  });
                  Object.defineProperty(Document.prototype, "head", {
                    configurable: true,
                    get: fail("Document.prototype.head"),
                  });
                  Object.defineProperty(Element.prototype, "append", {
                    configurable: true,
                    value: fail("Element.prototype.append"),
                  });
                  globalThis.Promise = class {
                    constructor() {
                      throw new Error("page poisoned Promise");
                    }
                  };
                  globalThis.requestAnimationFrame = fail("requestAnimationFrame");
                  globalThis.setTimeout = fail("setTimeout");
                  root.setAttribute("data-freeze-poisoned", "true");
                </script>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        0,
        Duration::from_secs(10),
    )
    .await?;
    page.settle(&ReadinessPolicy {
        mode: ReadinessMode::Load,
        ..ReadinessPolicy::default()
    })
    .await?;

    let observation = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &ContentPolicy::default(),
    )
    .await?;

    assert!(observation.html.contains(r#"data-freeze-poisoned="true""#));
    assert!(observation.html.contains("stable content"));
    assert!(observation.html.contains("data-offprint-freeze"));
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn capture_delay_uses_a_browser_host_timer() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Capture delay</title>
                <script>
                  globalThis.setTimeout = () => {
                    throw new Error("page poisoned setTimeout");
                  };
                </script>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        0,
        Duration::from_secs(10),
    )
    .await?;
    let started = tokio::time::Instant::now();

    let readiness = page
        .settle(&ReadinessPolicy {
            mode: ReadinessMode::Load,
            delay: offprint_model::Milliseconds::new(50),
            ..ReadinessPolicy::default()
        })
        .await?;

    assert!(started.elapsed() >= Duration::from_millis(50));
    assert_eq!(readiness.reason, "load");
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn collector_serializes_svg_mathml_and_html_integration_points() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r##"<!doctype html><title>Foreign namespaces</title>
                <svg id="graphic" xmlns="http://www.w3.org/2000/svg"
                  xmlns:xlink="http://www.w3.org/1999/xlink">
                  <defs>
                    <linearGradient id="paint"><stop offset="50%"></stop></linearGradient>
                    <path id="shape" d="M0 0L8 8"></path>
                  </defs>
                  <use id="painted" xlink:href="#shape"></use>
                  <foreignObject>
                    <article id="foreign-html" xmlns="http://www.w3.org/1999/xhtml">
                      foreign HTML
                    </article>
                  </foreignObject>
                </svg>
                <math id="formula" xmlns="http://www.w3.org/1998/Math/MathML">
                  <mfrac><mi>x</mi><mn>2</mn></mfrac>
                </math>"##,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        0,
        Duration::from_secs(10),
    )
    .await?;

    let observation = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &ContentPolicy::default(),
    )
    .await?;
    assert!(observation.html.contains("<linearGradient"));
    assert!(observation.html.contains(r##"xlink:href="#shape""##));

    let captured = format!("{}{}", observation.doctype, observation.html);
    let encoded = base64::engine::general_purpose::STANDARD.encode(captured);
    let replay_url = url::Url::parse(&format!("data:text/html;base64,{encoded}"))?;
    let replay = process.new_page(&BrowserEnvironment::default()).await?;
    replay
        .navigate(&replay_url, ReadinessMode::Load, 0, Duration::from_secs(10))
        .await?;
    let namespaces = replay
        .evaluate(
            r##"(() => {
              const svg = document.getElementById("graphic");
              const gradient = document.getElementById("paint");
              const use = document.getElementById("painted");
              const foreignHtml = document.getElementById("foreign-html");
              const math = document.getElementById("formula");
              return svg?.namespaceURI === "http://www.w3.org/2000/svg"
                && gradient?.localName === "linearGradient"
                && use?.getAttributeNS("http://www.w3.org/1999/xlink", "href") === "#shape"
                && foreignHtml?.namespaceURI === "http://www.w3.org/1999/xhtml"
                && math?.namespaceURI === "http://www.w3.org/1998/Math/MathML";
            })()"##,
        )
        .await?;
    assert_eq!(namespaces.as_bool(), Some(true));

    replay.close().await?;
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn direct_page_navigation_keeps_standard_network_boundaries() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html("<!doctype html><title>initial</title>"),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        0,
        Duration::from_secs(10),
    )
    .await?;
    let metadata = url::Url::parse("http://169.254.169.254/latest/meta-data/")?;

    let blocked = page
        .navigate(&metadata, ReadinessMode::Load, 0, Duration::from_secs(2))
        .await;

    assert!(blocked.is_err());
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn capture_contexts_deny_download_output_in_headless_and_headed_modes() -> TestResult {
    const DOWNLOAD_NAME: &str = "offprint-context-download.bin";

    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(format!(
                r#"<!doctype html><title>Download fixture</title>
                <a id="download" download="{DOWNLOAD_NAME}" href="/download.bin">download</a>"#
            )),
        )
        .await?;
    server
        .register(
            "/download.bin",
            FixtureResponse {
                status: 200,
                content_type: "application/octet-stream".to_owned(),
                headers: BTreeMap::from([(
                    "content-disposition".to_owned(),
                    format!("attachment; filename=\"{DOWNLOAD_NAME}\""),
                )]),
                body: vec![42_u8; 1024],
            },
        )
        .await?;

    for headless in [true, false] {
        let mut options = ChromiumLaunchOptions::new(local_executable().await?);
        options.headless = headless;
        let process = ChromiumProcess::launch(options).await?;
        let page = process.new_page(&BrowserEnvironment::default()).await?;
        page.navigate(
            &server.url("/")?,
            ReadinessMode::Load,
            0,
            Duration::from_secs(10),
        )
        .await?;
        page.evaluate("(() => { document.getElementById('download').click(); return true; })()")
            .await?;
        tokio::time::sleep(Duration::from_millis(500)).await;

        assert!(!profile_contains_download(
            process.profile_path(),
            DOWNLOAD_NAME
        )?);
        page.close().await?;
        process.close().await?;
    }

    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn observed_svg_response_preserves_its_mime_and_bytes() -> TestResult {
    let server = FixtureServer::start().await?;
    let svg =
        br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="8" height="8"/></svg>"#.to_vec();
    server
        .register(
            "/asset.svg",
            FixtureResponse {
                status: 200,
                content_type: "image/svg+xml".to_owned(),
                headers: BTreeMap::new(),
                body: svg.clone(),
            },
        )
        .await?;
    server
        .register("/", FixtureResponse::html(r#"<img src="/asset.svg">"#))
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    let root_url = server.url("/")?;
    page.enable_network_guard(
        NetworkGuard::new(NetworkPolicy::Standard, &root_url)?,
        Vec::new(),
        root_url.clone(),
    )
    .await?;
    let navigation = page
        .navigate(&root_url, ReadinessMode::Load, 0, Duration::from_secs(10))
        .await?;
    let resource_url = server.url("/asset.svg")?;
    let mut resource = page
        .load_resource(&navigation.frame_id, &resource_url, 1024 * 1024)
        .await?;
    let mut body = Vec::new();
    while let Some(chunk) = resource.body.next().await {
        body.extend_from_slice(&chunk?);
    }

    assert_eq!(resource.source, ResourceRetrievalSource::ObservedResponse);
    assert_eq!(resource.media_type.as_deref(), Some("image/svg+xml"));
    assert_eq!(body, svg);
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn observed_response_honors_a_lower_per_resource_limit() -> TestResult {
    let server = FixtureServer::start().await?;
    let svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg'><metadata>{}</metadata></svg>",
        "x".repeat(1024)
    );
    server
        .register(
            "/asset.svg",
            FixtureResponse {
                status: 200,
                content_type: "image/svg+xml".to_owned(),
                headers: BTreeMap::new(),
                body: svg.into_bytes(),
            },
        )
        .await?;
    server
        .register("/", FixtureResponse::html(r#"<img src="/asset.svg">"#))
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = ChromiumPage::create_with_resource_limits(
        process.client().clone(),
        &BrowserEnvironment::default(),
        16,
        ResourceObservationLimits {
            maximum_resource_bytes: 256,
            maximum_total_resource_bytes: 4096,
        },
    )
    .await?;
    let root_url = server.url("/")?;
    page.enable_network_guard(
        NetworkGuard::new(NetworkPolicy::Standard, &root_url)?,
        Vec::new(),
        root_url.clone(),
    )
    .await?;
    let navigation = page
        .navigate(&root_url, ReadinessMode::Load, 0, Duration::from_secs(10))
        .await?;
    let result = page
        .load_resource(&navigation.frame_id, &server.url("/asset.svg")?, 4096)
        .await;
    let Err(error) = result else {
        return Err(
            std::io::Error::other("the observed body stayed below its configured limit").into(),
        );
    };

    assert_eq!(error.code.as_str(), "offprint.resource.limit", "{error:?}");
    assert_eq!(error.details.get("limit"), Some(&serde_json::json!(256)));
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn observed_responses_share_the_configured_total_limit() -> TestResult {
    let server = FixtureServer::start().await?;
    let first = format!(
        "<svg xmlns='http://www.w3.org/2000/svg'><metadata>{}</metadata></svg>",
        "a".repeat(300)
    );
    let second = format!(
        "<svg xmlns='http://www.w3.org/2000/svg'><metadata>{}</metadata></svg>",
        "b".repeat(300)
    );
    server
        .register(
            "/first.svg",
            FixtureResponse {
                status: 200,
                content_type: "image/svg+xml".to_owned(),
                headers: BTreeMap::new(),
                body: first.into_bytes(),
            },
        )
        .await?;
    server
        .register(
            "/second.svg",
            FixtureResponse {
                status: 200,
                content_type: "image/svg+xml".to_owned(),
                headers: BTreeMap::new(),
                body: second.into_bytes(),
            },
        )
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html(r#"<img src="/first.svg"><img src="/second.svg">"#),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = ChromiumPage::create_with_resource_limits(
        process.client().clone(),
        &BrowserEnvironment::default(),
        16,
        ResourceObservationLimits {
            maximum_resource_bytes: 512,
            maximum_total_resource_bytes: 600,
        },
    )
    .await?;
    let root_url = server.url("/")?;
    page.enable_network_guard(
        NetworkGuard::new(NetworkPolicy::Standard, &root_url)?,
        Vec::new(),
        root_url.clone(),
    )
    .await?;
    let navigation = page
        .navigate(&root_url, ReadinessMode::Load, 0, Duration::from_secs(10))
        .await?;
    let first = page
        .load_resource(&navigation.frame_id, &server.url("/first.svg")?, 512)
        .await;
    let second = page
        .load_resource(&navigation.frame_id, &server.url("/second.svg")?, 512)
        .await;
    let errors = [first.as_ref().err(), second.as_ref().err()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].code.as_str(),
        "offprint.resource.limit",
        "{:?}",
        errors[0]
    );
    assert_eq!(
        errors[0].details.get("limit"),
        Some(&serde_json::json!(600))
    );
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn observed_gzip_response_yields_rendered_bytes() -> TestResult {
    let server = FixtureServer::start().await?;
    let svg =
        br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="8" height="8"/></svg>"#.to_vec();
    server
        .register(
            "/asset.svg",
            FixtureResponse {
                status: 200,
                content_type: "image/svg+xml".to_owned(),
                headers: BTreeMap::new(),
                body: svg.clone(),
            }
            .gzip()?,
        )
        .await?;
    server
        .register("/", FixtureResponse::html(r#"<img src="/asset.svg">"#))
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    let root_url = server.url("/")?;
    page.enable_network_guard(
        NetworkGuard::new(NetworkPolicy::Standard, &root_url)?,
        Vec::new(),
        root_url.clone(),
    )
    .await?;
    let navigation = page
        .navigate(&root_url, ReadinessMode::Load, 0, Duration::from_secs(10))
        .await?;
    let mut resource = page
        .load_resource(
            &navigation.frame_id,
            &server.url("/asset.svg")?,
            1024 * 1024,
        )
        .await?;
    let mut body = Vec::new();
    while let Some(chunk) = resource.body.next().await {
        body.extend_from_slice(&chunk?);
    }

    assert_eq!(resource.source, ResourceRetrievalSource::ObservedResponse);
    assert_eq!(resource.media_type.as_deref(), Some("image/svg+xml"));
    assert_eq!(body, svg);
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn collector_rejects_an_observation_over_the_frame_payload_limit() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(format!(
                "<!doctype html><title>Payload limit</title><main>{}</main>",
                "bounded collector payload ".repeat(256)
            )),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        20,
        Duration::from_secs(10),
    )
    .await?;

    let result = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64, 128, 1_000_000),
        &ContentPolicy::default(),
    )
    .await;

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.collector.payload_limit")
    );
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn collector_stops_streaming_a_large_css_rule_list_at_the_payload_limit() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>CSS rule list limit</title>
                <main>bounded CSS rule list</main>
                <script>
                  const sheet = new CSSStyleSheet();
                  for (let index = 0; index < 4096; index += 1) {
                    sheet.insertRule(`.rule-${index} { color: red; }`);
                  }
                  document.adoptedStyleSheets = [sheet];
                </script>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        0,
        Duration::from_secs(10),
    )
    .await?;

    let result = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 4 * 1024, 1_000_000),
        &ContentPolicy::default(),
    )
    .await;

    let error = result.as_ref().err();
    assert_eq!(
        error.map(|error| error.code.as_str()),
        Some("offprint.collector.payload_limit")
    );
    assert_eq!(
        error.and_then(|error| error.details.get("attempted")),
        Some(&serde_json::json!(4 * 1024 + 1))
    );
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn collector_reserves_oversized_adopted_css_before_native_serialization() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Adopted CSS limit</title>
                <main class="payload">bounded adopted stylesheet</main>
                <script>
                  const sheet = new CSSStyleSheet();
                  sheet.replaceSync(`.payload { --large: "${"x".repeat(128 * 1024)}"; }`);
                  document.adoptedStyleSheets = [sheet];
                </script>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        0,
        Duration::from_secs(10),
    )
    .await?;

    let result = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 8 * 1024, 1_000_000),
        &ContentPolicy::default(),
    )
    .await;

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.collector.payload_limit")
    );
    assert_eq!(
        result
            .as_ref()
            .err()
            .and_then(|error| error.details.get("limit")),
        Some(&serde_json::json!(8 * 1024))
    );
    assert_eq!(
        result
            .as_ref()
            .err()
            .and_then(|error| error.details.get("attempted")),
        Some(&serde_json::json!(8 * 1024 + 1))
    );
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn collector_bounds_canvas_png_before_data_url_encoding() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Canvas limit</title>
                <canvas width="4096" height="4096"></canvas>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        0,
        Duration::from_secs(10),
    )
    .await?;

    let result = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 16 * 1024, 1_000_000),
        &ContentPolicy::default(),
    )
    .await;

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.collector.payload_limit")
    );
    assert_eq!(
        result
            .as_ref()
            .err()
            .and_then(|error| error.details.get("limit")),
        Some(&serde_json::json!(16 * 1024))
    );
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn collector_rejects_the_node_limit_before_snapshot_allocation() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                "<!doctype html><title>Node limit</title><main>bounded DOM</main>",
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        20,
        Duration::from_secs(10),
    )
    .await?;
    page.evaluate(
        r#"(() => {
          Object.defineProperty(Node.prototype, "cloneNode", {
            configurable: true,
            value() {
              throw new Error("cloneNode ran after the node budget was exceeded");
            },
          });
          JSON.stringify = () => {
            throw new Error("JSON serialization ran after the node budget was exceeded");
          };
          Object.defineProperty(globalThis, "TextEncoder", {
            configurable: true,
            value: class {
              constructor() {
                throw new Error("UTF-8 encoding ran after the node budget was exceeded");
              }
            },
          });
          return true;
        })()"#,
    )
    .await?;

    let result = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64, 1024 * 1024, 0),
        &ContentPolicy::default(),
    )
    .await;
    let error = result.as_ref().err();

    assert_eq!(
        error.map(|error| error.code.as_str()),
        Some("offprint.frame.nodes")
    );
    assert_eq!(
        error.and_then(|error| error.details.get("attempted")),
        Some(&serde_json::json!(1))
    );
    assert_eq!(
        error.and_then(|error| error.details.get("limit")),
        Some(&serde_json::json!(0))
    );
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn same_origin_frame_limits_precede_child_snapshot_allocation() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/child",
            FixtureResponse::html(format!(
                "<!doctype html><title>Large child</title><main>{}</main>",
                "<span>child payload</span>".repeat(20_000)
            )),
        )
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Frame budget</title><iframe src="/child"></iframe>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        0,
        Duration::from_secs(10),
    )
    .await?;

    let frame_result = collect_page_observation(
        &page,
        &CaptureId::new(),
        ObservationLimits {
            frame_depth: 0,
            maximum_chunk_bytes: 64 * 1024,
            maximum_frame_depth: 8,
            maximum_frames: 1,
            maximum_nodes: 10,
            maximum_payload_bytes: 1024 * 1024,
        },
        &ContentPolicy::default(),
    )
    .await;
    let frame_error = frame_result.as_ref().err();
    assert_eq!(
        frame_error.map(|error| error.code.as_str()),
        Some("offprint.frame.limit")
    );
    assert_eq!(
        frame_error.and_then(|error| error.details.get("attempted")),
        Some(&serde_json::json!(2))
    );
    assert_eq!(
        frame_error.and_then(|error| error.details.get("limit")),
        Some(&serde_json::json!(1))
    );

    let depth_result = collect_page_observation(
        &page,
        &CaptureId::new(),
        ObservationLimits {
            frame_depth: 0,
            maximum_chunk_bytes: 64 * 1024,
            maximum_frame_depth: 0,
            maximum_frames: 8,
            maximum_nodes: 10,
            maximum_payload_bytes: 1024 * 1024,
        },
        &ContentPolicy::default(),
    )
    .await;
    let depth_error = depth_result.as_ref().err();
    assert_eq!(
        depth_error.map(|error| error.code.as_str()),
        Some("offprint.frame.depth")
    );
    assert_eq!(
        depth_error.and_then(|error| error.details.get("attempted")),
        Some(&serde_json::json!(1))
    );
    assert_eq!(
        depth_error.and_then(|error| error.details.get("limit")),
        Some(&serde_json::json!(0))
    );

    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn collector_materializes_motion_and_shadow_state_at_the_freeze_boundary() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><html><head><title>State fidelity</title>
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
                    <div id="shadow-animated"></div>`;
                    const shadowScroller = root.getElementById("shadow-scroll");
                    shadowScroller.scrollLeft = 25;
                    shadowScroller.scrollTop = 45;
                    const shadowAnimation =
                      root.getElementById("shadow-animated").getAnimations()[0];
                    shadowAnimation.pause();
                    shadowAnimation.currentTime = 1000;
                  };
                  attachStatefulShadow(document.getElementById("open-host"), "open");
                  attachStatefulShadow(document.getElementById("closed-host"), "closed");
                </script></body></html>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        20,
        Duration::from_secs(10),
    )
    .await?;
    page.freeze_attached_frames().await?;

    let observation = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &ContentPolicy::default(),
    )
    .await?;

    assert_eq!(observation.viewport.scroll_x, "0");
    assert_eq!(observation.viewport.scroll_y, "240");
    assert!(
        observation.html.contains(r#"data-offprint-scroll-y="240""#),
        "{}",
        observation.html
    );
    assert!(
        observation
            .html
            .contains(r#"data-offprint-scroll-left="30""#),
        "{}",
        observation.html
    );
    assert!(
        observation
            .html
            .contains(r#"data-offprint-scroll-top="70""#),
        "{}",
        observation.html
    );
    assert!(
        observation
            .html
            .matches("matrix(1, 0, 0, 1, 60, 0)")
            .count()
            >= 4,
        "warnings: {:?}\n{}",
        observation.warnings,
        observation.html
    );
    assert!(
        observation
            .html
            .matches("data-offprint-scroll-left")
            .count()
            >= 3
            && observation.html.matches("data-offprint-scroll-top").count() >= 3,
        "{}",
        observation.html
    );
    assert!(
        observation.html.matches(r#"shadowrootmode="open""#).count() >= 2
            && observation
                .html
                .contains(r#"data-offprint-shadow-mode="closed""#)
            && !observation.html.contains(r#"shadowrootmode="closed""#),
        "{}",
        observation.html
    );

    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn local_browser_can_be_owned_by_a_spawned_job() -> TestResult {
    let task = tokio::spawn(async {
        let executable = local_executable().await?;
        let process = ChromiumProcess::launch(ChromiumLaunchOptions::new(executable)).await?;
        let page = process.new_page(&BrowserEnvironment::default()).await?;
        page.close().await?;
        process.close().await?;
        Ok::<(), Box<dyn Error + Send + Sync>>(())
    });

    task.await??;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn cross_site_iframe_uses_an_attached_collector_session() -> TestResult {
    let parent_server = FixtureServer::start().await?;
    let child_server = FixtureServer::start().await?;
    child_server
        .register(
            "/child",
            FixtureResponse::html(
                r#"<title>Child fixture</title><main id="child">cross-site frame</main>"#,
            ),
        )
        .await?;
    let mut child_url = child_server.url("/child")?;
    child_url.set_host(Some("localhost"))?;
    parent_server
        .register(
            "/",
            FixtureResponse::html(format!(
                r#"<title>Parent fixture</title><iframe src="{child_url}"></iframe>"#
            )),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    let parent_url = parent_server.url("/")?;
    page.enable_network_guard(
        NetworkGuard::new(
            NetworkPolicy::Custom(NetworkRules {
                allowed_hosts: BTreeSet::new(),
                allowed_cidrs: BTreeSet::new(),
                allow_loopback: true,
                allow_private: false,
                allow_link_local: false,
            }),
            &parent_url,
        )?,
        Vec::new(),
        parent_url.clone(),
    )
    .await?;

    page.navigate(
        &parent_url,
        ReadinessMode::Load,
        20,
        Duration::from_secs(10),
    )
    .await?;
    let frames = page.attached_frames().await?;
    assert_eq!(frames.len(), 1);
    let frame = &frames[0];
    let observation = collect_frame_observation(
        &page,
        &frame.session_id,
        FrameId::new(2),
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &ContentPolicy::default(),
    )
    .await?;

    assert_eq!(observation.title, "Child fixture");
    assert_eq!(page.frame_owner_path(frame).await?, vec![0]);
    page.close().await?;
    process.close().await?;
    parent_server.close().await;
    child_server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn nested_oopif_owner_path_uses_isolated_world_intrinsics() -> TestResult {
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
                r#"<!doctype html><title>Nested owner</title>
                <iframe srcdoc="<p>root decoy</p>"></iframe>
                <iframe src="/outer"></iframe>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &parent_server.url("/")?,
        ReadinessMode::Load,
        20,
        Duration::from_secs(10),
    )
    .await?;

    let frames = page.attached_frames().await?;

    assert_eq!(frames.len(), 1);
    assert_eq!(page.frame_owner_path(&frames[0]).await?, vec![1, 1]);
    page.close().await?;
    process.close().await?;
    parent_server.close().await;
    child_server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn cross_site_iframe_enforces_the_collector_payload_limit() -> TestResult {
    let parent_server = FixtureServer::start().await?;
    let child_server = FixtureServer::start().await?;
    child_server
        .register(
            "/child",
            FixtureResponse::html(format!(
                "<title>Child payload</title><main>{}</main>",
                "bounded OOPIF payload ".repeat(256)
            )),
        )
        .await?;
    let mut child_url = child_server.url("/child")?;
    child_url.set_host(Some("localhost"))?;
    parent_server
        .register(
            "/",
            FixtureResponse::html(format!(
                r#"<title>Parent fixture</title><iframe src="{child_url}"></iframe>"#
            )),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    let parent_url = parent_server.url("/")?;
    page.enable_network_guard(
        NetworkGuard::new(
            NetworkPolicy::Custom(NetworkRules {
                allowed_hosts: BTreeSet::new(),
                allowed_cidrs: BTreeSet::new(),
                allow_loopback: true,
                allow_private: false,
                allow_link_local: false,
            }),
            &parent_url,
        )?,
        Vec::new(),
        parent_url.clone(),
    )
    .await?;
    page.navigate(
        &parent_url,
        ReadinessMode::Load,
        20,
        Duration::from_secs(10),
    )
    .await?;
    let frames = page.attached_frames().await?;
    assert_eq!(frames.len(), 1);
    let frame = &frames[0];

    let result = collect_frame_observation(
        &page,
        &frame.session_id,
        FrameId::new(2),
        &CaptureId::new(),
        collector_limits(64, 128, 1_000_000),
        &ContentPolicy::default(),
    )
    .await;

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.collector.payload_limit")
    );
    page.close().await?;
    process.close().await?;
    parent_server.close().await;
    child_server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn offline_verification_accepts_deferred_lazy_images() -> TestResult {
    let mut artifact = tempfile::Builder::new().suffix(".html").tempfile()?;
    artifact.write_all(
        br#"<!doctype html>
        <title>Deferred image fixture</title>
        <main style="height: 100000px">visible content</main>
        <img loading="lazy" width="1" height="1"
             src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==">
        <div id="host"><template shadowrootmode="open"><img loading="lazy"
          src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw=="></template></div>
        <iframe srcdoc="<img loading='lazy' src='data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw=='>"></iframe>
"#,
    )?;
    artifact.flush()?;
    let artifact_url = url::Url::from_file_path(artifact.path())
        .map_err(|()| std::io::Error::other("artifact path is not a file URL"))?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;

    let observation = page
        .verify_offline_url(
            &artifact_url,
            Duration::from_secs(10),
            offprint_browser::RenderingMedia::Screen,
        )
        .await?;

    assert!(observation.stable, "{observation:?}");
    assert!(observation.attempted_urls.is_empty());
    assert!(observation.page_errors.is_empty());
    assert_eq!(
        page.evaluate(
            r#"({
      offscreen: document.querySelector('iframe').getBoundingClientRect().top >= innerHeight,
      loading: document.querySelector('iframe').contentDocument.querySelector('img').loading
    })"#
        )
        .await?,
        serde_json::json!({"offscreen":true,"loading":"lazy"})
    );
    let printed = page
        .verify_offline_url(
            &artifact_url,
            Duration::from_secs(10),
            offprint_browser::RenderingMedia::Print,
        )
        .await?;
    assert!(printed.stable, "{printed:?}");
    assert!(printed.attempted_urls.is_empty());
    assert!(printed.page_errors.is_empty());
    assert_eq!(
        page.evaluate(
            r#"({
      media: matchMedia('print').matches,
      images: [document.querySelector('img'),
        document.getElementById('host').shadowRoot.querySelector('img'),
        document.querySelector('iframe').contentDocument.querySelector('img')]
        .map(image => ({loading:image.loading, complete:image.complete, width:image.naturalWidth}))
    })"#
        )
        .await?,
        serde_json::json!({
            "media": true,
            "images": [{"loading":"eager","complete":true,"width":1},
                       {"loading":"eager","complete":true,"width":1},
                       {"loading":"eager","complete":true,"width":1}],
        })
    );
    page.close().await?;
    process.close().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn offline_verification_accepts_large_inline_resources() -> TestResult {
    let padding = "x".repeat(900 * 1024);
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">
        <metadata>{padding}</metadata><rect width="1" height="1"/>
        </svg>"#
    );
    let encoded = base64::engine::general_purpose::STANDARD.encode(svg);
    let mut artifact = tempfile::Builder::new().suffix(".html").tempfile()?;
    artifact.write_all(
        format!(
            r#"<!doctype html><title>Large inline resource fixture</title>
            <img src="data:image/svg+xml;base64,{encoded}">"#
        )
        .as_bytes(),
    )?;
    artifact.flush()?;
    let artifact_url = url::Url::from_file_path(artifact.path())
        .map_err(|()| std::io::Error::other("artifact path is not a file URL"))?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;

    let observation = page
        .verify_offline_url(
            &artifact_url,
            Duration::from_secs(10),
            offprint_browser::RenderingMedia::Screen,
        )
        .await?;

    assert!(observation.stable);
    assert!(observation.attempted_urls.is_empty());
    assert!(observation.page_errors.is_empty());
    page.close().await?;
    process.close().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn offline_verification_keeps_up_with_large_event_stream() -> TestResult {
    let images = (0..512)
        .map(|index| {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1">
                <metadata>{index}</metadata><rect width="1" height="1"/>
                </svg>"#
            );
            let encoded = base64::engine::general_purpose::STANDARD.encode(svg);
            format!(r#"<img src="data:image/svg+xml;base64,{encoded}">"#)
        })
        .collect::<String>();
    let mut artifact = tempfile::Builder::new().suffix(".html").tempfile()?;
    artifact.write_all(
        format!("<!doctype html><title>Event stream fixture</title>{images}").as_bytes(),
    )?;
    artifact.flush()?;
    let artifact_url = url::Url::from_file_path(artifact.path())
        .map_err(|()| std::io::Error::other("artifact path is not a file URL"))?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;

    // Each image decodes a separate SVG document within the capture budget.
    let observation = page
        .verify_offline_url(
            &artifact_url,
            Duration::from(offprint_model::CaptureLimits::default().duration),
            offprint_browser::RenderingMedia::Screen,
        )
        .await?;

    assert!(observation.stable, "{observation:#?}");
    assert!(observation.attempted_urls.is_empty());
    assert!(observation.page_errors.is_empty());
    page.close().await?;
    process.close().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn offline_verification_ignores_rendering_diagnostics() -> TestResult {
    let mut artifact = tempfile::Builder::new().suffix(".html").tempfile()?;
    artifact.write_all(
        br#"<!doctype html>
        <title>Rendering diagnostic fixture</title>
        <svg xmlns="http://www.w3.org/2000/svg" width="32" height="auto">
          <path d="M0 31 L16 1 L31 31" stroke="blue" fill="none"/>
        </svg>"#,
    )?;
    artifact.flush()?;
    let artifact_url = url::Url::from_file_path(artifact.path())
        .map_err(|()| std::io::Error::other("artifact path is not a file URL"))?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;

    let observation = page
        .verify_offline_url(
            &artifact_url,
            Duration::from_secs(10),
            offprint_browser::RenderingMedia::Screen,
        )
        .await?;

    assert!(observation.stable);
    assert!(observation.attempted_urls.is_empty());
    assert!(observation.page_errors.is_empty());
    page.close().await?;
    process.close().await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn offline_verification_contains_page_and_nested_worker_transports() -> TestResult {
    let server = FixtureServer::start().await?;
    let fetch_url = server.url("/worker-fetch")?.to_string();
    let mut socket_url = server.url("/worker-socket")?;
    socket_url
        .set_scheme("ws")
        .map_err(|()| std::io::Error::other("fixture URL cannot use WebSocket scheme"))?;
    let nested_source = format!(
        "setTimeout(() => postMessage('nested-started'), 100); \
         setInterval(() => {{}}, 1000); \
         fetch({}).catch(() => {{}}); \
         try {{ new WebSocket({}); }} catch (_) {{}}",
        serde_json::to_string(&server.url("/nested-fetch")?.to_string())?,
        serde_json::to_string(socket_url.as_str())?,
    );
    let outer_source = format!(
        "setTimeout(() => postMessage('outer-started'), 100); \
         setInterval(() => {{}}, 1000); \
         fetch({}).catch(() => {{}}); \
         try {{ new WebSocket({}); }} catch (_) {{}} \
         const source = {}; \
         const nested = new Worker(URL.createObjectURL(new Blob([source], {{type: 'text/javascript'}}))); \
         nested.onmessage = (event) => postMessage(event.data);",
        serde_json::to_string(&fetch_url)?,
        serde_json::to_string(socket_url.as_str())?,
        serde_json::to_string(&nested_source)?,
    );
    let transport_probe = format!(
        r#"const outcomes = {{}};
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
        document.querySelector("main").dataset.transports = JSON.stringify(outcomes);"#,
        serde_json::to_string(socket_url.as_str())?,
        serde_json::to_string(&server.url("/event-source")?.to_string())?,
    );
    let mut artifact = tempfile::Builder::new().suffix(".html").tempfile()?;
    artifact.write_all(
        format!(
            r#"<!doctype html><title>Offline transport fixture</title>
            <main>visible content</main>
            <script>
            document.querySelector("main").dataset.scriptStarted = "true";
            const source = {};
            const workerMessages = [];
            try {{
                const worker = new Worker(URL.createObjectURL(new Blob([source], {{type: "text/javascript"}})));
                worker.onerror = (event) => {{
                    document.querySelector("main").dataset.workerError = event.message;
                }};
                worker.onmessage = (event) => {{
                    workerMessages.push(event.data);
                    document.querySelector("main").dataset.workers =
                        workerMessages.slice().sort().join(",");
                }};
            }} catch (error) {{
                document.querySelector("main").dataset.workerError =
                    `${{error.name}}:${{error.message}}`;
            }}
            fetch({}).catch(() => {{}});
            {transport_probe}
            </script>"#,
            serde_json::to_string(&outer_source)?,
            serde_json::to_string(&fetch_url)?,
        )
        .as_bytes(),
    )?;
    artifact.flush()?;
    let artifact_url = url::Url::from_file_path(artifact.path())
        .map_err(|()| std::io::Error::other("artifact path is not a file URL"))?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;

    let observation = page
        .verify_offline_url(
            &artifact_url,
            Duration::from_secs(10),
            offprint_browser::RenderingMedia::Screen,
        )
        .await?;

    assert!(observation.stable);
    assert!(!observation.attempted_urls.is_empty());
    let worker_state = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let state = page
                .evaluate("document.querySelector('main')?.dataset.workers || ''")
                .await?;
            if state.as_str() == Some("nested-started,outer-started") {
                return Ok::<Value, offprint_model::OffprintError>(state);
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await??;
    assert_eq!(worker_state.as_str(), Some("nested-started,outer-started"));
    let transports = page
        .evaluate("JSON.parse(document.querySelector('main')?.dataset.transports || '{}')")
        .await?;
    assert_eq!(
        transports,
        serde_json::json!({
            "webSocket": "allowed",
            "eventSource": "allowed",
            "rtc": "SecurityError"
        })
    );
    assert!(server.requests().await.is_empty());
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn collector_applies_selection_and_visual_optimizers() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r#"<!doctype html><title>Optimizer fixture</title>
                <style>
                  .used { color: rgb(1, 2, 3) }
                  .unused { background-image: url("/unused.png") }
                  @font-face { font-family: "Used"; src: url("data:font/woff2;base64,d09GMgAB") }
                  @font-face { font-family: "Unused"; src: url("/unused.woff2") }
                  .used-font { font-family: "Used", sans-serif }
                </style>
                <main>
                  <p id="selected" class="used used-font">selected text</p>
                  <p id="other">other text</p>
                  <p id="hidden" style="display: none">hidden text</p>
                </main>"#,
            ),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &server.url("/")?,
        ReadinessMode::Load,
        20,
        Duration::from_secs(10),
    )
    .await?;
    page.evaluate(
        r#"(() => {
          const range = document.createRange();
          range.selectNodeContents(document.getElementById("selected"));
          const selection = getSelection();
          selection.removeAllRanges();
          selection.addRange(range);
          return selection.toString();
        })()"#,
    )
    .await?;
    let policy = ContentPolicy {
        scope: CaptureScope::Selection,
        optimizations: OptimizationPolicy {
            remove_unused_css: true,
            remove_unused_fonts: true,
            remove_hidden_elements: true,
        },
        ..ContentPolicy::default()
    };

    let observation = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &policy,
    )
    .await?;

    assert_eq!(observation.selection.ranges, 1);
    assert!(observation.html.contains("selected text"));
    assert!(observation.html.contains(".used"));
    assert!(!observation.html.contains("other text"));
    assert!(!observation.html.contains("hidden text"));
    assert!(!observation.html.contains(".unused"));
    assert!(!observation.html.contains("font-family: \"Unused\""));
    page.close().await?;
    process.close().await?;
    server.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn collector_maps_frame_owners_after_selection_pruning() -> TestResult {
    let parent = FixtureServer::start().await?;
    let child = FixtureServer::start().await?;
    child
        .register("/one", FixtureResponse::html("<p>frame one</p>"))
        .await?;
    child
        .register("/two", FixtureResponse::html("<p>frame two</p>"))
        .await?;
    let mut one = child.url("/one")?;
    one.set_host(Some("localhost"))?;
    let mut two = child.url("/two")?;
    two.set_host(Some("localhost"))?;
    parent
        .register(
            "/",
            FixtureResponse::html(format!(
                r#"<iframe src="{one}"></iframe>
                <article id="selected">selected <iframe src="{two}"></iframe></article>
                <script>
                  const range = document.createRange();
                  range.selectNodeContents(document.getElementById("selected"));
                  const selection = getSelection();
                  selection.removeAllRanges();
                  selection.addRange(range);
                </script>"#
            )),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &parent.url("/")?,
        ReadinessMode::Load,
        20,
        Duration::from_secs(10),
    )
    .await?;

    let frames = page.attached_frames().await?;
    assert_eq!(frames.len(), 2);
    let observation = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &ContentPolicy {
            scope: CaptureScope::Selection,
            ..ContentPolicy::default()
        },
    )
    .await?;

    assert_eq!(observation.frame_owners.len(), 1);
    assert_eq!(observation.frame_owners[0].original_path, vec![1]);
    assert_eq!(observation.frame_owners[0].retained_path, vec![0]);
    page.close().await?;
    process.close().await?;
    parent.close().await;
    child.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn collector_captures_the_first_selector_match_and_reports_selector_errors() -> TestResult {
    let parent = FixtureServer::start().await?;
    let child = FixtureServer::start().await?;
    child
        .register("/one", FixtureResponse::html("<p>first frame</p>"))
        .await?;
    child
        .register("/two", FixtureResponse::html("<p>second frame</p>"))
        .await?;
    let mut one = child.url("/one")?;
    one.set_host(Some("localhost"))?;
    let mut two = child.url("/two")?;
    two.set_host(Some("localhost"))?;
    parent
        .register(
            "/",
            FixtureResponse::html(format!(
                r#"<!doctype html>
                <title>Selector fixture</title>
                <style>.capture {{ color: rgb(1, 2, 3) }}</style>
                <p>outside content</p>
                <article class="capture" id="first">
                  first match
                  <iframe src="{one}"></iframe>
                </article>
                <article class="capture" id="second">
                  second match
                  <iframe src="{two}"></iframe>
                </article>"#
            )),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &parent.url("/")?,
        ReadinessMode::Load,
        20,
        Duration::from_secs(10),
    )
    .await?;

    let selected = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &ContentPolicy {
            selector: Some(".capture".to_owned()),
            ..ContentPolicy::default()
        },
    )
    .await?;

    assert!(selected.html.contains("first match"));
    assert!(selected.html.contains(".capture"));
    assert!(!selected.html.contains("outside content"));
    assert!(!selected.html.contains("second match"));
    assert_eq!(selected.frame_owners.len(), 1);
    assert_eq!(selected.frame_owners[0].original_path, vec![0]);
    assert_eq!(selected.frame_owners[0].retained_path, vec![0]);

    let invalid = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &ContentPolicy {
            selector: Some("[".to_owned()),
            ..ContentPolicy::default()
        },
    )
    .await;
    assert_eq!(
        invalid.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.selector.invalid")
    );

    let missing = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &ContentPolicy {
            selector: Some(".missing".to_owned()),
            ..ContentPolicy::default()
        },
    )
    .await;
    assert_eq!(
        missing.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.selector.not_found")
    );

    page.close().await?;
    process.close().await?;
    parent.close().await;
    child.close().await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn collector_maps_frame_owners_after_reserved_ancestor_cleanup() -> TestResult {
    let parent = FixtureServer::start().await?;
    let child = FixtureServer::start().await?;
    child
        .register("/reserved-id", FixtureResponse::html("<p>reserved id</p>"))
        .await?;
    child
        .register(
            "/reserved-marker",
            FixtureResponse::html("<p>reserved marker</p>"),
        )
        .await?;
    child
        .register("/retained", FixtureResponse::html("<p>retained frame</p>"))
        .await?;
    let mut reserved_id = child.url("/reserved-id")?;
    reserved_id.set_host(Some("localhost"))?;
    let mut reserved_marker = child.url("/reserved-marker")?;
    reserved_marker.set_host(Some("localhost"))?;
    let mut retained = child.url("/retained")?;
    retained.set_host(Some("localhost"))?;
    parent
        .register(
            "/",
            FixtureResponse::html(format!(
                r#"<section id="offprint-state-script">
                  <iframe src="{reserved_id}"></iframe>
                </section>
                <section data-offprint-animation-style>
                  <iframe src="{reserved_marker}"></iframe>
                </section>
                <iframe src="{retained}"></iframe>"#
            )),
        )
        .await?;
    let process =
        ChromiumProcess::launch(ChromiumLaunchOptions::new(local_executable().await?)).await?;
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    page.navigate(
        &parent.url("/")?,
        ReadinessMode::Load,
        20,
        Duration::from_secs(10),
    )
    .await?;

    assert_eq!(page.attached_frames().await?.len(), 3);
    let observation = collect_page_observation(
        &page,
        &CaptureId::new(),
        collector_limits(64 * 1024, 1024 * 1024, 1_000_000),
        &ContentPolicy::default(),
    )
    .await?;

    assert_eq!(observation.frame_owners.len(), 1);
    assert_eq!(observation.frame_owners[0].original_path, vec![2]);
    assert_eq!(observation.frame_owners[0].retained_path, vec![0]);
    page.close().await?;
    process.close().await?;
    parent.close().await;
    child.close().await;
    Ok(())
}

async fn local_executable() -> TestResult<offprint_model::PortablePath> {
    if let Some(path) = std::env::var_os("OFFPRINT_BROWSER_PATH") {
        return offprint_model::PortablePath::from_path_buf(path.into()).map_err(Into::into);
    }
    ChromiumDiscovery::new()
        .discover()
        .await
        .selected
        .and_then(|browser| browser.executable_path)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no compatible Chromium browser was discovered",
            )
            .into()
        })
}

fn profile_contains_download(profile: &std::path::Path, name: &str) -> std::io::Result<bool> {
    let mut pending = vec![profile.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                pending.push(entry.path());
                continue;
            }
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            if file_name == name || file_name == format!("{name}.crdownload") {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
