use std::future::Future;
use std::sync::{Arc, Weak};

use offprint_browser::{BrowserContext, BrowserContextRequest, BrowserLease, PageSession};
use offprint_model::{BrowserInfo, ErrorStage, OffprintError, Result};
use tokio::sync::OwnedSemaphorePermit;

use super::{CONTEXT_CLEANUP_TIMEOUT, RuntimePageRequest, RuntimeState};

#[derive(Debug)]
pub(super) struct ContextPermit {
    permit: Option<OwnedSemaphorePermit>,
    runtime: Weak<RuntimeState>,
}

impl ContextPermit {
    pub(super) fn new(permit: OwnedSemaphorePermit, runtime: &Arc<RuntimeState>) -> Self {
        Self {
            permit: Some(permit),
            runtime: Arc::downgrade(runtime),
        }
    }
}

impl Drop for ContextPermit {
    fn drop(&mut self) {
        drop(self.permit.take());
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.contexts_changed.notify_waiters();
        }
    }
}

#[derive(Debug)]
pub(super) struct PendingRuntimePage {
    page: Option<Box<dyn PageSession>>,
    context: Option<Box<dyn BrowserContext>>,
    lease: Option<Box<dyn BrowserLease>>,
    runtime: Weak<RuntimeState>,
    permit: Option<ContextPermit>,
}

impl PendingRuntimePage {
    pub(super) fn new(runtime: &Arc<RuntimeState>, permit: ContextPermit) -> Self {
        Self {
            page: None,
            context: None,
            lease: None,
            runtime: Arc::downgrade(runtime),
            permit: Some(permit),
        }
    }

    pub(super) fn set_lease(&mut self, lease: Box<dyn BrowserLease>) {
        self.lease = Some(lease);
    }

    pub(super) async fn acquire_context_and_page(
        mut self,
        request: RuntimePageRequest,
        browser: BrowserInfo,
    ) -> Result<RuntimePage> {
        let context = match self
            .lease
            .as_ref()
            .ok_or_else(|| acquisition_owner_error("browser lease"))?
            .create_context(
                BrowserContextRequest {
                    environment: request.environment,
                    network: request.network,
                    maximum_frames: request.maximum_frames,
                    resource_observation: request.resource_observation,
                    deny_network: request.purpose.denies_network(),
                },
                request.cancellation.clone(),
            )
            .await
        {
            Ok(context) => context,
            Err(error) => {
                let _ignored = self.close().await;
                return Err(error);
            }
        };
        self.context = Some(context);
        let page = match self
            .context
            .as_ref()
            .ok_or_else(|| acquisition_owner_error("browser context"))?
            .open_page(request.cancellation)
            .await
        {
            Ok(page) => page,
            Err(error) => {
                let _ignored = self.close().await;
                return Err(error);
            }
        };
        self.page = Some(page);
        Ok(self.finish(browser))
    }

    pub(super) fn finish(mut self, browser: BrowserInfo) -> RuntimePage {
        RuntimePage {
            page: self.page.take(),
            browser,
            context: self.context.take(),
            lease: self.lease.take(),
            runtime: self.runtime.clone(),
            permit: self.permit.take(),
        }
    }

    async fn close(mut self) -> Result<()> {
        close_runtime_page_parts(
            self.page.take(),
            self.context.take(),
            self.lease.take(),
            self.permit.take(),
            self.runtime.upgrade(),
        )
        .await
    }
}

impl Drop for PendingRuntimePage {
    fn drop(&mut self) {
        spawn_runtime_page_cleanup(
            self.page.take(),
            self.context.take(),
            self.lease.take(),
            self.permit.take(),
            self.runtime.upgrade(),
        );
    }
}

#[derive(Debug)]
pub(crate) struct RuntimePage {
    page: Option<Box<dyn PageSession>>,
    pub(crate) browser: BrowserInfo,
    context: Option<Box<dyn BrowserContext>>,
    lease: Option<Box<dyn BrowserLease>>,
    runtime: Weak<RuntimeState>,
    permit: Option<ContextPermit>,
}

impl RuntimePage {
    pub(crate) fn page(&self) -> Result<&dyn PageSession> {
        self.page.as_deref().ok_or_else(|| {
            OffprintError::new(
                "offprint.browser.context",
                ErrorStage::Internal,
                "browser page is unavailable after context cleanup",
            )
        })
    }

    pub(crate) async fn close(mut self) -> Result<()> {
        close_runtime_page_parts(
            self.page.take(),
            self.context.take(),
            self.lease.take(),
            self.permit.take(),
            self.runtime.upgrade(),
        )
        .await
    }
}

impl Drop for RuntimePage {
    fn drop(&mut self) {
        if self.page.is_none()
            && self.context.is_none()
            && self.lease.is_none()
            && self.permit.is_none()
        {
            return;
        }
        spawn_runtime_page_cleanup(
            self.page.take(),
            self.context.take(),
            self.lease.take(),
            self.permit.take(),
            self.runtime.upgrade(),
        );
    }
}

fn spawn_runtime_page_cleanup(
    page: Option<Box<dyn PageSession>>,
    context: Option<Box<dyn BrowserContext>>,
    lease: Option<Box<dyn BrowserLease>>,
    permit: Option<ContextPermit>,
    runtime: Option<Arc<RuntimeState>>,
) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        let _cleanup = handle.spawn(async move {
            let _ignored = close_runtime_page_parts(page, context, lease, permit, runtime).await;
        });
    }
}

async fn close_runtime_page_parts(
    page: Option<Box<dyn PageSession>>,
    context: Option<Box<dyn BrowserContext>>,
    lease: Option<Box<dyn BrowserLease>>,
    permit: Option<ContextPermit>,
    runtime: Option<Arc<RuntimeState>>,
) -> Result<()> {
    let mut result = match page {
        Some(page) => bounded_context_cleanup(page.close(), "browser page").await,
        None => Ok(()),
    };
    if let Some(context) = context {
        result = result.and(bounded_context_cleanup(context.close(), "browser context").await);
    }
    if let Some(lease) = lease {
        result = result.and(bounded_context_cleanup(lease.close(), "browser lease").await);
    }
    drop(permit);
    if let Some(runtime) = runtime {
        result = result.and(runtime.recycle_idle_browser_if_due().await);
    }
    result
}

async fn bounded_context_cleanup(
    cleanup: impl Future<Output = Result<()>>,
    owner: &'static str,
) -> Result<()> {
    tokio::time::timeout(CONTEXT_CLEANUP_TIMEOUT, cleanup)
        .await
        .map_err(|_| {
            OffprintError::new(
                "offprint.browser.shutdown",
                ErrorStage::Shutdown,
                format!("{owner} cleanup exceeded its deadline"),
            )
            .retryable(true)
        })?
}

fn acquisition_owner_error(owner: &'static str) -> OffprintError {
    OffprintError::new(
        "offprint.runtime.acquisition",
        ErrorStage::Internal,
        format!("{owner} ownership was lost during acquisition"),
    )
}
