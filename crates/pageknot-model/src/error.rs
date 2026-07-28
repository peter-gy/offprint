use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

pub type Result<T, E = PageKnotError> = std::result::Result<T, E>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ErrorCodeDefinition {
    pub code: &'static str,
    pub stage: ErrorStage,
    pub retryable: bool,
    pub description: &'static str,
}

pub const ERROR_CODE_REGISTRY: &[ErrorCodeDefinition] = &[
    ErrorCodeDefinition {
        code: "pageknot.artifact.head",
        stage: ErrorStage::Encoding,
        retryable: false,
        description: "The captured document has no HTML head element.",
    },
    ErrorCodeDefinition {
        code: "pageknot.artifact.manifest",
        stage: ErrorStage::Encoding,
        retryable: false,
        description: "The artifact manifest could not be serialized.",
    },
    ErrorCodeDefinition {
        code: "pageknot.artifact.policy",
        stage: ErrorStage::Encoding,
        retryable: false,
        description: "The artifact policy is inconsistent with the captured document.",
    },
    ErrorCodeDefinition {
        code: "pageknot.artifact.read",
        stage: ErrorStage::Verification,
        retryable: true,
        description: "The artifact could not be read.",
    },
    ErrorCodeDefinition {
        code: "pageknot.artifact.resource_summary",
        stage: ErrorStage::Encoding,
        retryable: false,
        description: "The artifact resource summary is incomplete.",
    },
    ErrorCodeDefinition {
        code: "pageknot.artifact.serialize",
        stage: ErrorStage::Encoding,
        retryable: false,
        description: "The HTML artifact could not be serialized.",
    },
    ErrorCodeDefinition {
        code: "pageknot.artifact.size",
        stage: ErrorStage::Encoding,
        retryable: false,
        description: "The artifact exceeds a configured or platform byte limit.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.active",
        stage: ErrorStage::Browser,
        retryable: true,
        description: "The selected managed browser has active leases or requires replacement.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.containment",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "The owned browser process tree could not be contained.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.executable",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "The selected browser executable is unavailable.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.cdp_close_timeout",
        stage: ErrorStage::Browser,
        retryable: true,
        description: "The CDP transport did not close before its deadline.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.cdp_closed",
        stage: ErrorStage::Browser,
        retryable: true,
        description: "The CDP connection closed.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.cdp_command",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "Chromium rejected a CDP command.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.cdp_connect",
        stage: ErrorStage::Browser,
        retryable: true,
        description: "PageKnot could not connect to a CDP endpoint.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.cdp_decode",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "A CDP message could not be decoded.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.cdp_encode",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "A CDP command could not be encoded.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.cdp_event_lag",
        stage: ErrorStage::Browser,
        retryable: true,
        description: "A CDP event consumer exceeded its bounded buffer.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.cdp_shape",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "A CDP response did not match the pinned protocol shape.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.cdp_task",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "The CDP transport task failed.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.cdp_timeout",
        stage: ErrorStage::Browser,
        retryable: true,
        description: "A CDP command exceeded its deadline.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.cdp_transport",
        stage: ErrorStage::Browser,
        retryable: true,
        description: "The CDP WebSocket transport failed.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.devtools_port",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "Chromium published an invalid DevTools port.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.devtools_url",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "Chromium published an invalid DevTools endpoint.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.incompatible",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "The selected browser is incompatible with PageKnot.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.install",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "A managed browser installation needs attention.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.launch",
        stage: ErrorStage::Browser,
        retryable: true,
        description: "Chromium could not be launched.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.launch_timeout",
        stage: ErrorStage::Browser,
        retryable: true,
        description: "Chromium did not start before its deadline.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.management_unavailable",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "The requested managed browser operation is unavailable.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.offline_verifier_unavailable",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "Browser-backed offline verification is unavailable.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.pdf_unavailable",
        stage: ErrorStage::Encoding,
        retryable: false,
        description: "The selected browser backend cannot render PDF output.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.probe",
        stage: ErrorStage::Browser,
        retryable: true,
        description: "The browser executable probe failed.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.probe_timeout",
        stage: ErrorStage::Browser,
        retryable: true,
        description: "The browser executable probe exceeded its deadline.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.profile",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "An ephemeral browser profile could not be created.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.selection_conflict",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "The browser selection options request incompatible ownership modes.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.shutdown",
        stage: ErrorStage::Shutdown,
        retryable: true,
        description: "Chromium could not be stopped cleanly.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.target_crashed",
        stage: ErrorStage::Navigation,
        retryable: true,
        description: "The Chromium target crashed during an active operation.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.terminate",
        stage: ErrorStage::Shutdown,
        retryable: true,
        description: "The owned Chromium process tree could not be terminated.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.unavailable",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "No compatible browser is available.",
    },
    ErrorCodeDefinition {
        code: "pageknot.browser.version",
        stage: ErrorStage::Browser,
        retryable: false,
        description: "The browser version could not be identified.",
    },
    ErrorCodeDefinition {
        code: "pageknot.binding.serialization",
        stage: ErrorStage::Internal,
        retryable: false,
        description: "A language binding could not serialize a canonical record.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.base_url",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "A collected frame has an invalid document base URL.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.capability",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "The collector lacks a requested capability.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.document",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "The rendered document could not be collected.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.evaluate",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "The in-page collector evaluation failed.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.chunk_checksum",
        stage: ErrorStage::Collection,
        retryable: true,
        description: "A collector chunk checksum is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.chunk_length",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "A collector chunk length is inconsistent.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.chunk_limit",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "A collector chunk exceeds the negotiated limit.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.chunk_owner",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "A collector chunk belongs to another capture or frame.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.chunk_sequence",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "Collector chunks are out of sequence.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.chunk_total",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "The collector changed a declared chunk count.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.payload_limit",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "Collector payloads exceed the configured capture limit.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.payload_checksum",
        stage: ErrorStage::Collection,
        retryable: true,
        description: "A collector payload digest is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.protocol_shape",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "A collector message does not match the protocol shape.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.protocol_major",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "The collector protocol major version is incompatible.",
    },
    ErrorCodeDefinition {
        code: "pageknot.collector.value",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "The collector returned a value that could not cross the protocol boundary.",
    },
    ErrorCodeDefinition {
        code: "pageknot.config.directory",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The PageKnot configuration directory is unavailable.",
    },
    ErrorCodeDefinition {
        code: "pageknot.config.parse",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A PageKnot configuration file contains invalid TOML.",
    },
    ErrorCodeDefinition {
        code: "pageknot.config.profile",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The selected capture profile is undefined.",
    },
    ErrorCodeDefinition {
        code: "pageknot.config.read",
        stage: ErrorStage::Validation,
        retryable: true,
        description: "A PageKnot configuration file could not be read.",
    },
    ErrorCodeDefinition {
        code: "pageknot.config.value",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A PageKnot configuration value is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.export.encode",
        stage: ErrorStage::Encoding,
        retryable: false,
        description: "An artifact variant could not be encoded.",
    },
    ErrorCodeDefinition {
        code: "pageknot.export.files",
        stage: ErrorStage::Encoding,
        retryable: false,
        description: "An artifact variant exceeds its file count limit.",
    },
    ErrorCodeDefinition {
        code: "pageknot.export.markdown_asset",
        stage: ErrorStage::Encoding,
        retryable: false,
        description: "A Markdown asset is malformed or has an invalid relative path.",
    },
    ErrorCodeDefinition {
        code: "pageknot.export.name",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The artifact export base name is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.export.output",
        stage: ErrorStage::Commit,
        retryable: false,
        description: "An artifact variant output could not be staged or committed.",
    },
    ErrorCodeDefinition {
        code: "pageknot.export.pdf",
        stage: ErrorStage::Encoding,
        retryable: false,
        description: "Chromium returned an invalid PDF payload.",
    },
    ErrorCodeDefinition {
        code: "pageknot.export.size",
        stage: ErrorStage::Encoding,
        retryable: false,
        description: "An artifact variant exceeds its byte limit.",
    },
    ErrorCodeDefinition {
        code: "pageknot.export.variant",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The requested artifact variant set is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.export.verify",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "An artifact variant failed its format-specific verifier.",
    },
    ErrorCodeDefinition {
        code: "pageknot.export.zip",
        stage: ErrorStage::Encoding,
        retryable: false,
        description: "A ZIP artifact could not be encoded or decoded.",
    },
    ErrorCodeDefinition {
        code: "pageknot.frame.collection",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "A frame could not be collected into the capture graph.",
    },
    ErrorCodeDefinition {
        code: "pageknot.frame.depth",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "The captured frame graph exceeds its configured depth.",
    },
    ErrorCodeDefinition {
        code: "pageknot.frame.detached",
        stage: ErrorStage::Collection,
        retryable: true,
        description: "An out-of-process frame detached before collection completed.",
    },
    ErrorCodeDefinition {
        code: "pageknot.frame.limit",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "The captured frame graph exceeds its configured count.",
    },
    ErrorCodeDefinition {
        code: "pageknot.frame.nodes",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "The captured frame graph exceeds its configured node budget.",
    },
    ErrorCodeDefinition {
        code: "pageknot.frame.owner",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "A captured node or frame has an inconsistent owner.",
    },
    ErrorCodeDefinition {
        code: "pageknot.frame.topology",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "The captured frame graph has an invalid topology.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.artifact",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The artifact input is invalid for the requested operation.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.browser_recycle",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The browser recycling limit is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.browser_selection",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The selected browser policy is invalid for the requested operation.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.cdp_url",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The remote browser endpoint URL is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.concurrency",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The browser context concurrency limit is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.credentials",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The capture credential input is invalid or insufficiently protected.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.duration",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A command duration is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.error_code",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "An error code is malformed.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.export_option",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "An artifact export option does not apply to the selected variants.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.file_root",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A file URL is outside the configured roots.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.json",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A JSON command input is unreadable, oversized, or invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.limit",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A capture limit is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.network_cidr",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A custom network CIDR is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.output",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The output target is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.path_encoding",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A filesystem path cannot be represented as UTF-8.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.remote_network_policy",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A remote browser capture selected a restricted network policy.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.remote_verification_policy",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A remote browser capture selected browser-backed verification.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.selector_scope",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A capture request combines a DOM selector with active selection scope.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.url",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The capture URL is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.url_scheme",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The capture URL uses a blocked scheme.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.value",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A language binding input does not match its public record.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.verification_option",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A verification option does not apply to the selected artifact format.",
    },
    ErrorCodeDefinition {
        code: "pageknot.input.viewport",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The browser viewport is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.internal.panic",
        stage: ErrorStage::Internal,
        retryable: false,
        description: "A capture task panicked behind the runtime boundary.",
    },
    ErrorCodeDefinition {
        code: "pageknot.navigation.address_blocked",
        stage: ErrorStage::Navigation,
        retryable: false,
        description: "The network policy blocked a resolved address.",
    },
    ErrorCodeDefinition {
        code: "pageknot.navigation.dns",
        stage: ErrorStage::Navigation,
        retryable: true,
        description: "The navigation host resolved to no addresses.",
    },
    ErrorCodeDefinition {
        code: "pageknot.navigation.failed",
        stage: ErrorStage::Navigation,
        retryable: true,
        description: "Chromium could not navigate to the requested page.",
    },
    ErrorCodeDefinition {
        code: "pageknot.navigation.host",
        stage: ErrorStage::Navigation,
        retryable: false,
        description: "The navigation URL has no host.",
    },
    ErrorCodeDefinition {
        code: "pageknot.navigation.policy_state",
        stage: ErrorStage::Navigation,
        retryable: false,
        description: "The navigation request has no validated network policy state.",
    },
    ErrorCodeDefinition {
        code: "pageknot.navigation.port",
        stage: ErrorStage::Navigation,
        retryable: false,
        description: "The navigation URL has no usable port.",
    },
    ErrorCodeDefinition {
        code: "pageknot.navigation.redirect_limit",
        stage: ErrorStage::Navigation,
        retryable: false,
        description: "Navigation exceeded the configured redirect count.",
    },
    ErrorCodeDefinition {
        code: "pageknot.navigation.scheme",
        stage: ErrorStage::Navigation,
        retryable: false,
        description: "The network URL scheme is blocked.",
    },
    ErrorCodeDefinition {
        code: "pageknot.navigation.url",
        stage: ErrorStage::Navigation,
        retryable: false,
        description: "Chromium reported an invalid navigation URL.",
    },
    ErrorCodeDefinition {
        code: "pageknot.output.commit",
        stage: ErrorStage::Commit,
        retryable: true,
        description: "The verified artifact could not be committed.",
    },
    ErrorCodeDefinition {
        code: "pageknot.output.directory",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The output directory is unavailable.",
    },
    ErrorCodeDefinition {
        code: "pageknot.output.diagnostics",
        stage: ErrorStage::Commit,
        retryable: true,
        description: "The sanitized diagnostic bundle could not be committed.",
    },
    ErrorCodeDefinition {
        code: "pageknot.output.exists",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The output path already exists.",
    },
    ErrorCodeDefinition {
        code: "pageknot.output.flush",
        stage: ErrorStage::Encoding,
        retryable: true,
        description: "The staging artifact could not be flushed.",
    },
    ErrorCodeDefinition {
        code: "pageknot.output.path",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The output path is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.output.read",
        stage: ErrorStage::Verification,
        retryable: true,
        description: "The staging artifact could not be read.",
    },
    ErrorCodeDefinition {
        code: "pageknot.output.staging",
        stage: ErrorStage::Encoding,
        retryable: true,
        description: "The staging artifact could not be created.",
    },
    ErrorCodeDefinition {
        code: "pageknot.output.stderr",
        stage: ErrorStage::Commit,
        retryable: true,
        description: "Human diagnostics could not be written to stderr.",
    },
    ErrorCodeDefinition {
        code: "pageknot.output.stdout",
        stage: ErrorStage::Commit,
        retryable: true,
        description: "A structured command result could not be written to stdout.",
    },
    ErrorCodeDefinition {
        code: "pageknot.output.sync",
        stage: ErrorStage::Encoding,
        retryable: true,
        description: "The staging artifact could not be synchronized.",
    },
    ErrorCodeDefinition {
        code: "pageknot.output.uniquify",
        stage: ErrorStage::Commit,
        retryable: false,
        description: "A unique output filename could not be selected.",
    },
    ErrorCodeDefinition {
        code: "pageknot.readiness.request_count",
        stage: ErrorStage::Readiness,
        retryable: false,
        description: "The readiness tracker observed an inconsistent request count.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.css_cycle",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A stylesheet import graph contains a cycle.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.css_depth",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A stylesheet import graph exceeds its configured depth.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.css_encoding",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A captured stylesheet has an unsupported text encoding.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.data_url",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A captured data URL could not be decoded.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.decode",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A browser resource body could not be decoded.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.identifier",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A resource identifier is absent from the graph.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.incomplete",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A resource graph contains unresolved references.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.limit",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A capture resource budget was exceeded.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.load",
        stage: ErrorStage::Resource,
        retryable: true,
        description: "Chromium could not load a required resource body.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.protocol_shape",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A browser resource response did not match the pinned protocol shape.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.rewrite",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A controlled resource reference could not be rewritten.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.scheme",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A resource reference uses an unsupported URL scheme.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.status",
        stage: ErrorStage::Resource,
        retryable: true,
        description: "A required resource returned an unsuccessful HTTP status.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.store",
        stage: ErrorStage::Internal,
        retryable: false,
        description: "The capture content store is internally inconsistent.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.stream",
        stage: ErrorStage::Resource,
        retryable: true,
        description: "A required resource stream ended with an error.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.svg_cycle",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "An SVG resource graph contains a cycle.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.svg_depth",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "An SVG resource graph exceeds its configured depth.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.svg_parse",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A captured SVG document has no usable root element.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.svg_serialize",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A transformed SVG document could not be serialized.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.terminal",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A resource received more than one terminal outcome.",
    },
    ErrorCodeDefinition {
        code: "pageknot.resource.url",
        stage: ErrorStage::Resource,
        retryable: false,
        description: "A captured resource URL is invalid.",
    },
    ErrorCodeDefinition {
        code: "pageknot.selector.invalid",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "The DOM selector is not a valid CSS selector.",
    },
    ErrorCodeDefinition {
        code: "pageknot.selector.not_found",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "The top-level document has no element matching the DOM selector.",
    },
    ErrorCodeDefinition {
        code: "pageknot.selection.empty",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "The selected document scope has no active top-level selection.",
    },
    ErrorCodeDefinition {
        code: "pageknot.runtime.cancelled",
        stage: ErrorStage::Shutdown,
        retryable: false,
        description: "The active capture was cancelled.",
    },
    ErrorCodeDefinition {
        code: "pageknot.runtime.state_transition",
        stage: ErrorStage::Internal,
        retryable: false,
        description: "The capture state machine rejected a transition.",
    },
    ErrorCodeDefinition {
        code: "pageknot.runtime.capture_unavailable",
        stage: ErrorStage::Internal,
        retryable: false,
        description: "The capture runtime is unavailable.",
    },
    ErrorCodeDefinition {
        code: "pageknot.runtime.closed",
        stage: ErrorStage::Shutdown,
        retryable: false,
        description: "The PageKnot service is closed.",
    },
    ErrorCodeDefinition {
        code: "pageknot.runtime.interrupted",
        stage: ErrorStage::Shutdown,
        retryable: false,
        description: "The active command received an interruption signal.",
    },
    ErrorCodeDefinition {
        code: "pageknot.runtime.job",
        stage: ErrorStage::Internal,
        retryable: false,
        description: "The capture job registry is internally inconsistent.",
    },
    ErrorCodeDefinition {
        code: "pageknot.runtime.lock",
        stage: ErrorStage::Internal,
        retryable: false,
        description: "A runtime state lock is poisoned.",
    },
    ErrorCodeDefinition {
        code: "pageknot.runtime.signal",
        stage: ErrorStage::Shutdown,
        retryable: false,
        description: "The command could not subscribe to interruption signals.",
    },
    ErrorCodeDefinition {
        code: "pageknot.runtime.terminal_event",
        stage: ErrorStage::Internal,
        retryable: false,
        description: "A capture emitted more than one terminal event.",
    },
    ErrorCodeDefinition {
        code: "pageknot.runtime.timeout",
        stage: ErrorStage::Shutdown,
        retryable: true,
        description: "The requested operation exceeded its total deadline.",
    },
    ErrorCodeDefinition {
        code: "pageknot.scheduler.artifact_read",
        stage: ErrorStage::Internal,
        retryable: true,
        description: "A captured crawl artifact could not be read for link discovery.",
    },
    ErrorCodeDefinition {
        code: "pageknot.scheduler.concurrency",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "Scheduler concurrency is outside the supported range.",
    },
    ErrorCodeDefinition {
        code: "pageknot.scheduler.depth",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "Crawl depth is outside the supported range.",
    },
    ErrorCodeDefinition {
        code: "pageknot.scheduler.failed",
        stage: ErrorStage::Internal,
        retryable: false,
        description: "One or more scheduled captures reached a failed terminal state.",
    },
    ErrorCodeDefinition {
        code: "pageknot.scheduler.job_count",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The number of scheduled jobs is outside the supported range.",
    },
    ErrorCodeDefinition {
        code: "pageknot.scheduler.job_id",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A batch job identifier is invalid or duplicated.",
    },
    ErrorCodeDefinition {
        code: "pageknot.scheduler.manifest_mismatch",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "Resume state belongs to a different scheduler plan.",
    },
    ErrorCodeDefinition {
        code: "pageknot.scheduler.manifest_read",
        stage: ErrorStage::Validation,
        retryable: true,
        description: "Resume state could not be inspected or decoded.",
    },
    ErrorCodeDefinition {
        code: "pageknot.scheduler.manifest_write",
        stage: ErrorStage::Commit,
        retryable: true,
        description: "Resume state could not be staged or committed.",
    },
    ErrorCodeDefinition {
        code: "pageknot.scheduler.output",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "The scheduler output directory is unavailable.",
    },
    ErrorCodeDefinition {
        code: "pageknot.scheduler.plan",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A scheduler plan could not be serialized.",
    },
    ErrorCodeDefinition {
        code: "pageknot.scheduler.resume_target",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A resumable job does not have a persistent artifact target.",
    },
    ErrorCodeDefinition {
        code: "pageknot.scheduler.state",
        stage: ErrorStage::Internal,
        retryable: false,
        description: "The scheduler reached an inconsistent internal state.",
    },
    ErrorCodeDefinition {
        code: "pageknot.scheduler.url",
        stage: ErrorStage::Validation,
        retryable: false,
        description: "A crawl URL uses an unsupported scheme.",
    },
    ErrorCodeDefinition {
        code: "pageknot.readiness.timeout",
        stage: ErrorStage::Readiness,
        retryable: true,
        description: "The page did not reach the requested readiness state before its deadline.",
    },
    ErrorCodeDefinition {
        code: "pageknot.transform.node",
        stage: ErrorStage::Transform,
        retryable: false,
        description: "A document transform referenced an unknown node.",
    },
    ErrorCodeDefinition {
        code: "pageknot.transform.node_limit",
        stage: ErrorStage::Transform,
        retryable: false,
        description: "A document exceeds the node identifier range.",
    },
    ErrorCodeDefinition {
        code: "pageknot.transform.structural_repair",
        stage: ErrorStage::Transform,
        retryable: false,
        description: "The structural repair program is invalid or exceeds its bounds.",
    },
    ErrorCodeDefinition {
        code: "pageknot.transform.visual_fallback",
        stage: ErrorStage::Transform,
        retryable: false,
        description: "A visual fallback could not be attached to its captured node.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.active_content",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "The artifact contains active captured content.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.csp",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "The artifact content security policy is inconsistent.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.embedded_resource",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "An embedded resource is malformed or inconsistent with its manifest record.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.external_reference",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "The artifact contains an external render-fetch reference.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.format",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "The artifact format or schema is incompatible.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.frames",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "The embedded frame graph is incomplete or inconsistent.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.manifest",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "The artifact manifest is missing or malformed.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.network",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "The artifact attempted a network request during offline verification.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.page_error",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "The artifact raised a page error during offline verification.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.path",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "The staged artifact path cannot be opened by the verifier.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.record",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "The artifact verification record is inconsistent.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.reference",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "The artifact contains an invalid controlled resource reference.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.resource_provenance",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "An embedded resource has incomplete retrieval provenance.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.resource_summary",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "The artifact resource summary is inconsistent.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.size",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "The artifact size exceeds the supported range.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.structural_repair",
        stage: ErrorStage::Verification,
        retryable: false,
        description: "The artifact structural repair data is malformed or inconsistent.",
    },
    ErrorCodeDefinition {
        code: "pageknot.verification.unstable",
        stage: ErrorStage::Verification,
        retryable: true,
        description: "The artifact did not reach a stable browser state during offline verification.",
    },
    ErrorCodeDefinition {
        code: "pageknot.visual_fallback.bounds",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "A visual fallback has invalid or out-of-range browser bounds.",
    },
    ErrorCodeDefinition {
        code: "pageknot.visual_fallback.encoding",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "A visual fallback image could not be decoded.",
    },
    ErrorCodeDefinition {
        code: "pageknot.visual_fallback.pixel_limit",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "A visual fallback exceeds its configured pixel budget.",
    },
    ErrorCodeDefinition {
        code: "pageknot.visual_fallback.protocol_shape",
        stage: ErrorStage::Collection,
        retryable: false,
        description: "A visual fallback response did not match the pinned protocol shape.",
    },
];

