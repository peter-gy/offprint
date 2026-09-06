use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{PortablePath, RedactedUrl};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrowserProduct {
    Chrome,
    Chromium,
    Edge,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrowserSource {
    Managed,
    System,
    Explicit,
    Remote,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrowserChannel {
    Auto,
    Managed,
    System,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrowserInstallationPolicy {
    Explicit,
    InstallManaged,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInfo {
    pub product: BrowserProduct,
    pub version: String,
    pub source: BrowserSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<PortablePath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<RedactedUrl>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub protocol_version: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrowserCandidateState {
    Selected,
    Compatible,
    Shadowed,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCandidate {
    pub browser: BrowserInfo,
    pub state: BrowserCandidateState,
    pub reason_code: String,
    pub priority: u32,
    pub active_leases: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedBrowserState {
    pub cache_dir: PortablePath,
    #[serde(default)]
    pub installed_revisions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_revision: Option<String>,
    pub catalog_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityCheck {
    pub compatible: bool,
    pub host_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_version: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub missing_capabilities: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputCapability {
    pub directory: PortablePath,
    pub writable: bool,
    pub atomic_create: bool,
    pub atomic_replace: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfigProvenance {
    Flag,
    Environment,
    ExplicitConfig,
    Profile,
    UserConfig,
    Default,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveConfigValue {
    pub field: String,
    pub value: serde_json::Value,
    pub provenance: ConfigProvenance,
    pub redacted: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPolicySummary {
    pub profile: String,
    pub permits_loopback_initial_origin: bool,
    pub permits_private_addresses: bool,
    pub revalidates_redirects: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryAction {
    pub code: String,
    pub description: String,
    pub command: String,
    #[serde(default)]
    pub arguments: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserDoctorReport {
    pub schema_version: u32,
    pub ready: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<BrowserInfo>,
    #[serde(default)]
    pub candidates: Vec<BrowserCandidate>,
    pub managed_cache: ManagedBrowserState,
    pub collector: CapabilityCheck,
    pub output: OutputCapability,
    #[serde(default)]
    pub configuration: Vec<EffectiveConfigValue>,
    pub network: NetworkPolicySummary,
    #[serde(default)]
    pub recovery: Vec<RecoveryAction>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInstallRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrowserAction {
    Install,
    List,
    Remove,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserOperationResult {
    pub schema_version: u32,
    pub action: BrowserAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<BrowserInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub cache_dir: PortablePath,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<BrowserCandidate>,
}
