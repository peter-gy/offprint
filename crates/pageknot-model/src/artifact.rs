use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    BrowserEnvironment, BrowserInfo, ConflictPolicy, ContentDigest, PortablePath, ResourceRecord,
    ResourceSummary, SourceSummary, VerificationPolicy,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactKind {
    Html,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ArtifactTarget {
    File(PortablePath),
    Bytes { max_bytes: u64 },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HtmlArtifactSpec {
    pub target: ArtifactTarget,
    pub conflict: ConflictPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", content = "options", rename_all = "kebab-case")]
pub enum ArtifactSpec {
    Html(HtmlArtifactSpec),
}

impl ArtifactSpec {
    #[must_use]
    pub fn html_bytes(max_bytes: u64) -> Self {
        Self::Html(HtmlArtifactSpec {
            target: ArtifactTarget::Bytes { max_bytes },
            conflict: ConflictPolicy::Replace,
        })
    }

    #[must_use]
    pub fn html_file(path: PortablePath) -> Self {
        Self::Html(HtmlArtifactSpec {
            target: ArtifactTarget::File(path),
            conflict: ConflictPolicy::Replace,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum ArtifactInput {
    File(PortablePath),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactFormat {
    pub kind: ArtifactKind,
    pub version: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestGenerator {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestSource {
    pub requested_url: crate::RedactedUrl,
    pub requested_url_sha256: ContentDigest,
    pub final_url: crate::RedactedUrl,
    pub final_url_sha256: ContentDigest,
}

impl From<SourceSummary> for ManifestSource {
    fn from(value: SourceSummary) -> Self {
        Self {
            requested_url: value.requested_url,
            requested_url_sha256: value.requested_url_sha256,
            final_url: value.final_url,
            final_url_sha256: value.final_url_sha256,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuralRepair {
    pub applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_sha256: Option<ContentDigest>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestVerification {
    pub level: VerificationPolicy,
    pub passed: bool,
    pub network_requests: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewState {
    pub scroll_x: String,
    pub scroll_y: String,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            scroll_x: "0".to_owned(),
            scroll_y: "0".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactManifest {
    pub schema_version: u32,
    pub format: ArtifactFormat,
    pub generator: ManifestGenerator,
    pub source: ManifestSource,
    pub captured_at: DateTime<Utc>,
    pub browser: BrowserInfo,
    pub environment: BrowserEnvironment,
    pub view_state: ViewState,
    pub policy_sha256: ContentDigest,
    pub frames: u32,
    pub resources: ResourceSummary,
    #[serde(default)]
    pub resource_records: Vec<ResourceRecord>,
    #[serde(default)]
    pub warning_codes: Vec<String>,
    pub structural_repair: StructuralRepair,
    pub verification: ManifestVerification,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ArtifactResult {
    File {
        path: PortablePath,
        bytes: u64,
        sha256: ContentDigest,
    },
    Bytes {
        bytes: u64,
        sha256: ContentDigest,
        content: Vec<u8>,
    },
}

impl ArtifactResult {
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        match self {
            Self::File { bytes, .. } | Self::Bytes { bytes, .. } => *bytes,
        }
    }

    #[must_use]
    pub const fn sha256(&self) -> ContentDigest {
        match self {
            Self::File { sha256, .. } | Self::Bytes { sha256, .. } => *sha256,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::ArtifactManifest;

    fn warning_code_strategy() -> impl Strategy<Value = String> {
        "[a-z][a-z0-9]*(\\.[a-z][a-z0-9_]*){1,5}"
    }

    proptest! {
        #![proptest_config({
            let mut config = ProptestConfig::default();
            if std::env::var_os("PROPTEST_CASES").is_none() {
                config.cases = 128;
            }
            config
        })]

        #[test]
        fn artifact_manifest_round_trips_through_canonical_json(
            warning_codes in proptest::collection::vec(warning_code_strategy(), 0..32),
        ) {
            let mut manifest: ArtifactManifest = serde_json::from_str(include_str!(
                "../../../schemas/examples/artifact-manifest.json"
            ))
            .map_err(|error| TestCaseError::fail(error.to_string()))?;
            manifest.warning_codes = warning_codes;

            let json = serde_json::to_vec(&manifest)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let decoded = serde_json::from_slice::<ArtifactManifest>(&json)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;

            prop_assert_eq!(decoded, manifest);
        }
    }
}
