//! Capture rendered web pages as verified self-contained HTML artifacts.
//!
//! [`PageKnot`] owns browser discovery, browser processes, capture jobs, and
//! artifact verification. Clone the handle when several tasks share one
//! service, then call [`PageKnot::close`] after the final job.
//!
//! ```no_run
//! use pageknot::PageKnot;
//!
//! # async fn capture() -> pageknot::Result<()> {
//! let pageknot = PageKnot::builder().build()?;
//! let result = pageknot
//!     .capture("https://example.com")?
//!     .save("example.html")
//!     .await?;
//! assert!(result.verification.passed);
//! pageknot.close().await?;
//! # Ok(())
//! # }
//! ```
//!
//! Use [`CaptureService`] and [`CaptureJob`] when a caller needs progress
//! events or cancellation. [`ArtifactService`] inspects and verifies existing
//! artifacts. [`BrowserService`] manages local and managed Chromium builds.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![warn(missing_docs)]

mod artifact_service;
mod browser_service;
mod capture_builder;
mod capture_service;
mod dependencies;
mod diagnostics;
mod export_transaction;
mod pipeline;
mod runtime;
mod scheduler;

use std::collections::BTreeMap;
use std::sync::Arc;

pub use artifact_service::ArtifactService;
pub use browser_service::BrowserService;
pub use capture_builder::CaptureBuilder;
pub use capture_service::{CaptureEvents, CaptureJob, CaptureService};
pub use dependencies::{CaptureIdGenerator, Clock, SystemClock, UlidCaptureIdGenerator};
pub use pageknot_browser::{
    BrowserAcquireRequest, BrowserBackend, BrowserContext, BrowserContextRequest, BrowserLease,
    CollectedPageObservation, CollectorLimits, PageSession,
};
pub use pageknot_model::*;
use runtime::{RuntimeOptions, RuntimeState};

#[derive(Clone, Debug)]
/// A shared PageKnot service handle.
pub struct PageKnot {
    state: Arc<RuntimeState>,
}

impl PageKnot {
    /// Starts configuring a PageKnot service.
    #[must_use]
    pub fn builder() -> PageKnotBuilder {
        PageKnotBuilder::new()
    }

    /// Returns the artifact inspection and verification service.
    #[must_use]
    pub fn artifacts(&self) -> ArtifactService {
        ArtifactService::new(Arc::clone(&self.state))
    }

    /// Returns the browser discovery and management service.
    #[must_use]
    pub fn browsers(&self) -> BrowserService {
        BrowserService::new(Arc::clone(&self.state))
    }

    /// Returns the typed capture job service.
    #[must_use]
    pub fn captures(&self) -> CaptureService {
        CaptureService::new(Arc::clone(&self.state))
    }

    /// Starts a one-shot capture builder for an HTTP or HTTPS URL.
    ///
    /// The builder inherits the selected default profile. URL and service-state
    /// failures are returned before a browser is acquired.
    pub fn capture(&self, url: impl AsRef<str>) -> Result<CaptureBuilder> {
        self.state.ensure_open()?;
        let mut request = CaptureRequest::builder(url)?.build()?;
        self.state.default_capture_profile().apply_to(&mut request);
        Ok(CaptureBuilder::new(self.captures(), request))
    }

    /// Cancels active work and releases every browser owned by this service.
    ///
    /// Every close waiter receives the same terminal shutdown result. Repeating
    /// `close` after a successful shutdown succeeds. New operations fail after
    /// the first close begins.
    pub async fn close(&self) -> Result<()> {
        self.state.close().await
    }
}

#[derive(Clone, Debug)]
/// Configuration for a [`PageKnot`] service.
pub struct PageKnotBuilder {
    browser_path: Option<camino::Utf8PathBuf>,
    cdp_url: Option<url::Url>,
    browser_backend: Option<Arc<dyn BrowserBackend>>,
    cache_dir: Option<camino::Utf8PathBuf>,
    browser_channel: BrowserChannel,
    browser_installation: BrowserInstallationPolicy,
    maximum_contexts: u16,
    browser_recycle_after_jobs: u32,
    headed: bool,
    clock: Arc<dyn Clock>,
    capture_id_generator: Arc<dyn CaptureIdGenerator>,
    effective_configuration: Vec<EffectiveConfigValue>,
    default_network_policy: NetworkPolicy,
    selected_profile: String,
    profiles: BTreeMap<String, CaptureProfile>,
}

impl Default for PageKnotBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PageKnotBuilder {
    /// Creates a builder with the default profile, headless browser discovery,
    /// and managed-browser provisioning when no compatible browser is present.
    #[must_use]
    pub fn new() -> Self {
        Self {
            browser_path: None,
            cdp_url: None,
            browser_backend: None,
            cache_dir: None,
            browser_channel: BrowserChannel::Auto,
            browser_installation: BrowserInstallationPolicy::InstallManaged,
            maximum_contexts: 4,
            browser_recycle_after_jobs: 100,
            headed: false,
            clock: Arc::new(SystemClock),
            capture_id_generator: Arc::new(UlidCaptureIdGenerator),
            effective_configuration: Vec::new(),
            default_network_policy: NetworkPolicy::Standard,
            selected_profile: "default".to_owned(),
            profiles: BTreeMap::new(),
        }
    }

