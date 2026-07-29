mod activity;
mod frame_owner;
mod interception;
mod lifecycle;
mod navigation;
mod offline;
mod resource;
mod visual_fallback;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use base64::Engine as _;
use pageknot_browser::{
    AttachedFrame, CollectedPageObservation, CollectorLimits, LoadedResource, NavigationResult,
    NetworkGuard, OfflineBrowserObservation, PageSession, ReadinessObservation,
};
use pageknot_model::{
    CaptureCredentials, CaptureId, CookieSameSite, ErrorStage, FrameId, LazyLoadPolicy,
    Milliseconds, NetworkPolicy, PageKnotError, ReadinessMode, ReadinessPolicy, RequestHeader,
    Result,
};
use pageknot_protocol::VisualFallback;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::CdpClient;
use crate::proxy::ValidatingProxy;
use crate::resources::ObservedResources;
use crate::targets::{FrameTargetManager, SessionRegistry};
use activity::NetworkActivity;
#[cfg(test)]
use activity::{is_long_lived_resource_type, is_long_lived_response};
use interception::{NetworkInterception, cookie_domain_matches, validate_guard_destination};
#[cfg(test)]
use interception::{continued_headers, same_origin};
use lifecycle::required_string;

const PAGE_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const COLLECTOR_PREPARE_TIMEOUT: Duration = Duration::from_secs(90);
const MAXIMUM_SCREENSHOT_PIXELS: f64 = 2_000_000.0;
const RUNTIME_PROBE_TIMEOUT: Duration = Duration::from_millis(750);
const RUNTIME_PROBE_ATTEMPTS: u8 = 4;
const FONT_READY_TIMEOUT_MILLISECONDS: u64 = 5_000;
const MUTATION_QUIET_TIMEOUT_MILLISECONDS: u64 = 5_000;

#[derive(Debug)]
pub struct ChromiumPage {
    client: CdpClient,
    browser_context_id: String,
    session_id: String,
    sessions: SessionRegistry,
    targets: FrameTargetManager,
    activity: NetworkActivity,
    observed_resources: ObservedResources,
    interception: Mutex<Option<NetworkInterception>>,
    validating_proxy: Mutex<Option<ValidatingProxy>>,
    resource_cancellation: CancellationToken,
    closed: AtomicBool,
}

impl ChromiumPage {
    pub async fn install_document_start_script(&self, source: &str) -> Result<String> {
        let response = self
            .client
            .command(
                "Page.addScriptToEvaluateOnNewDocument",
                json!({"source": source, "runImmediately": true}),
                Some(&self.session_id),
            )
            .await?;
        required_string(&response, "identifier", "install document-start collector")
    }

    pub async fn apply_credentials(
        &self,
        credentials: &CaptureCredentials,
        initial_url: &Url,
        guard: &NetworkGuard,
    ) -> Result<()> {
        if credentials.cookies.is_empty() {
            return Ok(());
        }
        let mut cookies = Vec::with_capacity(credentials.cookies.len());
        for cookie in &credentials.cookies {
            if let Some(url) = &cookie.url {
                validate_guard_destination(guard, url).await?;
            }
            if let Some(domain) = &cookie.domain
                && !cookie_domain_matches(domain, initial_url.host_str())
            {
                return Err(PageKnotError::new(
                    "pageknot.input.credentials",
                    ErrorStage::Validation,
                    "cookie domain is outside the initial capture host",
                ));
            }
            let mut value = serde_json::Map::new();
            value.insert("name".to_owned(), Value::String(cookie.name.clone()));
            value.insert(
                "value".to_owned(),
                Value::String(cookie.value.expose_secret().to_owned()),
            );
            if let Some(url) = &cookie.url {
                value.insert("url".to_owned(), Value::String(url.to_string()));
            }
            if let Some(domain) = &cookie.domain {
                value.insert("domain".to_owned(), Value::String(domain.clone()));
            }
            if let Some(path) = &cookie.path {
                value.insert("path".to_owned(), Value::String(path.clone()));
            }
            if let Some(secure) = cookie.secure {
                value.insert("secure".to_owned(), Value::Bool(secure));
            }
            if let Some(http_only) = cookie.http_only {
                value.insert("httpOnly".to_owned(), Value::Bool(http_only));
            }
            if let Some(same_site) = cookie.same_site {
                value.insert(
                    "sameSite".to_owned(),
                    Value::String(
                        match same_site {
                            CookieSameSite::Strict => "Strict",
                            CookieSameSite::Lax => "Lax",
                            CookieSameSite::None => "None",
                        }
                        .to_owned(),
                    ),
                );
            }
            if let Some(expires) = cookie.expires {
                value.insert("expires".to_owned(), Value::Number(expires.into()));
            }
            cookies.push(Value::Object(value));
        }
        self.client
            .command(
                "Network.setCookies",
                json!({"cookies": cookies}),
                Some(&self.session_id),
            )
            .await
            .map(|_| ())
    }

