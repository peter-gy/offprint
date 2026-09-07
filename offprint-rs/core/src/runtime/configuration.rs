use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};

use camino::Utf8PathBuf;
use directories::ProjectDirs;
use offprint_browser::BrowserBackend;
use offprint_chromium::{ChromiumBackend, ChromiumBackendOptions, ChromiumDiscovery};
use offprint_model::{
    BrowserInstallationPolicy, BrowserSourcePolicy, BrowserSpec, CaptureId, CaptureProfile,
    EffectiveConfigValue, ErrorStage, NetworkPolicy, OffprintError, Result,
};
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::{ActiveJob, RuntimeLifecycle, RuntimeState};
use crate::{CaptureIdGenerator, Clock};

#[derive(Debug)]
pub(crate) struct RuntimeOptions {
    pub(crate) browser_path: Option<Utf8PathBuf>,
    pub(crate) cdp_url: Option<Url>,
    pub(crate) browser_backend: Option<Arc<dyn BrowserBackend>>,
    pub(crate) cache_dir: Option<Utf8PathBuf>,
    pub(crate) browser_source: BrowserSourcePolicy,
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
            browser_source,
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
            return Err(OffprintError::new(
                "offprint.input.concurrency",
                ErrorStage::Validation,
                "browser context concurrency must be greater than zero",
            ));
        }
        if browser_backend.is_some() && (browser_path.is_some() || cdp_url.is_some()) {
            return Err(OffprintError::new(
                "offprint.input.browser_selection",
                ErrorStage::Validation,
                "custom browser backends cannot be combined with a browser path or remote endpoint",
            ));
        }
        let browser_recycle_after_jobs =
            NonZeroU32::new(browser_recycle_after_jobs).ok_or_else(|| {
                OffprintError::new(
                    "offprint.input.browser_recycle",
                    ErrorStage::Validation,
                    "browser recycling threshold must be greater than zero",
                )
            })?;
        let default_browser = match (browser_path, cdp_url) {
            (Some(path), None) => BrowserSpec::Executable(path.into()),
            (None, Some(endpoint)) => BrowserSpec::Remote(endpoint),
            (None, None) => BrowserSpec::Auto,
            (Some(_), Some(_)) => {
                return Err(OffprintError::new(
                    "offprint.input.browser_selection",
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
                .with_source_policy(browser_source),
            BrowserSpec::Auto | BrowserSpec::Remote(_) => ChromiumDiscovery::new()
                .with_managed_cache(cache_dir.clone())
                .with_source_policy(browser_source),
        };
        let injected_browser_backend = browser_backend.is_some();
        let browser_backend = browser_backend.unwrap_or_else(|| {
            Arc::new(ChromiumBackend::new(
                ChromiumBackendOptions::new(discovery.clone(), cache_dir.clone())
                    .with_browser_source(browser_source)
                    .with_browser_installation(browser_installation),
            ))
        });
        let maximum_contexts = usize::from(maximum_contexts);
        Ok(Self {
            lifecycle: RuntimeLifecycle::new(),
            discovery,
            cache_dir,
            default_browser,
            browser_backend,
            injected_browser_backend,
            owned_backend_gate: (!injected_browser_backend).then(tokio::sync::Mutex::default),
            browser_source,
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
            jobs: Mutex::new(BTreeMap::<CaptureId, ActiveJob>::new()),
            jobs_changed: Notify::new(),
            shutdown: CancellationToken::new(),
        })
    }
}

fn default_cache_dir() -> Result<Utf8PathBuf> {
    let project = ProjectDirs::from("dev", "Offprint", "Offprint").ok_or_else(|| {
        OffprintError::new(
            "offprint.config.directory",
            ErrorStage::Validation,
            "platform cache directory is unavailable",
        )
    })?;
    Utf8PathBuf::from_path_buf(project.cache_dir().join("browsers")).map_err(|path| {
        OffprintError::new(
            "offprint.input.path_encoding",
            ErrorStage::Validation,
            format!("cache path is not valid UTF-8: {}", path.display()),
        )
    })
}
