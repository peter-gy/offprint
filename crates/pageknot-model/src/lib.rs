//! Canonical request, event, result, error, browser, and artifact records.
//!
//! These binding-friendly records define PageKnot's serialized contract.
//! Serde field names and enum values are versioned through
//! [`PUBLIC_SCHEMA_VERSION`]. The `schemas` workspace directory is generated
//! from these Rust types.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

mod artifact;
mod browser;
mod credentials;
mod error;
mod event;
mod export;
mod id;
mod path;
mod policy;
mod request;
mod resource;
mod result;
mod schedule;
mod structural;
mod url_value;

pub use artifact::{
    ArtifactFormat, ArtifactInput, ArtifactKind, ArtifactManifest, ArtifactResult, ArtifactSpec,
    ArtifactTarget, HtmlArtifactSpec, ManifestGenerator, ManifestSource, ManifestVerification,
    StructuralRepair, ViewState,
};
pub use browser::{
    BrowserAction, BrowserCandidate, BrowserCandidateState, BrowserChannel, BrowserDoctorReport,
    BrowserInfo, BrowserInstallRequest, BrowserInstallationPolicy, BrowserOperationResult,
    BrowserProduct, BrowserSource, CapabilityCheck, ConfigProvenance, EffectiveConfigValue,
    ManagedBrowserState, NetworkPolicySummary, OutputCapability, RecoveryAction,
};
pub use credentials::{
    BrowserCookie, CaptureCredentials, CookieSameSite, RequestHeader, SecretString,
};
pub use error::{
    ERROR_CODE_REGISTRY, ErrorCode, ErrorCodeDefinition, ErrorStage, PageKnotError, Result,
};
pub use event::{CaptureEvent, CaptureStatus};
pub use export::{
    ArtifactExportRequest, ArtifactExportResult, ArtifactVariant, ArtifactVariantKind,
    ArtifactVariantVerification, ExportedArtifact, MarkdownOptions, PdfOptions,
};
pub use id::{CaptureId, ContentDigest, FrameId, NodeId, ResourceId};
pub use path::PortablePath;
pub use policy::{
    BrowserEnvironment, CaptureLimits, CapturePolicy, CaptureProfile, CaptureScope, ColorScheme,
    ConflictPolicy, DiagnosticsPolicy, LazyLoadPolicy, MAXIMUM_CAPTURE_NODES, Milliseconds,
    MissingResourcePolicy, NetworkPolicy, NetworkRules, OptimizationPolicy, ReadinessMode,
    ReadinessPolicy, ReducedMotion, UserAgentPolicy, VerificationPolicy, Viewport,
    ViewportSweepPolicy,
};
pub use request::{BrowserSpec, CaptureRequest, CaptureRequestBuilder};
pub use resource::{
    CaptureWarning, ExternalReason, OmissionReason, ResourceError, ResourceOutcome,
    ResourceProvenance, ResourceRecord, ResourceRetrievalSource, ResourceSummary,
};
pub use result::{
    ArtifactVerificationRecord, CaptureResult, CaptureTerminalStatus, CaptureTimings,
    VerificationResult,
};
pub use schedule::{
    BatchJob, BatchRequest, BatchResult, CrawlFrontierItem, CrawlPageOutcome, CrawlRequest,
    CrawlResult, ResumeJobRecord, ResumeJobStatus, ResumeManifest, ResumeOptions, ScheduleKind,
    ScheduledCaptureOutcome,
};
pub use structural::{RepairNode, StructuralRepairTree};
pub use url_value::{RedactedUrl, RedactionPolicy, SourceSummary};

pub const PUBLIC_SCHEMA_VERSION: u32 = 1;
pub const ARTIFACT_FORMAT_VERSION: u32 = 1;
