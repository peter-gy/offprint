use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use camino::Utf8PathBuf;
use directories::ProjectDirs;
use futures_util::FutureExt as _;
use pageknot_browser::{BrowserAcquireRequest, BrowserBackend, ResourceObservationLimits};
use pageknot_chromium::{
    CdpClient, ChromiumDiscovery, ChromiumLaunchOptions, ChromiumPage, ChromiumProcess,
    ManagedBrowserLease, ManagedBrowserManager, resolve_remote_endpoint,
};
use pageknot_model::{
    BrowserChannel, BrowserEnvironment, BrowserInfo, BrowserInstallationPolicy, BrowserProduct,
    BrowserSource, BrowserSpec, CaptureId, CaptureProfile, EffectiveConfigValue, ErrorStage,
    NetworkPolicy, PageKnotError, RedactedUrl, RedactionPolicy, Result,
};
use serde_json::json;
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::capture_service::JobControl;
use crate::{CaptureIdGenerator, Clock};

mod context;
mod lifecycle;

use context::{ContextPermit, PendingRuntimePage, RuntimePage};
use lifecycle::{RuntimeLifecycle, closed_error};

const CONTEXT_CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug)]
pub(crate) struct RuntimeState {
    lifecycle: RuntimeLifecycle,
    pub(crate) discovery: ChromiumDiscovery,
    pub(crate) cache_dir: Utf8PathBuf,
    default_browser: BrowserSpec,
    custom_backend: Option<Arc<dyn BrowserBackend>>,
    browser: tokio::sync::Mutex<Option<BrowserOwner>>,
    pub(crate) browser_channel: BrowserChannel,
    pub(crate) browser_installation: BrowserInstallationPolicy,
    context_slots: Arc<Semaphore>,
    contexts_changed: Notify,
    maximum_contexts: usize,
    completed_jobs: AtomicU32,
    browser_recycle_after_jobs: NonZeroU32,
    headed: bool,
    clock: Arc<dyn Clock>,
    capture_id_generator: Arc<dyn CaptureIdGenerator>,
    pub(crate) effective_configuration: Vec<EffectiveConfigValue>,
    pub(crate) default_network_policy: NetworkPolicy,
    default_capture_profile: CaptureProfile,
    profiles: BTreeMap<String, CaptureProfile>,
    jobs: Mutex<BTreeMap<CaptureId, ActiveJob>>,
    jobs_changed: Notify,
}

#[derive(Debug)]
pub(crate) struct RuntimeOptions {
    pub(crate) browser_path: Option<Utf8PathBuf>,
    pub(crate) cdp_url: Option<Url>,
    pub(crate) browser_backend: Option<Arc<dyn BrowserBackend>>,
    pub(crate) cache_dir: Option<Utf8PathBuf>,
    pub(crate) browser_channel: BrowserChannel,
    pub(crate) browser_installation: BrowserInstallationPolicy,
    pub(crate) maximum_contexts: u16,
    pub(crate) browser_recycle_after_jobs: u32,
    pub(crate) headed: bool,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) capture_id_generator: Arc<dyn CaptureIdGenerator>,
    pub(crate) effective_configuration: Vec<EffectiveConfigValue>,
    pub(crate) default_network_policy: NetworkPolicy,
    pub(crate) default_capture_profile: CaptureProfile,
    pub(crate) profiles: BTreeMap<String, CaptureProfile>,
}

