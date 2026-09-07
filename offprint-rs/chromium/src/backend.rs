use std::time::Duration;

use async_trait::async_trait;
use camino::Utf8PathBuf;
use offprint_browser::{
    BrowserAcquireRequest, BrowserBackend, BrowserContext, BrowserContextRequest, BrowserLease,
    PageSession,
};
use offprint_model::{
    BrowserCandidate, BrowserCandidateState, BrowserDoctorReport, BrowserEnvironment, BrowserInfo,
    BrowserInstallationPolicy, BrowserSource, BrowserSourcePolicy, BrowserSpec, CapabilityCheck,
    CaptureId, ErrorStage, ManagedBrowserState, NetworkPolicy, NetworkPolicySummary, OffprintError,
    OutputCapability, RecoveryAction, Result,
};
use offprint_protocol::{
    COLLECTOR_PROTOCOL_VERSION, COLLECTOR_PROTOCOL_VERSION_STRING, CollectorHandshake,
    negotiate_protocol,
};
use serde_json::json;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::remote::connect_remote_browser;
use crate::{
    CdpClient, ChromiumDiscovery, ChromiumLaunchOptions, ChromiumPage, ChromiumProcess,
    DiscoveryResult, MANAGED_BROWSER_CATALOG_VERSION, ManagedBrowserLease, ManagedBrowserManager,
    probe_collector_handshake,
};

const COLLECTOR_PROBE_CHUNK_BYTES: u64 = 64 * 1024;
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// Configuration for a shared Chromium browser backend.
#[derive(Clone, Debug)]
pub struct ChromiumBackendOptions {
    discovery: ChromiumDiscovery,
    cache_dir: Utf8PathBuf,
    browser_source: BrowserSourcePolicy,
    browser_installation: BrowserInstallationPolicy,
}

impl ChromiumBackendOptions {
    /// Creates backend options from the browser discovery policy and managed
    /// browser cache.
    #[must_use]
    pub const fn new(discovery: ChromiumDiscovery, cache_dir: Utf8PathBuf) -> Self {
        Self {
            discovery,
            cache_dir,
            browser_source: BrowserSourcePolicy::Auto,
            browser_installation: BrowserInstallationPolicy::InstallManaged,
        }
    }

    /// Selects automatic, managed, or system browser discovery.
    #[must_use]
    pub const fn with_browser_source(mut self, source: BrowserSourcePolicy) -> Self {
        self.browser_source = source;
        self
    }

    /// Controls managed-browser installation during automatic acquisition.
    #[must_use]
    pub const fn with_browser_installation(
        mut self,
        installation: BrowserInstallationPolicy,
    ) -> Self {
        self.browser_installation = installation;
        self
    }
}

/// Shares one local or remote Chromium instance across browser leases.
#[derive(Debug)]
pub struct ChromiumBackend {
    options: ChromiumBackendOptions,
    owner: Mutex<Option<BrowserOwner>>,
}

impl ChromiumBackend {
    /// Creates a Chromium backend with an initially empty browser pool.
    #[must_use]
    pub const fn new(options: ChromiumBackendOptions) -> Self {
        Self {
            options,
            owner: Mutex::const_new(None),
        }
    }

    async fn acquire_lease(
        &self,
        request: BrowserAcquireRequest,
        cancellation: &CancellationToken,
    ) -> Result<ChromiumBrowserLease> {
        let mut owner = tokio::select! {
            () = cancellation.cancelled() => return Err(acquisition_cancelled()),
            owner = self.owner.lock() => owner,
        };
        if cancellation.is_cancelled() {
            return Err(acquisition_cancelled());
        }
        if owner
            .as_ref()
            .is_some_and(|owner| !owner.matches(&request.browser, request.headed))
        {
            return Err(OffprintError::new(
                "offprint.browser.selection_conflict",
                ErrorStage::Browser,
                "the running browser does not match the requested browser selection",
            ));
        }
        if let Some(active) = owner.as_ref()
            && !active.is_healthy().await
            && let Some(crashed) = owner.take()
        {
            let _ignored = crashed.close().await;
        }
        if owner.is_none() {
            *owner = Some(self.start_browser(&request.browser, request.headed).await?);
        }
        if cancellation.is_cancelled() {
            return Err(acquisition_cancelled());
        }
        let owner = owner.as_ref().ok_or_else(browser_unavailable)?;
        Ok(ChromiumBrowserLease {
            client: owner.client(),
            info: owner.info().clone(),
        })
    }

