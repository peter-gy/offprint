#![allow(unsafe_code)]
#![deny(missing_debug_implementations)]

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Duration;

use futures_util::{FutureExt as _, StreamExt as _};
use pageknot::{
    ArtifactExportRequest, ArtifactInput, ArtifactVariantKind, BatchRequest, BrowserChannel,
    BrowserInstallationPolicy, CaptureJob, CaptureRequest, CaptureScope, CaptureStatus,
    ConflictPolicy, CrawlRequest, ErrorStage, PageKnot, PageKnotError, PortablePath, ReadinessMode,
    VerificationPolicy, Viewport,
};
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use serde::Deserialize;
use serde::Serialize;
use tokio::sync::Mutex;

const ERROR_MARKER: &str = "__PAGEKNOT_ERROR__";

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct PageKnotOptions {
    browser_path: Option<String>,
    cdp_url: Option<String>,
    cache_dir: Option<String>,
    browser_channel: Option<BrowserChannel>,
    browser_installation: Option<BrowserInstallationPolicy>,
    maximum_contexts: Option<u16>,
    browser_recycle_after_jobs: Option<u32>,
    headed: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct CaptureOptions {
    output: Option<String>,
    max_bytes: Option<u64>,
    profile: Option<String>,
    timeout_ms: Option<u64>,
    wait_until: Option<ReadinessMode>,
    delay_ms: Option<u64>,
    viewport: Option<Viewport>,
    strict: bool,
    headed: Option<bool>,
    conflict: Option<ConflictPolicy>,
    scope: Option<CaptureScope>,
    selector: Option<String>,
    remove_unused_css: bool,
    remove_unused_fonts: bool,
    remove_hidden_elements: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
struct VerifyOptions {
    level: Option<VerificationPolicy>,
}

#[pyclass(module = "pageknot._native", name = "NativePageKnot")]
#[derive(Debug)]
struct NativePageKnot {
    inner: PageKnot,
}

#[pymethods]
impl NativePageKnot {
    #[new]
    #[pyo3(signature = (options_json=None))]
    fn new(options_json: Option<&str>) -> PyResult<Self> {
        contained_sync(|| build_pageknot(options_json)).map(|inner| Self { inner })
    }

    #[pyo3(signature = (url, options_json=None))]
    fn capture_json<'py>(
        &self,
        py: Python<'py>,
        url: String,
        options_json: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let pageknot = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            contained(async move {
                let options =
                    decode_options::<CaptureOptions>(options_json.as_deref(), "capture options")?;
                run_capture(pageknot, url, options).await
            })
            .await
        })
    }

    fn start<'py>(&self, py: Python<'py>, request_json: String) -> PyResult<Bound<'py, PyAny>> {
        let pageknot = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            contained(async move {
                let request: CaptureRequest = serde_json::from_str(&request_json)
                    .map_err(|error| invalid_input("capture request", error))?;
                request.validate()?;
                let job = pageknot.captures().start(request).await?;
                Ok(NativeCaptureJob { inner: job })
            })
            .await
        })
    }

    fn batch_json<'py>(
        &self,
        py: Python<'py>,
        request_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let pageknot = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            contained(async move {
                let request: BatchRequest = serde_json::from_str(&request_json)
                    .map_err(|error| invalid_input("batch request", error))?;
                let result = pageknot.captures().batch(request).await?;
                to_json(result)
            })
            .await
        })
    }

    fn crawl_json<'py>(
        &self,
        py: Python<'py>,
        request_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let pageknot = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            contained(async move {
                let request: CrawlRequest = serde_json::from_str(&request_json)
                    .map_err(|error| invalid_input("crawl request", error))?;
                let result = pageknot.captures().crawl(request).await?;
                to_json(result)
            })
            .await
        })
    }

    fn inspect_json<'py>(&self, py: Python<'py>, path: String) -> PyResult<Bound<'py, PyAny>> {
        let pageknot = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            contained(async move {
                let manifest = pageknot
                    .artifacts()
                    .inspect(ArtifactInput::File(PortablePath::from(path)))
                    .await?;
                to_json(manifest)
            })
            .await
        })
    }

    #[pyo3(signature = (path, options_json=None))]
    fn verify_json<'py>(
        &self,
        py: Python<'py>,
        path: String,
        options_json: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let pageknot = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            contained(async move {
                let options =
                    decode_options::<VerifyOptions>(options_json.as_deref(), "verify options")?;
                let result = pageknot
                    .artifacts()
                    .verify(
                        ArtifactInput::File(PortablePath::from(path)),
                        options.level.unwrap_or(VerificationPolicy::Offline),
                    )
                    .await?;
                to_json(result)
            })
            .await
        })
    }

    fn export_json<'py>(
        &self,
        py: Python<'py>,
        path: String,
        request_json: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let pageknot = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            contained(async move {
                let request: ArtifactExportRequest = serde_json::from_str(&request_json)
                    .map_err(|error| invalid_input("artifact export request", error))?;
                let result = pageknot
                    .artifacts()
                    .export(ArtifactInput::File(PortablePath::from(path)), request)
                    .await?;
                to_json(result)
            })
            .await
        })
    }

    fn verify_variant_json<'py>(
        &self,
        py: Python<'py>,
        path: String,
        kind: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let pageknot = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            contained(async move {
                let kind: ArtifactVariantKind =
                    serde_json::from_value(serde_json::Value::String(kind))
                        .map_err(|error| invalid_input("artifact variant kind", error))?;
                let result = pageknot
                    .artifacts()
                    .verify_variant(PortablePath::from(path), kind)
                    .await?;
                to_json(result)
            })
            .await
        })
    }

    fn ensure_browser_json<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let pageknot = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            contained(async move {
                let browser = pageknot.browsers().ensure().await?;
                to_json(browser)
            })
            .await
        })
    }

    fn close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let pageknot = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            contained(async move { pageknot.close().await }).await
        })
    }

    fn close_blocking(&self) -> PyResult<()> {
        let pageknot = self.inner.clone();
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("pageknot-python-shutdown".to_owned())
            .spawn(move || {
                let result = match catch_unwind(AssertUnwindSafe(|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|error| {
                            PageKnotError::new(
                                "pageknot.binding.runtime",
                                ErrorStage::Internal,
                                format!("failed to start the binding shutdown runtime: {error}"),
                            )
                        })?;
                    runtime.block_on(pageknot.close())
                })) {
                    Ok(result) => result,
                    Err(_) => Err(panic_error()),
                };
                let _ = sender.send(result);
            })
            .map_err(|error| {
                native_error(PageKnotError::new(
                    "pageknot.binding.runtime",
                    ErrorStage::Internal,
                    format!("failed to start the binding shutdown thread: {error}"),
                ))
            })?;
        match receiver.recv_timeout(Duration::from_secs(15)) {
            Ok(result) => result.map_err(native_error),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                Err(native_error(PageKnotError::new(
                    "pageknot.binding.shutdown_timeout",
                    ErrorStage::Shutdown,
                    "binding shutdown exceeded 15 seconds",
                )))
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                Err(native_error(PageKnotError::new(
                    "pageknot.binding.shutdown",
                    ErrorStage::Shutdown,
                    "binding shutdown ended before reporting its result",
                )))
            }
        }
    }

    #[cfg(feature = "binding-test-hooks")]
    fn test_panic<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            contained(async move {
                std::panic::resume_unwind(Box::new("PageKnot binding fault injection"));
                #[allow(unreachable_code)]
                Ok(())
            })
            .await
        })
    }
}

