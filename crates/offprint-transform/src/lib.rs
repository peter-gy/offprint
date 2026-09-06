//! Shared safe-static transformation for the native capture pipeline.
//!
//! Browser sessions collect rendered state and resolve resources before this
//! boundary. The transformer freezes rendering, removes active content,
//! applies structural repair, encodes the manifest, and runs the independent
//! static verifier.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

use std::io::Write;

use offprint_document::{
    Document, ensure_render_freeze_styles, sanitize_safe_static, serialize_document,
};
use offprint_model::{
    ArtifactManifest, ContentDigest, ErrorStage, OffprintError, Result, VerificationReport,
};

const MAXIMUM_INLINE_FRAME_DEPTH: u16 = 64;

#[derive(Clone, Debug)]
/// Parsed document state after the shared safe-static transform.
pub struct SafeStaticDocument {
    document: Document,
    structural_repair: bool,
}

impl SafeStaticDocument {
    /// Returns the transformed document.
    #[must_use]
    pub const fn document(&self) -> &Document {
        &self.document
    }

    /// Reports whether this document or one of its inline frames uses repair.
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
    pub verification: VerificationReport,
}

/// Parses and applies the host-independent safe-static transformation.
pub fn transform_document(html: &[u8]) -> Result<SafeStaticDocument> {
    transform_parsed_document(Document::parse(html))
}

/// Applies the safe-static transformation to an already parsed document.
pub fn transform_parsed_document(mut document: Document) -> Result<SafeStaticDocument> {
    let structural_repair = transform_document_tree(&mut document, 0)?;
    Ok(SafeStaticDocument {
        document,
        structural_repair,
    })
}

fn transform_document_tree(document: &mut Document, depth: u16) -> Result<bool> {
    if depth > MAXIMUM_INLINE_FRAME_DEPTH {
        return Err(OffprintError::new(
            "offprint.frame.depth",
            ErrorStage::Transform,
            "inline frame depth exceeds the safe-static transform limit",
        ));
    }

    let frames = document.inline_frames();
    let mut structural_repair = false;
    for frame in frames {
        let mut child = Document::parse(frame.html.as_bytes());
        structural_repair |= transform_document_tree(&mut child, depth.saturating_add(1))?;
        let html = serialize_transformed_document(&child)?;
        document.replace_inline_frame(&frame, &html, None)?;
    }

    ensure_render_freeze_styles(document)?;
    let _sanitization = sanitize_safe_static(document);
    structural_repair |= offprint_html::apply_structural_repair(document)?;
    Ok(structural_repair)
}

fn serialize_transformed_document(document: &Document) -> Result<String> {
    let bytes = serialize_document(document).map_err(|error| {
        OffprintError::new(
            "offprint.artifact.serialize",
            ErrorStage::Transform,
            format!("failed to serialize the transformed document: {error}"),
        )
    })?;
    String::from_utf8(bytes).map_err(|error| {
        OffprintError::new(
            "offprint.artifact.serialize",
            ErrorStage::Transform,
            format!("transformed document is not UTF-8: {error}"),
        )
    })
}

/// Encodes and statically verifies one transformed document.
pub fn encode_artifact(
    transformed: &SafeStaticDocument,
    manifest: ArtifactManifest,
) -> Result<EncodedArtifact> {
    let verified = encode_verified_artifact(transformed, manifest)?;
    let (bytes, _document, manifest, sha256, verification) = verified.into_parts();
    Ok(EncodedArtifact {
        bytes,
        sha256,
        manifest,
        verification,
    })
}

/// Encodes and returns one proof-bearing safe-static HTML artifact.
pub fn encode_verified_artifact(
    transformed: &SafeStaticDocument,
    manifest: ArtifactManifest,
) -> Result<offprint_html::VerifiedHtml<Vec<u8>>> {
    let mut bytes = Vec::new();
    let manifest = encode_artifact_to(transformed, manifest, &mut bytes)?;
    let verified = offprint_html::verify_html(bytes)?;
    debug_assert_eq!(&manifest, verified.manifest());
    Ok(verified)
}

