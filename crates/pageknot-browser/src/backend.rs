use std::fmt;
use std::pin::Pin;

use async_trait::async_trait;
use bytes::Bytes;
use futures_core::Stream;
use pageknot_model::{
    BrowserDoctorReport, BrowserEnvironment, BrowserInfo, BrowserSpec, CaptureCredentials,
    CaptureId, CapturePolicy, ErrorStage, FrameId, Milliseconds, NetworkPolicy, PageKnotError,
    ReadinessMode, ReadinessPolicy, RequestHeader, ResourceRetrievalSource, Result,
};
use pageknot_protocol::{PageObservation, VisualFallback};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::NetworkGuard;

/// A bounded asynchronous resource body.
pub type BodyStream = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send + Sync + 'static>>;

#[derive(Clone, Debug)]
/// Browser acquisition parameters for one capture.
pub struct BrowserAcquireRequest {
    /// The capture that owns the lease.
    pub capture_id: CaptureId,
    /// The requested browser selection.
    pub browser: BrowserSpec,
    /// Whether a locally launched browser should be visible.
    pub headed: bool,
}

#[derive(Clone, Debug)]
/// Isolation and emulation settings for one browser context.
pub struct BrowserContextRequest {
    /// Viewport, locale, timezone, color, motion, and user-agent settings.
    pub environment: BrowserEnvironment,
    /// The capture network policy.
    pub network: NetworkPolicy,
    /// Maximum frame count, including the top-level document.
    pub maximum_frames: u32,
    /// Byte limits applied while buffering browser-observed responses.
    pub resource_observation: ResourceObservationLimits,
    /// Whether every external network request must be denied.
    pub deny_network: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Byte limits for browser-observed response bodies.
pub struct ResourceObservationLimits {
    /// Maximum decoded bytes retained for one response.
    pub maximum_resource_bytes: u64,
    /// Maximum decoded bytes retained across all responses in one context.
    pub maximum_total_resource_bytes: u64,
}

impl Default for ResourceObservationLimits {
    fn default() -> Self {
        Self {
            maximum_resource_bytes: 64 * 1024 * 1024,
            maximum_total_resource_bytes: 512 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug)]
/// Browser signals observed when the page became ready for collection.
pub struct ReadinessObservation {
    /// The readiness condition that completed.
    pub reason: String,
    /// Time spent settling after navigation.
    pub elapsed: Milliseconds,
    /// Render-affecting requests still active at the capture epoch.
    pub in_flight_requests: u32,
    /// Whether the mutation quiet window completed.
    pub mutation_quiet: bool,
    /// Whether document fonts reached a terminal readiness state.
    pub fonts_ready: bool,
}

#[derive(Clone, Debug)]
/// A flattened child-frame target attached to the top-level page.
pub struct AttachedFrame {
    /// The backend session used to address the child target.
    pub session_id: String,
    /// The browser target identifier.
    pub target_id: String,
    /// The session that owns the frame element.
    pub parent_session_id: String,
    /// The frame URL at attachment time.
    pub url: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Limits applied while collecting one frame observation.
pub struct CollectorLimits {
    /// Depth of the frame addressed by this collector call.
    pub frame_depth: u16,
    /// Maximum encoded bytes in one transferred chunk.
    pub maximum_chunk_bytes: u64,
    /// Maximum depth for recursive same-origin frame collection.
    pub maximum_frame_depth: u16,
    /// Maximum remaining frames, including the addressed frame.
    pub maximum_frames: u32,
    /// Maximum cloneable DOM nodes allowed before snapshot allocation.
    pub maximum_nodes: u64,
    /// Maximum encoded bytes in the complete frame observation.
    pub maximum_payload_bytes: u64,
}

#[derive(Clone, Debug)]
/// One decoded frame observation and its encoded transfer size.
pub struct CollectedPageObservation {
    /// The decoded collector payload.
    pub observation: PageObservation,
    /// Encoded bytes declared by the validated observation descriptor.
    pub encoded_bytes: u64,
}

#[derive(Clone, Debug)]
/// One browser-observed navigation redirect.
pub struct NavigationRedirect {
    /// The URL that returned the redirect.
    pub from: Url,
    /// The resolved redirect destination.
    pub to: Url,
    /// The HTTP redirect status.
    pub status: u16,
}

#[derive(Clone, Debug)]
/// The completed top-level navigation.
pub struct NavigationResult {
    /// The browser frame identifier for the top-level document.
    pub frame_id: String,
    /// The committed final URL.
    pub final_url: Url,
    /// Redirects in browser-observed order.
    pub redirects: Vec<NavigationRedirect>,
}

/// A resource body loaded in the security context of its owning frame.
pub struct LoadedResource {
    /// The final URL after resource redirects.
    pub final_url: Url,
    /// Resource redirect destinations in order.
    pub redirects: Vec<Url>,
    /// The HTTP response status.
    pub status: u16,
    /// The response media type when known.
    pub media_type: Option<String>,
    /// The encoded response length when reported by the browser.
    pub encoded_length: Option<u64>,
    /// The bounded response body stream.
    pub body: BodyStream,
    /// The browser retrieval mechanism that produced the body.
    pub source: ResourceRetrievalSource,
}

impl fmt::Debug for LoadedResource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LoadedResource")
            .field("final_url", &self.final_url)
            .field("redirects", &self.redirects)
            .field("status", &self.status)
            .field("media_type", &self.media_type)
            .field("encoded_length", &self.encoded_length)
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
/// Browser evidence from a denied-network artifact reopen.
pub struct OfflineBrowserObservation {
    /// External URLs the artifact attempted to request.
    pub attempted_urls: Vec<String>,
    /// Page errors raised while opening the artifact.
    pub page_errors: Vec<String>,
    /// Whether the document reached the verification stability window.
    pub stable: bool,
}

#[async_trait]
/// Acquires browser leases for PageKnot capture jobs.
pub trait BrowserBackend: fmt::Debug + Send + Sync {
    /// Acquires a browser lease owned by one capture.
    async fn acquire(
        &self,
        request: BrowserAcquireRequest,
        cancellation: CancellationToken,
    ) -> Result<Box<dyn BrowserLease>>;

