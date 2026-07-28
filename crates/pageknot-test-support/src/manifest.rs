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
    AdoptedStylesheet,
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
    FontLoading,
    FormState,
    FrameDetachment,
    HeaderValidation,
    LazyImages,
    MalformedDomNesting,
    MultipleOrigins,
    NavigationTimeout,
    OfflineReopen,
    OpenShadowRoot,
    OptimizationPolicy,
    OversizedPayload,
    PartialResponse,
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
    ProcessRecovers,
    ProcessRecycles,
    SecretsRedacted,
    TypedFailure,
    VisibleOutputPreserved,
    ZeroExternalRequests,
}

#[must_use]
pub fn fixture_manifest() -> FixtureManifest {
    FixtureManifest {
        schema_version: FIXTURE_MANIFEST_SCHEMA_VERSION,
        fixtures: fixture_definitions(),
    }
}

#[must_use]
pub fn fixture_definition(id: &str) -> Option<FixtureDefinition> {
    fixture_definitions()
        .into_iter()
        .find(|fixture| fixture.id == id)
}

fn fixture_definitions() -> Vec<FixtureDefinition> {
    vec![
        fixture(
            "static-article",
            FixtureGroup::Resources,
            "Captures an article with a stylesheet, font, and responsive image.",
            &[
                FixtureCapability::StaticArticle,
                FixtureCapability::FontLoading,
                FixtureCapability::ResponsiveImages,
            ],
            success(),
        ),
        fixture(
            "script-rendered-document",
            FixtureGroup::BrowserState,
            "Preserves nodes and text created by page scripts before collection.",
            &[FixtureCapability::ScriptRenderedDocument],
            state_success(),
        ),
        fixture(
            "dom-mutation-settling",
            FixtureGroup::BrowserState,
            "Waits for a bounded sequence of DOM mutations to become quiet.",
            &[FixtureCapability::DomMutationSettling],
            state_success(),
        ),
        fixture(
            "lazy-images",
            FixtureGroup::Resources,
            "Loads images reached by the configured viewport sweep.",
            &[FixtureCapability::LazyImages],
            success(),
        ),
        fixture(
            "responsive-images",
            FixtureGroup::Resources,
            "Embeds the image selected by Chromium for the capture viewport.",
            &[FixtureCapability::ResponsiveImages],
            success(),
        ),
        fixture(
            "open-shadow-root",
            FixtureGroup::BrowserState,
            "Materializes an observed open shadow root into the artifact.",
            &[FixtureCapability::OpenShadowRoot],
            state_success(),
        ),
        fixture(
            "closed-shadow-root",
            FixtureGroup::BrowserState,
            "Materializes a closed shadow root observed by the document-start hook.",
            &[FixtureCapability::ClosedShadowRoot],
            state_success(),
        ),
        fixture(
            "adopted-stylesheet",
            FixtureGroup::BrowserState,
            "Preserves rules from a constructable stylesheet adopted by a shadow root.",
            &[FixtureCapability::AdoptedStylesheet],
            state_success(),
        ),
        fixture(
            "same-origin-iframe",
            FixtureGroup::Frames,
            "Embeds a same-origin frame with its rendered state and resources.",
            &[FixtureCapability::SameOriginFrame],
            state_success(),
        ),
        fixture(
            "cross-origin-oopif",
            FixtureGroup::Frames,
            "Embeds an out-of-process frame through its attached CDP session.",
            &[FixtureCapability::CrossOriginFrame],
            state_success(),
        ),
        fixture(
            "srcdoc-frame",
            FixtureGroup::Frames,
            "Embeds a srcdoc frame and resolves its resource base against the parent.",
            &[FixtureCapability::SrcdocFrame],
            state_success(),
        ),
        fixture(
            "sandboxed-iframe",
            FixtureGroup::Frames,
            "Preserves rendered sandboxed-frame content under an opaque origin.",
            &[FixtureCapability::SandboxedFrame],
            state_success(),
        ),
        fixture(
            "canvas-2d",
            FixtureGroup::BrowserState,
            "Preserves the visible pixels of a two-dimensional canvas.",
            &[FixtureCapability::Canvas],
            state_success(),
        ),
        fixture(
            "tainted-canvas",
            FixtureGroup::BrowserState,
            "Uses a clipped browser screenshot when canvas pixels cannot be read.",
            &[FixtureCapability::TaintedCanvas],
            state_success(),
        ),
        fixture(
            "webgl-canvas",
            FixtureGroup::BrowserState,
            "Preserves pixels rendered by a WebGL context.",
            &[FixtureCapability::WebGlCanvas],
            state_success(),
        ),
        fixture(
            "form-state",
            FixtureGroup::BrowserState,
            "Preserves current input, selection, option, and disclosure state.",
            &[FixtureCapability::FormState],
            state_success(),
        ),
        fixture(
            "rendered-view-state",
            FixtureGroup::BrowserState,
            "Reopens scroll positions and paused animation state across light and shadow trees.",
            &[
                FixtureCapability::RenderedViewState,
                FixtureCapability::OpenShadowRoot,
                FixtureCapability::ClosedShadowRoot,
            ],
            state_success(),
        ),
        fixture(
            "selection-optimization",
            FixtureGroup::Artifact,
            "Preserves visible output while selection scope and output optimizations reshape the artifact.",
            &[
                FixtureCapability::SelectionScope,
                FixtureCapability::OptimizationPolicy,
            ],
            &[
                FixtureExpectation::CaptureSucceeds,
                FixtureExpectation::VisibleOutputPreserved,
                FixtureExpectation::EveryResourceHasOutcome,
                FixtureExpectation::ZeroExternalRequests,
            ],
        ),
        fixture(
            "css-imports",
            FixtureGroup::Resources,
            "Resolves nested CSS imports and their render-affecting resources.",
            &[FixtureCapability::CssImports],
            success(),
        ),
        fixture(
            "css-import-cycle",
            FixtureGroup::Resources,
            "Terminates a cyclic CSS import graph with one outcome per reference.",
            &[FixtureCapability::CssImportCycle],
            success(),
        ),
        fixture(
            "font-loading",
            FixtureGroup::Resources,
            "Waits for the document font set and embeds the selected font bytes.",
            &[FixtureCapability::FontLoading],
            success(),
        ),
        fixture(
            "blob-resource",
            FixtureGroup::Resources,
            "Preserves a render-affecting resource addressed by a blob URL.",
            &[FixtureCapability::BlobResource],
            success(),
        ),
        fixture(
            "data-resource",
            FixtureGroup::Resources,
            "Preserves an inline data URL without a network request.",
            &[FixtureCapability::DataResource],
            success(),
        ),
        fixture(
            "svg-resource-graph",
            FixtureGroup::Resources,
            "Embeds resources referenced from an SVG document.",
            &[FixtureCapability::SvgResourceGraph],
            success(),
        ),
        fixture(
            "video-poster",
            FixtureGroup::BrowserState,
            "Preserves a video poster and the selected media presentation state.",
            &[FixtureCapability::VideoPoster],
            state_success(),
        ),
        fixture(
            "malformed-dom-nesting",
            FixtureGroup::Artifact,
            "Repairs browser-owned DOM nesting that changes during HTML parsing.",
            &[FixtureCapability::MalformedDomNesting],
            state_success(),
        ),
        fixture(
            "permanent-network-activity",
            FixtureGroup::Network,
            "Settles while WebSocket and EventSource connections remain open.",
            &[
                FixtureCapability::PermanentNetworkActivity,
                FixtureCapability::WebSocket,
                FixtureCapability::EventSource,
            ],
            success(),
        ),
        fixture(
            "navigation-timeout",
            FixtureGroup::Lifecycle,
            "Returns a typed timeout when navigation exceeds its deadline.",
            &[
                FixtureCapability::NavigationTimeout,
                FixtureCapability::SlowResponse,
            ],
            typed_failure(),
        ),
        fixture(
            "browser-crash",
            FixtureGroup::Lifecycle,
            "Returns a typed crash and starts the next capture in a healthy browser.",
            &[FixtureCapability::BrowserCrash],
            &[
                FixtureExpectation::TypedFailure,
                FixtureExpectation::ProcessRecovers,
            ],
        ),
        fixture(
            "browser-job-recycling",
            FixtureGroup::Lifecycle,
            "Replaces the managed browser after the configured completed-job threshold.",
            &[FixtureCapability::BrowserJobRecycling],
            &[FixtureExpectation::ProcessRecycles],
        ),
        fixture(
            "frame-detachment",
            FixtureGroup::Frames,
            "Records a frame that detaches after attachment and before collection.",
            &[FixtureCapability::FrameDetachment],
            typed_failure(),
        ),
        FixtureDefinition {
            id: "cancellation-every-stage".to_owned(),
            group: FixtureGroup::Lifecycle,
            summary: "Cancels each pipeline stage and releases every owned resource.".to_owned(),
            required: true,
            capabilities: vec![FixtureCapability::StageCancellation],
            expectations: vec![
                FixtureExpectation::TypedFailure,
                FixtureExpectation::DestinationUnchanged,
            ],
            runner: runner("cancellation-every-stage"),
            variants: [
                "validating",
                "waiting-for-browser",
                "navigating",
                "settling",
                "collecting",
                "resolving-resources",
                "transforming",
                "encoding",
                "verifying",
                "committing",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        },
        fixture(
            "explicit-output-conflict",
            FixtureGroup::Artifact,
            "Leaves an existing destination unchanged when conflict policy is fail.",
            &[FixtureCapability::ExplicitOutputConflict],
            &[
                FixtureExpectation::TypedFailure,
                FixtureExpectation::DestinationUnchanged,
            ],
        ),
        fixture(
            "atomic-replacement",
            FixtureGroup::Artifact,
            "Commits a verified staging artifact as one destination replacement.",
            &[FixtureCapability::AtomicReplace],
            success(),
        ),
        fixture(
            "cli-committed-path",
            FixtureGroup::Artifact,
            "Writes the committed artifact path to command standard output.",
            &[
                FixtureCapability::CliCaptureOutput,
                FixtureCapability::AtomicReplace,
            ],
            &[
                FixtureExpectation::CaptureSucceeds,
                FixtureExpectation::CommittedPathReported,
            ],
        ),
        fixture(
            "cli-interrupted-capture",
            FixtureGroup::Lifecycle,
            "Cancels an interrupted command, preserves its destination, and redacts progress diagnostics.",
            &[
                FixtureCapability::CliInterrupt,
                FixtureCapability::DiagnosticRedaction,
                FixtureCapability::StageCancellation,
            ],
            &[
                FixtureExpectation::TypedFailure,
                FixtureExpectation::DestinationUnchanged,
                FixtureExpectation::SecretsRedacted,
            ],
        ),
        fixture(
            "network-denied-reopen",
            FixtureGroup::Artifact,
            "Reopens the saved artifact in a browser with network access denied.",
            &[
                FixtureCapability::OfflineReopen,
                FixtureCapability::ContentSecurityPolicy,
            ],
            success(),
        ),
        fixture(
            "redirect-chain",
            FixtureGroup::Network,
            "Validates each redirect destination and records the final response provenance.",
            &[FixtureCapability::Redirects],
            success(),
        ),
        fixture(
            "request-credentials",
            FixtureGroup::Network,
            "Applies scoped cookies and headers before the first navigation request.",
            &[
                FixtureCapability::Cookies,
                FixtureCapability::HeaderValidation,
                FixtureCapability::Authentication,
            ],
            success(),
        ),
        fixture(
            "referrer-sensitive-response",
            FixtureGroup::Network,
            "Captures the response variant selected by the browser referrer.",
            &[FixtureCapability::ReferrerSensitiveResponse],
            success(),
        ),
        fixture(
            "service-worker-resource",
            FixtureGroup::Network,
            "Captures a render-affecting response supplied by a service worker.",
            &[FixtureCapability::ServiceWorker],
            success(),
        ),
        fixture(
            "partial-response",
            FixtureGroup::Network,
            "Records a resource response that closes before its declared length.",
            &[FixtureCapability::PartialResponse],
            success(),
        ),
        fixture(
            "compressed-response",
            FixtureGroup::Network,
            "Captures a gzip-compressed resource through Chromium response handling.",
            &[FixtureCapability::Compression],
            success(),
        ),
        fixture(
            "oversized-payload",
            FixtureGroup::Lifecycle,
            "Stops before a payload exceeds the configured resource byte limit.",
            &[FixtureCapability::OversizedPayload],
            typed_failure(),
        ),
        fixture(
            "cache-variation",
            FixtureGroup::Network,
            "Keeps response variants distinct when request metadata changes their bytes.",
            &[FixtureCapability::CacheVariation],
            success(),
        ),
        fixture(
            "dns-address-policy",
            FixtureGroup::Network,
            "Applies address policy to resolved hosts and redirect destinations.",
            &[
                FixtureCapability::DnsAddressPolicy,
                FixtureCapability::MultipleOrigins,
            ],
            typed_failure(),
        ),
    ]
}

fn fixture(
    id: &str,
    group: FixtureGroup,
    summary: &str,
    capabilities: &[FixtureCapability],
    expectations: &[FixtureExpectation],
) -> FixtureDefinition {
    FixtureDefinition {
        id: id.to_owned(),
        group,
        summary: summary.to_owned(),
        required: true,
        capabilities: capabilities.to_vec(),
        expectations: expectations.to_vec(),
        runner: runner(id),
        variants: Vec::new(),
    }
}

fn runner(id: &str) -> FixtureRunner {
    let (package, test_target, filter, ignored) = match id {
        "static-article" | "font-loading" => (
            "pageknot",
            Some("fixture_matrix"),
            "static_article_with_font_round_trips",
            true,
        ),
        "script-rendered-document"
        | "dom-mutation-settling"
        | "lazy-images"
        | "responsive-images"
        | "open-shadow-root"
        | "closed-shadow-root"
        | "adopted-stylesheet"
        | "canvas-2d"
        | "webgl-canvas"
        | "form-state"
        | "blob-resource"
        | "data-resource"
        | "video-poster" => (
            "pageknot",
            Some("fixture_matrix"),
            "browser_state_fixture_round_trips",
            true,
        ),
        "same-origin-iframe" | "cross-origin-oopif" | "srcdoc-frame" | "sandboxed-iframe" => (
            "pageknot",
            Some("fixture_matrix"),
            "frame_fixture_round_trips",
            true,
        ),
        "tainted-canvas" => (
            "pageknot",
            Some("capture_regressions"),
            "tainted_canvas_uses_a_clipped_browser_fallback",
            true,
        ),
        "css-imports" | "css-import-cycle" | "svg-resource-graph" => (
            "pageknot",
            Some("fixture_matrix"),
            "resource_graph_fixture_round_trips",
            true,
        ),
        "malformed-dom-nesting" => (
            "pageknot",
            Some("capture_regressions"),
            "malformed_browser_dom_reopens_with_the_observed_tree",
            true,
        ),
        "permanent-network-activity" => (
            "pageknot",
            Some("fixture_matrix"),
            "permanent_connections_do_not_block_render_idle",
            true,
        ),
        "navigation-timeout" => (
            "pageknot",
            Some("fixture_matrix"),
            "navigation_deadline_returns_a_typed_timeout",
            true,
        ),
        "browser-crash" => (
            "pageknot",
            None,
            "runtime::tests::local_browser_restarts_after_its_process_tree_crashes",
            true,
        ),
        "browser-job-recycling" => (
            "pageknot",
            None,
            "runtime::tests::local_browser_recycles_after_the_configured_job_threshold",
            true,
        ),
        "frame-detachment" => (
            "pageknot",
            Some("capture_regressions"),
            "detached_oopif_returns_a_typed_frame_outcome",
            true,
        ),
        "cancellation-every-stage" => (
            "pageknot",
            Some("fixture_matrix"),
            "cancellation_rolls_back_every_pipeline_stage",
            true,
        ),
        "explicit-output-conflict" => (
            "pageknot",
            Some("fixture_matrix"),
            "explicit_output_conflict_preserves_the_destination",
            false,
        ),
        "atomic-replacement" => (
            "pageknot",
            Some("fixture_matrix"),
            "atomic_replacement_commits_the_verified_artifact",
            true,
        ),
        "rendered-view-state" => (
            "pageknot",
            Some("fixture_matrix"),
            "rendered_state_reopens_across_light_and_shadow_trees",
            true,
        ),
        "selection-optimization" => (
            "pageknot",
            Some("fixture_matrix"),
            "selection_and_optimizers_preserve_visible_output",
            true,
        ),
        "cli-committed-path" => (
            "pageknot-cli",
            None,
            "runner::tests::capture_writes_the_committed_path_to_stdout",
            true,
        ),
        "cli-interrupted-capture" => (
            "pageknot-cli",
            None,
            "runner::tests::interrupted_capture_preserves_the_existing_destination_and_redacts_progress",
            true,
        ),
        "network-denied-reopen" => (
            "pageknot",
            Some("fixture_matrix"),
            "browser_state_fixture_round_trips",
            true,
        ),
        "redirect-chain"
        | "request-credentials"
        | "referrer-sensitive-response"
        | "compressed-response" => (
            "pageknot",
            Some("fixture_matrix"),
            "request_policy_fixture_round_trips",
            true,
        ),
        "service-worker-resource" => (
            "pageknot",
            Some("fixture_matrix"),
            "service_worker_response_round_trips",
            true,
        ),
        "partial-response" => (
            "pageknot",
            Some("fixture_matrix"),
            "partial_resource_has_a_typed_outcome",
            true,
        ),
        "oversized-payload" => (
            "pageknot",
            Some("fixture_matrix"),
            "oversized_resource_fails_before_artifact_commit",
            true,
        ),
        "cache-variation" => (
            "pageknot",
            Some("capture_regressions"),
            "request_variants_keep_distinct_resource_bytes",
            true,
        ),
        "dns-address-policy" => (
            "pageknot",
            Some("capture_regressions"),
            "redirect_address_policy_rejects_a_disallowed_host",
            true,
        ),
        _ => (
            "pageknot",
            Some("fixture_matrix"),
            "browser_state_fixture_round_trips",
            true,
        ),
    };
    FixtureRunner {
        package: package.to_owned(),
        test_target: test_target.map(str::to_owned),
        filter: filter.to_owned(),
        ignored,
        serial: true,
    }
}

const fn success() -> &'static [FixtureExpectation] {
    &[
        FixtureExpectation::CaptureSucceeds,
        FixtureExpectation::EveryResourceHasOutcome,
        FixtureExpectation::ZeroExternalRequests,
    ]
}