    async fn start_browser(&self, selection: &BrowserSpec, headed: bool) -> Result<BrowserOwner> {
        match selection {
            BrowserSpec::Auto => {
                let info = self.resolve_auto_browser().await?;
                let managed_lease = self.managed_browser_lease(&info).await?;
                start_local(info, headed, managed_lease).await
            }
            BrowserSpec::Executable(path) => {
                let discovery = ChromiumDiscovery::new()
                    .with_explicit_path(path.as_utf8_path().to_owned())
                    .discover()
                    .await;
                let info = discovery
                    .candidates
                    .into_iter()
                    .find(|candidate| candidate.browser.source == BrowserSource::Explicit)
                    .map(|candidate| candidate.browser)
                    .ok_or_else(|| {
                        discovery
                            .failures
                            .into_iter()
                            .next()
                            .unwrap_or_else(browser_unavailable)
                    })?;
                start_local(info, headed, None).await
            }
            BrowserSpec::Remote(endpoint) => start_remote(endpoint.clone()).await,
        }
    }

    async fn resolve_auto_browser(&self) -> Result<BrowserInfo> {
        let discovery = self.options.discovery.discover().await;
        if let Some(selected) = discovery.selected {
            return Ok(selected);
        }
        if self.options.browser_installation == BrowserInstallationPolicy::InstallManaged
            && self.options.browser_source != BrowserSourcePolicy::System
        {
            ManagedBrowserManager::new(self.options.cache_dir.clone())
                .install(None)
                .await?;
            let discovery = self.options.discovery.discover().await;
            return discovery.selected.ok_or_else(browser_unavailable);
        }
        Err(discovery
            .failures
            .into_iter()
            .next()
            .unwrap_or_else(browser_unavailable))
    }

    async fn managed_browser_lease(
        &self,
        browser: &BrowserInfo,
    ) -> Result<Option<ManagedBrowserLease>> {
        if browser.source != BrowserSource::Managed {
            return Ok(None);
        }
        let revision = browser.revision.as_deref().ok_or_else(|| {
            OffprintError::new(
                "offprint.browser.install",
                ErrorStage::Browser,
                "managed browser record omitted its revision",
            )
        })?;
        ManagedBrowserManager::new(self.options.cache_dir.clone())
            .lease(revision)
            .await
            .map(Some)
    }

    async fn discover(&self, selection: &BrowserSpec) -> DiscoveryResult {
        match selection {
            BrowserSpec::Auto => self.options.discovery.discover().await,
            BrowserSpec::Executable(path) => {
                ChromiumDiscovery::new()
                    .with_explicit_path(path.as_utf8_path().to_owned())
                    .discover()
                    .await
            }
            BrowserSpec::Remote(endpoint) => {
                match crate::remote::probe_remote_browser(endpoint).await {
                    Ok(browser) => DiscoveryResult {
                        selected: Some(browser.clone()),
                        candidates: vec![BrowserCandidate {
                            browser,
                            state: BrowserCandidateState::Selected,
                            reason_code: "offprint.browser.remote".to_owned(),
                            priority: 0,
                            active_leases: 0,
                        }],
                        failures: Vec::new(),
                    },
                    Err(error) => DiscoveryResult {
                        selected: None,
                        candidates: Vec::new(),
                        failures: vec![error],
                    },
                }
            }
        }
    }

