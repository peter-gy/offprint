use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ConflictPolicy, ContentDigest, PortablePath, ResourceSummary};

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactFormat {
    Html,
    Pdf,
    Markdown,
    Zip,
    SelfExtractingHtml,
    Mhtml,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfOptions {
    #[serde(default)]
    pub landscape: bool,
    #[serde(default)]
    pub prefer_css_page_size: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
#[serde(tag = "format", content = "options", rename_all = "kebab-case")]
pub enum FormatSpec {
    Pdf(PdfOptions),
    Markdown(MarkdownOptions),
    Zip,
    SelfExtractingHtml,
    Mhtml,
}

impl FormatSpec {
    #[must_use]
    pub const fn format(self) -> ArtifactFormat {
        match self {
            Self::Pdf(_) => ArtifactFormat::Pdf,
            Self::Markdown(_) => ArtifactFormat::Markdown,
            Self::Zip => ArtifactFormat::Zip,
            Self::SelfExtractingHtml => ArtifactFormat::SelfExtractingHtml,
            Self::Mhtml => ArtifactFormat::Mhtml,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportRequest {
    pub schema_version: u32,
    pub output_directory: PortablePath,
    pub base_name: String,
    pub formats: Vec<FormatSpec>,
    pub conflict: ConflictPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormatVerification {
    pub format: ArtifactFormat,
    pub bytes: u64,
    pub sha256: ContentDigest,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportedArtifact {
    pub format: ArtifactFormat,
    pub path: PortablePath,
    pub entrypoint: PortablePath,
    pub bytes: u64,
    pub sha256: ContentDigest,
    pub verification: FormatVerification,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportResult {
    pub schema_version: u32,
    pub source_artifact_sha256: ContentDigest,
    pub capture_policy_sha256: ContentDigest,
    pub resources: ResourceSummary,
    pub artifacts: Vec<ExportedArtifact>,
}
