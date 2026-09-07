use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ContentDigest, ErrorCode, FrameId, RedactedUrl, ResourceId};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExternalReason {
    Policy,
    UnsupportedScheme,
    ExplicitlyPreserved,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OmissionReason {
    NonRendering,
    Sanitized,
    Sensitive,
    Limit,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ResourceOutcome {
    Embedded {
        digest: ContentDigest,
        media_type: String,
        bytes: u64,
    },
    External {
        url: RedactedUrl,
        url_sha256: ContentDigest,
        reason: ExternalReason,
    },
    Omitted {
        reason: OmissionReason,
    },
    Failed {
        error: ResourceError,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResourceRetrievalSource {
    InlineData,
    ObservedResponse,
    BrowserContextFetch,
    OwningFrameRead,
    LocalFileRead,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceProvenance {
    pub source: ResourceRetrievalSource,
    pub final_url: RedactedUrl,
    pub final_url_sha256: ContentDigest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default)]
    pub redirects: Vec<RedactedUrl>,
    pub received_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRecord {
    pub id: ResourceId,
    pub frame_id: FrameId,
    pub requested_url: RedactedUrl,
    pub requested_url_sha256: ContentDigest,
    pub outcome: ResourceOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ResourceProvenance>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceSummary {
    pub discovered: u32,
    pub embedded: u32,
    pub external: u32,
    pub omitted: u32,
    pub failed: u32,
    pub embedded_bytes: u64,
}

impl ResourceSummary {
    #[must_use]
    pub const fn outcomes(self) -> u32 {
        self.embedded + self.external + self.omitted + self.failed
    }

    #[must_use]
    pub const fn is_complete(self) -> bool {
        self.discovered == self.outcomes()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureWarning {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<crate::FrameId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<crate::ResourceId>,
}

#[cfg(test)]
mod tests {
    use super::ResourceSummary;

    #[test]
    fn resource_summary_requires_one_outcome_per_reference() {
        let summary = ResourceSummary {
            discovered: 4,
            embedded: 2,
            external: 1,
            omitted: 1,
            failed: 0,
            embedded_bytes: 20,
        };

        assert!(summary.is_complete());
    }
}