    pub async fn enable_network_guard(
        &self,
        guard: NetworkGuard,
        headers: Vec<RequestHeader>,
        header_origin: Url,
    ) -> Result<()> {
        if !self.client.is_owned_browser() && !matches!(guard.policy(), NetworkPolicy::Unrestricted)
        {
            return Err(PageKnotError::new(
                "pageknot.input.remote_network_policy",
                ErrorStage::Validation,
                "restricted network policy requires a browser process owned by PageKnot",
            ));
        }
        let mut interception = self.interception.lock().await;
        if interception.is_some() {
            return Err(PageKnotError::new(
                "pageknot.navigation.policy_state",
                ErrorStage::Navigation,
                "network policy interception is already active",
            ));
        }
        if let Some(proxy) = self.validating_proxy.lock().await.as_ref() {
            proxy.set_guard(guard.clone()).await;
        }
        let intercept_subresource_requests = !headers.is_empty();
        let controller = NetworkInterception::start(
            self.client.clone(),
            Arc::clone(&self.sessions),
            self.observed_resources.recorder(),
            guard,
            headers,
            header_origin,
        );
        let enabled = self
            .client
            .command(
                "Fetch.enable",
                crate::targets::fetch_enable_parameters(intercept_subresource_requests),
                Some(&self.session_id),
            )
            .await;
        if let Err(error) = enabled {
            controller.close().await;
            return Err(error);
        }
        if let Err(error) = self
            .targets
            .enable_fetch(intercept_subresource_requests)
            .await
        {
            let _ignored = self
                .client
                .command("Fetch.disable", json!({}), Some(&self.session_id))
                .await;
            controller.close().await;
            return Err(error);
        }
        *interception = Some(controller);
        Ok(())
    }

    pub async fn evaluate(&self, expression: &str) -> Result<Value> {
        self.evaluate_in_session(&self.session_id, expression).await
    }

    pub(crate) async fn evaluate_in_session(
        &self,
        session_id: &str,
        expression: &str,
    ) -> Result<Value> {
        self.evaluate_in_session_with_timeout(session_id, expression, PAGE_COMMAND_TIMEOUT)
            .await
    }

