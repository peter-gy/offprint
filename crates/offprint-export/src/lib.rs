//! Alternate artifact encoders and format-specific structural verification.
//!
//! Every encoder consumes an already verified Offprint HTML artifact. The
//! resulting formats preserve the source manifest and resource report while
//! exposing a representation suited to printing, text workflows, packaging,
//! compressed transport, or browser-native archive import.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

use std::collections::BTreeMap;

use offprint_model::{
    ArtifactFormat, ArtifactManifest, ContentDigest, ErrorStage, FormatVerification,
    MarkdownOptions, OffprintError, Result,
};
mod markdown;
mod mhtml;
mod pdf;
mod self_extracting;
mod support;
mod zip;

pub use markdown::{encode_markdown, verify_markdown};
pub use mhtml::{encode_mhtml, verify_mhtml};
pub use pdf::{embed_pdf_metadata, verify_offprint_pdf, verify_pdf};
pub use self_extracting::{encode_self_extracting, verify_self_extracting};
pub use zip::{encode_zip, verify_zip};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Format-specific verification evidence before filesystem commit.
pub struct FormatEvidence {
    format: ArtifactFormat,
    pub(crate) structure_valid: bool,
    pub(crate) content_valid: bool,
}

impl FormatEvidence {
    /// Converts format evidence into the public verification record for `payload`.
    #[must_use]
    pub fn into_verification(self, bytes: u64, sha256: ContentDigest) -> FormatVerification {
        FormatVerification {
            format: self.format,
            bytes,
            sha256,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A representation payload returned after its format verifier succeeds.
pub struct VerifiedFormat<T> {
    payload: T,
    evidence: FormatEvidence,
}

impl<T> VerifiedFormat<T> {
    fn new(format: ArtifactFormat, payload: T) -> Self {
        Self {
            payload,
            evidence: FormatEvidence {
                format,
                structure_valid: true,
                content_valid: true,
            },
        }
    }

    /// Consumes the verified representation into its payload and evidence.
    #[must_use]
    pub fn into_parts(self) -> (T, FormatEvidence) {
        (self.payload, self.evidence)
    }

    /// Consumes the verified representation and returns its payload.
    #[must_use]
    pub fn into_payload(self) -> T {
        self.payload
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Markdown entrypoint and its content-addressed relative assets.
pub struct MarkdownBundle {
    pub markdown: Vec<u8>,
    pub assets: BTreeMap<String, Vec<u8>>,
}

impl MarkdownBundle {
    /// Validates the asset count and aggregate byte size of this bundle.
    pub fn validate_limits(
        &self,
        maximum_assets: usize,
        maximum_bytes: u64,
        stage: ErrorStage,
    ) -> Result<()> {
        if self.assets.len() > maximum_assets {
            return Err(markdown_file_limit_error(stage));
        }
        let mut bytes = u64::try_from(self.markdown.len()).unwrap_or(u64::MAX);
        for content in self.assets.values() {
            bytes = bytes.saturating_add(u64::try_from(content.len()).unwrap_or(u64::MAX));
            if bytes > maximum_bytes {
                return Err(markdown_byte_limit_error(stage));
            }
        }
        if bytes > maximum_bytes {
            return Err(markdown_byte_limit_error(stage));
        }
        Ok(())
    }

    /// Converts the bundle into a portable directory file tree.
    #[must_use]
    pub fn into_files(mut self) -> BTreeMap<String, Vec<u8>> {
        self.assets.insert("index.md".to_owned(), self.markdown);
        self.assets
    }
}

/// Returns the canonical Markdown asset-count error for `stage`.
#[must_use]
pub fn markdown_file_limit_error(stage: ErrorStage) -> OffprintError {
    OffprintError::new(
        "offprint.export.files",
        stage,
        "Markdown bundle exceeds the asset count limit",
    )
}

/// Returns the canonical Markdown aggregate-size error for `stage`.
#[must_use]
pub fn markdown_byte_limit_error(stage: ErrorStage) -> OffprintError {
    OffprintError::new(
        "offprint.export.size",
        stage,
        "Markdown bundle exceeds the aggregate byte limit",
    )
}

/// Encodes and verifies one Markdown directory bundle.
pub fn prepare_markdown(
    html: &[u8],
    manifest: &ArtifactManifest,
    options: MarkdownOptions,
) -> Result<VerifiedFormat<MarkdownBundle>> {
    encode_markdown(html, manifest, options)
        .map(|payload| VerifiedFormat::new(ArtifactFormat::Markdown, payload))
}

/// Encodes and verifies one deterministic ZIP representation.
pub fn prepare_zip(html: &[u8], manifest: &ArtifactManifest) -> Result<VerifiedFormat<Vec<u8>>> {
    encode_zip(html, manifest).map(|payload| VerifiedFormat::new(ArtifactFormat::Zip, payload))
}

/// Encodes and verifies one self-extracting HTML representation.
pub fn prepare_self_extracting(html: &[u8]) -> Result<VerifiedFormat<Vec<u8>>> {
    encode_self_extracting(html)
        .map(|payload| VerifiedFormat::new(ArtifactFormat::SelfExtractingHtml, payload))
}

/// Encodes and verifies one browser-native MHTML representation.
pub fn prepare_mhtml(html: &[u8], manifest: &ArtifactManifest) -> Result<VerifiedFormat<Vec<u8>>> {
    encode_mhtml(html, manifest).map(|payload| VerifiedFormat::new(ArtifactFormat::Mhtml, payload))
}

/// Embeds source metadata into a Chromium PDF and verifies the result.
pub fn prepare_pdf(
    pdf: &[u8],
    html: &[u8],
    manifest: &ArtifactManifest,
    source_artifact_sha256: ContentDigest,
    maximum_bytes: u64,
) -> Result<VerifiedFormat<Vec<u8>>> {
    embed_pdf_metadata(pdf, html, manifest, source_artifact_sha256, maximum_bytes)
        .map(|payload| VerifiedFormat::new(ArtifactFormat::Pdf, payload))
}