#[pyclass(
    module = "pageknot._native",
    name = "NativeCaptureJob",
    skip_from_py_object
)]
#[derive(Clone, Debug)]
struct NativeCaptureJob {
    inner: CaptureJob,
}

#[pymethods]
impl NativeCaptureJob {
    #[getter]
    fn id(&self) -> String {
        self.inner.id().to_string()
    }

    #[getter]
    fn status(&self) -> String {
        capture_status_name(self.inner.status()).to_owned()
    }

    fn events(&self) -> NativeCaptureEvents {
        NativeCaptureEvents {
            inner: Arc::new(Mutex::new(self.inner.events())),
        }
    }

    fn cancel(&self) {
        self.inner.cancel();
    }

    fn result_json<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let job = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            contained(async move {
                let result = job.wait().await?;
                to_json(result)
            })
            .await
        })
    }
}

#[pyclass(
    module = "pageknot._native",
    name = "NativeCaptureEvents",
    skip_from_py_object
)]
#[derive(Clone, Debug)]
struct NativeCaptureEvents {
    inner: Arc<Mutex<pageknot::CaptureEvents>>,
}

#[pymethods]
impl NativeCaptureEvents {
    fn next_json<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let events = Arc::clone(&self.inner);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            contained(async move {
                let event = events.lock().await.next().await;
                event.map(to_json).transpose()
            })
            .await
        })
    }
}

