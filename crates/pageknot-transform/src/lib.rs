//! Shared safe-static transformation for the native capture pipeline.
//!
//! Browser sessions collect rendered state and resolve resources before this
//! boundary. The transformer freezes rendering, removes active content,
//! applies structural repair, encodes the manifest, and runs the independent
//! static verifier.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

use std::io::Write;

use pageknot_document::{
    Document, ensure_render_freeze_styles, sanitize_safe_static, serialize_document,
};
use pageknot_model::{
    ArtifactManifest, ContentDigest, ErrorStage, PageKnotError, Result, VerificationResult,
};

#[derive(Clone, Debug, Eq, PartialEq)]
/// Serialized document state after the shared safe-static transform.
pub struct TransformedDocument {
    html: Vec<u8>,
    structural_repair: bool,
}

impl TransformedDocument {
    #[must_use]
    pub fn html(&self) -> &[u8] {
        &self.html
    }

    #[must_use]
    pub const fn structural_repair(&self) -> bool {
        self.structural_repair
    }
}

#[derive(Clone, Debug, PartialEq)]
/// Encoded artifact and static verification produced from one transform.
pub struct EncodedArtifact {
    pub bytes: Vec<u8>,
    pub sha256: ContentDigest,
    pub manifest: ArtifactManifest,
    pub verification: VerificationResult,
}

/// Applies the host-independent safe-static document transformation.
pub fn transform_document(html: &[u8]) -> Result<TransformedDocument> {
    let mut document = Document::parse(html);
    ensure_render_freeze_styles(&mut document)?;
    let _sanitization = sanitize_safe_static(&mut document);
    let structural_repair = pageknot_html::apply_structural_repair(&mut document)?;
    let html = serialize_document(&document).map_err(|error| {
        PageKnotError::new(
            "pageknot.artifact.serialize",
            ErrorStage::Transform,
            format!("failed to serialize the transformed document: {error}"),
        )
    })?;
    Ok(TransformedDocument {
        html,
        structural_repair,
    })
}

/// Encodes and statically verifies one transformed document.
pub fn encode_artifact(
    transformed: &TransformedDocument,
    manifest: ArtifactManifest,
) -> Result<EncodedArtifact> {
    let mut bytes = Vec::new();
    let manifest = encode_artifact_to(transformed, manifest, &mut bytes)?;
    let verification = pageknot_html::verify_static(&bytes)?;
    Ok(EncodedArtifact {
        sha256: ContentDigest::sha256(&bytes),
        bytes,
        manifest,
        verification,
    })
}

/// Encodes one transformed document into a caller-owned output.
///
/// The output receives serialized bytes as the document is traversed. Writer
/// limits therefore stop encoding before a full artifact buffer is allocated.
pub fn encode_artifact_to(
    transformed: &TransformedDocument,
    mut manifest: ArtifactManifest,
    output: &mut (impl Write + ?Sized),
) -> Result<ArtifactManifest> {
    if transformed.structural_repair {
        manifest.structural_repair.applied = true;
        manifest.structural_repair.script_sha256 =
            Some(pageknot_html::structural_repair_script_digest());
    }
    let document = Document::parse(&transformed.html);
    pageknot_html::encode_html_to(&document, &manifest, output)?;
    Ok(manifest)
}

/// Transforms, encodes, and verifies one already collected document.
pub fn transform_artifact(html: &[u8], manifest: ArtifactManifest) -> Result<EncodedArtifact> {
    let transformed = transform_document(html)?;
    encode_artifact(&transformed, manifest)
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io::{self, Write};

    use pageknot_model::ArtifactManifest;

    use super::{encode_artifact_to, transform_artifact, transform_document};

    type TestResult = std::result::Result<(), Box<dyn Error + Send + Sync>>;

    #[test]
    fn shared_transform_freezes_and_sanitizes_a_document() -> TestResult {
        let manifest: ArtifactManifest = serde_json::from_str(include_str!(
            "../../../schemas/examples/artifact-manifest.json"
        ))?;
        let artifact = transform_artifact(
            br#"<!doctype html><html><head><style>h1{color:navy}</style></head>
                <body><h1>shared transform</h1><script>globalThis.active = true</script>
                </body></html>"#,
            manifest,
        )?;
        let output = String::from_utf8(artifact.bytes)?;

        assert!(output.contains("shared transform"));
        assert!(output.contains("data-pageknot-freeze"));
        assert!(!output.contains("globalThis.active"));
        assert!(artifact.verification.passed);
        Ok(())
    }

    #[test]
    fn streaming_encoder_stops_at_the_writer_limit() -> TestResult {
        #[derive(Debug)]
        struct LimitedWriter {
            bytes: usize,
            limit: usize,
        }

        impl Write for LimitedWriter {
            fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
                if self.bytes.saturating_add(buffer.len()) > self.limit {
                    return Err(io::Error::new(
                        io::ErrorKind::FileTooLarge,
                        "test output limit reached",
                    ));
                }
                self.bytes += buffer.len();
                Ok(buffer.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let manifest: ArtifactManifest = serde_json::from_str(include_str!(
            "../../../schemas/examples/artifact-manifest.json"
        ))?;
        let transformed =
            transform_document(b"<html><head></head><body><p>bounded encoder</p></body></html>")?;
        let mut writer = LimitedWriter {
            bytes: 0,
            limit: 64,
        };

        let result = encode_artifact_to(&transformed, manifest, &mut writer);

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.artifact.size")
        );
        assert!(writer.bytes <= writer.limit);
        Ok(())
    }
}
