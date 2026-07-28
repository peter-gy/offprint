use pageknot_model::{ArtifactManifest, ArtifactVariantKind, ErrorStage, PageKnotError, Result};

use crate::VariantEvidence;

pub(crate) const MAXIMUM_DECODED_HTML_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAXIMUM_DECODED_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

pub(crate) fn ensure_verified(
    kind: ArtifactVariantKind,
    structure_valid: bool,
    content_valid: bool,
) -> Result<VariantEvidence> {
    if structure_valid && content_valid {
        Ok(VariantEvidence {
            structure_valid,
            content_valid,
        })
    } else {
        Err(export_error(
            "pageknot.export.verify",
            format!("{kind:?} output failed format-specific verification"),
        )
        .with_detail("structureValid", structure_valid)
        .with_detail("contentValid", content_valid))
    }
}

pub(crate) fn export_error(code: &'static str, message: impl Into<String>) -> PageKnotError {
    PageKnotError::new(code, ErrorStage::Verification, message)
}

pub(crate) fn export_io_error(error: std::io::Error) -> PageKnotError {
    PageKnotError::new(
        "pageknot.export.encode",
        ErrorStage::Encoding,
        format!("artifact variant I/O failed: {error}"),
    )
}

pub(crate) fn export_verify_io_error(error: std::io::Error) -> PageKnotError {
    export_error(
        "pageknot.export.verify",
        format!("artifact variant verification I/O failed: {error}"),
    )
}

pub(crate) fn export_serialize_error(error: serde_json::Error) -> PageKnotError {
    PageKnotError::new(
        "pageknot.export.encode",
        ErrorStage::Encoding,
        format!("artifact variant metadata could not be serialized: {error}"),
    )
}

pub(crate) fn manifest_encoding_matches(
    bytes: &[u8],
    manifest: &ArtifactManifest,
    trailing_newline: bool,
) -> bool {
    serde_json::to_vec_pretty(manifest).is_ok_and(|mut canonical| {
        if trailing_newline {
            canonical.push(b'\n');
        }
        canonical == bytes
    })
}

pub(crate) fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let trimmed = trim_ascii_end(bytes);
    trimmed
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map_or(&[], |start| &trimmed[start..])
}

pub(crate) fn trim_ascii_end(bytes: &[u8]) -> &[u8] {
    let length = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(0, |index| index.saturating_add(1));
    &bytes[..length]
}

pub(crate) fn rfind_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}
