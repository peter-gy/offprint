use std::time::Duration;

use offprint_browser::{OfflineBrowserObservation, RenderingMedia};
use offprint_model::{ErrorStage, OffprintError, ReadinessMode, Result};
use url::Url;

use super::ChromiumPage;
use crate::offline::OfflineVerifier;

impl ChromiumPage {
    pub async fn verify_offline_url(
        &self,
        url: &Url,
        deadline: Duration,
        media: RenderingMedia,
    ) -> Result<OfflineBrowserObservation> {
        if !self.client.is_owned_browser() {
            return Err(OffprintError::new(
                "offprint.browser.offline_verifier_unavailable",
                ErrorStage::Browser,
                "browser-backed offline verification requires a browser process owned by Offprint",
            ));
        }
        if let Some(proxy) = self.validating_proxy.lock().await.as_ref() {
            proxy.deny_all().await;
        }
        let expires = tokio::time::Instant::now() + deadline;
        let verifier = OfflineVerifier::new(
            &self.client,
            &self.session_id,
            &self.sessions,
            &self.targets,
            &self.environment,
            media,
        );
        let events = verifier.subscribe();
        verifier.block_network().await?;
        self.client
            .command_with_timeout(
                "Emulation.setEmulatedMedia",
                crate::offline::emulated_media_parameters(&self.environment, media),
                Some(&self.session_id),
                expires.saturating_duration_since(tokio::time::Instant::now()),
            )
            .await?;
        let remaining = expires.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(OffprintError::new(
                "offprint.verification.timeout",
                ErrorStage::Verification,
                "offline verification exceeded its deadline before navigation",
            ));
        }
        self.navigate(url, ReadinessMode::Load, 0, remaining)
            .await?;
        let (stable, failures) = verifier.wait_for_stability(expires).await?;
        verifier.fence_events(expires).await?;
        verifier.finish(events, stable, failures).await
    }
}
