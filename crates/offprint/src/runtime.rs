use std::collections::BTreeMap;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use camino::Utf8PathBuf;
use futures_util::FutureExt as _;
use offprint_browser::BrowserBackend;
use offprint_chromium::ChromiumDiscovery;
use offprint_model::{
    BrowserInstallationPolicy, BrowserSourcePolicy, BrowserSpec, CaptureId, CaptureProfile,
    EffectiveConfigValue, ErrorStage, NetworkPolicy, OffprintError, Result,
};
use tokio::sync::{Notify, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::capture_service::JobControl;
use crate::{CaptureIdGenerator, Clock};

mod browser;
mod configuration;
mod context;
mod lifecycle;

pub(crate) use browser::{RuntimePagePurpose, RuntimePageRequest, probe_remote_browser};
pub(crate) use configuration::RuntimeOptions;
use context::{ContextPermit, PendingRuntimePage, RuntimePage};
use lifecycle::{RuntimeLifecycle, closed_error};

const CONTEXT_CLEANUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const RUNTIME_SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug)]
pub(crate) struct RuntimeState {
    lifecycle: RuntimeLifecycle,
    pub(crate) discovery: ChromiumDiscovery,
    pub(crate) cache_dir: Utf8PathBuf,
    default_browser: BrowserSpec,
    browser_backend: Arc<dyn BrowserBackend>,
    injected_browser_backend: bool,
    owned_backend_gate: Option<tokio::sync::Mutex<()>>,
    pub(crate) browser_source: BrowserSourcePolicy,
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
    shutdown: CancellationToken,
}

impl RuntimeState {
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
            OffprintError::new(
                "offprint.runtime.lock",
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

    pub(crate) fn operation_cancellation(&self) -> CancellationToken {
        self.shutdown.child_token()
    }

    pub(crate) fn set_job_task(
        &self,
        capture_id: &CaptureId,
        task: tokio::task::JoinHandle<()>,
    ) -> Result<()> {
        let mut jobs = self.jobs.lock().map_err(|_| {
            OffprintError::new(
                "offprint.runtime.lock",
                ErrorStage::Internal,
                "capture registry lock is unavailable",
            )
        })?;
        let job = jobs.get_mut(capture_id).ok_or_else(|| {
            OffprintError::new(
                "offprint.runtime.job",
                ErrorStage::Internal,
                "capture job disappeared before its task was registered",
            )
        })?;
        job.task = Some(task);
        Ok(())
    }

    pub(crate) fn unregister_job(self: &Arc<Self>, capture_id: &CaptureId) {
        let removed = self
            .jobs
            .lock()
            .is_ok_and(|mut jobs| jobs.remove(capture_id).is_some());
        if !removed {
            return;
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

    pub(crate) async fn close(self: &Arc<Self>) -> Result<()> {
        let (result, leader) = self.lifecycle.begin_close();
        if leader {
            self.shutdown.cancel();
            let runtime = Arc::clone(self);
            tokio::spawn(async move {
                let result = std::panic::AssertUnwindSafe(runtime.finish_close())
                    .catch_unwind()
                    .await
                    .unwrap_or_else(|_| {
                        Err(OffprintError::new(
                            "offprint.runtime.shutdown",
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
        let jobs = tokio::time::timeout(RUNTIME_SHUTDOWN_TIMEOUT, async {
            loop {
                let notified = self.jobs_changed.notified();
                let empty = self
                    .jobs
                    .lock()
                    .map_err(|_| {
                        OffprintError::new(
                            "offprint.runtime.lock",
                            ErrorStage::Shutdown,
                            "capture registry lock is unavailable during shutdown",
                        )
                    })?
                    .is_empty();
                if empty {
                    return Ok(());
                }
                notified.await;
            }
        })
        .await
        .map_err(|_| shutdown_deadline_error("capture jobs"))?;
        let contexts = tokio::time::timeout(RUNTIME_SHUTDOWN_TIMEOUT, async {
            loop {
                let notified = self.contexts_changed.notified();
                if self.context_slots.available_permits() == self.maximum_contexts {
                    return;
                }
                notified.await;
            }
        })
        .await
        .map_err(|_| shutdown_deadline_error("browser contexts"));
        self.context_slots.close();
        let backend = tokio::time::timeout(RUNTIME_SHUTDOWN_TIMEOUT, self.browser_backend.close())
            .await
            .map_err(|_| shutdown_deadline_error("browser backend"))?;
        jobs.and(contexts).and(backend)
    }
}

impl Drop for RuntimeState {
    fn drop(&mut self) {
        self.lifecycle.mark_dropped();
        self.shutdown.cancel();
        if let Ok(jobs) = self.jobs.lock() {
            for job in jobs.values() {
                job.control.request_cancel();
                if let Some(task) = &job.task {
                    task.abort();
                }
            }
        }
    }
}

pub(crate) fn operation_cancelled_error() -> OffprintError {
    OffprintError::new(
        "offprint.runtime.cancelled",
        ErrorStage::Shutdown,
        "Offprint operation was cancelled",
    )
}

fn shutdown_deadline_error(owner: &'static str) -> OffprintError {
    OffprintError::new(
        "offprint.runtime.shutdown",
        ErrorStage::Shutdown,
        format!("{owner} did not stop before the shutdown deadline"),
    )
    .retryable(true)
}

#[derive(Debug)]
struct ActiveJob {
    control: Arc<JobControl>,
    task: Option<tokio::task::JoinHandle<()>>,
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use offprint_model::{BrowserInstallationPolicy, BrowserSourcePolicy, CaptureId};
    use std::error::Error;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use crate::capture_service::JobControl;
    use crate::{CaptureIdGenerator, Clock, Offprint};

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
    fn system_source_disables_managed_auto_installation() {
        let result = Offprint::builder()
            .browser_source(BrowserSourcePolicy::System)
            .build();

        assert!(result.is_ok());
        assert_eq!(
            result
                .as_ref()
                .ok()
                .map(|offprint| offprint.state.browser_installation),
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
        let offprint = Offprint::builder()
            .clock(Arc::new(FixedClock(timestamp)))
            .capture_id_generator(Arc::new(FixedCaptureId(capture_id.clone())))
            .build()?;

        assert_eq!(offprint.state.now(), timestamp);
        assert_eq!(offprint.state.next_capture_id(), capture_id);
        Ok(())
    }

    #[tokio::test]
    async fn close_cancels_runtime_operations() -> TestResult {
        let offprint = Offprint::builder().build()?;
        let cancellation = offprint.state.operation_cancellation();

        offprint.close().await?;

        assert!(cancellation.is_cancelled());
        Ok(())
    }

    #[tokio::test]
    async fn duplicate_job_cleanup_counts_one_completion() -> TestResult {
        let offprint = Offprint::builder().build()?;
        let capture_id = CaptureId::new();
        let control = Arc::new(JobControl::new(offprint.state.operation_cancellation()));
        offprint.state.register_job(capture_id.clone(), control)?;

        offprint.state.unregister_job(&capture_id);
        offprint.state.unregister_job(&capture_id);
        tokio::task::yield_now().await;

        assert_eq!(offprint.state.completed_jobs.load(Ordering::Acquire), 1);
        offprint.close().await?;
        Ok(())
    }
}
