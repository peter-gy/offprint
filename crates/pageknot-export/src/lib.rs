//! Alternate artifact encoders and format-specific structural verification.
//!
//! Every encoder consumes an already verified PageKnot HTML artifact. The
//! resulting formats preserve the source manifest and resource report while
//! exposing a representation suited to printing, text workflows, packaging,
//! compressed transport, or browser-native archive import.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

use std::collections::BTreeMap;

use pageknot_model::{
    ArtifactManifest, ArtifactVariantKind, ArtifactVariantVerification, ContentDigest, ErrorStage,
    MarkdownOptions, PageKnotError, Result,
};
mod markdown;
mod mhtml;
mod pdf;
mod self_extracting;
mod support;
mod zip;

pub use markdown::{encode_markdown, verify_markdown};
pub use mhtml::{encode_mhtml, verify_mhtml};
pub use pdf::{embed_pdf_metadata, verify_pageknot_pdf, verify_pdf};
pub use self_extracting::{encode_self_extracting, verify_self_extracting};
pub use zip::{encode_zip, verify_zip};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Format-specific verification evidence before filesystem commit.
pub struct VariantEvidence {
    kind: ArtifactVariantKind,
    pub(crate) structure_valid: bool,
    pub(crate) content_valid: bool,
}

impl VariantEvidence {
    /// Converts format evidence into the public verification record for `payload`.
    #[must_use]
    pub fn into_verification(
        self,
        bytes: u64,
        sha256: ContentDigest,
    ) -> ArtifactVariantVerification {
        ArtifactVariantVerification {
            kind: self.kind,
            passed: self.structure_valid && self.content_valid,
            bytes,
            sha256,
            structure_valid: self.structure_valid,
            content_valid: self.content_valid,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A representation payload returned after its format verifier succeeds.
pub struct VerifiedVariant<T> {
    payload: T,
    evidence: VariantEvidence,
}

impl<T> VerifiedVariant<T> {
    fn new(kind: ArtifactVariantKind, payload: T) -> Self {
        Self {
            payload,
            evidence: VariantEvidence {
                kind,
                structure_valid: true,
                content_valid: true,
            },
        }
    }

    /// Consumes the verified representation into its payload and evidence.
    #[must_use]
    pub fn into_parts(self) -> (T, VariantEvidence) {
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
pub fn markdown_file_limit_error(stage: ErrorStage) -> PageKnotError {
    PageKnotError::new(
        "pageknot.export.files",
        stage,
        "Markdown bundle exceeds the asset count limit",
    )
}

/// Returns the canonical Markdown aggregate-size error for `stage`.
#[must_use]
pub fn markdown_byte_limit_error(stage: ErrorStage) -> PageKnotError {
    PageKnotError::new(
        "pageknot.export.size",
        stage,
        "Markdown bundle exceeds the aggregate byte limit",
    )
}

/// Encodes and verifies one Markdown directory bundle.
pub fn prepare_markdown(
    html: &[u8],
    manifest: &ArtifactManifest,
    options: MarkdownOptions,
) -> Result<VerifiedVariant<MarkdownBundle>> {
    encode_markdown(html, manifest, options)
        .map(|payload| VerifiedVariant::new(ArtifactVariantKind::Markdown, payload))
}

/// Encodes and verifies one deterministic ZIP representation.
pub fn prepare_zip(html: &[u8], manifest: &ArtifactManifest) -> Result<VerifiedVariant<Vec<u8>>> {
    encode_zip(html, manifest)
        .map(|payload| VerifiedVariant::new(ArtifactVariantKind::Zip, payload))
}

/// Encodes and verifies one self-extracting HTML representation.
pub fn prepare_self_extracting(html: &[u8]) -> Result<VerifiedVariant<Vec<u8>>> {
    encode_self_extracting(html)
        .map(|payload| VerifiedVariant::new(ArtifactVariantKind::SelfExtracting, payload))
}

/// Encodes and verifies one browser-native MHTML representation.
pub fn prepare_mhtml(html: &[u8], manifest: &ArtifactManifest) -> Result<VerifiedVariant<Vec<u8>>> {
    encode_mhtml(html, manifest)
        .map(|payload| VerifiedVariant::new(ArtifactVariantKind::Mhtml, payload))
}

/// Embeds source metadata into a Chromium PDF and verifies the result.
pub fn prepare_pdf(
    pdf: &[u8],
    html: &[u8],
    manifest: &ArtifactManifest,
    source_artifact_sha256: ContentDigest,
    maximum_bytes: u64,
) -> Result<VerifiedVariant<Vec<u8>>> {
    embed_pdf_metadata(pdf, html, manifest, source_artifact_sha256, maximum_bytes)
        .map(|payload| VerifiedVariant::new(ArtifactVariantKind::Pdf, payload))
}
