#![allow(unsafe_code)]
#![deny(missing_debug_implementations)]

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{FutureExt as _, StreamExt as _};
use napi::bindgen_prelude::{Error as NapiError, Status};
use napi_derive::napi;
use offprint::{
    ArtifactFormat, ArtifactSource, BatchRequest, BrowserInstallRequest, BrowserInstallationPolicy,
    BrowserSourcePolicy, CaptureJob, CaptureRequest, CaptureScope, CaptureStatus, ConflictPolicy,
    CrawlRequest, ErrorStage, ExportRequest, Offprint, OffprintError, PortablePath, ReadinessMode,
    VerificationMode, Viewport,
};
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::Mutex;

const ERROR_MARKER: &str = "__OFFPRINT_ERROR__";

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct OffprintOptions {
    browser_path: Option<String>,
    cdp_url: Option<String>,
    cache_dir: Option<String>,
    browser_source: Option<BrowserSourcePolicy>,
    browser_installation: Option<BrowserInstallationPolicy>,
    maximum_contexts: Option<u16>,
    browser_recycle_after_jobs: Option<u32>,
    headed: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct CaptureOptions {
    output: Option<String>,
    profile: Option<String>,
    timeout_ms: Option<u64>,
    wait_until: Option<ReadinessMode>,
    delay_ms: Option<u64>,
    viewport: Option<Viewport>,
    strict: bool,
    headed: Option<bool>,
    conflict: Option<ConflictPolicy>,
    network_policy: Option<offprint::NetworkPolicy>,
    verification: Option<VerificationMode>,
    scope: Option<CaptureScope>,
    selector: Option<String>,
    remove_unused_css: bool,
    remove_unused_fonts: bool,
    remove_hidden_elements: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct VerifyOptions {
    verification: Option<VerificationMode>,
}

#[napi(js_name = "NativeOffprint")]
#[derive(Debug)]
pub struct NativeOffprint {
    inner: offprint::Result<Offprint>,
}

#[napi]
impl NativeOffprint {
    #[napi(factory)]
    pub fn create(options: Option<Value>) -> Self {
        let inner = match catch_unwind(AssertUnwindSafe(|| build_offprint(options))) {
            Ok(result) => result,
            Err(_) => Err(panic_error()),
        };
        Self { inner }
    }

    #[napi(getter, js_name = "initializationError")]
    pub fn initialization_error(&self) -> Option<Value> {
        self.inner.as_ref().err().map(error_value)
    }

    #[napi(js_name = "capture")]
    pub async fn capture(&self, url: String, options: Option<Value>) -> napi::Result<Value> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move {
            let options = decode_options::<CaptureOptions>(options, "capture options")?;
            run_capture(offprint, url, options).await
        })
        .await
    }

    #[napi(js_name = "start")]
    pub async fn start(&self, request: Value) -> napi::Result<NativeCaptureJob> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move {
            let request: CaptureRequest = serde_json::from_value(request)
                .map_err(|error| invalid_input("capture request", error))?;
            request.validate()?;
            let job = offprint.captures().start(request).await?;
            Ok(NativeCaptureJob { inner: job })
        })
        .await
    }

    #[napi(js_name = "batch")]
    pub async fn batch(&self, request: Value) -> napi::Result<Value> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move {
            let request: BatchRequest = serde_json::from_value(request)
                .map_err(|error| invalid_input("batch request", error))?;
            let result = offprint.captures().batch(request).await?;
            to_value(result)
        })
        .await
    }

    #[napi(js_name = "crawl")]
    pub async fn crawl(&self, request: Value) -> napi::Result<Value> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move {
            let request: CrawlRequest = serde_json::from_value(request)
                .map_err(|error| invalid_input("crawl request", error))?;
            let result = offprint.captures().crawl(request).await?;
            to_value(result)
        })
        .await
    }

    #[napi(js_name = "inspect")]
    pub async fn inspect(&self, path: String) -> napi::Result<Value> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move {
            let manifest = offprint
                .artifacts()
                .inspect(ArtifactSource::File(PortablePath::from(path)))
                .await?;
            to_value(manifest)
        })
        .await
    }

    #[napi(js_name = "verify")]
    pub async fn verify(&self, path: String, options: Option<Value>) -> napi::Result<Value> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move {
            let options = decode_options::<VerifyOptions>(options, "verify options")?;
            let result = offprint
                .artifacts()
                .verify(
                    ArtifactSource::File(PortablePath::from(path)),
                    options.verification.unwrap_or(VerificationMode::Offline),
                )
                .await?;
            to_value(result)
        })
        .await
    }

    #[napi(js_name = "exportArtifacts")]
    pub async fn export_artifacts(&self, path: String, request: Value) -> napi::Result<Value> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move {
            let request: ExportRequest = serde_json::from_value(request)
                .map_err(|error| invalid_input("artifact export request", error))?;
            let result = offprint
                .artifacts()
                .export(ArtifactSource::File(PortablePath::from(path)), request)
                .await?;
            to_value(result)
        })
        .await
    }

    #[napi(js_name = "verifyFormat")]
    pub async fn verify_format(&self, path: String, format: String) -> napi::Result<Value> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move {
            let format: ArtifactFormat = serde_json::from_value(Value::String(format))
                .map_err(|error| invalid_input("artifact format", error))?;
            let result = offprint
                .artifacts()
                .verify_format(PortablePath::from(path), format)
                .await?;
            to_value(result)
        })
        .await
    }

    #[napi(js_name = "ensureBrowser")]
    pub async fn ensure_browser(&self) -> napi::Result<Value> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move {
            let browser = offprint.browsers().ensure().await?;
            to_value(browser)
        })
        .await
    }

    #[napi(js_name = "listBrowsers")]
    pub async fn list_browsers(&self) -> napi::Result<Value> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move { to_value(offprint.browsers().list().await?) }).await
    }

    #[napi(js_name = "installBrowser")]
    pub async fn install_browser(&self, revision: Option<String>) -> napi::Result<Value> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move {
            to_value(
                offprint
                    .browsers()
                    .install(BrowserInstallRequest { revision })
                    .await?,
            )
        })
        .await
    }

    #[napi(js_name = "removeBrowser")]
    pub async fn remove_browser(&self, revision: String, force: bool) -> napi::Result<Value> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move { to_value(offprint.browsers().remove(&revision, force).await?) })
            .await
    }

    #[napi(js_name = "doctor")]
    pub async fn doctor(&self) -> napi::Result<Value> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move { to_value(offprint.browsers().doctor().await) }).await
    }

    #[napi(js_name = "closeIdleBrowser")]
    pub async fn close_idle_browser(&self) -> napi::Result<()> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move { offprint.browsers().close_idle().await }).await
    }

    #[napi]
    pub async fn close(&self) -> napi::Result<()> {
        let offprint = self.inner.clone().map_err(native_error)?;
        contained(async move { offprint.close().await }).await
    }

    #[napi(js_name = "closeBlocking")]
    pub fn close_blocking(&self) -> napi::Result<()> {
        let offprint = self.inner.clone().map_err(native_error)?;
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("offprint-node-shutdown".to_owned())
            .spawn(move || {
                let result = match catch_unwind(AssertUnwindSafe(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|error| {
                            OffprintError::new(
                                "offprint.binding.runtime",
                                ErrorStage::Internal,
                                format!("failed to start the binding shutdown runtime: {error}"),
                            )
                        })?;
                    runtime.block_on(offprint.close())
                })) {
                    Ok(result) => result,
                    Err(_) => Err(panic_error()),
                };
                let _ = sender.send(result);
            })
            .map_err(|error| {
                native_error(OffprintError::new(
                    "offprint.binding.runtime",
                    ErrorStage::Internal,
                    format!("failed to start the binding shutdown thread: {error}"),
                ))
            })?;
        match receiver.recv_timeout(Duration::from_secs(15)) {
            Ok(result) => result.map_err(native_error),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                Err(native_error(OffprintError::new(
                    "offprint.binding.shutdown_timeout",
                    ErrorStage::Shutdown,
                    "binding shutdown exceeded 15 seconds",
                )))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(native_error(OffprintError::new(
                    "offprint.binding.shutdown",
                    ErrorStage::Shutdown,
                    "binding shutdown ended before reporting its result",
                )))
            }
        }
    }
}