impl RuntimeState {
    pub(crate) fn new(options: RuntimeOptions) -> Result<Self> {
        let RuntimeOptions {
            browser_path,
            cdp_url,
            browser_backend,
            cache_dir,
            browser_channel,
            browser_installation,
            maximum_contexts,
            browser_recycle_after_jobs,
            headed,
            clock,
            capture_id_generator,
            effective_configuration,
            default_network_policy,
            default_capture_profile,
            profiles,
        } = options;
        if maximum_contexts == 0 {
            return Err(PageKnotError::new(
                "pageknot.input.concurrency",
                ErrorStage::Validation,
                "browser context concurrency must be greater than zero",
            ));
        }
        let browser_recycle_after_jobs =
            NonZeroU32::new(browser_recycle_after_jobs).ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.input.browser_recycle",
                    ErrorStage::Validation,
                    "browser recycling threshold must be greater than zero",
                )
            })?;
        let default_browser = match (browser_path, cdp_url) {
            (Some(path), None) => BrowserSpec::Executable(path.into()),
            (None, Some(endpoint)) => BrowserSpec::Remote(endpoint),
            (None, None) => BrowserSpec::Auto,
            (Some(_), Some(_)) => {
                return Err(PageKnotError::new(
                    "pageknot.input.browser_selection",
                    ErrorStage::Validation,
                    "browser path and remote browser endpoint are mutually exclusive",
                ));
            }
        };
        let cache_dir = match cache_dir {
            Some(path) => path,
            None => default_cache_dir()?,
        };
        let discovery = match &default_browser {
            BrowserSpec::Executable(path) => ChromiumDiscovery::new()
                .with_explicit_path(path.as_utf8_path().to_owned())
                .with_managed_cache(cache_dir.clone())
                .with_channel(browser_channel),
            BrowserSpec::Auto | BrowserSpec::Remote(_) => ChromiumDiscovery::new()
                .with_managed_cache(cache_dir.clone())
                .with_channel(browser_channel),
        };
        let maximum_contexts = usize::from(maximum_contexts);
        Ok(Self {
            lifecycle: RuntimeLifecycle::new(),
            discovery,
            cache_dir,
            default_browser,
            custom_backend: browser_backend,
            browser: tokio::sync::Mutex::new(None),
            browser_channel,
            browser_installation,
            context_slots: Arc::new(Semaphore::new(maximum_contexts)),
            contexts_changed: Notify::new(),
            maximum_contexts,
            completed_jobs: AtomicU32::new(0),
            browser_recycle_after_jobs,
            headed,
            clock,
            capture_id_generator,
            effective_configuration,
            default_network_policy,
            default_capture_profile,
            profiles,
            jobs: Mutex::new(BTreeMap::new()),
            jobs_changed: Notify::new(),
        })
    }

    pub(crate) fn ensure_open(&self) -> Result<()> {
        self.lifecycle.ensure_open()
    }

    pub(crate) fn now(&self) -> chrono::DateTime<chrono::Utc> {
        self.clock.now()
    }

    pub(crate) fn next_capture_id(&self) -> CaptureId {
        self.capture_id_generator.next_capture_id()
    }

    pub(crate) fn register_job(
        &self,
        capture_id: CaptureId,
        control: Arc<JobControl>,
    ) -> Result<()> {
        let mut jobs = self.jobs.lock().map_err(|_| {
            PageKnotError::new(
                "pageknot.runtime.lock",
                ErrorStage::Internal,
                "capture registry lock is unavailable",
            )
        })?;
        self.lifecycle.ensure_open()?;
        jobs.insert(
            capture_id,
            ActiveJob {
                control,
                task: None,
            },
        );
        self.jobs_changed.notify_waiters();
        Ok(())
    }

    pub(crate) fn set_job_task(
        &self,
        capture_id: &CaptureId,
        task: tokio::task::JoinHandle<()>,
    ) -> Result<()> {
        let mut jobs = self.jobs.lock().map_err(|_| {
            PageKnotError::new(
                "pageknot.runtime.lock",
                ErrorStage::Internal,
                "capture registry lock is unavailable",
            )
        })?;
        let job = jobs.get_mut(capture_id).ok_or_else(|| {
            PageKnotError::new(
                "pageknot.runtime.job",
                ErrorStage::Internal,
                "capture job disappeared before its task was registered",
            )
        })?;
        job.task = Some(task);
        Ok(())
    }

    pub(crate) fn unregister_job(self: &Arc<Self>, capture_id: &CaptureId) {
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.remove(capture_id);
        }
        let _previous =
            self.completed_jobs
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |completed| {
                    Some(completed.saturating_add(1))
                });
        self.jobs_changed.notify_waiters();
        let runtime = Arc::clone(self);
        tokio::spawn(async move {
            let _ignored = runtime.recycle_idle_browser_if_due().await;
        });
    }

    pub(crate) async fn open_page(
        self: &Arc<Self>,
        request: RuntimePageRequest,
    ) -> Result<RuntimePage> {
        self.ensure_open()?;
        let permit = Arc::clone(&self.context_slots)
            .acquire_owned()
            .await
            .map_err(|_| closed_error())?;
        let permit = ContextPermit::new(permit, self);
        self.ensure_open()?;
        let runtime = Arc::clone(self);
        tokio::spawn(async move { runtime.acquire_page(request, permit).await })
            .await
            .map_err(|error| {
                PageKnotError::new(
                    "pageknot.runtime.acquisition",
                    ErrorStage::Internal,
                    format!("browser acquisition task failed: {error}"),
                )
            })?
    }

    async fn acquire_page(
        self: &Arc<Self>,
        request: RuntimePageRequest,
        permit: ContextPermit,
    ) -> Result<RuntimePage> {
        let mut pending = PendingRuntimePage::new(self, permit);
        if let Some(backend) = &self.custom_backend {
            return pending.acquire_custom(backend, request, self.headed).await;
        }
        let selection = self.effective_selection(&request.browser);
        let headed = request.headed.unwrap_or(self.headed);
        let mut owner = self.browser.lock().await;
        let browser_is_idle =
            self.context_slots.available_permits() == self.maximum_contexts.saturating_sub(1);
        if owner
            .as_ref()
            .is_some_and(|owner| !owner.matches(selection, headed))
        {
            return Err(PageKnotError::new(
                "pageknot.browser.selection_conflict",
                ErrorStage::Browser,
                "the running browser does not match the requested browser selection",
            ));
        }
        if browser_is_idle
            && owner.as_ref().is_some_and(|owner| {
                owner.should_recycle(
                    self.completed_jobs.load(Ordering::Acquire),
                    self.browser_recycle_after_jobs,
                )
            })
            && let Some(stale) = owner.take()
        {
            stale.close().await?;
            self.completed_jobs.store(0, Ordering::Release);
        }
        if browser_is_idle
            && let Some(active) = owner.as_ref()
            && !active.is_healthy().await
            && let Some(crashed) = owner.take()
        {
            let _ignored = crashed.close().await;
            self.completed_jobs.store(0, Ordering::Release);
        }
        if owner.is_none() {
            *owner = Some(self.start_browser(selection, headed).await?);
            self.completed_jobs.store(0, Ordering::Release);
        }
        let owner = owner.as_ref().ok_or_else(|| {
            PageKnotError::new(
                "pageknot.browser.unavailable",
                ErrorStage::Browser,
                "browser ownership was lost during acquisition",
            )
        })?;
        let browser = owner.info().clone();
        let page = if request.deny_network {
            ChromiumPage::create_verifier_with_resource_limits(
                owner.client(),
                &request.environment,
                request.maximum_frames,
                request.resource_observation,
            )
            .await?
        } else {
            ChromiumPage::create_with_resource_limits(
                owner.client(),
                &request.environment,
                request.maximum_frames,
                request.resource_observation,
            )
            .await?
        };
        pending.set_page(Box::new(page));
        Ok(pending.finish(browser))
    }

    pub(crate) async fn ensure_browser(&self, browser: &BrowserSpec) -> Result<BrowserInfo> {
        self.ensure_open()?;
        if let Some(backend) = &self.custom_backend {
            let lease = backend
                .acquire(
                    BrowserAcquireRequest {
                        capture_id: CaptureId::new(),
                        browser: browser.clone(),
                        headed: self.headed,
                    },
                    CancellationToken::new(),
                )
                .await?;
            let info = lease.info().clone();
            lease.close().await?;
            return Ok(info);
        }
        let selection = self.effective_selection(browser);
        let mut owner = self.browser.lock().await;
        if owner
            .as_ref()
            .is_some_and(|owner| !owner.matches(selection, self.headed))
        {
            return Err(PageKnotError::new(
                "pageknot.browser.selection_conflict",
                ErrorStage::Browser,
                "the running browser does not match the requested browser selection",
            ));
        }
        if self.context_slots.available_permits() == self.maximum_contexts
            && owner.as_ref().is_some_and(|owner| {
                owner.should_recycle(
                    self.completed_jobs.load(Ordering::Acquire),
                    self.browser_recycle_after_jobs,
                )
            })
            && let Some(stale) = owner.take()
        {
            stale.close().await?;
            self.completed_jobs.store(0, Ordering::Release);
        }
        if self.context_slots.available_permits() == self.maximum_contexts
            && let Some(active) = owner.as_ref()
            && !active.is_healthy().await
            && let Some(crashed) = owner.take()
        {
            let _ignored = crashed.close().await;
            self.completed_jobs.store(0, Ordering::Release);
        }
        if owner.is_none() {
            *owner = Some(self.start_browser(selection, self.headed).await?);
            self.completed_jobs.store(0, Ordering::Release);
        }
        owner
            .as_ref()
            .map(|owner| owner.info().clone())
            .ok_or_else(browser_unavailable)
    }

    pub(crate) fn default_browser(&self) -> &BrowserSpec {
        &self.default_browser
    }

    pub(crate) const fn default_capture_profile(&self) -> &CaptureProfile {
        &self.default_capture_profile
    }

    pub(crate) fn capture_profile(&self, name: &str) -> Result<CaptureProfile> {
        self.profiles
            .get(name)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| CaptureProfile::named(name))
    }

    fn effective_selection<'a>(&'a self, requested: &'a BrowserSpec) -> &'a BrowserSpec {
        if matches!(requested, BrowserSpec::Auto) {
            &self.default_browser
        } else {
            requested
        }
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
        let discovery = self.discovery.discover().await;
        if let Some(selected) = discovery.selected {
            return Ok(selected);
        }
        if self.browser_installation == BrowserInstallationPolicy::InstallManaged
            && self.browser_channel != BrowserChannel::System
        {
            ManagedBrowserManager::new(self.cache_dir.clone())
                .install(None)
                .await?;
            let discovery = self.discovery.discover().await;
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
            PageKnotError::new(
                "pageknot.browser.install",
                ErrorStage::Browser,
                "managed browser record omitted its revision",
            )
        })?;
        ManagedBrowserManager::new(self.cache_dir.clone())
            .lease(revision)
            .await
            .map(Some)
    }

    pub(crate) async fn close_idle_browser(&self) -> Result<()> {
        self.ensure_open()?;
        if self.context_slots.available_permits() != self.maximum_contexts {
            return Err(PageKnotError::new(
                "pageknot.browser.active",
                ErrorStage::Shutdown,
                "browser contexts are active",
            ));
        }
        if let Some(backend) = &self.custom_backend {
            return backend.close().await;
        }
        if let Some(browser) = self.browser.lock().await.take() {
            browser.close().await?;
        }
        if let Some(backend) = &self.custom_backend {
            backend.close().await?;
        }
        Ok(())
    }

    pub(crate) fn custom_backend(&self) -> Option<&Arc<dyn BrowserBackend>> {
        self.custom_backend.as_ref()
    }

    async fn recycle_idle_browser_if_due(&self) -> Result<()> {
        if self.context_slots.available_permits() != self.maximum_contexts
            || self.completed_jobs.load(Ordering::Acquire) < self.browser_recycle_after_jobs.get()
        {
            return Ok(());
        }
        let mut owner = self.browser.lock().await;
        if self.context_slots.available_permits() != self.maximum_contexts {
            return Ok(());
        }
        if owner.as_ref().is_some_and(|browser| {
            browser.should_recycle(
                self.completed_jobs.load(Ordering::Acquire),
                self.browser_recycle_after_jobs,
            )
        }) && let Some(stale) = owner.take()
        {
            stale.close().await?;
            self.completed_jobs.store(0, Ordering::Release);
        }
        Ok(())
    }

    pub(crate) async fn active_browser_snapshot(&self) -> (Option<BrowserInfo>, u32) {
        let browser = self
            .browser
            .lock()
            .await
            .as_ref()
            .map(|owner| owner.info().clone());
        let active = self
            .maximum_contexts
            .saturating_sub(self.context_slots.available_permits());
        (browser, u32::try_from(active).unwrap_or(u32::MAX))
    }

    pub(crate) async fn close(self: &Arc<Self>) -> Result<()> {
        let (result, leader) = self.lifecycle.begin_close();
        if leader {
            let runtime = Arc::clone(self);
            tokio::spawn(async move {
                let result = std::panic::AssertUnwindSafe(runtime.finish_close())
                    .catch_unwind()
                    .await
                    .unwrap_or_else(|_| {
                        Err(PageKnotError::new(
                            "pageknot.runtime.shutdown",
                            ErrorStage::Shutdown,
                            "runtime shutdown task panicked",
                        ))
                    });
                runtime.lifecycle.complete(result);
            });
        }
        RuntimeLifecycle::wait(result).await
    }

    async fn finish_close(&self) -> Result<()> {
        if let Ok(jobs) = self.jobs.lock() {
            for job in jobs.values() {
                job.control.request_cancel();
            }
        }
        loop {
            let notified = self.jobs_changed.notified();
            let empty = self.jobs.lock().map_or(true, |jobs| jobs.is_empty());
            if empty {
                break;
            }
            notified.await;
        }
        loop {
            let notified = self.contexts_changed.notified();
            if self.context_slots.available_permits() == self.maximum_contexts {
                break;
            }
            notified.await;
        }
        self.context_slots.close();
        let mut result = Ok(());
        if let Some(browser) = self.browser.lock().await.take() {
            result = result.and(browser.close().await);
        }
        if let Some(backend) = &self.custom_backend {
            result = result.and(backend.close().await);
        }
        result
    }
}

