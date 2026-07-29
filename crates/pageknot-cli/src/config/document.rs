use std::collections::BTreeMap;

use pageknot::{
    BrowserChannel, BrowserInstallationPolicy, CaptureScope, ColorScheme, MissingResourcePolicy,
    ReadinessMode, ReducedMotion, VerificationPolicy, Viewport,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ConfigFile {
    pub(super) default_profile: Option<String>,
    pub(super) browser: BrowserConfig,
    pub(super) profile: BTreeMap<String, ProfilePatch>,
    pub(super) headers: Option<String>,
    pub(super) cookies: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct BrowserConfig {
    pub(super) channel: Option<BrowserChannel>,
    pub(super) installation: Option<BrowserInstallationPolicy>,
    pub(super) path: Option<String>,
    pub(super) cdp_url: Option<String>,
    pub(super) cache_dir: Option<String>,
    pub(super) headless: Option<bool>,
    pub(super) maximum_contexts: Option<u16>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ProfilePatch {
    pub(super) verification: Option<VerificationPolicy>,
    pub(super) missing_resources: Option<MissingResourcePolicy>,
    pub(super) network_policy: Option<SimpleNetworkPolicy>,
    pub(super) preserve_password_values: Option<bool>,
    pub(super) scope: Option<CaptureScope>,
    pub(super) selector: Option<String>,
    pub(super) allowed_file_roots: Option<Vec<String>>,
    pub(super) optimizations: OptimizationConfig,
    pub(super) environment: EnvironmentConfig,
    pub(super) readiness: ReadinessConfig,
    pub(super) limits: LimitsConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct OptimizationConfig {
    pub(super) remove_unused_css: Option<bool>,
    pub(super) remove_unused_fonts: Option<bool>,
    pub(super) remove_hidden_elements: Option<bool>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum SimpleNetworkPolicy {
    Standard,
    Server,
    Unrestricted,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct EnvironmentConfig {
    pub(super) viewport: Option<Viewport>,
    pub(super) locale: Option<String>,
    pub(super) timezone: Option<String>,
    pub(super) color_scheme: Option<ColorScheme>,
    pub(super) reduced_motion: Option<ReducedMotion>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ReadinessConfig {
    pub(super) mode: Option<ReadinessMode>,
    pub(super) network_quiet: Option<ConfigScalar>,
    pub(super) mutation_quiet: Option<ConfigScalar>,
    pub(super) delay: Option<ConfigScalar>,
    pub(super) lazy_load: Option<LazyLoadName>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum LazyLoadName {
    Disabled,
    ViewportSweep,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct LimitsConfig {
    pub(super) duration: Option<ConfigScalar>,
    pub(super) redirects: Option<u32>,
    pub(super) frames: Option<u32>,
    pub(super) nodes: Option<u64>,
    pub(super) resources: Option<u32>,
    pub(super) resource_bytes: Option<ConfigScalar>,
    pub(super) total_resource_bytes: Option<ConfigScalar>,
    pub(super) collector_chunk_bytes: Option<ConfigScalar>,
    pub(super) concurrent_resources: Option<u16>,
    pub(super) artifact_bytes: Option<ConfigScalar>,
    pub(super) css_import_depth: Option<u16>,
    pub(super) frame_depth: Option<u16>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum ConfigScalar {
    Integer(u64),
    String(String),
}