#[cfg(feature = "binding-test-hooks")]
#[napi]
impl NativeOffprint {
    #[napi(js_name = "testPanic")]
    pub async fn test_panic(&self) -> napi::Result<()> {
        contained(async move {
            std::panic::resume_unwind(Box::new("Offprint binding fault injection"));
            #[allow(unreachable_code)]
            Ok(())
        })
        .await
    }
}

#[napi(js_name = "NativeCaptureJob")]
#[derive(Clone, Debug)]
pub struct NativeCaptureJob {
    inner: CaptureJob,
}

#[napi]
impl NativeCaptureJob {
    #[napi(getter)]
    pub fn id(&self) -> String {
        self.inner.id().to_string()
    }

    #[napi(getter)]
    pub fn status(&self) -> String {
        capture_status_name(self.inner.status()).to_owned()
    }

    #[napi]
    pub fn events(&self) -> NativeCaptureEvents {
        NativeCaptureEvents {
            inner: Arc::new(Mutex::new(self.inner.events())),
        }
    }

    #[napi]
    pub fn cancel(&self) {
        self.inner.cancel();
    }

    #[napi(js_name = "result")]
    pub async fn result(&self) -> napi::Result<Value> {
        let job = self.inner.clone();
        contained(async move {
            let result = job.result().await?;
            to_value(result)
        })
        .await
    }
}