fn build_pageknot(options_json: Option<&str>) -> pageknot::Result<PageKnot> {
    let options = decode_options::<PageKnotOptions>(options_json, "PageKnot options")?;
    let mut builder = PageKnot::builder();
    if let Some(path) = options.browser_path {
        builder = builder.browser_path(path);
    }
    if let Some(endpoint) = options.cdp_url {
        let endpoint = url::Url::parse(&endpoint).map_err(|error| {
            PageKnotError::new(
                "pageknot.input.cdp_url",
                ErrorStage::Validation,
                format!("invalid remote browser endpoint: {error}"),
            )
        })?;
        builder = builder.cdp_url(endpoint);
    }
    if let Some(path) = options.cache_dir {
        builder = builder.cache_dir(path);
    }
    if let Some(channel) = options.browser_channel {
        builder = builder.browser_channel(channel);
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
    pageknot: PageKnot,
    url: String,
    options: CaptureOptions,
) -> pageknot::Result<String> {
    if options.output.is_some() && options.max_bytes.is_some() {
        return Err(PageKnotError::new(
            "pageknot.input.output",
            ErrorStage::Validation,
            "capture options must select one output target",
        ));
    }
    let mut capture = pageknot.capture(url)?;
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
    let result = match (options.output, options.max_bytes) {
        (Some(path), None) => capture.save(path).await?,
        (None, Some(maximum)) => capture.to_bytes(maximum).await?,
        (None, None) => capture.run().await?,
        (Some(_), Some(_)) => {
            return Err(PageKnotError::new(
                "pageknot.input.output",
                ErrorStage::Validation,
                "capture options must select one output target",
            ));
        }
    };
    to_json(result)
}

fn decode_options<T>(json: Option<&str>, label: &str) -> pageknot::Result<T>
where
    T: Default + for<'de> Deserialize<'de>,
{
    json.map_or_else(
        || Ok(T::default()),
        |json| serde_json::from_str(json).map_err(|error| invalid_input(label, error)),
    )
}

fn invalid_input(label: &str, error: serde_json::Error) -> PageKnotError {
    PageKnotError::new(
        "pageknot.input.value",
        ErrorStage::Validation,
        format!("invalid {label}: {error}"),
    )
}

fn to_json(value: impl Serialize) -> pageknot::Result<String> {
    serde_json::to_string(&value).map_err(|error| {
        PageKnotError::new(
            "pageknot.binding.serialization",
            ErrorStage::Internal,
            format!("failed to serialize a binding result: {error}"),
        )
    })
}

async fn contained<T, F>(future: F) -> PyResult<T>
where
    T: Send + 'static,
    F: Future<Output = pageknot::Result<T>> + Send + 'static,
{
    match AssertUnwindSafe(future).catch_unwind().await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(native_error(error)),
        Err(_) => Err(native_error(panic_error())),
    }
}

fn contained_sync<T>(operation: impl FnOnce() -> pageknot::Result<T>) -> PyResult<T> {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(native_error(error)),
        Err(_) => Err(native_error(panic_error())),
    }
}

fn native_error(error: PageKnotError) -> PyErr {
    let encoded = serde_json::to_string(&error).unwrap_or_else(|_| {
        "{\"code\":\"pageknot.binding.serialization\",\"message\":\"failed to serialize a native error\",\"stage\":\"internal\",\"retryable\":false}".to_owned()
    });
    PyRuntimeError::new_err(format!("{ERROR_MARKER}{encoded}"))
}

fn panic_error() -> PageKnotError {
    PageKnotError::new(
        "pageknot.internal.panic",
        ErrorStage::Internal,
        "PageKnot encountered an unexpected internal failure",
    )
}

const fn capture_status_name(status: CaptureStatus) -> &'static str {
    match status {
        CaptureStatus::Created => "created",
        CaptureStatus::Validating => "validating",
        CaptureStatus::WaitingForBrowser => "waitingForBrowser",
        CaptureStatus::Navigating => "navigating",
        CaptureStatus::Settling => "settling",
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

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<NativePageKnot>()?;
    module.add_class::<NativeCaptureJob>()?;
    module.add_class::<NativeCaptureEvents>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_options_use_the_same_camel_case_contract() {
        let options = decode_options::<PageKnotOptions>(
            Some(r#"{"maximumContexts":2,"headed":true}"#),
            "PageKnot options",
        );

        assert_eq!(
            options
                .as_ref()
                .ok()
                .and_then(|value| value.maximum_contexts),
            Some(2)
        );
        assert_eq!(
            options.as_ref().ok().and_then(|value| value.headed),
            Some(true)
        );
    }

    #[test]
    fn capture_options_accept_readiness_and_delay() {
        let options = decode_options::<CaptureOptions>(
            Some(r#"{"waitUntil":"network-idle","delayMs":750}"#),
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
}