    /// Selects a local Chrome or Chromium executable.
    #[must_use]
    pub fn browser_path(mut self, path: impl Into<camino::Utf8PathBuf>) -> Self {
        self.browser_path = Some(path.into());
        self.cdp_url = None;
        self.browser_backend = None;
        self
    }

    /// Attaches to a remote browser through its HTTP or WebSocket CDP URL.
    #[must_use]
    pub fn cdp_url(mut self, endpoint: url::Url) -> Self {
        self.browser_path = None;
        self.cdp_url = Some(endpoint);
        self.browser_backend = None;
        self
    }

    /// Installs an advanced browser backend implementation.
    #[must_use]
    pub fn browser_backend(mut self, backend: Arc<dyn BrowserBackend>) -> Self {
        self.browser_path = None;
        self.cdp_url = None;
        self.browser_backend = Some(backend);
        self
    }

    /// Sets the managed-browser and service cache directory.
    #[must_use]
    pub fn cache_dir(mut self, path: impl Into<camino::Utf8PathBuf>) -> Self {
        self.cache_dir = Some(path.into());
        self
    }

    /// Selects automatic, managed, or system browser discovery.
    #[must_use]
    pub const fn browser_channel(mut self, channel: BrowserChannel) -> Self {
        self.browser_channel = channel;
        self
    }

    /// Controls whether first browser acquisition may install the managed
    /// browser.
    #[must_use]
    pub const fn browser_installation(mut self, policy: BrowserInstallationPolicy) -> Self {
        self.browser_installation = policy;
        self
    }

    /// Sets the maximum number of live browser contexts.
    #[must_use]
    pub const fn maximum_contexts(mut self, maximum_contexts: u16) -> Self {
        self.maximum_contexts = maximum_contexts;
        self
    }

    /// Recycles the browser after this many completed jobs.
    ///
    /// [`PageKnotBuilder::build`] rejects zero.
    #[must_use]
    pub const fn browser_recycle_after_jobs(mut self, jobs: u32) -> Self {
        self.browser_recycle_after_jobs = jobs;
        self
    }

    /// Controls whether locally launched browser windows are visible.
    #[must_use]
    pub const fn headed(mut self, headed: bool) -> Self {
        self.headed = headed;
        self
    }

    /// Replaces the clock used in timestamps and deterministic tests.
    #[must_use]
    pub fn clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Replaces capture identifier generation.
    #[must_use]
    pub fn capture_id_generator(
        mut self,
        capture_id_generator: Arc<dyn CaptureIdGenerator>,
    ) -> Self {
        self.capture_id_generator = capture_id_generator;
        self
    }

    /// Records resolved configuration values for doctor output.
    #[must_use]
    pub fn effective_configuration(mut self, configuration: Vec<EffectiveConfigValue>) -> Self {
        self.effective_configuration = configuration;
        self
    }

    /// Sets the network policy used when verifying existing artifacts.
    #[must_use]
    pub fn default_network_policy(mut self, policy: NetworkPolicy) -> Self {
        self.default_network_policy = policy;
        self
    }

    /// Selects the profile applied by [`PageKnot::capture`].
    #[must_use]
    pub fn profile(mut self, name: impl Into<String>) -> Self {
        self.selected_profile = name.into();
        self
    }

    /// Registers a named profile for one-shot and service captures.
    #[must_use]
    pub fn register_profile(mut self, name: impl Into<String>, profile: CaptureProfile) -> Self {
        self.profiles.insert(name.into(), profile);
        self
    }

    /// Builds the service and validates browser-pool and profile settings.
    pub fn build(self) -> Result<PageKnot> {
        let selected_profile = self
            .profiles
            .get(&self.selected_profile)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| CaptureProfile::named(&self.selected_profile))?;
        let state = RuntimeState::new(RuntimeOptions {
            browser_path: self.browser_path,
            cdp_url: self.cdp_url,
            browser_backend: self.browser_backend,
            cache_dir: self.cache_dir,
            browser_channel: self.browser_channel,
            browser_installation: self.browser_installation,
            maximum_contexts: self.maximum_contexts,
            browser_recycle_after_jobs: self.browser_recycle_after_jobs,
            headed: self.headed,
            clock: self.clock,
            capture_id_generator: self.capture_id_generator,
            effective_configuration: self.effective_configuration,
            default_network_policy: self.default_network_policy,
            default_capture_profile: selected_profile,
            profiles: self.profiles,
        })?;
        Ok(PageKnot {
            state: Arc::new(state),
        })
    }
}
