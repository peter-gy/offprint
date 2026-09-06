use std::time::Duration;

use offprint_browser::{NavigationRedirect, NavigationResult};
use offprint_model::{ErrorStage, OffprintError, ReadinessMode, Result};
use serde_json::{Value, json};
use tokio::sync::broadcast;
use tokio::time::{sleep, timeout};
use url::Url;

use super::lifecycle::required_string;
use super::{ChromiumPage, PAGE_COMMAND_TIMEOUT};

const READINESS_PROBE_INTERVAL: Duration = Duration::from_millis(100);
const READINESS_PROBE_TIMEOUT: Duration = Duration::from_millis(500);

impl ChromiumPage {
    pub async fn navigate(
        &self,
        url: &Url,
        readiness: ReadinessMode,
        redirect_limit: u32,
        deadline: Duration,
    ) -> Result<NavigationResult> {
        if let Some(proxy) = self.validating_proxy.lock().await.as_ref() {
            proxy.ensure_standard_guard(url).await?;
        }
        let mut events = self.client.subscribe_navigation();
        let response = self
            .client
            .command_with_timeout(
                "Page.navigate",
                json!({"url": url.as_str()}),
                Some(&self.session_id),
                deadline.min(PAGE_COMMAND_TIMEOUT),
            )
            .await?;
        if let Some(error_text) = response.get("errorText").and_then(Value::as_str) {
            if error_text == "net::ERR_BLOCKED_BY_CLIENT" {
                return Err(OffprintError::new(
                    "offprint.navigation.address_blocked",
                    ErrorStage::Navigation,
                    "network policy blocked the navigation destination",
                ));
            }
            return Err(OffprintError::new(
                "offprint.navigation.failed",
                ErrorStage::Navigation,
                format!("navigation failed: {error_text}"),
            )
            .retryable(true));
        }
        let frame_id = required_string(&response, "frameId", "navigate")?;
        let loader_id = response
            .get("loaderId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let milestone = match readiness {
            ReadinessMode::NetworkIdle | ReadinessMode::DomContentLoaded => {
                "Page.domContentEventFired"
            }
            ReadinessMode::RenderIdle | ReadinessMode::Load => "Page.loadEventFired",
        };
        let mut final_url = url.clone();
        let mut redirects = Vec::new();
        let mut main_request_id = None;
        if loader_id.is_none() {
            if let Some(error) = self.interception_error(&frame_id, false).await {
                return Err(error);
            }
            return Ok(NavigationResult {
                frame_id,
                final_url,
                redirects,
            });
        }
        let readiness_probe = self.wait_for_document_readiness(readiness);
        tokio::pin!(readiness_probe);
        timeout(deadline, async {
            loop {
                let event = tokio::select! {
                    result = &mut readiness_probe => {
                        result?;
                        if let Some(error) = self.interception_error(&frame_id, false).await {
                            return Err(error);
                        }
                        return Ok(());
                    }
                    event = events.recv() => event.map_err(|error| match error {
                        broadcast::error::RecvError::Lagged(skipped) => OffprintError::new(
                            "offprint.browser.cdp_event_lag",
                            ErrorStage::Navigation,
                            format!("navigation event consumer fell behind by {skipped} events"),
                        )
                        .retryable(true),
                        broadcast::error::RecvError::Closed => OffprintError::new(
                            "offprint.browser.cdp_closed",
                            ErrorStage::Navigation,
                            "the CDP connection closed during navigation",
                        )
                        .retryable(true),
                    })?,
                };
                if event.session_id.as_deref() != Some(&self.session_id) {
                    continue;
                }
                match event.method.as_ref() {
                    "Network.requestWillBeSent" => {
                        let event_loader = event.params.get("loaderId").and_then(Value::as_str);
                        let event_frame = event.params.get("frameId").and_then(Value::as_str);
                        let resource_type = event.params.get("type").and_then(Value::as_str);
                        if event_frame != Some(frame_id.as_str())
                            || resource_type != Some("Document")
                            || loader_id
                                .as_deref()
                                .zip(event_loader)
                                .is_some_and(|(expected, actual)| expected != actual)
                        {
                            continue;
                        }
                        main_request_id = event
                            .params
                            .get("requestId")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                        let Some(next_url) = event
                            .params
                            .pointer("/request/url")
                            .and_then(Value::as_str)
                            .and_then(|value| Url::parse(value).ok())
                        else {
                            continue;
                        };
                        if let Some(redirect) = event.params.get("redirectResponse") {
                            let status = redirect
                                .get("status")
                                .and_then(Value::as_f64)
                                .and_then(|value| {
                                    if value.fract() == 0.0 {
                                        u16::try_from(value as u64).ok()
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0);
                            redirects.push(NavigationRedirect {
                                from: final_url.clone(),
                                to: next_url.clone(),
                                status,
                            });
                            if redirects.len()
                                > usize::try_from(redirect_limit).unwrap_or(usize::MAX)
                            {
                                let _ignored = self
                                    .client
                                    .command("Page.stopLoading", json!({}), Some(&self.session_id))
                                    .await;
                                return Err(OffprintError::new(
                                    "offprint.navigation.redirect_limit",
                                    ErrorStage::Navigation,
                                    "navigation exceeded the configured redirect limit",
                                ));
                            }
                        }
                        final_url = next_url;
                    }
                    "Network.loadingFailed"
                        if event
                            .params
                            .get("requestId")
                            .and_then(Value::as_str)
                            .zip(main_request_id.as_deref())
                            .is_some_and(|(failed, main)| failed == main) =>
                    {
                        if let Some(error) = self.interception_error(&frame_id, true).await {
                            return Err(error);
                        }
                        let reason = event
                            .params
                            .get("errorText")
                            .and_then(Value::as_str)
                            .unwrap_or("main document request failed");
                        if reason == "net::ERR_BLOCKED_BY_CLIENT" {
                            return Err(OffprintError::new(
                                "offprint.navigation.address_blocked",
                                ErrorStage::Navigation,
                                "network policy blocked the navigation destination",
                            ));
                        }
                        return Err(OffprintError::new(
                            "offprint.navigation.failed",
                            ErrorStage::Navigation,
                            format!("navigation failed: {reason}"),
                        )
                        .retryable(true));
                    }
                    "Inspector.targetCrashed" => {
                        return Err(OffprintError::new(
                            "offprint.browser.target_crashed",
                            ErrorStage::Navigation,
                            "Chromium renderer crashed during navigation",
                        )
                        .retryable(true));
                    }
                    method if method == milestone => {
                        if let Some(error) = self.interception_error(&frame_id, false).await {
                            return Err(error);
                        }
                        return Ok(());
                    }
                    _ => {}
                }
            }
        })
        .await
        .map_err(|_| {
            OffprintError::new(
                "offprint.readiness.timeout",
                ErrorStage::Readiness,
                format!("timed out waiting for `{milestone}`"),
            )
            .retryable(true)
        })??;
        Ok(NavigationResult {
            frame_id,
            final_url,
            redirects,
        })
    }

    async fn wait_for_document_readiness(&self, readiness: ReadinessMode) -> Result<()> {
        let expression = match readiness {
            ReadinessMode::NetworkIdle | ReadinessMode::DomContentLoaded => {
                "location.href !== 'about:blank' && document.readyState !== 'loading'"
            }
            ReadinessMode::RenderIdle | ReadinessMode::Load => {
                "location.href !== 'about:blank' && document.readyState === 'complete'"
            }
        };
        loop {
            sleep(READINESS_PROBE_INTERVAL).await;
            let reached = match self
                .client
                .command_with_timeout(
                    "Runtime.evaluate",
                    json!({
                        "expression": expression,
                        "returnByValue": true,
                        "awaitPromise": false,
                        "userGesture": false,
                    }),
                    Some(&self.session_id),
                    READINESS_PROBE_TIMEOUT,
                )
                .await
            {
                Ok(response) => response
                    .pointer("/result/value")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                Err(error)
                    if matches!(
                        error.code.as_str(),
                        "offprint.browser.cdp_timeout" | "offprint.browser.cdp_command"
                    ) =>
                {
                    false
                }
                Err(error) => {
                    return Err(error.with_detail("readinessStep", "document-readiness-probe"));
                }
            };
            if reached {
                return Ok(());
            }
        }
    }
}
