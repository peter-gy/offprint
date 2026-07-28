use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ArtifactResult, CaptureId, CaptureWarning, ContentDigest, Milliseconds, ResourceSummary,
    SourceSummary, VerificationPolicy,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureTerminalStatus {
    Succeeded,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationResult {
    pub schema_version: u32,
    pub level: VerificationPolicy,
    pub passed: bool,
    pub artifact_sha256: ContentDigest,
    pub bytes: u64,
    pub network_requests: u32,
    #[serde(default)]
    pub attempted_urls: Vec<crate::RedactedUrl>,
    #[serde(default)]
    pub page_errors: Vec<String>,
    #[serde(default)]
    pub frame_failures: Vec<crate::FrameId>,
    pub stable: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTimings {
    pub total: Milliseconds,
    pub validation: Milliseconds,
    pub browser: Milliseconds,
    pub navigation: Milliseconds,
    pub settle: Milliseconds,
    pub collection: Milliseconds,
    pub resources: Milliseconds,
    pub transform: Milliseconds,
    pub encoding: Milliseconds,
    pub verification: Milliseconds,
    pub commit: Milliseconds,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResult {
    pub schema_version: u32,
    pub capture_id: CaptureId,
    pub status: CaptureTerminalStatus,
    pub source: SourceSummary,
    pub artifact: ArtifactResult,
    pub verification: VerificationResult,
    pub resources: ResourceSummary,
    #[serde(default)]
    pub warnings: Vec<CaptureWarning>,
    pub timings: CaptureTimings,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactVerificationRecord {
    pub artifact_sha256: ContentDigest,
    pub result: VerificationResult,
}
