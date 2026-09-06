mod apply;
mod model;
mod source;
mod verify;
mod xmp;

use offprint_model::{
    ArtifactFormat, ArtifactManifest, ContentDigest, ErrorStage, OffprintError, Result,
};

use super::semantics::{PdfSemantics, load_pdf};
use super::verify_pdf;
use crate::FormatEvidence;
use crate::support::{ensure_verified, export_error, export_io_error};

use self::apply::apply_metadata;
use self::model::PdfMetadata;
use self::source::SourceMetadata;
use self::verify::verify_metadata;
use self::xmp::encode_xmp;

/// Adds source metadata and Offprint provenance to a passive tagged PDF.
///
/// `pdf` must be the native Chromium print result for `html`. The returned PDF
/// preserves Chromium's text, links, outlines, fonts, and structure tree while
/// adding standard document properties and a UTF-8 XMP packet.
pub fn embed_pdf_metadata(
    pdf: &[u8],
    html: &[u8],
    manifest: &ArtifactManifest,
    source_artifact_sha256: ContentDigest,
    maximum_bytes: u64,
) -> Result<Vec<u8>> {
    if ContentDigest::sha256(html) != source_artifact_sha256 {
        return Err(OffprintError::new(
            "offprint.export.source",
            ErrorStage::Encoding,
            "PDF metadata source digest does not match the verified HTML artifact",
        ));
    }
    verify_pdf(pdf)?;
    let mut document = load_pdf(pdf, ErrorStage::Encoding)?;
    let semantics = PdfSemantics::inspect(&document, ErrorStage::Encoding)?;
    if !semantics.tagged {
        return Err(OffprintError::new(
            "offprint.export.pdf_semantics",
            ErrorStage::Encoding,
            "Chromium PDF output has no tagged document structure",
        ));
    }

    let source = SourceMetadata::from_html(html, manifest);
    let metadata = PdfMetadata::new(
        source,
        manifest,
        source_artifact_sha256,
        semantics,
        document.version.clone(),
    )?;
    let xmp = encode_xmp(&metadata)?;
    apply_metadata(&mut document, &metadata, xmp)?;

    let mut encoded = Vec::with_capacity(pdf.len().saturating_add(16 * 1024));
    document.save_to(&mut encoded).map_err(export_io_error)?;
    if u64::try_from(encoded.len()).unwrap_or(u64::MAX) > maximum_bytes {
        return Err(OffprintError::new(
            "offprint.export.size",
            ErrorStage::Encoding,
            format!("PDF output exceeds the {maximum_bytes}-byte export limit"),
        ));
    }

    let finalized = load_pdf(&encoded, ErrorStage::Verification)?;
    let actual_metadata = verify_metadata(&finalized)?;
    if actual_metadata != metadata {
        return Err(export_error(
            "offprint.export.verify",
            "PDF metadata did not survive file serialization",
        ));
    }
    verify_offprint_pdf(&encoded)?;
    Ok(encoded)
}

/// Verifies the passive file structure and semantic metadata of an Offprint PDF.
pub fn verify_offprint_pdf(bytes: &[u8]) -> Result<FormatEvidence> {
    let evidence = verify_pdf(bytes)?;
    let semantic_valid = load_pdf(bytes, ErrorStage::Verification)
        .ok()
        .is_some_and(|document| verify_metadata(&document).is_ok());
    ensure_verified(
        ArtifactFormat::Pdf,
        evidence.structure_valid,
        evidence.content_valid && semantic_valid,
    )
}

#[cfg(test)]
mod tests;