    async fn probe_collector(&self, browser: &BrowserSpec) -> Result<CollectorHandshake> {
        let lease = self
            .acquire_lease(
                BrowserAcquireRequest {
                    capture_id: CaptureId::new(),
                    browser: browser.clone(),
                    headed: false,
                },
                &CancellationToken::new(),
            )
            .await?;
        let page = lease
            .create_page(BrowserContextRequest {
                environment: BrowserEnvironment::default(),
                network: NetworkPolicy::Standard,
                maximum_frames: 1,
                resource_observation: offprint_browser::ResourceObservationLimits::default(),
                deny_network: false,
            })
            .await?;
        let capture_id = CaptureId::new();
        let probe = probe_collector_handshake(
            &page,
            page.session_id(),
            &capture_id,
            COLLECTOR_PROBE_CHUNK_BYTES,
        )
        .await;
        operation_with_cleanup(probe, page.close().await)
    }
}

#[async_trait]
impl BrowserBackend for ChromiumBackend {
    async fn acquire(
        &self,
        request: BrowserAcquireRequest,
        cancellation: CancellationToken,
    ) -> Result<Box<dyn BrowserLease>> {
        if cancellation.is_cancelled() {
            return Err(acquisition_cancelled());
        }
        self.acquire_lease(request, &cancellation)
            .await
            .map(|lease| Box::new(lease) as Box<dyn BrowserLease>)
    }

    async fn doctor(&self, browser: &BrowserSpec) -> BrowserDoctorReport {
        let discovery = self.discover(browser).await;
        let selected = discovery.selected.clone();
        let mut recovery = discovery
            .failures
            .iter()
            .map(recovery_action)
            .collect::<Vec<_>>();
        let collector = match selected.as_ref() {
            Some(_) => match self.probe_collector(browser).await {
                Ok(handshake) => collector_capability(&handshake),
                Err(error) => {
                    recovery.push(recovery_action(&error));
                    unavailable_collector()
                }
            },
            None => unavailable_collector(),
        };
        let mut managed_cache = match ManagedBrowserManager::new(self.options.cache_dir.clone())
            .state()
            .await
        {
            Ok(state) => state,
            Err(error) => {
                recovery.push(recovery_action(&error));
                ManagedBrowserState {
                    cache_dir: self.options.cache_dir.clone().into(),
                    installed_revisions: Vec::new(),
                    selected_revision: None,
                    catalog_version: MANAGED_BROWSER_CATALOG_VERSION.to_owned(),
                }
            }
        };
        managed_cache.selected_revision = selected
            .as_ref()
            .filter(|browser| browser.source == BrowserSource::Managed)
            .and_then(|browser| browser.revision.clone());
        let output = output_capability();
        BrowserDoctorReport {
            schema_version: offprint_model::PUBLIC_SCHEMA_VERSION,
            ready: selected.is_some() && collector.compatible && output.writable,
            selected,
            candidates: discovery.candidates,
            managed_cache,
            collector,
            output,
            configuration: Vec::new(),
            network: NetworkPolicySummary {
                policy: "standard".to_owned(),
                permits_loopback_initial_origin: true,
                permits_private_addresses: false,
                revalidates_redirects: true,
            },
            recovery,
        }
    }

    async fn active_browser(&self) -> Option<BrowserInfo> {
        self.owner
            .lock()
            .await
            .as_ref()
            .map(|owner| owner.info().clone())
    }

    async fn close(&self) -> Result<()> {
        let mut owner = self.owner.lock().await;
        match owner.take() {
            Some(owner) => owner.close().await,
            None => Ok(()),
        }
    }
}

#[derive(Debug)]
struct ChromiumBrowserLease {
    client: CdpClient,
    info: BrowserInfo,
}

