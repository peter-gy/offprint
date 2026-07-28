use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ConflictPolicy, ContentDigest, PortablePath, ResourceSummary};

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactVariantKind {
    Pdf,
    Markdown,
    Zip,
    SelfExtracting,
    Mhtml,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfOptions {
    #[serde(default)]
    pub landscape: bool,
    #[serde(default)]
    pub prefer_css_page_size: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkdownOptions {
    #[serde(default = "default_true")]
    pub front_matter: bool,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        Self { front_matter: true }
    }
}

const fn default_true() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", content = "options", rename_all = "kebab-case")]
pub enum ArtifactVariant {
    Pdf(PdfOptions),
    Markdown(MarkdownOptions),
    Zip,
    SelfExtracting,
    Mhtml,
}

impl ArtifactVariant {
    #[must_use]
    pub const fn kind(self) -> ArtifactVariantKind {
        match self {
            Self::Pdf(_) => ArtifactVariantKind::Pdf,
            Self::Markdown(_) => ArtifactVariantKind::Markdown,
            Self::Zip => ArtifactVariantKind::Zip,
            Self::SelfExtracting => ArtifactVariantKind::SelfExtracting,
            Self::Mhtml => ArtifactVariantKind::Mhtml,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactExportRequest {
    pub output_directory: PortablePath,
    pub base_name: String,
    pub variants: Vec<ArtifactVariant>,
    pub conflict: ConflictPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactVariantVerification {
    pub kind: ArtifactVariantKind,
    pub passed: bool,
    pub bytes: u64,
    pub sha256: ContentDigest,
    pub structure_valid: bool,
    pub content_valid: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedArtifact {
    pub kind: ArtifactVariantKind,
    pub path: PortablePath,
    pub entrypoint: PortablePath,
    pub bytes: u64,
    pub sha256: ContentDigest,
    pub verification: ArtifactVariantVerification,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactExportResult {
    pub schema_version: u32,
    pub source_artifact_sha256: ContentDigest,
    pub policy_sha256: ContentDigest,
    pub resources: ResourceSummary,
    pub variants: Vec<ExportedArtifact>,
}