    async fn evaluate_in_session_with_timeout(
        &self,
        session_id: &str,
        expression: &str,
        deadline: Duration,
    ) -> Result<Value> {
        let response = self
            .client
            .command_with_timeout(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": true,
                    "userGesture": false,
                }),
                Some(session_id),
                deadline,
            )
            .await
            .map_err(|error| {
                error.with_detail(
                    "target",
                    if session_id == self.session_id {
                        "main-frame"
                    } else {
                        "attached-frame"
                    },
                )
            })?;
        if let Some(exception) = response.get("exceptionDetails") {
            let text = exception
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("page evaluation failed");
            return Err(PageKnotError::new(
                "pageknot.collector.evaluate",
                ErrorStage::Collection,
                text,
            ));
        }
        response.pointer("/result/value").cloned().ok_or_else(|| {
            PageKnotError::new(
                "pageknot.collector.value",
                ErrorStage::Collection,
                "page evaluation returned no serializable value",
            )
        })
    }

    pub(crate) async fn collector_call_in_session(
        &self,
        session_id: &str,
        method: &str,
        arguments: Value,
    ) -> Result<Value> {
        if !arguments.is_array() {
            return Err(PageKnotError::new(
                "pageknot.collector.protocol_shape",
                ErrorStage::Collection,
                "collector call arguments must be a JSON array",
            ));
        }
        let collector_method = method.to_owned();
        let method = serde_json::to_string(method).map_err(|error| {
            PageKnotError::new(
                "pageknot.collector.protocol_shape",
                ErrorStage::Collection,
                format!("collector method could not be encoded: {error}"),
            )
        })?;
        let expression = format!("__pageknotCollector.call({method}, {})", arguments);
        let deadline = if collector_method == "prepare" {
            COLLECTOR_PREPARE_TIMEOUT
        } else {
            PAGE_COMMAND_TIMEOUT
        };
        let result = self
            .evaluate_in_session_with_timeout(session_id, &expression, deadline)
            .await;
        self.prefer_frame_target_error(result)
            .await
            .map_err(|error| error.with_detail("collectorMethod", collector_method))
    }

    pub async fn attached_frames(&self) -> Result<Vec<AttachedFrame>> {
        self.targets.frames().await
    }

    async fn prefer_frame_target_error<T>(&self, result: Result<T>) -> Result<T> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => Err(self.targets.error().await.unwrap_or(error)),
        }
    }

    pub async fn rendered_html(&self) -> Result<String> {
        self.evaluate("document.documentElement.outerHTML")
            .await?
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.collector.document",
                    ErrorStage::Collection,
                    "the page has no serializable document element",
                )
            })
    }

    pub async fn settle(&self, policy: &ReadinessPolicy) -> Result<ReadinessObservation> {
        let started = Instant::now();
        let mut fonts_ready = false;
        let mut mutation_quiet = false;
        match policy.mode {
            ReadinessMode::RenderIdle => {
                let lazy = match &policy.lazy_load {
                    LazyLoadPolicy::Disabled => Value::Null,
                    LazyLoadPolicy::ViewportSweep(sweep) => json!({
                        "stepPixels": sweep.step_pixels,
                        "settleMilliseconds": sweep.settle.get(),
                        "maximumSteps": sweep.max_steps,
                    }),
                };
                let expression = format!(
                    r#"(async (lazy) => {{
                        const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
                        const originalX = scrollX;
                        const originalY = scrollY;
                        if (lazy) {{
                            const maximumY = Math.max(0, document.documentElement.scrollHeight - innerHeight);
                            for (let step = 0; step < lazy.maximumSteps; step += 1) {{
                                const y = Math.min(maximumY, step * lazy.stepPixels);
                                scrollTo(originalX, y);
                                await sleep(lazy.settleMilliseconds);
                                if (y >= maximumY) break;
                            }}
                            scrollTo(originalX, originalY);
                        }}
                        const fontsReady = !document.fonts?.ready || await Promise.race([
                            document.fonts.ready.then(() => true, () => false),
                            sleep({FONT_READY_TIMEOUT_MILLISECONDS}).then(() => false),
                        ]);
                        return {{fontsReady}};
                    }})({lazy})"#
                );
                fonts_ready = self
                    .evaluate(&expression)
                    .await
                    .map_err(|error| error.with_detail("readinessStep", "lazy-fonts"))?
                    .get("fontsReady")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                self.wait_for_mutation_quiet(policy.mutation_quiet).await?;
                self.activity
                    .wait_for_quiet(Duration::from_millis(policy.network_quiet.get()))
                    .await?;
                mutation_quiet = self.wait_for_mutation_quiet(policy.mutation_quiet).await?;
                self.activity
                    .wait_for_quiet(Duration::from_millis(policy.network_quiet.get()))
                    .await?;
            }
            ReadinessMode::NetworkIdle => {
                self.activity
                    .wait_for_idle(Duration::from_millis(policy.network_quiet.get()))
                    .await?;
            }
            ReadinessMode::Load | ReadinessMode::DomContentLoaded => {}
        }
        if policy.delay.get() > 0 {
            tokio::time::sleep(Duration::from_millis(policy.delay.get())).await;
        }
        self.freeze_rendering().await?;
        let in_flight_requests = self.activity.in_flight().await?;
        Ok(ReadinessObservation {
            reason: match policy.mode {
                ReadinessMode::RenderIdle => "render-idle",
                ReadinessMode::NetworkIdle => "network-idle",
                ReadinessMode::Load => "load",
                ReadinessMode::DomContentLoaded => "dom-content-loaded",
            }
            .to_owned(),
            elapsed: Milliseconds::from(started.elapsed()),
            in_flight_requests,
            mutation_quiet,
            fonts_ready,
        })
    }

    async fn wait_for_mutation_quiet(&self, quiet: Milliseconds) -> Result<bool> {
        self.evaluate(&format!(
            r#"(async (milliseconds) => {{
                return await new Promise((resolve) => {{
                    let observer;
                    let timer;
                    let deadline;
                    let settled = false;
                    const finish = (quiet) => {{
                        if (settled) return;
                        settled = true;
                        clearTimeout(timer);
                        clearTimeout(deadline);
                        observer?.disconnect();
                        resolve(quiet);
                    }};
                    timer = setTimeout(() => finish(true), milliseconds);
                    deadline = setTimeout(
                        () => finish(false),
                        {MUTATION_QUIET_TIMEOUT_MILLISECONDS}
                    );
                    observer = new MutationObserver(() => {{
                        clearTimeout(timer);
                        timer = setTimeout(() => finish(true), milliseconds);
                    }});
                    observer.observe(document, {{
                        attributes: true,
                        childList: true,
                        characterData: true,
                        subtree: true
                    }});
                }});
            }})({})"#,
            quiet.get()
        ))
        .await
        .map_err(|error| error.with_detail("readinessStep", "mutation-quiet"))
        .map(|value| value.as_bool().unwrap_or(false))
    }

    async fn freeze_rendering(&self) -> Result<()> {
        self.collector_call_in_session(&self.session_id, "freeze", json!([]))
            .await
            .map(|_| ())
            .map_err(|error| error.with_detail("readinessStep", "freeze"))
    }

    pub async fn freeze_attached_frames(&self) -> Result<()> {
        for frame in self.attached_frames().await? {
            self.ensure_session_runtime(&frame.session_id).await?;
            self.collector_call_in_session(&frame.session_id, "freeze", json!([]))
                .await?;
        }
        Ok(())
    }

    async fn ensure_session_runtime(&self, session_id: &str) -> Result<()> {
        let mut attempt = 0_u8;
        loop {
            attempt = attempt.saturating_add(1);
            let result = self
                .client
                .command_with_timeout(
                    "Runtime.evaluate",
                    json!({
                        "expression": "1",
                        "returnByValue": true,
                        "awaitPromise": false,
                        "silent": true,
                    }),
                    Some(session_id),
                    RUNTIME_PROBE_TIMEOUT,
                )
                .await;
            match result {
                Ok(_) => return Ok(()),
                Err(error)
                    if error.code.as_str() == "pageknot.browser.cdp_timeout"
                        && attempt < RUNTIME_PROBE_ATTEMPTS =>
                {
                    let backoff = match attempt {
                        1 => Duration::from_millis(50),
                        2 => Duration::from_millis(100),
                        _ => Duration::from_millis(200),
                    };
                    tokio::time::sleep(backoff).await;
                }
                Err(error) => {
                    let busy = error.code.as_str() == "pageknot.browser.cdp_timeout";
                    return Err(PageKnotError::new(
                        "pageknot.collector.evaluate",
                        ErrorStage::Collection,
                        if busy {
                            "attached-frame runtime remained busy after bounded retries"
                        } else {
                            "attached-frame runtime is unavailable for full-fidelity collection"
                        },
                    )
                    .retryable(busy || error.retryable)
                    .with_detail("attempts", attempt)
                    .with_detail("cause", error.code.as_str())
                    .with_detail("target", "attached-frame"));
                }
            }
        }
    }

    pub async fn capture_visual_fallback(
        &self,
        session_id: &str,
        fallback: &VisualFallback,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>> {
        let fallback = self.position_visual_fallback(session_id, fallback).await?;
        let clip = self.visual_fallback_clip(session_id, &fallback).await?;
        if clip.width * clip.height > MAXIMUM_SCREENSHOT_PIXELS {
            return Err(PageKnotError::new(
                "pageknot.visual_fallback.pixel_limit",
                ErrorStage::Collection,
                "visual fallback bounds exceed the screenshot pixel limit",
            )
            .with_detail("pixels", clip.width * clip.height)
            .with_detail("limit", MAXIMUM_SCREENSHOT_PIXELS));
        }
        let response = self
            .client
            .command(
                "Page.captureScreenshot",
                json!({
                    "format": "png",
                    "fromSurface": true,
                    "captureBeyondViewport": false,
                    "clip": {
                        "x": clip.x,
                        "y": clip.y,
                        "width": clip.width,
                        "height": clip.height,
                        "scale": 1,
                    },
                }),
                Some(&self.session_id),
            )
            .await?;
        let encoded = response
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.visual_fallback.protocol_shape",
                    ErrorStage::Collection,
                    "screenshot response has no encoded PNG",
                )
            })?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| {
                PageKnotError::new(
                    "pageknot.visual_fallback.encoding",
                    ErrorStage::Collection,
                    format!("screenshot response is not valid base64: {error}"),
                )
            })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum_bytes {
            return Err(PageKnotError::new(
                "pageknot.resource.limit",
                ErrorStage::Resource,
                "visual fallback exceeds the configured resource byte limit",
            )
            .with_detail("attempted", bytes.len())
            .with_detail("limit", maximum_bytes));
        }
        Ok(bytes)
    }

    pub async fn capture_page_screenshot(&self, maximum_bytes: u64) -> Result<Vec<u8>> {
        let response = self
            .client
            .command(
                "Page.captureScreenshot",
                json!({
                    "format": "png",
                    "fromSurface": true,
                    "captureBeyondViewport": false,
                }),
                Some(self.session_id()),
            )
            .await?;
        let encoded = response
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.screenshot.protocol_shape",
                    ErrorStage::Verification,
                    "screenshot response has no encoded PNG",
                )
            })?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| {
                PageKnotError::new(
                    "pageknot.screenshot.encoding",
                    ErrorStage::Verification,
                    format!("screenshot response is not valid base64: {error}"),
                )
            })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum_bytes {
            return Err(PageKnotError::new(
                "pageknot.artifact.size",
                ErrorStage::Verification,
                "screenshot exceeds the configured byte limit",
            )
            .with_detail("attempted", bytes.len())
            .with_detail("limit", maximum_bytes));
        }
        Ok(bytes)
    }

    pub async fn print_to_pdf(
        &self,
        source_url: &Url,
        landscape: bool,
        prefer_css_page_size: bool,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>> {
        self.evaluate(&crate::pdf::prepare_pdf_links_expression(source_url))
            .await?;
        crate::pdf::print_to_pdf(
            self.client.clone(),
            self.session_id.clone(),
            landscape,
            prefer_css_page_size,
            maximum_bytes,
        )
        .await
    }

    async fn interception_error(
        &self,
        frame_id: &str,
        use_single_document_error: bool,
    ) -> Option<PageKnotError> {
        let interception = self.interception.lock().await;
        let interception = interception.as_ref()?;
        interception
            .error(frame_id, use_single_document_error)
            .await
    }
}

