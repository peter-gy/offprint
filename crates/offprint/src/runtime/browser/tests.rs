use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;
use offprint_browser::{
    BrowserAcquireRequest, BrowserBackend, BrowserContext, BrowserContextRequest, BrowserLease,
};
use offprint_model::{
    BrowserDoctorReport, BrowserInfo, BrowserProduct, BrowserSource, BrowserSpec, Result,
};
use tokio_util::sync::CancellationToken;

use crate::Offprint;

type TestResult = std::result::Result<(), Box<dyn Error + Send + Sync>>;

#[derive(Debug, Default)]
struct BackendCounts {
    active: AtomicBool,
    acquires: AtomicUsize,
    closes: AtomicUsize,
    contexts: AtomicUsize,
    doctors: AtomicUsize,
}

#[derive(Debug)]
struct CountingBackend {
    counts: Arc<BackendCounts>,
    doctor: BrowserDoctorReport,
}

#[async_trait]
impl BrowserBackend for CountingBackend {
    async fn acquire(
        &self,
        _request: BrowserAcquireRequest,
        _cancellation: CancellationToken,
    ) -> Result<Box<dyn BrowserLease>> {
        self.counts.acquires.fetch_add(1, Ordering::AcqRel);
        self.counts.active.store(true, Ordering::Release);
        Ok(Box::new(CountingLease {
            info: browser_info(),
            counts: Arc::clone(&self.counts),
        }))
    }

    async fn doctor(&self, _browser: &BrowserSpec) -> BrowserDoctorReport {
        self.counts.doctors.fetch_add(1, Ordering::AcqRel);
        self.doctor.clone()
    }

    async fn active_browser(&self) -> Option<BrowserInfo> {
        self.counts
            .active
            .load(Ordering::Acquire)
            .then(browser_info)
    }

    async fn close(&self) -> Result<()> {
        if self.counts.active.swap(false, Ordering::AcqRel) {
            self.counts.closes.fetch_add(1, Ordering::AcqRel);
        }
        Ok(())
    }
}

#[derive(Debug)]
struct CountingLease {
    info: BrowserInfo,
    counts: Arc<BackendCounts>,
}

#[async_trait]
impl BrowserLease for CountingLease {
    fn info(&self) -> &BrowserInfo {
        &self.info
    }

    async fn create_context(
        &self,
        _request: BrowserContextRequest,
        _cancellation: CancellationToken,
    ) -> Result<Box<dyn BrowserContext>> {
        self.counts.contexts.fetch_add(1, Ordering::AcqRel);
        Err(offprint_model::OffprintError::new(
            "offprint.internal.test",
            offprint_model::ErrorStage::Internal,
            "recycling fixture does not create browser contexts",
        ))
    }

    async fn close(self: Box<Self>) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn local_browser_recycles_after_the_configured_job_threshold() -> TestResult {
    let counts = Arc::new(BackendCounts::default());
    let doctor = serde_json::from_str(include_str!(
        "../../../../../schemas/examples/browser-doctor-report.json"
    ))?;
    let backend = Arc::new(CountingBackend {
        counts: Arc::clone(&counts),
        doctor,
    });
    let mut offprint = Offprint::builder()
        .browser_backend(backend)
        .browser_recycle_after_jobs(1)
        .build()?;
    let state = Arc::get_mut(&mut offprint.state)
        .ok_or_else(|| std::io::Error::other("runtime state is unexpectedly shared"))?;
    // Model the built-in ownership policy while keeping process lifecycle
    // observable through counters.
    state.injected_browser_backend = false;
    state.owned_backend_gate = Some(tokio::sync::Mutex::default());

    offprint.state.ensure_browser(&BrowserSpec::Auto).await?;
    offprint.state.completed_jobs.store(1, Ordering::Release);
    offprint.state.recycle_idle_browser_if_due().await?;

    assert_eq!(counts.acquires.load(Ordering::Acquire), 1);
    assert_eq!(counts.closes.load(Ordering::Acquire), 1);

    offprint.state.ensure_browser(&BrowserSpec::Auto).await?;

    assert_eq!(counts.acquires.load(Ordering::Acquire), 2);
    assert_eq!(counts.closes.load(Ordering::Acquire), 1);
    assert_eq!(counts.contexts.load(Ordering::Acquire), 0);
    assert_eq!(counts.doctors.load(Ordering::Acquire), 0);
    offprint.close().await?;
    assert_eq!(counts.closes.load(Ordering::Acquire), 2);
    Ok(())
}

fn browser_info() -> BrowserInfo {
    BrowserInfo {
        product: BrowserProduct::Chromium,
        version: "fixture".to_owned(),
        source: BrowserSource::System,
        executable_path: None,
        endpoint: None,
        revision: None,
        protocol_version: "1.3".to_owned(),
    }
}
