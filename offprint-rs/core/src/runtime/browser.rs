use std::sync::Arc;
use std::sync::atomic::Ordering;

use offprint_browser::{BrowserAcquireRequest, BrowserBackend, ResourceObservationLimits};
use offprint_model::{
    BrowserEnvironment, BrowserInfo, BrowserSpec, CaptureId, ErrorStage, NetworkPolicy,
    OffprintError, Result,
};
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;

use super::{
    ContextPermit, PendingRuntimePage, RuntimePage, RuntimeState, closed_error,
    operation_cancelled_error,
};

pub(crate) use offprint_chromium::probe_remote_browser;

#[derive(Debug)]
pub(crate) struct RuntimePageRequest {
    pub(crate) capture_id: CaptureId,
    pub(crate) browser: BrowserSpec,
    pub(crate) environment: BrowserEnvironment,
    pub(crate) headed: Option<bool>,
    pub(crate) network: NetworkPolicy,
    pub(crate) maximum_frames: u32,
    pub(crate) resource_observation: ResourceObservationLimits,
    pub(crate) purpose: RuntimePagePurpose,
    pub(crate) cancellation: CancellationToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimePagePurpose {
    Capture,
    OfflineVerification,
}

impl RuntimePagePurpose {
    pub(crate) const fn denies_network(self) -> bool {
        matches!(self, Self::OfflineVerification)
    }
}

impl RuntimeState {
    pub(crate) async fn open_page(
        self: &Arc<Self>,
        request: RuntimePageRequest,
    ) -> Result<RuntimePage> {
        self.ensure_open()?;
        let cancellation = request.cancellation.clone();
        let permit = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(operation_cancelled_error()),
            permit = Arc::clone(&self.context_slots).acquire_owned() => {
                permit.map_err(|_| closed_error())?
            }
        };
        let permit = ContextPermit::new(permit, self);
        self.ensure_open()?;
        let runtime = Arc::clone(self);
        let mut task = AbortOnDropHandle::new(tokio::spawn(async move {
            runtime.acquire_page(request, permit).await
        }));
        tokio::select! {
            biased;
            () = cancellation.cancelled() => {
                task.abort();
                let _cancelled = task.await;
                Err(operation_cancelled_error())
            }
            result = &mut task => result.map_err(|error| {
                OffprintError::new(
                    "offprint.runtime.acquisition",
                    ErrorStage::Internal,
                    format!("browser acquisition task failed: {error}"),
                )
            })?,
        }
    }

    async fn acquire_page(
        self: &Arc<Self>,
        request: RuntimePageRequest,
        permit: ContextPermit,
    ) -> Result<RuntimePage> {
        let mut pending = PendingRuntimePage::new(self, permit);
        let browser = self.effective_selection(&request.browser).clone();
        let acquire = BrowserAcquireRequest {
            capture_id: request.capture_id.clone(),
            browser,
            headed: request.headed.unwrap_or(self.headed),
        };
        let lease = if let Some(gate) = &self.owned_backend_gate {
            let _operation = gate.lock().await;
            self.recycle_backend_if_due_locked(self.maximum_contexts.saturating_sub(1))
                .await?;
            self.browser_backend
                .acquire(acquire, request.cancellation.clone())
                .await?
        } else {
            self.browser_backend
                .acquire(acquire, request.cancellation.clone())
                .await?
        };
        let browser = lease.info().clone();
        pending.set_lease(lease);
        pending.acquire_context_and_page(request, browser).await
    }

    pub(crate) async fn ensure_browser(&self, browser: &BrowserSpec) -> Result<BrowserInfo> {
        self.ensure_open()?;
        let request = BrowserAcquireRequest {
            capture_id: CaptureId::new(),
            browser: self.effective_selection(browser).clone(),
            headed: self.headed,
        };
        if let Some(gate) = &self.owned_backend_gate {
            let _operation = gate.lock().await;
            self.recycle_backend_if_due_locked(self.maximum_contexts)
                .await?;
            return self.acquire_and_release(request).await;
        }
        self.acquire_and_release(request).await
    }

    async fn acquire_and_release(&self, request: BrowserAcquireRequest) -> Result<BrowserInfo> {
        let cancellation = self.operation_cancellation();
        let lease = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(operation_cancelled_error()),
            lease = self.browser_backend.acquire(request, cancellation.clone()) => lease?,
        };
        let info = lease.info().clone();
        lease.close().await?;
        Ok(info)
    }

    pub(crate) fn default_browser(&self) -> &BrowserSpec {
        &self.default_browser
    }

    fn effective_selection<'a>(&'a self, requested: &'a BrowserSpec) -> &'a BrowserSpec {
        if matches!(requested, BrowserSpec::Auto) {
            &self.default_browser
        } else {
            requested
        }
    }

    pub(crate) async fn close_idle_browser(&self) -> Result<()> {
        self.ensure_open()?;
        if self.context_slots.available_permits() != self.maximum_contexts {
            return Err(OffprintError::new(
                "offprint.browser.active",
                ErrorStage::Shutdown,
                "browser contexts are active",
            ));
        }
        if let Some(gate) = &self.owned_backend_gate {
            let _operation = gate.lock().await;
            if self.context_slots.available_permits() != self.maximum_contexts {
                return Err(OffprintError::new(
                    "offprint.browser.active",
                    ErrorStage::Shutdown,
                    "browser contexts are active",
                ));
            }
            return self.close_backend().await;
        }
        self.close_backend().await
    }

    pub(crate) fn custom_backend(&self) -> Option<&Arc<dyn BrowserBackend>> {
        self.injected_browser_backend
            .then_some(&self.browser_backend)
    }

    pub(super) async fn recycle_idle_browser_if_due(&self) -> Result<()> {
        let Some(gate) = &self.owned_backend_gate else {
            return Ok(());
        };
        if self.context_slots.available_permits() != self.maximum_contexts
            || self.completed_jobs.load(Ordering::Acquire) < self.browser_recycle_after_jobs.get()
        {
            return Ok(());
        }
        let _operation = gate.lock().await;
        self.recycle_backend_if_due_locked(self.maximum_contexts)
            .await
    }

    async fn recycle_backend_if_due_locked(&self, expected_available: usize) -> Result<()> {
        if self.context_slots.available_permits() != expected_available
            || self.completed_jobs.load(Ordering::Acquire) < self.browser_recycle_after_jobs.get()
        {
            return Ok(());
        }
        self.close_backend().await?;
        self.completed_jobs.store(0, Ordering::Release);
        Ok(())
    }

    async fn close_backend(&self) -> Result<()> {
        self.browser_backend.close().await
    }

    pub(crate) async fn active_browser_snapshot(&self) -> (Option<BrowserInfo>, u32) {
        let browser = self.browser_backend.active_browser().await;
        let active = self
            .maximum_contexts
            .saturating_sub(self.context_slots.available_permits());
        (browser, u32::try_from(active).unwrap_or(u32::MAX))
    }
}

#[cfg(test)]
mod tests;
