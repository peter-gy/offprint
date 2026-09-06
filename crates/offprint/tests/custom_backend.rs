use std::error::Error;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use data_url::DataUrl;
use offprint::ports::{
    BrowserAcquireRequest, BrowserBackend, BrowserContext, BrowserContextRequest, BrowserLease,
    PageSession,
};
use offprint::{
    BatchJob, BatchRequest, BrowserDoctorReport, BrowserInfo, BrowserProduct, BrowserSource,
    CapabilityCheck, CaptureArtifact, CaptureCredentials, CaptureOutput, CaptureRequest,
    ConflictPolicy, CrawlRequest, ErrorStage, ManagedBrowserState, Milliseconds, NetworkPolicy,
    NetworkPolicySummary, Offprint, OffprintError, OutputCapability, PortablePath, ReadinessMode,
    ReadinessPolicy, RequestHeader, ResourceRetrievalSource, Result, ResumeOptions,
    VerificationMode,
};
use offprint_browser::{
    AttachedFrame, FrameObservation, LoadedResource, NavigationResult, NetworkGuard,
    ObservationLimits, ObservationViewport, ObservedFrame, OfflineBrowserObservation,
    ReadinessObservation, SelectionObservation, VisualFallback,
};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use url::Url;

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Debug, Default)]
struct OwnershipCounts {
    acquires: AtomicUsize,
    contexts: AtomicUsize,
    pages: AtomicUsize,
    bounded_frame_queries: AtomicUsize,
    freeze_entries: AtomicUsize,
    page_closes: AtomicUsize,
    context_closes: AtomicUsize,
    lease_closes: AtomicUsize,
    backend_closes: AtomicUsize,
}

#[derive(Debug)]
struct FixtureBackend {
    counts: Arc<OwnershipCounts>,
    verification_attempts_network: bool,
    acquisition_gate: Option<Arc<AcquisitionGate>>,
}

impl FixtureBackend {
    fn new(verification_attempts_network: bool) -> (Arc<Self>, Arc<OwnershipCounts>) {
        let counts = Arc::new(OwnershipCounts::default());
        (
            Arc::new(Self {
                counts: Arc::clone(&counts),
                verification_attempts_network,
                acquisition_gate: None,
            }),
            counts,
        )
    }

