use lopdf::{Dictionary, Document as PdfDocument, LoadOptions as PdfLoadOptions, Object};
use offprint_model::{ErrorStage, OffprintError, Result};

use super::maximum_decoded_bytes;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PdfSemantics {
    pub(super) pages: u64,
    pub(super) structure_elements: u64,
    pub(super) tagged: bool,
    pub(super) has_document_outline: bool,
    pub(super) has_text_structure: bool,
}

impl PdfSemantics {
    pub(super) fn inspect(document: &PdfDocument, stage: ErrorStage) -> Result<Self> {
        let pages = document.get_pages();
        let catalog = document.catalog().map_err(|error| {
            OffprintError::new(
                "offprint.export.pdf_semantics",
                stage,
                format!("PDF catalog could not be read: {error}"),
            )
        })?;
        let tagged = catalog
            .get(b"StructTreeRoot")
            .ok()
            .and_then(|object| resolved_dictionary(document, object))
            .is_some_and(|dictionary| dictionary.has_type(b"StructTreeRoot"))
            && catalog
                .get(b"MarkInfo")
                .ok()
                .and_then(|object| resolved_dictionary(document, object))
                .and_then(|dictionary| dictionary.get(b"Marked").ok())
                .and_then(|value| value.as_bool().ok())
                == Some(true);
        let has_document_outline = catalog
            .get(b"Outlines")
            .ok()
            .and_then(|object| resolved_dictionary(document, object))
            .is_some_and(|dictionary| dictionary.has_type(b"Outlines"));
        let mut structure_elements = 0usize;
        let mut has_text_structure = false;
        for object in document.objects.values() {
            let Some(dictionary) =
                object_dictionary(object).filter(|dictionary| dictionary.has_type(b"StructElem"))
            else {
                continue;
            };
            structure_elements += 1;
            has_text_structure |= dictionary
                .get(b"S")
                .and_then(Object::as_name)
                .is_ok_and(structure_role_is_textual);
        }

        Ok(Self {
            pages: u64::try_from(pages.len()).unwrap_or(u64::MAX),
            structure_elements: u64::try_from(structure_elements).unwrap_or(u64::MAX),
            tagged,
            has_document_outline,
            has_text_structure,
        })
    }
}

pub(super) fn load_pdf(bytes: &[u8], stage: ErrorStage) -> Result<PdfDocument> {
    PdfDocument::load_mem_with_options(
        bytes,
        PdfLoadOptions {
            strict: true,
            max_decompressed_size: Some(maximum_decoded_bytes()),
            ..PdfLoadOptions::default()
        },
    )
    .map_err(|error| {
        OffprintError::new(
            "offprint.export.pdf",
            stage,
            format!("PDF document could not be parsed: {error}"),
        )
    })
}

pub(super) fn resolved_object<'a>(
    document: &'a PdfDocument,
    object: &'a Object,
) -> Option<&'a Object> {
    document.dereference(object).ok().map(|(_, value)| value)
}

pub(super) fn resolved_dictionary<'a>(
    document: &'a PdfDocument,
    object: &'a Object,
) -> Option<&'a Dictionary> {
    resolved_object(document, object).and_then(|value| value.as_dict().ok())
}

fn object_dictionary(object: &Object) -> Option<&Dictionary> {
    match object {
        Object::Dictionary(dictionary) => Some(dictionary),
        Object::Stream(stream) => Some(&stream.dict),
        _ => None,
    }
}

fn structure_role_is_textual(role: &[u8]) -> bool {
    matches!(
        role,
        b"H" | b"H1"
            | b"H2"
            | b"H3"
            | b"H4"
            | b"H5"
            | b"H6"
            | b"P"
            | b"Lbl"
            | b"LBody"
            | b"TH"
            | b"TD"
            | b"Span"
            | b"Quote"
            | b"Note"
            | b"Reference"
            | b"BibEntry"
            | b"Code"
            | b"Link"
            | b"Ruby"
            | b"Warichu"
            | b"Form"
    )
}
