use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use offprint_browser::ResourceObservationLimits;
use offprint_model::{BrowserEnvironment, ErrorStage, OffprintError, Result, UserAgentPolicy};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::ChromiumPage;
use super::activity::NetworkActivity;
use crate::cdp::generated::cdp_browser::{SetDownloadBehaviorCommand, SetDownloadBehaviorParams};
use crate::cdp::generated::cdp_target::{
    CreateBrowserContextCommand, CreateBrowserContextParams, CreateTargetCommand,
    CreateTargetParams, DisposeBrowserContextCommand, DisposeBrowserContextParams,
};
use crate::proxy::ValidatingProxy;
use crate::resources::ObservedResources;
use crate::targets::{FrameTargetManager, TargetLimits, auto_attach_parameters};
use crate::{COLLECTOR_BUNDLE, CdpClient};

impl ChromiumPage {
    pub async fn create(client: CdpClient, environment: &BrowserEnvironment) -> Result<Self> {
        Self::create_with_target_limits(
            client,
            environment,
            TargetLimits::default(),
            ResourceObservationLimits::default(),
            true,
        )
        .await
    }

    pub async fn create_with_limits(
        client: CdpClient,
        environment: &BrowserEnvironment,
        maximum_frames: u32,
    ) -> Result<Self> {
        Self::create_with_resource_limits(
            client,
            environment,
            maximum_frames,
            ResourceObservationLimits::default(),
        )
        .await
    }

    pub async fn create_with_resource_limits(
        client: CdpClient,
        environment: &BrowserEnvironment,
        maximum_frames: u32,
        resource_observation: ResourceObservationLimits,
    ) -> Result<Self> {
        Self::create_with_target_limits(
            client,
            environment,
            TargetLimits::for_capture(maximum_frames),
            resource_observation,
            true,
        )
        .await
    }

    pub async fn create_verifier_with_resource_limits(
        client: CdpClient,
        environment: &BrowserEnvironment,
        maximum_frames: u32,
        resource_observation: ResourceObservationLimits,
    ) -> Result<Self> {
        Self::create_with_target_limits(
            client,
            environment,
            TargetLimits::for_capture(maximum_frames),
            resource_observation,
            false,
        )
        .await
    }

    async fn create_with_target_limits(
        client: CdpClient,
        environment: &BrowserEnvironment,
        target_limits: TargetLimits,
        resource_observation: ResourceObservationLimits,
        install_collector: bool,
    ) -> Result<Self> {
        let environment = environment.clone();
        tokio::spawn(async move {
            Self::create_owned(
                client,
                &environment,
                target_limits,
                resource_observation,
                install_collector,
            )
            .await
        })
        .await
        .map_err(|error| {
            OffprintError::new(
                "offprint.browser.acquisition",
                ErrorStage::Internal,
                format!("Chromium page acquisition task failed: {error}"),
            )
        })?
    }

    async fn create_owned(
        client: CdpClient,
        environment: &BrowserEnvironment,
        target_limits: TargetLimits,
        resource_observation: ResourceObservationLimits,
        install_collector: bool,
    ) -> Result<Self> {
        let validating_proxy = if client.is_owned_browser() {
            Some(ValidatingProxy::start(client.proxy_connection_budget()).await?)
        } else {
            None
        };
        let mut context_params = CreateBrowserContextParams::new();
        context_params.dispose_on_detach = Some(true);
        if let Some(proxy) = &validating_proxy {
            context_params.proxy_server = Some(proxy.browser_address());
            context_params.proxy_bypass_list = Some("<-loopback>".to_owned());
        }
        let context = client
            .execute::<CreateBrowserContextCommand>(context_params, None)
            .await?;
        let browser_context_id = context.browser_context_id;
        let mut download_params = SetDownloadBehaviorParams::new("deny".to_owned());
        download_params.browser_context_id = Some(browser_context_id.clone());
        download_params.events_enabled = Some(true);
        if let Err(error) = client
            .execute::<SetDownloadBehaviorCommand>(download_params, None)
            .await
        {
            dispose_context(&client, &browser_context_id).await;
            return Err(error);
        }
        let mut target_params = CreateTargetParams::new("about:blank".to_owned());
        target_params.browser_context_id = Some(browser_context_id.clone());
        target_params.new_window = Some(true);
        target_params.background = Some(false);
        let target = match client
            .execute::<CreateTargetCommand>(target_params, None)
            .await
        {
            Ok(target) => target,
            Err(error) => {
                dispose_context(&client, &browser_context_id).await;
                return Err(error);
            }
        };
        let target_id = target.target_id;
        let attached = match client
            .command(
                "Target.attachToTarget",
                json!({"targetId": target_id, "flatten": true}),
                None,
            )
            .await
        {
            Ok(attached) => attached,
            Err(error) => {
                dispose_context(&client, &browser_context_id).await;
                return Err(error);
            }
        };
        let session_id = match required_string(&attached, "sessionId", "attach page target") {
            Ok(session_id) => session_id,
            Err(error) => {
                dispose_context(&client, &browser_context_id).await;
                return Err(error);
            }
        };

        let sessions = Arc::new(tokio::sync::RwLock::new(HashSet::from(
            [session_id.clone()],
        )));
        let targets = FrameTargetManager::start_with_limits(
            client.clone(),
            session_id.clone(),
            Arc::clone(&sessions),
            !client.is_owned_browser(),
            target_limits,
            install_collector,
        );
        let activity = NetworkActivity::start(client.clone(), Arc::clone(&sessions));
        let observed_resources =
            ObservedResources::start(client.clone(), Arc::clone(&sessions), resource_observation);
        let page = Self {
            client,
            environment: environment.clone(),
            browser_context_id,
            session_id,
            sessions,
            targets,
            activity,
            observed_resources,
            interception: Mutex::new(None),
            validating_proxy: Mutex::new(validating_proxy),
            resource_cancellation: CancellationToken::new(),
            closed: AtomicBool::new(false),
        };
        if let Err(error) = page.configure(environment, install_collector).await {
            page.targets.close().await;
            page.activity.close().await;
            page.observed_resources.close().await;
            dispose_context(&page.client, &page.browser_context_id).await;
            if let Some(proxy) = page.validating_proxy.lock().await.take() {
                proxy.close().await;
            }
            return Err(error);
        }
        Ok(page)
    }