impl ChromiumBrowserLease {
    async fn create_page(&self, request: BrowserContextRequest) -> Result<ChromiumPage> {
        let BrowserContextRequest {
            environment,
            network: _,
            maximum_frames,
            resource_observation,
            deny_network,
        } = request;
        if deny_network {
            ChromiumPage::create_verifier_with_resource_limits(
                self.client.clone(),
                &environment,
                maximum_frames,
                resource_observation,
            )
            .await
        } else {
            ChromiumPage::create_with_resource_limits(
                self.client.clone(),
                &environment,
                maximum_frames,
                resource_observation,
            )
            .await
        }
    }
}

#[async_trait]
impl BrowserLease for ChromiumBrowserLease {
    fn info(&self) -> &BrowserInfo {
        &self.info
    }

    async fn create_context(
        &self,
        request: BrowserContextRequest,
        cancellation: CancellationToken,
    ) -> Result<Box<dyn BrowserContext>> {
        if cancellation.is_cancelled() {
            return Err(acquisition_cancelled());
        }
        let page = self.create_page(request).await?;
        if cancellation.is_cancelled() {
            let _ignored = page.close().await;
            return Err(acquisition_cancelled());
        }
        Ok(Box::new(ChromiumBrowserContext {
            page: Mutex::new(Some(page)),
        }))
    }

    async fn close(self: Box<Self>) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct ChromiumBrowserContext {
    page: Mutex<Option<ChromiumPage>>,
}

#[async_trait]
impl BrowserContext for ChromiumBrowserContext {
    async fn open_page(&self, cancellation: CancellationToken) -> Result<Box<dyn PageSession>> {
        if cancellation.is_cancelled() {
            return Err(acquisition_cancelled());
        }
        self.page
            .lock()
            .await
            .take()
            .map(|page| Box::new(page) as Box<dyn PageSession>)
            .ok_or_else(|| {
                OffprintError::new(
                    "offprint.browser.context",
                    ErrorStage::Browser,
                    "browser context already opened its page",
                )
            })
    }

    async fn close(mut self: Box<Self>) -> Result<()> {
        match self.page.get_mut().take() {
            Some(page) => page.close().await,
            None => Ok(()),
        }
    }
}

#[derive(Debug)]
enum BrowserOwner {
    Local {
        process: Box<ChromiumProcess>,
        info: BrowserInfo,
        headed: bool,
        managed_lease: Option<ManagedBrowserLease>,
    },
    Remote {
        client: CdpClient,
        info: BrowserInfo,
        endpoint: Url,
    },
}

impl BrowserOwner {
    fn client(&self) -> CdpClient {
        match self {
            Self::Local { process, .. } => process.client().clone(),
            Self::Remote { client, .. } => client.clone(),
        }
    }

    const fn info(&self) -> &BrowserInfo {
        match self {
            Self::Local { info, .. } | Self::Remote { info, .. } => info,
        }
    }

    fn matches(&self, selection: &BrowserSpec, headed: bool) -> bool {
        match (self, selection) {
            (Self::Local { headed: active, .. }, BrowserSpec::Auto) => *active == headed,
            (
                Self::Local {
                    info,
                    headed: active,
                    ..
                },
                BrowserSpec::Executable(path),
            ) => {
                *active == headed
                    && info
                        .executable_path
                        .as_ref()
                        .is_some_and(|selected| selected == path)
            }
            (Self::Remote { endpoint, .. }, BrowserSpec::Remote(requested)) => {
                endpoint == requested
            }
            _ => false,
        }
    }

    async fn is_healthy(&self) -> bool {
        match self {
            Self::Local { process, .. } => process.is_healthy().await,
            Self::Remote { client, .. } => client
                .command_with_timeout("Browser.getVersion", json!({}), None, HEALTH_CHECK_TIMEOUT)
                .await
                .is_ok(),
        }
    }