#[napi(js_name = "NativeCaptureEvents")]
#[derive(Clone, Debug)]
pub struct NativeCaptureEvents {
    inner: Arc<Mutex<offprint::CaptureEvents>>,
}

#[napi]
impl NativeCaptureEvents {
    #[napi(js_name = "next")]
    pub async fn next(&self) -> napi::Result<Option<Value>> {
        let events = Arc::clone(&self.inner);
        contained(async move {
            let event = events.lock().await.next().await;
            event.map(to_value).transpose()
        })
        .await
    }
}

fn build_offprint(options: Option<Value>) -> offprint::Result<Offprint> {
    let options = decode_options::<OffprintOptions>(options, "Offprint options")?;
    let mut builder = Offprint::builder();
    if let Some(path) = options.browser_path {
        builder = builder.browser_path(path);
    }
    if let Some(endpoint) = options.cdp_url {
        let endpoint = url::Url::parse(&endpoint).map_err(|error| {
            OffprintError::new(
                "offprint.input.cdp_url",
                ErrorStage::Validation,
                format!("invalid remote browser endpoint: {error}"),
            )
        })?;
        builder = builder.cdp_url(endpoint);
    }
    if let Some(path) = options.cache_dir {
        builder = builder.cache_dir(path);
    }
    if let Some(source) = options.browser_source {
        builder = builder.browser_source(source);
    }
    if let Some(policy) = options.browser_installation {
        builder = builder.browser_installation(policy);
    }
    if let Some(maximum) = options.maximum_contexts {
        builder = builder.maximum_contexts(maximum);
    }
    if let Some(jobs) = options.browser_recycle_after_jobs {
        builder = builder.browser_recycle_after_jobs(jobs);
    }
    if let Some(headed) = options.headed {
        builder = builder.headed(headed);
    }
    builder.build()
}

async fn run_capture(
    offprint: Offprint,
    url: String,
    options: CaptureOptions,
) -> offprint::Result<Value> {
    let mut capture = offprint.capture(url)?;
    let output = options.output.ok_or_else(|| {
        OffprintError::new(
            "offprint.input.output",
            ErrorStage::Validation,
            "capture requires an output path",
        )
    })?;
    if let Some(profile) = options.profile {
        capture = capture.profile(profile)?;
    }
    if let Some(timeout_ms) = options.timeout_ms {
        capture = capture.timeout(Duration::from_millis(timeout_ms));
    }
    if let Some(wait_until) = options.wait_until {
        capture = capture.wait_until(wait_until);
    }
    if let Some(delay_ms) = options.delay_ms {
        capture = capture.delay(Duration::from_millis(delay_ms));
    }
    if let Some(viewport) = options.viewport {
        capture = capture.viewport(viewport);
    }
    if options.strict {
        capture = capture.strict();
    }
    if let Some(headed) = options.headed {
        capture = capture.headed(headed);
    }
    if let Some(network) = options.network_policy {
        capture = capture.network(network);
    }
    if let Some(verification) = options.verification {
        capture = capture.verification(verification);
    }
    if let Some(conflict) = options.conflict {
        capture = capture.conflict(conflict);
    }
    if let Some(scope) = options.scope {
        capture = capture.scope(scope);
    }
    if let Some(selector) = options.selector {
        capture = capture.selector(selector);
    }
    if options.remove_unused_css {
        capture = capture.remove_unused_css();
    }
    if options.remove_unused_fonts {
        capture = capture.remove_unused_fonts();
    }
    if options.remove_hidden_elements {
        capture = capture.remove_hidden_elements();
    }
    let result = capture.save(output).await?;
    to_value(result)
}

