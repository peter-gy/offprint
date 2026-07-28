use std::time::Duration;

use pageknot_browser::OfflineBrowserObservation;
use pageknot_model::{ErrorStage, PageKnotError, ReadinessMode, Result};
use url::Url;

use super::ChromiumPage;
use crate::offline::OfflineVerifier;

impl ChromiumPage {
    pub async fn verify_offline_url(
        &self,
        url: &Url,
        deadline: Duration,
    ) -> Result<OfflineBrowserObservation> {
        if !self.client.is_owned_browser() {
            return Err(PageKnotError::new(
                "pageknot.browser.offline_verifier_unavailable",
                ErrorStage::Browser,
                "browser-backed offline verification requires a browser process owned by PageKnot",
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
        );
        let events = verifier.subscribe();
        verifier.block_network().await?;
        let remaining = expires.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(PageKnotError::new(
                "pageknot.verification.timeout",
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