#[async_trait::async_trait]
impl PageSession for ChromiumPage {
    fn session_id(&self) -> &str {
        ChromiumPage::session_id(self)
    }

    async fn apply_credentials(
        &self,
        credentials: &CaptureCredentials,
        initial_url: &Url,
        guard: &NetworkGuard,
    ) -> Result<()> {
        ChromiumPage::apply_credentials(self, credentials, initial_url, guard).await
    }

    async fn enable_network_guard(
        &self,
        guard: NetworkGuard,
        headers: Vec<RequestHeader>,
        header_origin: Url,
    ) -> Result<()> {
        ChromiumPage::enable_network_guard(self, guard, headers, header_origin).await
    }

    async fn navigate_page(
        &self,
        url: &Url,
        readiness: ReadinessMode,
        redirect_limit: u32,
        deadline: Duration,
    ) -> Result<NavigationResult> {
        ChromiumPage::navigate(self, url, readiness, redirect_limit, deadline).await
    }

    async fn settle(&self, policy: &ReadinessPolicy) -> Result<ReadinessObservation> {
        ChromiumPage::settle(self, policy).await
    }

    async fn freeze_attached_frames(&self) -> Result<()> {
        ChromiumPage::freeze_attached_frames(self).await
    }

    async fn attached_frames(&self) -> Result<Vec<AttachedFrame>> {
        ChromiumPage::attached_frames(self).await
    }

