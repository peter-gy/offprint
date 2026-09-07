use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const FIXTURE_MANIFEST_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixtureManifest {
    pub schema_version: u32,
    pub fixtures: Vec<FixtureDefinition>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixtureDefinition {
    pub id: String,
    pub group: FixtureGroup,
    pub summary: String,
    pub required: bool,
    pub capabilities: Vec<FixtureCapability>,
    pub expectations: Vec<FixtureExpectation>,
    pub runner: FixtureRunner,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixtureRunner {
    pub package: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_target: Option<String>,
    pub filter: String,
    pub ignored: bool,
    pub serial: bool,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureGroup {
    Artifact,
    BrowserState,
    Frames,
    Lifecycle,
    Network,
    Resources,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureCapability {
    AdoptedFontFace,
    AdoptedStylesheet,
    AdoptedStylesheetCascade,
    AtomicReplace,
    Authentication,
    BlobResource,
    BrowserCrash,
    BrowserJobRecycling,
    CacheVariation,
    Canvas,
    ClosedShadowRoot,
    CliCaptureOutput,
    CliInterrupt,
    CliJsonOutput,
    Compression,
    ContentSecurityPolicy,
    Cookies,
    CrossOriginFrame,
    CssImportCycle,
    CssImports,
    DataResource,
    DiagnosticRedaction,
    DnsAddressPolicy,
    DomMutationSettling,
    EventSource,
    ExplicitOutputConflict,
    ExternalStylesheetResources,
    FontLoading,
    FormState,
    FrameDetachment,
    HeaderValidation,
    LazyImages,
    MalformedDomNesting,
    MissingResourceDeduplication,
    MultipleOrigins,
    NavigationTimeout,
    NetworkIdleDeadline,
    OfflineReopen,
    OpenShadowRoot,
    OptimizationPolicy,
    OversizedPayload,
    PartialResponse,
    PdfOutput,
    PdfPagination,
    PermanentNetworkActivity,
    Redirects,
    ReferrerSensitiveResponse,
    RenderedViewState,
    ResponsiveImages,
    SameOriginFrame,
    SandboxedFrame,
    ScriptRenderedDocument,
    ServiceWorker,
    SelectionScope,
    SlowResponse,
    SrcdocFrame,
    StageCancellation,
    StaticArticle,
    SvgResourceGraph,
    TaintedCanvas,
    VideoPoster,
    WebGlCanvas,
    WebSocket,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixtureExpectation {
    ArtifactStateRoundTrips,
    CaptureSucceeds,
    CommittedPathReported,
    DestinationUnchanged,
    EveryResourceHasOutcome,
    HeadingWithContent,
    ProcessRecovers,
    ProcessRecycles,
    RepresentationVerified,
    SecretsRedacted,
    TypedFailure,
    VisibleOutputPreserved,
    ZeroExternalRequests,
}