/// Encodes one transformed document into a caller-owned output.
///
/// The output receives serialized bytes as the document is traversed. Writer
/// limits therefore stop encoding before a full artifact buffer is allocated.
pub fn encode_artifact_to(
    transformed: &SafeStaticDocument,
    mut manifest: ArtifactManifest,
    output: &mut (impl Write + ?Sized),
) -> Result<ArtifactManifest> {
    manifest.structural_repair.applied = transformed.structural_repair;
    manifest.structural_repair.script_sha256 = transformed
        .structural_repair
        .then(offprint_html::structural_repair_script_digest);
    offprint_html::encode_html_to(transformed.document(), &manifest, output)?;
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

    use offprint_model::{ArtifactManifest, ContentDigest, StructuralRepair};

    use super::{encode_artifact_to, transform_artifact, transform_document};

    type TestResult = std::result::Result<(), Box<dyn Error + Send + Sync>>;

    fn manifest() -> Result<ArtifactManifest, serde_json::Error> {
        serde_json::from_str(include_str!(
            "../../../schemas/examples/artifact-manifest.json"
        ))
    }

    #[test]
    fn shared_transform_freezes_and_sanitizes_a_document() -> TestResult {
        let artifact = transform_artifact(
            br#"<!doctype html><html><head><style>h1{color:navy}</style></head>
                <body><h1>shared transform</h1><script>globalThis.active = true</script>
                </body></html>"#,
            manifest()?,
        )?;
        let output = String::from_utf8(artifact.bytes)?;

        assert!(output.contains("shared transform"));
        assert!(output.contains("data-offprint-freeze"));
        assert!(!output.contains("globalThis.active"));
        Ok(())
    }

    #[test]
    fn structural_repair_state_replaces_stale_true_manifest_state() -> TestResult {
        let transformed =
            transform_document(b"<html><head></head><body><p>stable</p></body></html>")?;
        let mut stale = manifest()?;
        stale.structural_repair = StructuralRepair {
            applied: true,
            script_sha256: Some(ContentDigest::sha256(b"stale")),
        };

        let artifact = super::encode_artifact(&transformed, stale)?;

        assert!(!artifact.manifest.structural_repair.applied);
        assert_eq!(artifact.manifest.structural_repair.script_sha256, None);
        Ok(())
    }

    #[test]
    fn structural_repair_state_replaces_stale_false_manifest_state() -> TestResult {
        let source = format!(
            r#"<html data-offprint-node="0"><head data-offprint-node="1">
            <script id="{}" type="{}">{{
              "documentElement": {{
                "kind": "element",
                "marker": "0",
                "namespace": "http://www.w3.org/1999/xhtml",
                "name": "html",
                "children": [],
                "templateContent": []
              }}
            }}</script></head><body data-offprint-node="2"></body></html>"#,
            offprint_html::REPAIR_DATA_ELEMENT_ID,
            offprint_html::REPAIR_MEDIA_TYPE,
        );
        let transformed = transform_document(source.as_bytes())?;
        let artifact = super::encode_artifact(&transformed, manifest()?)?;

        assert!(artifact.manifest.structural_repair.applied);
        assert_eq!(
            artifact.manifest.structural_repair.script_sha256,
            Some(offprint_html::structural_repair_script_digest())
        );
        Ok(())
    }

    #[test]
    fn nested_inline_frames_are_transformed_before_encoding() -> TestResult {
        let artifact = transform_artifact(
            br#"<html><head></head><body>
                <iframe data-offprint-frame-id="2" srcdoc="<html><head></head><body><script>active()</script><p>frame</p></body></html>"></iframe>
                </body></html>"#,
            {
                let mut manifest = manifest()?;
                manifest.frames = 2;
                manifest
            },
        )?;
        let output = String::from_utf8(artifact.bytes)?;

        assert!(output.contains("frame"));
        assert!(!output.contains("active()"));
        assert_eq!(output.matches("data-offprint-freeze").count(), 2);
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

        let transformed =
            transform_document(b"<html><head></head><body><p>bounded encoder</p></body></html>")?;
        let mut writer = LimitedWriter {
            bytes: 0,
            limit: 64,
        };

        let result = encode_artifact_to(&transformed, manifest()?, &mut writer);

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.artifact.size")
        );
        assert!(writer.bytes <= writer.limit);
        Ok(())
    }
}