    async fn close(self) -> Result<()> {
        match self {
            Self::Local {
                process,
                managed_lease,
                ..
            } => {
                let result = process.close().await;
                drop(managed_lease);
                result
            }
            Self::Remote { client, .. } => client.close().await,
        }
    }
}

async fn start_local(
    info: BrowserInfo,
    headed: bool,
    managed_lease: Option<ManagedBrowserLease>,
) -> Result<BrowserOwner> {
    let executable = info.executable_path.clone().ok_or_else(|| {
        OffprintError::new(
            "offprint.browser.executable",
            ErrorStage::Browser,
            "selected local browser has no executable path",
        )
    })?;
    let mut options = ChromiumLaunchOptions::new(executable);
    options.headless = !headed;
    let process = ChromiumProcess::launch(options).await?;
    Ok(BrowserOwner::Local {
        process: Box::new(process),
        info,
        headed,
        managed_lease,
    })
}

async fn start_remote(endpoint: Url) -> Result<BrowserOwner> {
    let (client, info) = connect_remote_browser(&endpoint).await?;
    Ok(BrowserOwner::Remote {
        client,
        info,
        endpoint,
    })
}

fn collector_capability(handshake: &CollectorHandshake) -> CapabilityCheck {
    let capabilities = handshake
        .available_capabilities
        .iter()
        .map(|capability| capability.as_str().to_owned())
        .collect();
    let missing_capabilities = handshake
        .requested_capabilities
        .difference(&handshake.available_capabilities)
        .map(|capability| capability.as_str().to_owned())
        .collect();
    CapabilityCheck {
        compatible: negotiate_protocol(
            handshake,
            COLLECTOR_PROTOCOL_VERSION,
            COLLECTOR_PROBE_CHUNK_BYTES,
        )
        .is_ok(),
        host_version: COLLECTOR_PROTOCOL_VERSION_STRING.to_owned(),
        peer_version: Some(format!(
            "{}.{}",
            handshake.protocol.major, handshake.protocol.minor
        )),
        capabilities,
        missing_capabilities,
    }
}

fn unavailable_collector() -> CapabilityCheck {
    CapabilityCheck {
        compatible: false,
        host_version: COLLECTOR_PROTOCOL_VERSION_STRING.to_owned(),
        peer_version: None,
        capabilities: Vec::new(),
        missing_capabilities: Vec::new(),
    }
}

fn output_capability() -> OutputCapability {
    let directory = std::env::current_dir()
        .ok()
        .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
        .unwrap_or_else(|| Utf8PathBuf::from("."));
    match tempfile::Builder::new()
        .prefix(".offprint-doctor-")
        .tempfile_in(&directory)
    {
        Ok(file) => {
            drop(file);
            OutputCapability {
                directory: directory.into(),
                writable: true,
                atomic_create: true,
                atomic_replace: true,
                reason_code: None,
            }
        }
        Err(error) => OutputCapability {
            directory: directory.into(),
            writable: false,
            atomic_create: false,
            atomic_replace: false,
            reason_code: Some(format!("offprint.output.{}", error.kind() as u8)),
        },
    }
}

fn recovery_action(error: &OffprintError) -> RecoveryAction {
    RecoveryAction {
        code: error.code.to_string(),
        description: error.message.clone(),
        command: String::new(),
        arguments: Vec::new(),
    }
}

fn operation_with_cleanup<T>(operation: Result<T>, cleanup: Result<()>) -> Result<T> {
    match (operation, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
    }
}

fn acquisition_cancelled() -> OffprintError {
    OffprintError::new(
        "offprint.browser.acquisition_cancelled",
        ErrorStage::Shutdown,
        "browser acquisition was cancelled",
    )
}

fn browser_unavailable() -> OffprintError {
    OffprintError::new(
        "offprint.browser.unavailable",
        ErrorStage::Browser,
        "no compatible Chromium-based executable was found",
    )
    .with_detail("recoveryCommand", "offprint browser install")
}

#[cfg(test)]
#[path = "backend/tests.rs"]
mod tests;