#[derive(
    Clone, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct ErrorCode(String);

impl ErrorCode {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_error_code(&value)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn from_static(value: &'static str) -> Self {
        debug_assert!(validate_error_code(value).is_ok());
        Self(value.to_owned())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ErrorCode {
    type Err = PageKnotError;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

fn validate_error_code(value: &str) -> Result<()> {
    let valid = value.starts_with("pageknot.")
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'_'
        })
        && value.split('.').all(|segment| !segment.is_empty());
    if valid {
        Ok(())
    } else {
        Err(PageKnotError::new(
            "pageknot.input.error_code",
            ErrorStage::Validation,
            format!("invalid PageKnot error code `{value}`"),
        ))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorStage {
    Validation,
    Browser,
    Navigation,
    Readiness,
    Collection,
    Resource,
    Transform,
    Encoding,
    Verification,
    Commit,
    Shutdown,
    Internal,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageKnotError {
    pub code: ErrorCode,
    pub message: String,
    pub stage: ErrorStage,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics_path: Option<crate::PortablePath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Box<PageKnotError>>,
}

impl PageKnotError {
    #[must_use]
    pub fn new(code: &'static str, stage: ErrorStage, message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::from_static(code),
            message: message.into(),
            stage,
            retryable: false,
            details: BTreeMap::new(),
            diagnostics_path: None,
            source: None,
        }
    }

    #[must_use]
    pub const fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    #[must_use]
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<JsonValue>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_source(mut self, source: PageKnotError) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    #[must_use]
    pub fn with_diagnostics_path(mut self, path: crate::PortablePath) -> Self {
        self.diagnostics_path = Some(path);
        self
    }
}

impl fmt::Display for PageKnotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl Error for PageKnotError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{ERROR_CODE_REGISTRY, ErrorCode, ErrorStage, PageKnotError};

    #[test]
    fn error_code_rejects_non_namespaced_values() {
        let result = ErrorCode::new("invalid");

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.input.error_code")
        );
    }

    #[test]
    fn error_serialization_keeps_recovery_fields_structured() {
        let error = PageKnotError::new(
            "pageknot.browser.unavailable",
            ErrorStage::Browser,
            "no compatible browser was found",
        )
        .with_detail("recoveryCommand", "pageknot browser install");
        let json = serde_json::to_value(error);

        assert_eq!(
            json.as_ref()
                .ok()
                .and_then(|value| value["details"]["recoveryCommand"].as_str()),
            Some("pageknot browser install")
        );
    }

    #[test]
    fn public_error_code_registry_is_unique_and_well_formed() {
        let mut codes = BTreeSet::new();

        for definition in ERROR_CODE_REGISTRY {
            assert!(ErrorCode::new(definition.code).is_ok());
            assert!(codes.insert(definition.code), "{}", definition.code);
            assert!(!definition.description.trim().is_empty());
        }
    }

    #[test]
    fn public_error_code_registry_covers_cross_boundary_failures() {
        for code in [
            "pageknot.browser.target_crashed",
            "pageknot.input.credentials",
            "pageknot.output.diagnostics",
            "pageknot.runtime.interrupted",
            "pageknot.verification.embedded_resource",
        ] {
            assert!(
                ERROR_CODE_REGISTRY
                    .iter()
                    .any(|definition| definition.code == code),
                "{code}"
            );
        }
    }
}