    fn gated(stage: AcquisitionStage) -> (Arc<Self>, Arc<OwnershipCounts>, Arc<AcquisitionGate>) {
        let counts = Arc::new(OwnershipCounts::default());
        let acquisition_gate = Arc::new(AcquisitionGate::new(stage));
        (
            Arc::new(Self {
                counts: Arc::clone(&counts),
                verification_attempts_network: false,
                acquisition_gate: Some(Arc::clone(&acquisition_gate)),
            }),
            counts,
            acquisition_gate,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AcquisitionStage {
    Lease,
    Context,
    Page,
}

#[derive(Debug)]
struct AcquisitionGate {
    stage: AcquisitionStage,
    entered: Semaphore,
    release: Semaphore,
}

impl AcquisitionGate {
    fn new(stage: AcquisitionStage) -> Self {
        Self {
            stage,
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
        }
    }

    async fn pause(&self, stage: AcquisitionStage) {
        if self.stage != stage {
            return;
        }
        self.entered.add_permits(1);
        if let Ok(permit) = self.release.acquire().await {
            permit.forget();
        }
    }

    async fn wait_until_entered(&self) -> TestResult {
        tokio::time::timeout(Duration::from_secs(2), self.entered.acquire())
            .await??
            .forget();
        Ok(())
    }

    fn resume(&self) {
        self.release.add_permits(1);
    }
}

#[async_trait::async_trait]
impl BrowserBackend for FixtureBackend {
    async fn acquire(
        &self,
        _request: BrowserAcquireRequest,
        _cancellation: CancellationToken,
    ) -> Result<Box<dyn BrowserLease>> {
        self.counts.acquires.fetch_add(1, Ordering::AcqRel);
        if let Some(gate) = &self.acquisition_gate {
            gate.pause(AcquisitionStage::Lease).await;
        }
        Ok(Box::new(FixtureLease {
            counts: Arc::clone(&self.counts),
            info: browser_info(),
            verification_attempts_network: self.verification_attempts_network,
            acquisition_gate: self.acquisition_gate.clone(),
        }))
    }

    async fn doctor(&self, _browser: &offprint::BrowserSpec) -> BrowserDoctorReport {
        BrowserDoctorReport {
            schema_version: offprint::PUBLIC_SCHEMA_VERSION,
            ready: true,
            selected: Some(browser_info()),
            candidates: Vec::new(),
            managed_cache: ManagedBrowserState {
                cache_dir: PortablePath::new("."),
                installed_revisions: Vec::new(),
                selected_revision: None,
                catalog_version: "fixture".to_owned(),
            },
            collector: CapabilityCheck {
                compatible: true,
                host_version: "1.0".to_owned(),
                peer_version: Some("1.0".to_owned()),
                capabilities: Vec::new(),
                missing_capabilities: Vec::new(),
            },
            output: OutputCapability {
                directory: PortablePath::new("."),
                writable: true,
                atomic_create: true,
                atomic_replace: true,
                reason_code: None,
            },
            configuration: Vec::new(),
            network: NetworkPolicySummary {
                profile: "fixture".to_owned(),
                permits_loopback_initial_origin: true,
                permits_private_addresses: false,
                revalidates_redirects: true,
            },
            recovery: Vec::new(),
        }
    }

    async fn close(&self) -> Result<()> {
        self.counts.backend_closes.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

#[derive(Debug)]
struct CloseGate {
    entered: Semaphore,
    release: Semaphore,
}

impl CloseGate {
    fn new() -> Self {
        Self {
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
        }
    }

    async fn wait_until_entered(&self) -> TestResult {
        tokio::time::timeout(Duration::from_secs(2), self.entered.acquire())
            .await??
            .forget();
        Ok(())
    }

    fn resume(&self) {
        self.release.add_permits(1);
    }
}

#[derive(Debug)]
struct BlockingCloseBackend {
    inner: Arc<FixtureBackend>,
    gate: Arc<CloseGate>,
    fail: bool,
}

impl BlockingCloseBackend {
    fn new(fail: bool) -> (Arc<Self>, Arc<OwnershipCounts>, Arc<CloseGate>) {
        let (inner, counts) = FixtureBackend::new(false);
        let gate = Arc::new(CloseGate::new());
        (
            Arc::new(Self {
                inner,
                gate: Arc::clone(&gate),
                fail,
            }),
            counts,
            gate,
        )
    }
}

#[async_trait::async_trait]
impl BrowserBackend for BlockingCloseBackend {
    async fn acquire(
        &self,
        request: BrowserAcquireRequest,
        cancellation: CancellationToken,
    ) -> Result<Box<dyn BrowserLease>> {
        self.inner.acquire(request, cancellation).await
    }

    async fn doctor(&self, browser: &offprint::BrowserSpec) -> BrowserDoctorReport {
        self.inner.doctor(browser).await
    }

    async fn close(&self) -> Result<()> {
        self.gate.entered.add_permits(1);
        if let Ok(permit) = self.gate.release.acquire().await {
            permit.forget();
        }
        self.inner.close().await?;
        if self.fail {
            Err(fixture_error("fixture shutdown failed"))
        } else {
            Ok(())
        }
    }
}

#[derive(Debug)]
struct FixtureLease {
    counts: Arc<OwnershipCounts>,
    info: BrowserInfo,
    verification_attempts_network: bool,
    acquisition_gate: Option<Arc<AcquisitionGate>>,
}

#[async_trait::async_trait]
impl BrowserLease for FixtureLease {
    fn info(&self) -> &BrowserInfo {
        &self.info
    }

    async fn create_context(
        &self,
        request: BrowserContextRequest,
        _cancellation: CancellationToken,
    ) -> Result<Box<dyn BrowserContext>> {
        self.counts.contexts.fetch_add(1, Ordering::AcqRel);
        if let Some(gate) = &self.acquisition_gate {
            gate.pause(AcquisitionStage::Context).await;
        }
        Ok(Box::new(FixtureContext {
            counts: Arc::clone(&self.counts),
            deny_network: request.deny_network,
            verification_attempts_network: self.verification_attempts_network,
            acquisition_gate: self.acquisition_gate.clone(),
        }))
    }

    async fn close(self: Box<Self>) -> Result<()> {
        self.counts.lease_closes.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

#[derive(Debug)]
struct FixtureContext {
    counts: Arc<OwnershipCounts>,
    deny_network: bool,
    verification_attempts_network: bool,
    acquisition_gate: Option<Arc<AcquisitionGate>>,
}

#[async_trait::async_trait]
impl BrowserContext for FixtureContext {
    async fn open_page(&self, _cancellation: CancellationToken) -> Result<Box<dyn PageSession>> {
        self.counts.pages.fetch_add(1, Ordering::AcqRel);
        if let Some(gate) = &self.acquisition_gate {
            gate.pause(AcquisitionStage::Page).await;
        }
        Ok(Box::new(FixturePage {
            counts: Arc::clone(&self.counts),
            navigated_url: Mutex::new(None),
            deny_network: self.deny_network,
            verification_attempts_network: self.verification_attempts_network,
        }))
    }

    async fn close(self: Box<Self>) -> Result<()> {
        self.counts.context_closes.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

#[derive(Debug)]
struct FixturePage {
    counts: Arc<OwnershipCounts>,
    navigated_url: Mutex<Option<Url>>,
    deny_network: bool,
    verification_attempts_network: bool,
}

#[async_trait::async_trait]
impl PageSession for FixturePage {
    fn session_id(&self) -> &str {
        "fixture-session"
    }

    async fn apply_credentials(
        &self,
        _credentials: &CaptureCredentials,
        _initial_url: &Url,
        _guard: &NetworkGuard,
    ) -> Result<()> {
        Ok(())
    }

    async fn enable_network_guard(
        &self,
        _guard: NetworkGuard,
        _headers: Vec<RequestHeader>,
        _header_origin: Url,
    ) -> Result<()> {
        Ok(())
    }

    async fn navigate_page(
        &self,
        url: &Url,
        _readiness: ReadinessMode,
        _redirect_limit: u32,
        _deadline: Duration,
    ) -> Result<NavigationResult> {
        let mut navigated_url = self.navigated_url.lock().map_err(lock_error)?;
        *navigated_url = Some(url.clone());
        Ok(NavigationResult {
            frame_id: "fixture-frame".to_owned(),
            final_url: url.clone(),
            redirects: Vec::new(),
        })
    }

    async fn settle(&self, _policy: &ReadinessPolicy) -> Result<ReadinessObservation> {
        Ok(ReadinessObservation {
            reason: "fixture-ready".to_owned(),
            elapsed: Milliseconds::new(1),
            in_flight_requests: 0,
            mutation_quiet: true,
            fonts_ready: true,
        })
    }

    async fn freeze_attached_frames(&self) -> Result<()> {
        self.counts.freeze_entries.fetch_add(1, Ordering::AcqRel);
        let stalls = self
            .navigated_url
            .lock()
            .map_err(lock_error)?
            .as_ref()
            .is_some_and(|url| url.path() == "/stall-freeze");
        if stalls {
            std::future::pending::<()>().await;
        }
        Ok(())
    }

    async fn attached_frames_bounded(&self, maximum: u32) -> Result<Vec<AttachedFrame>> {
        self.counts
            .bounded_frame_queries
            .fetch_add(1, Ordering::AcqRel);
        let navigated = self.navigated_url.lock().map_err(lock_error)?.clone();
        let exceeds_limit = navigated
            .as_ref()
            .is_some_and(|url| url.path() == "/frame-overflow");
        if exceeds_limit {
            return Err(OffprintError::new(
                "offprint.frame.limit",
                ErrorStage::Collection,
                "fixture frame count exceeds the configured limit",
            )
            .with_detail("limit", maximum));
        }
        let file_frame = navigated.as_ref().and_then(|url| {
            (url.path() == "/file-frame")
                .then(|| {
                    url.query_pairs()
                        .find(|(name, _)| name == "frame")
                        .map(|(_, value)| value.into_owned())
                })
                .flatten()
        });
        Ok(file_frame
            .map(|url| {
                vec![AttachedFrame {
                    session_id: "file-frame-session".to_owned(),
                    target_id: "file-frame-target".to_owned(),
                    parent_session_id: "fixture-session".to_owned(),
                    url,
                }]
            })
            .unwrap_or_default())
    }

    async fn frame_owner_path(&self, _frame: &AttachedFrame) -> Result<Vec<u32>> {
        Err(fixture_error("fixture has no attached frame owners"))
    }

    async fn observe_frame(
        &self,
        _session_id: &str,
        _frame_id: offprint::FrameId,
        _capture_id: &offprint::CaptureId,
        _limits: ObservationLimits,
        _content: &offprint::ContentPolicy,
    ) -> Result<ObservedFrame> {
        let url = self
            .navigated_url
            .lock()
            .map_err(lock_error)?
            .clone()
            .ok_or_else(|| fixture_error("fixture page was collected before navigation"))?;
        if url.path() == "/runtime-failure" {
            return Err(fixture_error("fixture runtime failure"));
        }
        let links = match url.path() {
            "/crawl" => {
                r#"<a href="/b">b</a><a href="/a">a</a><a href="https://outside.example/">outside</a>"#
            }
            "/a" => r#"<a href="/c">c</a>"#,
            "/b" => r#"<a href="/c#duplicate">c duplicate</a>"#,
            "/resource-overflow" => r#"<img src="/one.png"><img src="/two.png">"#,
            "/embedded-html" => r#"<object data="/active-frame.html"></object>"#,
            _ => "",
        };
        let html = format!(
            "<html><head><title>Fixture backend</title></head><body><main>captured by fixture backend</main>{links}</body></html>"
        );
        let observation = FrameObservation {
            doctype: "<!DOCTYPE html>".to_owned(),
            html,
            requested_url: url.to_string(),
            final_url: url.to_string(),
            base_url: url.to_string(),
            title: "Fixture backend".to_owned(),
            encoding: "UTF-8".to_owned(),
            viewport: ObservationViewport {
                width: 1440,
                height: 900,
                device_scale_factor: "1".to_owned(),
                scroll_x: "0".to_owned(),
                scroll_y: "0".to_owned(),
            },
            frames: 1,
            nodes: 6,
            subtree_nodes: 6,
            warnings: Vec::new(),
            visual_fallbacks: Vec::new(),
            selection: SelectionObservation::default(),
            frame_owners: Vec::new(),
        };
        let encoded_bytes = u64::try_from(
            serde_json::to_vec(&observation)
                .map_err(|error| fixture_error(format!("fixture observation failed: {error}")))?
                .len(),
        )
        .map_err(|error| fixture_error(format!("fixture observation is too large: {error}")))?;
        Ok(ObservedFrame {
            observation,
            encoded_bytes,
        })
    }

    async fn capture_visual_fallback(
        &self,
        _session_id: &str,
        _fallback: &VisualFallback,
        _maximum_bytes: u64,
    ) -> Result<Vec<u8>> {
        Err(fixture_error("fixture has no visual fallback"))
    }

    async fn load_resource_in_session(
        &self,
        _session_id: &str,
        _browser_frame_id: &str,
        url: &Url,
        _maximum_bytes: u64,
    ) -> Result<LoadedResource> {
        if url.path() != "/active-frame.html" {
            return Err(fixture_error("fixture resource is unavailable"));
        }
        let body = Bytes::from_static(
            br#"<html><body onload="run()"><script>run()</script><p>safe frame</p></body></html>"#,
        );
        Ok(LoadedResource {
            final_url: url.clone(),
            redirects: Vec::new(),
            status: 200,
            media_type: Some("text/html".to_owned()),
            encoded_length: Some(u64::try_from(body.len()).unwrap_or(u64::MAX)),
            body: Box::pin(futures_util::stream::once(async { Ok(body) })),
            source: ResourceRetrievalSource::BrowserContextFetch,
        })
    }

    async fn verify_offline_url(
        &self,
        _url: &Url,
        _deadline: Duration,
    ) -> Result<OfflineBrowserObservation> {
        let attempted_urls = if self.deny_network && self.verification_attempts_network {
            vec!["https://blocked.example/asset.css?token=secret".to_owned()]
        } else {
            Vec::new()
        };
        Ok(OfflineBrowserObservation {
            attempted_urls,
            page_errors: Vec::new(),
            stable: true,
        })
    }

    async fn close(self: Box<Self>) -> Result<()> {
        self.counts.page_closes.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

fn browser_info() -> BrowserInfo {
    BrowserInfo {
        product: BrowserProduct::Chromium,
        version: "fixture".to_owned(),
        source: BrowserSource::Remote,
        executable_path: None,
        endpoint: None,
        revision: None,
        protocol_version: "1.3".to_owned(),
    }
}

fn lock_error<T>(_error: std::sync::PoisonError<T>) -> OffprintError {
    fixture_error("fixture state lock is unavailable")
}

fn fixture_error(message: impl Into<String>) -> OffprintError {
    OffprintError::new("offprint.internal.fixture", ErrorStage::Internal, message)
}

fn capture_request() -> Result<CaptureRequest> {
    let mut request = CaptureRequest::builder("http://127.0.0.1/")?.build()?;
    request.network = NetworkPolicy::Standard;
    request.verification = VerificationMode::Static;
    request.output = CaptureOutput::memory(2 * 1024 * 1024);
    Ok(request)
}

fn assert_owner_counts(counts: &OwnershipCounts, captures: usize, backend_closes: usize) {
    assert_eq!(counts.acquires.load(Ordering::Acquire), captures);
    assert_eq!(counts.contexts.load(Ordering::Acquire), captures);
    assert_eq!(counts.pages.load(Ordering::Acquire), captures);
    assert_eq!(counts.page_closes.load(Ordering::Acquire), captures);
    assert_eq!(counts.context_closes.load(Ordering::Acquire), captures);
    assert_eq!(counts.lease_closes.load(Ordering::Acquire), captures);
    assert_eq!(
        counts.backend_closes.load(Ordering::Acquire),
        backend_closes
    );
}

async fn wait_for_owner_cleanup(counts: &OwnershipCounts) -> TestResult {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if counts.page_closes.load(Ordering::Acquire) == 1
                && counts.context_closes.load(Ordering::Acquire) == 1
                && counts.lease_closes.load(Ordering::Acquire) == 1
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn custom_backend_repeated_captures_release_every_owner() -> TestResult {
    const CAPTURES: usize = 32;

    let (backend, counts) = FixtureBackend::new(false);
    let offprint = Offprint::builder().browser_backend(backend).build()?;
    for index in 0..CAPTURES {
        let job = offprint.captures().start(capture_request()?).await?;
        if index % 2 == 0 {
            drop(job.events());
        }
        let result = job.result().await?;
        let CaptureArtifact::Bytes { content, .. } = result.artifact else {
            return Err(std::io::Error::other("expected an in-memory artifact").into());
        };
        assert!(String::from_utf8_lossy(&content).contains("captured by fixture backend"));
    }

    assert_owner_counts(&counts, CAPTURES, 0);
    offprint.close().await?;
    offprint.close().await?;
    assert_owner_counts(&counts, CAPTURES, 1);
    Ok(())
}

#[tokio::test]
async fn file_capture_inside_an_allowed_root_reaches_the_browser_port() -> TestResult {
    let root = tempfile::tempdir()?;
    let source = root.path().join("source.html");
    std::fs::write(&source, "<main>local source</main>")?;
    let url = Url::from_file_path(&source)
        .map_err(|()| std::io::Error::other("fixture file URL is invalid"))?;
    let root = PortablePath::from_path_buf(root.path().to_path_buf())?;
    let capture = offprint::ContentPolicy {
        allowed_file_roots: vec![root],
        ..offprint::ContentPolicy::default()
    };
    let mut request = CaptureRequest::builder(url.as_str())?
        .content(capture)
        .build()?;
    request.verification = VerificationMode::Static;
    request.output = CaptureOutput::memory(2 * 1024 * 1024);
    let (backend, counts) = FixtureBackend::new(false);
    let offprint = Offprint::builder().browser_backend(backend).build()?;

    offprint.captures().start(request).await?.result().await?;

    offprint.close().await?;
    assert_owner_counts(&counts, 1, 1);
    Ok(())
}

#[tokio::test]
async fn embedded_html_resources_are_sanitized_before_static_verification() -> TestResult {
    let (backend, counts) = FixtureBackend::new(false);
    let offprint = Offprint::builder().browser_backend(backend).build()?;
    let mut request = capture_request()?;
    request.url = Url::parse("http://127.0.0.1/embedded-html")?;

    let result = offprint.captures().start(request).await?.result().await?;
    let CaptureArtifact::Bytes { content, .. } = result.artifact else {
        return Err(std::io::Error::other("expected an in-memory artifact").into());
    };
    let html = String::from_utf8(content)?;
    let document = offprint_document::Document::parse(html.as_bytes());
    let base = Url::parse("file:///capture.html")?;
    let resources = offprint_document::discover_document_resources(&document, &base)?;
    let embedded = resources
        .resources()
        .first()
        .ok_or_else(|| std::io::Error::other("embedded HTML resource is missing"))?;
    let (payload, _) = DataUrl::process(embedded.resolved_url.as_str())?.decode_to_vec()?;
    let embedded_html = String::from_utf8(payload)?;

    assert!(embedded_html.contains("safe frame"));
    assert!(!embedded_html.contains("<script>run()</script>"));
    assert!(!embedded_html.contains("onload=\"run()\""));
    offprint.close().await?;
    assert_owner_counts(&counts, 1, 1);
    Ok(())
}

#[tokio::test]
async fn attached_file_frames_must_resolve_inside_an_allowed_root() -> TestResult {
    let root = tempfile::tempdir()?;
    let outside = tempfile::NamedTempFile::new()?;
    let outside_url = Url::from_file_path(outside.path())
        .map_err(|()| std::io::Error::other("outside file URL is invalid"))?;
    let allowed_root = PortablePath::from_path_buf(root.path().to_path_buf())?;
    let mut url = Url::parse("http://127.0.0.1/file-frame")?;
    url.query_pairs_mut()
        .append_pair("frame", outside_url.as_str());
    let mut request = capture_request()?;
    request.url = url;
    request.content.allowed_file_roots = vec![allowed_root];
    let (backend, counts) = FixtureBackend::new(false);
    let offprint = Offprint::builder().browser_backend(backend).build()?;

    let result = offprint.captures().start(request).await?.result().await;

    assert_eq!(
        result.err().map(|error| error.code.as_str().to_owned()),
        Some("offprint.input.file_root".to_owned())
    );
    assert_eq!(counts.freeze_entries.load(Ordering::Acquire), 0);
    offprint.close().await?;
    assert_owner_counts(&counts, 1, 1);
    Ok(())
}

#[tokio::test]
async fn cancellation_during_each_acquisition_stage_releases_every_owner() -> TestResult {
    for stage in [
        AcquisitionStage::Lease,
        AcquisitionStage::Context,
        AcquisitionStage::Page,
    ] {
        let (backend, counts, gate) = FixtureBackend::gated(stage);
        let offprint = Offprint::builder().browser_backend(backend).build()?;
        let job = offprint.captures().start(capture_request()?).await?;
        gate.wait_until_entered().await?;

        job.cancel();
        let result = tokio::time::timeout(Duration::from_secs(2), job.result()).await?;
        let Err(error) = result else {
            return Err(std::io::Error::other(format!(
                "capture completed after cancellation at {stage:?}"
            ))
            .into());
        };
        assert_eq!(error.code.as_str(), "offprint.runtime.cancelled");
        gate.resume();
        wait_for_owner_cleanup(&counts).await?;
        assert_owner_counts(&counts, 1, 0);
        offprint.close().await?;
        assert_owner_counts(&counts, 1, 1);
    }
    Ok(())
}

#[tokio::test]
async fn total_deadline_interrupts_stalled_page_work_and_awaits_owner_cleanup() -> TestResult {
    let (backend, counts) = FixtureBackend::new(false);
    let offprint = Offprint::builder().browser_backend(backend).build()?;
    let mut request = capture_request()?;
    request.url = Url::parse("http://127.0.0.1/stall-freeze")?;
    request.limits.duration = Milliseconds::new(250);

    let result = tokio::time::timeout(
        Duration::from_secs(2),
        offprint.captures().start(request).await?.result(),
    )
    .await?;
    let Err(error) = result else {
        return Err(std::io::Error::other("stalled page work exceeded its deadline").into());
    };

    assert_eq!(error.code.as_str(), "offprint.runtime.timeout");
    assert_eq!(counts.freeze_entries.load(Ordering::Acquire), 1);
    assert_owner_counts(&counts, 1, 0);
    offprint.close().await?;
    assert_owner_counts(&counts, 1, 1);
    Ok(())
}

#[tokio::test]
async fn resource_inventory_stops_at_the_configured_limit_in_warning_mode() -> TestResult {
    let (backend, counts) = FixtureBackend::new(false);
    let offprint = Offprint::builder().browser_backend(backend).build()?;
    let mut request = capture_request()?;
    request.url = Url::parse("http://127.0.0.1/resource-overflow")?;
    request.limits.resources = 1;

    let Err(error) = offprint.captures().start(request).await?.result().await else {
        return Err(std::io::Error::other("resource inventory exceeded its limit").into());
    };

    assert_eq!(error.code.as_str(), "offprint.resource.limit");
    assert_eq!(error.stage, ErrorStage::Resource);
    assert_owner_counts(&counts, 1, 0);
    offprint.close().await?;
    assert_owner_counts(&counts, 1, 1);
    Ok(())
}

#[tokio::test]
async fn frame_topology_limit_runs_before_freezing_or_owner_lookup() -> TestResult {
    let (backend, counts) = FixtureBackend::new(false);
    let offprint = Offprint::builder().browser_backend(backend).build()?;
    let mut request = capture_request()?;
    request.url = Url::parse("http://127.0.0.1/frame-overflow")?;
    request.limits.frames = 1;

    let Err(error) = offprint.captures().start(request).await?.result().await else {
        return Err(std::io::Error::other("frame topology exceeded its limit").into());
    };

    assert_eq!(error.code.as_str(), "offprint.frame.limit");
    assert_eq!(error.stage, ErrorStage::Collection);
    assert_eq!(counts.bounded_frame_queries.load(Ordering::Acquire), 1);
    assert_eq!(counts.freeze_entries.load(Ordering::Acquire), 0);
    assert_owner_counts(&counts, 1, 0);
    offprint.close().await?;
    assert_owner_counts(&counts, 1, 1);
    Ok(())
}

#[tokio::test]
async fn close_survives_a_dropped_waiter_and_shares_its_terminal_error() -> TestResult {
    let (backend, counts, gate) = BlockingCloseBackend::new(true);
    let offprint = Offprint::builder().browser_backend(backend).build()?;
    let first_offprint = offprint.clone();
    let first = tokio::spawn(async move { first_offprint.close().await });
    gate.wait_until_entered().await?;

    first.abort();
    let _aborted = first.await;
    let second_offprint = offprint.clone();
    let third_offprint = offprint.clone();
    let second = tokio::spawn(async move { second_offprint.close().await });
    let third = tokio::spawn(async move { third_offprint.close().await });
    tokio::task::yield_now().await;
    assert!(!second.is_finished());
    assert!(!third.is_finished());

    gate.resume();
    let second_error = second.await?.err();
    let third_error = third.await?.err();
    let later_error = offprint.close().await.err();
    for error in [second_error, third_error, later_error] {
        let Some(error) = error else {
            return Err(std::io::Error::other("fixture shutdown error was lost").into());
        };
        assert_eq!(error.code.as_str(), "offprint.internal.fixture");
        assert_eq!(error.message, "fixture shutdown failed");
    }
    assert_eq!(counts.backend_closes.load(Ordering::Acquire), 1);
    Ok(())
}

#[tokio::test]
async fn encoding_limit_failure_releases_custom_backend_owners() -> TestResult {
    let (backend, counts) = FixtureBackend::new(false);
    let offprint = Offprint::builder().browser_backend(backend).build()?;
    let mut request = capture_request()?;
    request.output = CaptureOutput::memory(512);
    request.limits.artifact_bytes = 512;

    let Err(error) = offprint.captures().start(request).await?.result().await else {
        return Err(std::io::Error::other(
            "the encoded artifact stayed within an intentionally tiny limit",
        )
        .into());
    };

    assert_eq!(error.code.as_str(), "offprint.artifact.size");
    assert_eq!(error.stage, ErrorStage::Encoding);
    assert_owner_counts(&counts, 1, 0);
    offprint.close().await?;
    assert_owner_counts(&counts, 1, 1);
    Ok(())
}

#[tokio::test]
async fn offline_verification_failure_rolls_back_and_releases_both_pages() -> TestResult {
    let directory = tempfile::tempdir()?;
    let destination = directory.path().join("capture.html");
    std::fs::write(&destination, b"existing artifact")?;
    let destination = PortablePath::from_path_buf(destination)?;
    let (backend, counts) = FixtureBackend::new(true);
    let offprint = Offprint::builder().browser_backend(backend).build()?;
    let mut request = capture_request()?;
    request.verification = VerificationMode::Offline;
    request.output = CaptureOutput::file(destination.clone());
    request.output = request.output.with_conflict(ConflictPolicy::Replace);

    let Err(error) = offprint.captures().start(request).await?.result().await else {
        return Err(
            std::io::Error::other("offline verification accepted an external request").into(),
        );
    };

    assert_eq!(error.code.as_str(), "offprint.verification.network");
    assert_eq!(error.stage, ErrorStage::Verification);
    assert_eq!(
        std::fs::read(destination.as_utf8_path())?,
        b"existing artifact"
    );
    assert_owner_counts(&counts, 2, 0);
    offprint.close().await?;
    assert_owner_counts(&counts, 2, 1);
    Ok(())
}

#[tokio::test]
async fn batch_scheduler_isolates_runtime_failures() -> TestResult {
    let (backend, counts) = FixtureBackend::new(false);
    let offprint = Offprint::builder().browser_backend(backend).build()?;
    let success = capture_request()?;
    let mut failure = capture_request()?;
    failure.url = Url::parse("http://127.0.0.1/runtime-failure")?;

    let result = offprint
        .captures()
        .batch(BatchRequest {
            schema_version: offprint_model::PUBLIC_SCHEMA_VERSION,
            jobs: vec![
                BatchJob {
                    id: "success".to_owned(),
                    request: success,
                },
                BatchJob {
                    id: "failure".to_owned(),
                    request: failure,
                },
            ],
            concurrency: 2,
            resume: None,
        })
        .await?;

    assert_eq!(result.succeeded, 1);
    assert_eq!(result.failed, 1);
    assert_eq!(result.outcomes.len(), 2);
    assert_owner_counts(&counts, 2, 0);
    offprint.close().await?;
    Ok(())
}

#[tokio::test]
async fn batch_resume_reuses_current_outputs_and_recaptures_stale_outputs() -> TestResult {
    let directory = tempfile::tempdir()?;
    let manifest = PortablePath::from_path_buf(directory.path().join("resume.json"))?;
    let output_a = PortablePath::from_path_buf(directory.path().join("a.html"))?;
    let output_b = PortablePath::from_path_buf(directory.path().join("b.html"))?;
    let (backend, counts) = FixtureBackend::new(false);
    let offprint = Offprint::builder().browser_backend(backend).build()?;
    let make_request = |url: &str, output: PortablePath| -> Result<CaptureRequest> {
        let mut request = capture_request()?;
        request.url = Url::parse(url).map_err(|error| {
            OffprintError::new(
                "offprint.input.url",
                ErrorStage::Validation,
                error.to_string(),
            )
        })?;
        request.output = CaptureOutput::file(output);
        request.output = request.output.with_conflict(ConflictPolicy::Replace);
        Ok(request)
    };
    let batch = BatchRequest {
        schema_version: offprint_model::PUBLIC_SCHEMA_VERSION,
        jobs: vec![
            BatchJob {
                id: "a".to_owned(),
                request: make_request("http://127.0.0.1/a", output_a.clone())?,
            },
            BatchJob {
                id: "b".to_owned(),
                request: make_request("http://127.0.0.1/b", output_b.clone())?,
            },
        ],
        concurrency: 2,
        resume: Some(ResumeOptions {
            manifest,
            retry_failed: false,
        }),
    };

    let initial = offprint.captures().batch(batch.clone()).await?;
    assert_eq!(initial.succeeded, 2);
    assert_eq!(initial.resumed, 0);
    assert_owner_counts(&counts, 2, 0);

    let unchanged = offprint.captures().batch(batch.clone()).await?;
    assert_eq!(unchanged.succeeded, 2);
    assert_eq!(unchanged.resumed, 2);
    assert_owner_counts(&counts, 2, 0);

    std::fs::write(&output_b, b"stale")?;
    let repaired = offprint.captures().batch(batch).await?;
    assert_eq!(repaired.succeeded, 2);
    assert_eq!(repaired.resumed, 1);
    assert_owner_counts(&counts, 3, 0);
    offprint.close().await?;
    Ok(())
}

#[tokio::test]
async fn crawl_resume_preserves_breadth_first_pending_order() -> TestResult {
    let directory = tempfile::tempdir()?;
    let output = PortablePath::from_path_buf(directory.path().join("site"))?;
    let manifest = PortablePath::from_path_buf(directory.path().join("crawl.json"))?;
    let (backend, counts) = FixtureBackend::new(false);
    let offprint = Offprint::builder().browser_backend(backend).build()?;
    let mut seed = capture_request()?;
    seed.url = Url::parse("http://127.0.0.1/crawl")?;
    let crawl = CrawlRequest {
        schema_version: offprint_model::PUBLIC_SCHEMA_VERSION,
        seed,
        output_directory: output,
        maximum_pages: 4,
        maximum_depth: 2,
        concurrency: 2,
        same_origin: true,
        resume: Some(ResumeOptions {
            manifest,
            retry_failed: false,
        }),
    };

    let initial = offprint.captures().crawl(crawl.clone()).await?;
    let paths = initial
        .outcomes
        .iter()
        .map(|outcome| outcome.url.path().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(paths, ["/crawl", "/a", "/b", "/c"]);
    assert_eq!(initial.succeeded, 4);
    assert_eq!(initial.resumed, 0);
    assert_owner_counts(&counts, 4, 0);

    let resumed = offprint.captures().crawl(crawl).await?;
    assert_eq!(resumed.succeeded, 4);
    assert_eq!(resumed.resumed, 4);
    assert_owner_counts(&counts, 4, 0);
    offprint.close().await?;
    Ok(())
}
