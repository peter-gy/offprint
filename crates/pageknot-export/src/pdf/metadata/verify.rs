use lopdf::{Dictionary, Document as PdfDocument, Object, decode_text_string};
use pageknot_model::{ErrorStage, PageKnotError, Result};

use super::model::{InfoField, PdfMetadata};
use super::xmp::decode_xmp;
use crate::pdf::semantics::{PdfSemantics, resolved_dictionary, resolved_object};
use crate::support::MAXIMUM_DECODED_MANIFEST_BYTES;

pub(super) fn verify_metadata(document: &PdfDocument) -> Result<PdfMetadata> {
    let semantics = PdfSemantics::inspect(document, ErrorStage::Verification)?;
    if !semantics.tagged {
        return Err(verification_error(
            "PDF document has no tagged structure tree",
        ));
    }
    let catalog = document
        .catalog()
        .map_err(|error| verification_error(format!("PDF catalog could not be read: {error}")))?;
    let xmp = metadata_stream(document, catalog)?;
    let metadata = decode_xmp(&xmp)?;

    if metadata.pdf_version != document.version {
        return Err(verification_error(
            "XMP PDF version does not match the PDF header",
        ));
    }
    if metadata.semantics != semantics {
        return Err(verification_error(
            "XMP semantic counts do not match the PDF structure",
        ));
    }
    if !catalog_language_matches(document, catalog, &metadata.source.language) {
        return Err(verification_error(
            "PDF catalog language does not match XMP metadata",
        ));
    }
    if !display_title_is_enabled(document, catalog) {
        return Err(verification_error(
            "PDF viewer preferences do not display the document title",
        ));
    }
    if !info_dictionary_matches(document, &metadata) {
        return Err(verification_error(
            "PDF Info dictionary does not match XMP metadata",
        ));
    }
    Ok(metadata)
}

fn metadata_stream(document: &PdfDocument, catalog: &Dictionary) -> Result<Vec<u8>> {
    let stream = catalog
        .get(b"Metadata")
        .ok()
        .and_then(|object| resolved_object(document, object))
        .and_then(|object| object.as_stream().ok())
        .ok_or_else(|| verification_error("PDF XMP metadata stream is missing"))?;
    if !stream.dict.has_type(b"Metadata")
        || stream.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"XML".as_slice())
    {
        return Err(verification_error(
            "PDF XMP metadata stream has an invalid type",
        ));
    }
    stream
        .get_plain_content_with_limit(
            usize::try_from(MAXIMUM_DECODED_MANIFEST_BYTES).unwrap_or(usize::MAX),
        )
        .map_err(|error| verification_error(format!("PDF XMP metadata could not be read: {error}")))
}

fn catalog_language_matches(document: &PdfDocument, catalog: &Dictionary, expected: &str) -> bool {
    catalog
        .get(b"Lang")
        .ok()
        .and_then(|value| resolved_object(document, value))
        .and_then(|value| decode_text_string(value).ok())
        .is_some_and(|value| value == expected)
}

fn display_title_is_enabled(document: &PdfDocument, catalog: &Dictionary) -> bool {
    catalog
        .get(b"ViewerPreferences")
        .ok()
        .and_then(|value| resolved_dictionary(document, value))
        .and_then(|preferences| preferences.get(b"DisplayDocTitle").ok())
        .and_then(|value| value.as_bool().ok())
        == Some(true)
}

fn info_dictionary_matches(document: &PdfDocument, metadata: &PdfMetadata) -> bool {
    let Some(info) = document
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|value| resolved_dictionary(document, value))
    else {
        return false;
    };
    metadata
        .info_fields()
        .iter()
        .all(|field| info_field_matches(info, field))
}

fn info_field_matches(info: &Dictionary, field: &InfoField) -> bool {
    match (info.get(field.key.as_bytes()).ok(), field.value.as_deref()) {
        (None, None) => true,
        (Some(actual), Some(expected)) => {
            decode_text_string(actual).is_ok_and(|actual| actual == expected)
        }
        _ => false,
    }
}

fn verification_error(message: impl Into<String>) -> PageKnotError {
    PageKnotError::new("pageknot.export.verify", ErrorStage::Verification, message)
}
