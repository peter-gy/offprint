use lopdf::{Dictionary, Document as PdfDocument, Stream, dictionary, text_string};
use offprint_model::{ErrorStage, OffprintError, Result};

use super::model::PdfMetadata;
use crate::pdf::semantics::resolved_dictionary;

pub(super) fn apply_metadata(
    document: &mut PdfDocument,
    metadata: &PdfMetadata,
    xmp: Vec<u8>,
) -> Result<()> {
    let mut info = existing_info_dictionary(document);
    for field in metadata.info_fields() {
        info.remove(field.key.as_bytes());
        if let Some(value) = field.value {
            info.set(field.key, text_string(&value));
        }
    }

    let info_id = document.add_object(info);
    let metadata_id = document.add_object(Stream::new(
        dictionary! {
            "Type" => "Metadata",
            "Subtype" => "XML",
        },
        xmp,
    ));
    let mut viewer_preferences = existing_catalog_dictionary(document, b"ViewerPreferences");
    viewer_preferences.set("DisplayDocTitle", true);
    let viewer_preferences_id = document.add_object(viewer_preferences);

    document.trailer.set("Info", info_id);
    let catalog = document.catalog_mut().map_err(|error| {
        OffprintError::new(
            "offprint.export.pdf_metadata",
            ErrorStage::Encoding,
            format!("PDF catalog could not be updated: {error}"),
        )
    })?;
    catalog.set("Metadata", metadata_id);
    catalog.set("Lang", text_string(&metadata.source.language));
    catalog.set("ViewerPreferences", viewer_preferences_id);
    Ok(())
}

fn existing_info_dictionary(document: &PdfDocument) -> Dictionary {
    document
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|object| resolved_dictionary(document, object))
        .cloned()
        .unwrap_or_default()
}

fn existing_catalog_dictionary(document: &PdfDocument, key: &[u8]) -> Dictionary {
    document
        .catalog()
        .ok()
        .and_then(|catalog| catalog.get(key).ok())
        .and_then(|object| resolved_dictionary(document, object))
        .cloned()
        .unwrap_or_default()
}
