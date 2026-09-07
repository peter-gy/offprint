use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    ArtifactFormat, CaptureArtifact, CaptureId, CaptureWarning, ContentDigest, FormatVerification,
    Milliseconds, ResourceSummary, SourceSummary, VerificationMode,
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
pub struct VerificationReport {
    pub schema_version: u32,
    pub mode: VerificationMode,
    pub artifact_sha256: ContentDigest,
    pub bytes: u64,
    pub network_requests: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationMethod {
    Static,
    Offline,
    FormatSpecific,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactVerification {
    pub schema_version: u32,
    pub format: ArtifactFormat,
    pub method: VerificationMethod,
    pub bytes: u64,
    pub sha256: ContentDigest,
    pub network_requests: u32,
}

impl ArtifactVerification {
    #[must_use]
    pub fn html(report: VerificationReport) -> Self {
        Self {
            schema_version: report.schema_version,
            format: ArtifactFormat::Html,
            method: match report.mode {
                VerificationMode::Static => VerificationMethod::Static,
                VerificationMode::Offline => VerificationMethod::Offline,
            },
            bytes: report.bytes,
            sha256: report.artifact_sha256,
            network_requests: report.network_requests,
        }
    }

    #[must_use]
    pub fn format(report: FormatVerification) -> Self {
        Self {
            schema_version: crate::PUBLIC_SCHEMA_VERSION,
            format: report.format,
            method: VerificationMethod::FormatSpecific,
            bytes: report.bytes,
            sha256: report.sha256,
            network_requests: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTimings {
    pub total: Milliseconds,
    pub validation: Milliseconds,
    pub browser: Milliseconds,
    pub navigation: Milliseconds,
    pub readiness: Milliseconds,
    pub collection: Milliseconds,
    pub resources: Milliseconds,
    pub transform: Milliseconds,
    pub encoding: Milliseconds,
    pub verification: Milliseconds,
    pub commit: Milliseconds,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureReceipt {
    pub schema_version: u32,
    pub capture_id: CaptureId,
    pub source: SourceSummary,
    pub artifact: CaptureArtifact,
    pub verification: VerificationReport,
    pub resources: ResourceSummary,
    #[serde(default)]
    pub warnings: Vec<CaptureWarning>,
    pub timings: CaptureTimings,
}