fn decode_options<T>(value: Option<Value>, label: &str) -> offprint::Result<T>
where
    T: Default + for<'de> Deserialize<'de>,
{
    value.map_or_else(
        || Ok(T::default()),
        |value| serde_json::from_value(value).map_err(|error| invalid_input(label, error)),
    )
}

fn invalid_input(label: &str, error: serde_json::Error) -> OffprintError {
    OffprintError::new(
        "offprint.input.value",
        ErrorStage::Validation,
        format!("invalid {label}: {error}"),
    )
}

fn to_value(value: impl Serialize) -> offprint::Result<Value> {
    serde_json::to_value(value).map_err(|error| {
        OffprintError::new(
            "offprint.binding.serialization",
            ErrorStage::Internal,
            format!("failed to serialize a binding result: {error}"),
        )
    })
}

async fn contained<T, F>(future: F) -> napi::Result<T>
where
    T: Send + 'static,
    F: Future<Output = offprint::Result<T>> + Send + 'static,
{
    match AssertUnwindSafe(future).catch_unwind().await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(native_error(error)),
        Err(_) => Err(native_error(panic_error())),
    }
}

fn native_error(error: OffprintError) -> NapiError {
    let encoded = serde_json::to_string(&error).unwrap_or_else(|_| {
        "{\"code\":\"offprint.binding.serialization\",\"message\":\"failed to serialize a native error\",\"stage\":\"internal\",\"retryable\":false}".to_owned()
    });
    NapiError::new(Status::GenericFailure, format!("{ERROR_MARKER}{encoded}"))
}

fn error_value(error: &OffprintError) -> Value {
    serde_json::to_value(error).unwrap_or_else(|_| {
        serde_json::json!({
            "code": "offprint.binding.serialization",
            "message": "failed to serialize a native error",
            "stage": "internal",
            "retryable": false
        })
    })
}

fn panic_error() -> OffprintError {
    OffprintError::new(
        "offprint.internal.panic",
        ErrorStage::Internal,
        "Offprint encountered an unexpected internal failure",
    )
}

const fn capture_status_name(status: CaptureStatus) -> &'static str {
    match status {
        CaptureStatus::Created => "created",
        CaptureStatus::Validating => "validating",
        CaptureStatus::WaitingForBrowser => "waitingForBrowser",
        CaptureStatus::Navigating => "navigating",
        CaptureStatus::WaitingForReadiness => "waitingForReadiness",
        CaptureStatus::Collecting => "collecting",
        CaptureStatus::ResolvingResources => "resolvingResources",
        CaptureStatus::Transforming => "transforming",
        CaptureStatus::Encoding => "encoding",
        CaptureStatus::Verifying => "verifying",
        CaptureStatus::Committing => "committing",
        CaptureStatus::Cancelling => "cancelling",
        CaptureStatus::Succeeded => "succeeded",
        CaptureStatus::Cancelled => "cancelled",
        CaptureStatus::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn offprint_options_reject_unknown_fields() {
        let result = decode_options::<OffprintOptions>(
            Some(json!({ "browserPat": "/tmp/chrome" })),
            "Offprint options",
        );

        assert_eq!(
            result.err().map(|error| error.code.to_string()),
            Some("offprint.input.value".to_owned())
        );
    }

    #[test]
    fn capture_options_accept_readiness_and_delay() {
        let options = decode_options::<CaptureOptions>(
            Some(json!({
                "waitUntil": "network-idle",
                "delayMs": 750
            })),
            "capture options",
        );

        assert_eq!(
            options.as_ref().ok().and_then(|value| value.wait_until),
            Some(ReadinessMode::NetworkIdle)
        );
        assert_eq!(
            options.as_ref().ok().and_then(|value| value.delay_ms),
            Some(750)
        );
    }

    #[test]
    fn capture_status_uses_canonical_camel_case() {
        assert_eq!(
            capture_status_name(CaptureStatus::WaitingForBrowser),
            "waitingForBrowser"
        );
        assert_eq!(
            capture_status_name(CaptureStatus::ResolvingResources),
            "resolvingResources"
        );
    }
}