    async fn configure(
        &self,
        environment: &BrowserEnvironment,
        install_collector: bool,
    ) -> Result<()> {
        let session = Some(self.session_id.as_str());
        for method in [
            "Page.enable",
            "Runtime.enable",
            "DOM.enable",
            "Log.enable",
            "Security.enable",
        ] {
            self.client.command(method, json!({}), session).await?;
        }
        self.client
            .command(
                "Network.enable",
                crate::targets::network_enable_parameters(),
                session,
            )
            .await?;
        self.client
            .command("Target.setAutoAttach", auto_attach_parameters(), session)
            .await?;
        self.client
            .command(
                "Page.addScriptToEvaluateOnNewDocument",
                json!({
                    "source": self.targets.containment_script(),
                    "runImmediately": true
                }),
                session,
            )
            .await?;
        self.client
            .command(
                "Page.setLifecycleEventsEnabled",
                json!({"enabled": true}),
                session,
            )
            .await?;
        self.client
            .command(
                "Emulation.setDeviceMetricsOverride",
                json!({
                    "width": environment.viewport.width,
                    "height": environment.viewport.height,
                    "deviceScaleFactor": environment.viewport.scale,
                    "mobile": false,
                }),
                session,
            )
            .await?;
        self.client
            .command(
                "Emulation.setTimezoneOverride",
                json!({"timezoneId": environment.timezone}),
                session,
            )
            .await?;
        self.client
            .command(
                "Emulation.setLocaleOverride",
                json!({"locale": environment.locale}),
                session,
            )
            .await?;
        self.client
            .command(
                "Emulation.setEmulatedMedia",
                crate::offline::emulated_media_parameters(
                    environment,
                    offprint_browser::RenderingMedia::Screen,
                ),
                session,
            )
            .await?;
        if let UserAgentPolicy::Override(user_agent) = &environment.user_agent {
            self.client
                .command(
                    "Network.setUserAgentOverride",
                    json!({"userAgent": user_agent}),
                    session,
                )
                .await?;
        }
        if install_collector {
            self.install_document_start_script(COLLECTOR_BUNDLE).await?;
        }
        Ok(())
    }

    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub async fn close(self) -> Result<()> {
        self.resource_cancellation.cancel();
        if let Some(interception) = self.interception.lock().await.take() {
            let _ignored = self
                .client
                .command("Fetch.disable", json!({}), Some(&self.session_id))
                .await;
            interception.close().await;
        }
        self.targets.close().await;
        self.activity.close().await;
        self.observed_resources.close().await;
        let result = self
            .client
            .execute::<DisposeBrowserContextCommand>(
                DisposeBrowserContextParams::new(self.browser_context_id.clone()),
                None,
            )
            .await
            .map(|_| ());
        if let Some(proxy) = self.validating_proxy.lock().await.take() {
            proxy.close().await;
        }
        if result.is_ok() {
            self.closed.store(true, Ordering::Release);
        }
        result
    }
}

impl Drop for ChromiumPage {
    fn drop(&mut self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        self.resource_cancellation.cancel();
        if let Ok(mut proxy) = self.validating_proxy.try_lock() {
            let _dropped = proxy.take();
        }
        let client = self.client.clone();
        let browser_context_id = self.browser_context_id.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let _cleanup = handle.spawn(async move {
                dispose_context(&client, &browser_context_id).await;
            });
        }
    }
}

async fn dispose_context(client: &CdpClient, browser_context_id: &str) {
    let _ignored = client
        .execute::<DisposeBrowserContextCommand>(
            DisposeBrowserContextParams::new(browser_context_id.to_owned()),
            None,
        )
        .await;
}

pub(super) fn required_string(value: &Value, field: &str, operation: &str) -> Result<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            OffprintError::new(
                "offprint.browser.cdp_shape",
                ErrorStage::Browser,
                format!("CDP response for `{operation}` has no `{field}` field"),
            )
        })
}
