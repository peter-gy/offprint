use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    ArtifactSpec, BrowserEnvironment, CaptureCredentials, CaptureId, CaptureLimits, CapturePolicy,
    CaptureRequest, CaptureResult, ContentDigest, DiagnosticsPolicy, NetworkPolicy, PageKnotError,
    PortablePath, ReadinessPolicy, VerificationPolicy,
};

fn default_concurrency() -> u16 {
    4
}

fn default_maximum_pages() -> u32 {
    100
}

fn default_maximum_depth() -> u16 {
    3
}

fn default_same_origin() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// One independently scheduled capture.
pub struct BatchJob {
    /// Stable identifier used in results and resume state.
    pub id: String,
    /// Complete one-page capture request.
    pub request: CaptureRequest,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// Persistence options for a batch or crawl.
pub struct ResumeOptions {
    /// JSON manifest updated after each terminal job.
    pub manifest: PortablePath,
    /// Schedules jobs whose previous terminal result was a failure.
    #[serde(default)]
    pub retry_failed: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// A bounded collection of independent one-page capture requests.
pub struct BatchRequest {
    pub jobs: Vec<BatchJob>,
    #[serde(default = "default_concurrency")]
    pub concurrency: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<ResumeOptions>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// A deterministic breadth-first crawl rooted at one capture request.
pub struct CrawlRequest {
    /// Capture policy and seed URL. The scheduler derives one file target per
    /// crawled URL inside `output_directory`.
    #[schemars(with = "LocalCaptureRequest")]
    pub seed: CaptureRequest,
    pub output_directory: PortablePath,
    #[serde(default = "default_maximum_pages")]
    pub maximum_pages: u32,
    #[serde(default = "default_maximum_depth")]
    pub maximum_depth: u16,
    #[serde(default = "default_concurrency")]
    pub concurrency: u16,
    #[serde(default = "default_same_origin")]
    pub same_origin: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<ResumeOptions>,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
struct LocalCaptureRequest {
    url: Url,
    artifact: ArtifactSpec,
    browser: LocalBrowserSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    headed: Option<bool>,
    environment: BrowserEnvironment,
    readiness: ReadinessPolicy,
    capture: CapturePolicy,
    #[serde(default, skip_serializing_if = "CaptureCredentials::is_empty")]
    credentials: CaptureCredentials,
    network: NetworkPolicy,
    limits: CaptureLimits,
    verification: VerificationPolicy,
    diagnostics: DiagnosticsPolicy,
}

#[allow(dead_code)]
#[derive(JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
enum LocalBrowserSpec {
    Auto,
    Executable(PortablePath),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScheduleKind {
    Batch,
    Crawl,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResumeJobStatus {
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// Compact terminal state persisted by a resumable scheduler.
pub struct ResumeJobRecord {
    pub request_sha256: ContentDigest,
    pub status: ResumeJobStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_id: Option<CaptureId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_sha256: Option<ContentDigest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<PortablePath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<PageKnotError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<Url>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// One pending crawl page in deterministic breadth-first order.
pub struct CrawlFrontierItem {
    pub url: Url,
    pub depth: u16,
    pub ordinal: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// Versioned scheduler checkpoint written through an atomic file transaction.
pub struct ResumeManifest {
    pub schema_version: u32,
    pub kind: ScheduleKind,
    pub plan_sha256: ContentDigest,
    #[serde(default)]
    pub jobs: BTreeMap<String, ResumeJobRecord>,
    #[serde(default)]
    pub pending: Vec<String>,
    #[serde(default)]
    pub frontier: Vec<CrawlFrontierItem>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
/// Terminal state for one scheduled capture.
pub enum ScheduledCaptureOutcome {
    Succeeded {
        id: String,
        request_sha256: ContentDigest,
        result: CaptureResult,
    },
    Failed {
        id: String,
        request_sha256: ContentDigest,
        error: PageKnotError,
    },
    Resumed {
        id: String,
        record: ResumeJobRecord,
    },
}

impl ScheduledCaptureOutcome {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Succeeded { id, .. } | Self::Failed { id, .. } | Self::Resumed { id, .. } => id,
        }
    }

    #[must_use]
    pub const fn succeeded(&self) -> bool {
        matches!(
            self,
            Self::Succeeded { .. }
                | Self::Resumed {
                    record: ResumeJobRecord {
                        status: ResumeJobStatus::Succeeded,
                        ..
                    },
                    ..
                }
        )
    }

    #[must_use]
    pub const fn resumed(&self) -> bool {
        matches!(self, Self::Resumed { .. })
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    pub schema_version: u32,
    pub plan_sha256: ContentDigest,
    pub outcomes: Vec<ScheduledCaptureOutcome>,
    pub succeeded: u32,
    pub failed: u32,
    pub resumed: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlPageOutcome {
    pub url: Url,
    pub depth: u16,
    pub ordinal: u32,
    pub capture: ScheduledCaptureOutcome,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrawlResult {
    pub schema_version: u32,
    pub plan_sha256: ContentDigest,
    pub outcomes: Vec<CrawlPageOutcome>,
    pub succeeded: u32,
    pub failed: u32,
    pub resumed: u32,
}