    async fn attached_frames_bounded(&self, maximum: u32) -> Result<Vec<AttachedFrame>> {
        self.targets.frames_bounded(maximum).await
    }

    async fn frame_owner_path(&self, frame: &AttachedFrame) -> Result<Vec<u32>> {
        ChromiumPage::frame_owner_path(self, frame).await
    }

    async fn collect_frame_observation(
        &self,
        session_id: &str,
        frame_id: FrameId,
        capture_id: &CaptureId,
        limits: CollectorLimits,
        capture_policy: &pageknot_model::CapturePolicy,
    ) -> Result<CollectedPageObservation> {
        crate::collector::collect_frame_observation_with_metadata(
            self,
            session_id,
            frame_id,
            capture_id,
            limits,
            capture_policy,
        )
        .await
    }

    async fn capture_visual_fallback(
        &self,
        session_id: &str,
        fallback: &VisualFallback,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>> {
        ChromiumPage::capture_visual_fallback(self, session_id, fallback, maximum_bytes).await
    }

    async fn load_resource_in_session(
        &self,
        session_id: &str,
        cdp_frame_id: &str,
        url: &Url,
        maximum_bytes: u64,
    ) -> Result<LoadedResource> {
        ChromiumPage::load_resource_in_session(self, session_id, cdp_frame_id, url, maximum_bytes)
            .await
    }

    async fn verify_offline_url(
        &self,
        url: &Url,
        deadline: Duration,
    ) -> Result<OfflineBrowserObservation> {
        ChromiumPage::verify_offline_url(self, url, deadline).await
    }

    async fn print_to_pdf(
        &self,
        source_url: &Url,
        landscape: bool,
        prefer_css_page_size: bool,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>> {
        ChromiumPage::print_to_pdf(
            self,
            source_url,
            landscape,
            prefer_css_page_size,
            maximum_bytes,
        )
        .await
    }

    async fn close(self: Box<Self>) -> Result<()> {
        ChromiumPage::close(*self).await
    }
}

#[cfg(test)]
mod tests;
