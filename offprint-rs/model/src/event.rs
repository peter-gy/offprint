use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    BrowserInfo, CaptureId, CaptureTerminalStatus, CaptureWarning, FrameId, Milliseconds,
    OffprintError, ResourceId, VerificationReport,
};

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "camelCase")]
pub enum CaptureStatus {
    Created,
    Validating,
    WaitingForBrowser,
    Navigating,
    WaitingForReadiness,
    Collecting,
    ResolvingResources,
    Transforming,
    Encoding,
    Verifying,
    Committing,
    Cancelling,
    Succeeded,
    Cancelled,
    Failed,
}

impl CaptureStatus {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Cancelled | Self::Failed)
    }

    #[must_use]
    pub const fn terminal(self) -> Option<CaptureTerminalStatus> {
        match self {
            Self::Succeeded => Some(CaptureTerminalStatus::Succeeded),
            Self::Cancelled => Some(CaptureTerminalStatus::Cancelled),
            Self::Failed => Some(CaptureTerminalStatus::Failed),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum CaptureEvent {
    #[serde(rename = "capture.started")]
    CaptureStarted { capture_id: CaptureId },
    #[serde(rename = "browser.ready")]
    BrowserReady {
        capture_id: CaptureId,
        browser: BrowserInfo,
    },
    #[serde(rename = "navigation.started")]
    NavigationStarted {
        capture_id: CaptureId,
        url: crate::RedactedUrl,
    },
    #[serde(rename = "navigation.redirected")]
    NavigationRedirected {
        capture_id: CaptureId,
        from: crate::RedactedUrl,
        to: crate::RedactedUrl,
        status: u16,
    },
    #[serde(rename = "readiness.changed")]
    ReadinessChanged {
        capture_id: CaptureId,
        milestone: String,
        elapsed: Milliseconds,
    },
    #[serde(rename = "frame.collected")]
    FrameCollected {
        capture_id: CaptureId,
        frame_id: FrameId,
        nodes: u64,
    },
    #[serde(rename = "resource.discovered")]
    ResourceDiscovered {
        capture_id: CaptureId,
        resource_id: ResourceId,
    },
    #[serde(rename = "resource.progress")]
    ResourceProgress {
        capture_id: CaptureId,
        completed: u32,
        discovered: u32,
        bytes: u64,
    },
    #[serde(rename = "transform.started")]
    TransformStarted { capture_id: CaptureId },
    #[serde(rename = "artifact.encoding")]
    ArtifactEncoding { capture_id: CaptureId, bytes: u64 },
    #[serde(rename = "verification.started")]
    VerificationStarted { capture_id: CaptureId },
    #[serde(rename = "warning")]
    Warning {
        capture_id: CaptureId,
        warning: CaptureWarning,
    },
    #[serde(rename = "capture.succeeded")]
    CaptureSucceeded {
        capture_id: CaptureId,
        verification: VerificationReport,
    },
    #[serde(rename = "capture.failed")]
    CaptureFailed {
        capture_id: CaptureId,
        error: OffprintError,
    },
    #[serde(rename = "capture.cancelled")]
    CaptureCancelled { capture_id: CaptureId },
}

impl CaptureEvent {
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::CaptureSucceeded { .. }
                | Self::CaptureFailed { .. }
                | Self::CaptureCancelled { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::CaptureEvent;
    use crate::CaptureId;

    #[test]
    fn event_tag_uses_stable_protocol_spelling() {
        let capture_id = CaptureId::new();
        let event = CaptureEvent::CaptureCancelled {
            capture_id: capture_id.clone(),
        };
        let json = serde_json::to_value(&event);
        let expected_capture_id = capture_id.to_string();

        assert_eq!(
            json.as_ref().ok().and_then(|value| value["type"].as_str()),
            Some("capture.cancelled")
        );
        assert_eq!(
            json.as_ref()
                .ok()
                .and_then(|value| value["captureId"].as_str()),
            Some(expected_capture_id.as_str())
        );
        assert!(
            json.as_ref()
                .is_ok_and(|value| value.get("capture_id").is_none())
        );
    }
}