    /// Reports availability and recovery diagnostics for `browser`.
    async fn doctor(&self, browser: &BrowserSpec) -> BrowserDoctorReport;

    /// Returns the browser currently owned by this backend.
    ///
    /// The default implementation returns `None`. Shared backends override
    /// this method with their current browser identity.
    async fn active_browser(&self) -> Option<BrowserInfo> {
        None
    }

    /// Releases every browser resource owned by this backend.
    async fn close(&self) -> Result<()>;
}

#[async_trait]
/// An acquired browser process or remote browser connection.
pub trait BrowserLease: fmt::Debug + Send + Sync {
    /// Returns the selected browser identity.
    fn info(&self) -> &BrowserInfo;

    /// Creates an isolated context for one capture.
    async fn create_context(
        &self,
        request: BrowserContextRequest,
        cancellation: CancellationToken,
    ) -> Result<Box<dyn BrowserContext>>;

    /// Releases this lease and any remaining owned contexts.
    async fn close(self: Box<Self>) -> Result<()>;
}

#[async_trait]
/// An isolated browser context.
pub trait BrowserContext: fmt::Debug + Send + Sync {
    /// Opens one top-level page session.
    async fn open_page(&self, cancellation: CancellationToken) -> Result<Box<dyn PageSession>>;

    /// Releases the context and every page it owns.
    async fn close(self: Box<Self>) -> Result<()>;
}

#[async_trait]
/// Browser operations required by the PageKnot capture pipeline.
pub trait PageSession: fmt::Debug + Send + Sync {
    /// Returns the backend session identifier for the top-level target.
    fn session_id(&self) -> &str;

    /// Applies cookies and request credentials before navigation.
    async fn apply_credentials(
        &self,
        credentials: &CaptureCredentials,
        initial_url: &Url,
        guard: &NetworkGuard,
    ) -> Result<()>;

    /// Enables request interception and address-policy enforcement.
    async fn enable_network_guard(
        &self,
        guard: NetworkGuard,
        headers: Vec<RequestHeader>,
        header_origin: Url,
    ) -> Result<()>;

    /// Navigates the top-level page and records the redirect chain.
    async fn navigate_page(
        &self,
        url: &Url,
        readiness: ReadinessMode,
        redirect_limit: u32,
        deadline: std::time::Duration,
    ) -> Result<NavigationResult>;

    /// Waits for the configured browser readiness signals.
    async fn settle(&self, policy: &ReadinessPolicy) -> Result<ReadinessObservation>;

    /// Freezes animation and transition state in every attached frame.
    async fn freeze_attached_frames(&self) -> Result<()>;

    /// Returns the flattened child-frame target set.
    async fn attached_frames(&self) -> Result<Vec<AttachedFrame>>;

    /// Returns at most `maximum` attached child-frame targets.
    async fn attached_frames_bounded(&self, maximum: u32) -> Result<Vec<AttachedFrame>> {
        let frames = self.attached_frames().await?;
        if u64::try_from(frames.len()).unwrap_or(u64::MAX) > u64::from(maximum) {
            return Err(PageKnotError::new(
                "pageknot.frame.limit",
                ErrorStage::Collection,
                "frame count exceeds the configured limit",
            )
            .with_detail("limit", maximum));
        }
        Ok(frames)
    }

    /// Resolves the frame element path in its parent observation.
    async fn frame_owner_path(&self, frame: &AttachedFrame) -> Result<Vec<u32>>;

    /// Runs the versioned collector in one frame.
    async fn collect_frame_observation(
        &self,
        session_id: &str,
        frame_id: FrameId,
        capture_id: &CaptureId,
        limits: CollectorLimits,
        capture_policy: &CapturePolicy,
    ) -> Result<CollectedPageObservation>;

    /// Captures a clipped PNG for browser state that cannot be serialized.
    async fn capture_visual_fallback(
        &self,
        session_id: &str,
        fallback: &VisualFallback,
        maximum_bytes: u64,
    ) -> Result<Vec<u8>>;

    /// Loads a resource in the owning frame session with a hard byte limit.
    async fn load_resource_in_session(
        &self,
        session_id: &str,
        cdp_frame_id: &str,
        url: &Url,
        maximum_bytes: u64,
    ) -> Result<LoadedResource>;

    /// Opens an artifact with external networking denied.
    async fn verify_offline_url(
        &self,
        url: &Url,
        deadline: std::time::Duration,
    ) -> Result<OfflineBrowserObservation>;

    /// Resolves printable links against `source_url` and renders a bounded PDF.
    async fn print_to_pdf(
        &self,
        _source_url: &Url,
        _landscape: bool,
        _prefer_css_page_size: bool,
        _maximum_bytes: u64,
    ) -> Result<Vec<u8>> {
        Err(PageKnotError::new(
            "pageknot.browser.pdf_unavailable",
            ErrorStage::Encoding,
            "the selected browser backend cannot render PDF output",
        ))
    }

    /// Releases the page target and attached sessions.
    async fn close(self: Box<Self>) -> Result<()>;
}