impl Drop for RuntimeState {
    fn drop(&mut self) {
        self.lifecycle.mark_dropped();
        if let Ok(jobs) = self.jobs.lock() {
            for job in jobs.values() {
                job.control.request_cancel();
                if let Some(task) = &job.task {
                    task.abort();
                }
            }
        }
        if let Ok(mut browser) = self.browser.try_lock() {
            browser.take();
        }
    }
}

#[derive(Debug)]
struct ActiveJob {
    control: Arc<JobControl>,
    task: Option<tokio::task::JoinHandle<()>>,
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

    const fn should_recycle(&self, completed_jobs: u32, threshold: NonZeroU32) -> bool {
        matches!(self, Self::Local { .. }) && completed_jobs >= threshold.get()
    }

    async fn is_healthy(&self) -> bool {
        match self {
            Self::Local { process, .. } => process.is_healthy().await,
            Self::Remote { client, .. } => client
                .command_with_timeout(
                    "Browser.getVersion",
                    json!({}),
                    None,
                    std::time::Duration::from_secs(2),
                )
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

#[derive(Debug)]
pub(crate) struct RuntimePageRequest {
    pub(crate) capture_id: CaptureId,
    pub(crate) browser: BrowserSpec,
    pub(crate) environment: BrowserEnvironment,
    pub(crate) headed: Option<bool>,
    pub(crate) network: NetworkPolicy,
    pub(crate) maximum_frames: u32,
    pub(crate) resource_observation: ResourceObservationLimits,
    pub(crate) deny_network: bool,
    pub(crate) cancellation: CancellationToken,
}

async fn start_local(
    info: BrowserInfo,
    headed: bool,
    managed_lease: Option<ManagedBrowserLease>,
) -> Result<BrowserOwner> {
    let executable = info.executable_path.clone().ok_or_else(|| {
        PageKnotError::new(
            "pageknot.browser.executable",
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
    let (client, info) = connect_remote(&endpoint).await?;
    Ok(BrowserOwner::Remote {
        client,
        info,
        endpoint,
    })
}

pub(crate) async fn probe_remote_browser(endpoint: &Url) -> Result<BrowserInfo> {
    let (client, info) = connect_remote(endpoint).await?;
    client.close().await?;
    Ok(info)
}

async fn connect_remote(requested_endpoint: &Url) -> Result<(CdpClient, BrowserInfo)> {
    let websocket_endpoint = resolve_remote_endpoint(requested_endpoint).await?;
    let client = CdpClient::connect(websocket_endpoint).await?;
    let version = client
        .command("Browser.getVersion", json!({}), None)
        .await?;
    let product_value = version
        .get("product")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("Chromium/unknown");
    let (product, version_number) = parse_remote_product(product_value);
    let protocol_version = version
        .get("protocolVersion")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let info = BrowserInfo {
        product,
        version: version_number,
        source: BrowserSource::Remote,
        executable_path: None,
        endpoint: Some(RedactedUrl::from_url(
            requested_endpoint,
            &RedactionPolicy::default(),
        )),
        revision: None,
        protocol_version,
    };
    Ok((client, info))
}

fn parse_remote_product(value: &str) -> (BrowserProduct, String) {
    let (name, version) = value.split_once('/').unwrap_or((value, "unknown"));
    let product = if name.contains("Edge") || name.contains("Edg") {
        BrowserProduct::Edge
    } else if name.contains("Chrome") {
        BrowserProduct::Chrome
    } else {
        BrowserProduct::Chromium
    };
    (product, version.to_owned())
}

fn browser_unavailable() -> PageKnotError {
    PageKnotError::new(
        "pageknot.browser.unavailable",
        ErrorStage::Browser,
        "no compatible Chrome or Chromium executable was found",
    )
    .with_detail("recoveryCommand", "pageknot browser install")
}

fn default_cache_dir() -> Result<Utf8PathBuf> {
    let project = ProjectDirs::from("dev", "PageKnot", "PageKnot").ok_or_else(|| {
        PageKnotError::new(
            "pageknot.config.directory",
            ErrorStage::Validation,
            "platform cache directory is unavailable",
        )
    })?;
    Utf8PathBuf::from_path_buf(project.cache_dir().join("browsers")).map_err(|path| {
        PageKnotError::new(
            "pageknot.input.path_encoding",
            ErrorStage::Validation,
            format!("cache path is not valid UTF-8: {}", path.display()),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use chrono::{DateTime, Utc};
    use pageknot_model::{BrowserChannel, BrowserInstallationPolicy, BrowserSpec, CaptureId};

    use super::BrowserOwner;
    use crate::{CaptureIdGenerator, Clock, PageKnot};

    type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

    #[derive(Debug)]
    struct FixedClock(DateTime<Utc>);

    impl Clock for FixedClock {
        fn now(&self) -> DateTime<Utc> {
            self.0
        }
    }

    #[derive(Debug)]
    struct FixedCaptureId(CaptureId);

    impl CaptureIdGenerator for FixedCaptureId {
        fn next_capture_id(&self) -> CaptureId {
            self.0.clone()
        }
    }

    #[test]
    fn system_channel_overrides_managed_auto_installation() {
        let result = PageKnot::builder()
            .browser_channel(BrowserChannel::System)
            .build();

        assert!(result.is_ok());
        assert_eq!(
            result
                .as_ref()
                .ok()
                .map(|pageknot| pageknot.state.browser_installation),
            Some(BrowserInstallationPolicy::InstallManaged)
        );
    }

    #[test]
    fn runtime_uses_injected_clock_and_capture_identity_sources() -> TestResult {
        let timestamp = DateTime::parse_from_rfc3339("2026-07-27T12:34:56Z")
            .map(|value| value.with_timezone(&Utc))?;
        let capture_id = "cap_00000000000000000000000000"
            .parse::<CaptureId>()
            .map_err(std::io::Error::other)?;
        let pageknot = PageKnot::builder()
            .clock(Arc::new(FixedClock(timestamp)))
            .capture_id_generator(Arc::new(FixedCaptureId(capture_id.clone())))
            .build()?;

        assert_eq!(pageknot.state.now(), timestamp);
        assert_eq!(pageknot.state.next_capture_id(), capture_id);
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires a locally installed compatible Chromium"]
    async fn local_browser_restarts_after_its_process_tree_crashes() -> TestResult {
        let pageknot = PageKnot::builder().build()?;
        pageknot.browsers().ensure().await?;
        let first = local_endpoint(&pageknot).await?;
        {
            let owner = pageknot.state.browser.lock().await;
            let Some(BrowserOwner::Local { process, .. }) = owner.as_ref() else {
                return Err(std::io::Error::other("expected an owned local browser").into());
            };
            process.terminate_process_tree()?;
        }

        pageknot.state.ensure_browser(&BrowserSpec::Auto).await?;
        let second = local_endpoint(&pageknot).await?;

        assert_ne!(first, second);
        pageknot.close().await?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires a locally installed compatible Chromium"]
    async fn local_browser_recycles_after_the_configured_job_threshold() -> TestResult {
        let pageknot = PageKnot::builder().browser_recycle_after_jobs(1).build()?;
        pageknot.browsers().ensure().await?;
        let first = local_endpoint(&pageknot).await?;
        pageknot.state.completed_jobs.store(1, Ordering::Release);

        pageknot.state.ensure_browser(&BrowserSpec::Auto).await?;
        let second = local_endpoint(&pageknot).await?;

        assert_ne!(first, second);
        pageknot.close().await?;
        Ok(())
    }

    async fn local_endpoint(pageknot: &PageKnot) -> TestResult<String> {
        let owner = pageknot.state.browser.lock().await;
        match owner.as_ref() {
            Some(BrowserOwner::Local { process, .. }) => Ok(process.endpoint().to_string()),
            Some(BrowserOwner::Remote { .. }) | None => {
                Err(std::io::Error::other("expected an owned local browser").into())
            }
        }
    }
}