const fn state_success() -> &'static [FixtureExpectation] {
    &[
        FixtureExpectation::CaptureSucceeds,
        FixtureExpectation::ArtifactStateRoundTrips,
        FixtureExpectation::EveryResourceHasOutcome,
        FixtureExpectation::ZeroExternalRequests,
    ]
}

const fn typed_failure() -> &'static [FixtureExpectation] {
    &[FixtureExpectation::TypedFailure]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        FIXTURE_MANIFEST_SCHEMA_VERSION, FixtureCapability, FixtureExpectation, fixture_definition,
        fixture_manifest,
    };

    #[test]
    fn fixture_ids_are_unique_and_machine_safe() {
        let manifest = fixture_manifest();
        let ids = manifest
            .fixtures
            .iter()
            .map(|fixture| fixture.id.as_str())
            .collect::<BTreeSet<_>>();

        assert_eq!(manifest.schema_version, FIXTURE_MANIFEST_SCHEMA_VERSION);
        assert_eq!(ids.len(), manifest.fixtures.len());
        assert!(ids.iter().all(|id| {
            !id.is_empty()
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        }));
    }

    #[test]
    fn each_required_fixture_has_an_observable_expectation() {
        let manifest = fixture_manifest();

        assert!(manifest.fixtures.iter().all(|fixture| {
            !fixture.summary.is_empty()
                && !fixture.capabilities.is_empty()
                && !fixture.expectations.is_empty()
        }));
        assert!(manifest.fixtures.iter().all(|fixture| fixture.required));
    }

    #[test]
    fn cancellation_fixture_names_every_pipeline_stage() {
        let fixture = fixture_definition("cancellation-every-stage");

        assert_eq!(
            fixture.as_ref().map(|fixture| fixture.variants.len()),
            Some(10)
        );
        assert!(fixture.is_some_and(|fixture| {
            fixture
                .capabilities
                .contains(&FixtureCapability::StageCancellation)
                && fixture
                    .expectations
                    .contains(&FixtureExpectation::DestinationUnchanged)
        }));
    }

    #[test]
    fn fixture_catalog_covers_network_and_browser_state_boundaries() {
        let manifest = fixture_manifest();
        let capabilities = manifest
            .fixtures
            .iter()
            .flat_map(|fixture| fixture.capabilities.iter().copied())
            .collect::<BTreeSet<_>>();

        assert!(capabilities.contains(&FixtureCapability::MultipleOrigins));
        assert!(capabilities.contains(&FixtureCapability::ClosedShadowRoot));
        assert!(capabilities.contains(&FixtureCapability::CrossOriginFrame));
        assert!(capabilities.contains(&FixtureCapability::OfflineReopen));
        assert!(capabilities.contains(&FixtureCapability::BrowserCrash));
    }
}
