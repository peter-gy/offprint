use std::collections::BTreeSet;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document as PdfDocument, LoadOptions as PdfLoadOptions, Object, ObjectId};
use offprint_model::{ArtifactFormat, Result};

use crate::FormatEvidence;
use crate::support::{
    MAXIMUM_DECODED_HTML_BYTES, ensure_verified, rfind_bytes, trim_ascii, trim_ascii_end,
};

mod metadata;
mod semantics;

pub use metadata::{embed_pdf_metadata, verify_offprint_pdf};

const FORBIDDEN_KEYS: &[&[u8]] = &[
    b"AA",
    b"AcroForm",
    b"AF",
    b"Collection",
    b"EF",
    b"EmbeddedFiles",
    b"JS",
    b"JavaScript",
    b"OpenAction",
    b"RichMediaContent",
    b"RichMediaSettings",
    b"XFA",
];

const FORBIDDEN_DICTIONARY_KINDS: &[&[u8]] = &[
    b"3D",
    b"EmbeddedFile",
    b"FileAttachment",
    b"Filespec",
    b"JavaScript",
    b"Movie",
    b"PS",
    b"RichMedia",
    b"RichMediaInstance",
    b"RichMediaPresentation",
    b"Screen",
    b"Sound",
];

const FORBIDDEN_ACTIONS: &[&[u8]] = &[
    b"GoTo3DView",
    b"GoToE",
    b"GoToR",
    b"Hide",
    b"ImportData",
    b"JavaScript",
    b"Launch",
    b"Movie",
    b"Named",
    b"Rendition",
    b"ResetForm",
    b"SetOCGState",
    b"Sound",
    b"SubmitForm",
    b"Thread",
    b"Trans",
];

/// Verifies a Chromium PDF at the independent file-format boundary.
pub fn verify_pdf(bytes: &[u8]) -> Result<FormatEvidence> {
    let structure_valid = pdf_envelope_valid(bytes);
    let document = structure_valid
        .then(|| {
            PdfDocument::load_mem_with_options(
                bytes,
                PdfLoadOptions {
                    strict: true,
                    max_decompressed_size: Some(maximum_decoded_bytes()),
                    ..PdfLoadOptions::default()
                },
            )
            .ok()
        })
        .flatten();
    let content_valid = document.as_ref().is_some_and(|document| {
        validated_page_ids(document).is_some_and(|pages| {
            pdf_objects_are_passive(document)
                && pages
                    .iter()
                    .all(|page_id| page_content_streams_parse(document, *page_id))
        })
    });
    ensure_verified(ArtifactFormat::Pdf, structure_valid, content_valid)
}

fn maximum_decoded_bytes() -> usize {
    usize::try_from(MAXIMUM_DECODED_HTML_BYTES).unwrap_or(usize::MAX)
}

fn pdf_envelope_valid(bytes: &[u8]) -> bool {
    if bytes.len() < 16
        || !bytes.starts_with(b"%PDF-1.")
        || !bytes.get(7).is_some_and(u8::is_ascii_digit)
        || !bytes
            .get(8)
            .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
    {
        return false;
    }
    let trimmed = trim_ascii_end(bytes);
    let Some(prefix) = trimmed.strip_suffix(b"%%EOF") else {
        return false;
    };
    let prefix = trim_ascii_end(prefix);
    let Some(marker) = rfind_bytes(prefix, b"startxref") else {
        return false;
    };
    let offset_bytes = trim_ascii(&prefix[marker.saturating_add(b"startxref".len())..]);
    if offset_bytes.is_empty() || !offset_bytes.iter().all(u8::is_ascii_digit) {
        return false;
    }
    let Some(offset) = std::str::from_utf8(offset_bytes)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
    else {
        return false;
    };
    let Some(xref) = bytes.get(offset..) else {
        return false;
    };
    xref.starts_with(b"xref")
        || xref.get(..xref.len().min(1024)).is_some_and(|header| {
            [b"/Type /XRef".as_slice(), b"/Type/XRef"]
                .iter()
                .any(|marker| header.windows(marker.len()).any(|window| window == *marker))
        })
}

fn validated_page_ids(document: &PdfDocument) -> Option<Vec<ObjectId>> {
    if document.trailer.get(b"Encrypt").is_ok() {
        return None;
    }
    let catalog = document.catalog().ok()?;
    if catalog.get_type().ok() != Some(b"Catalog".as_slice()) {
        return None;
    }
    let pages_id = catalog.get(b"Pages").and_then(Object::as_reference).ok()?;
    let page_tree = document.get_dictionary(pages_id).ok()?;
    if page_tree.get_type().ok() != Some(b"Pages".as_slice()) {
        return None;
    }
    let declared_pages = page_tree.get(b"Count").and_then(Object::as_i64).ok()?;
    let pages = document.get_pages();
    if declared_pages <= 0 || usize::try_from(declared_pages).ok() != Some(pages.len()) {
        return None;
    }
    let page_ids = pages.into_values().collect::<Vec<_>>();
    page_ids
        .iter()
        .all(|page_id| {
            document.get_dictionary(*page_id).is_ok_and(|page| {
                page.get_type().ok() == Some(b"Page".as_slice())
                    && page
                        .get(b"Parent")
                        .and_then(Object::as_reference)
                        .and_then(|parent| document.get_dictionary(parent))
                        .and_then(Dictionary::get_type)
                        .is_ok_and(|kind| kind == b"Pages")
            })
        })
        .then_some(page_ids)
}

fn pdf_objects_are_passive(document: &PdfDocument) -> bool {
    dictionary_is_passive(document, &document.trailer, 0)
        && document
            .objects
            .values()
            .all(|object| object_is_passive(document, object, 0))
}

fn object_is_passive(document: &PdfDocument, object: &Object, depth: usize) -> bool {
    if depth > 128 {
        return false;
    }
    match object {
        Object::Array(values) => values
            .iter()
            .all(|value| object_is_passive(document, value, depth.saturating_add(1))),
        Object::Dictionary(dictionary) => {
            dictionary_is_passive(document, dictionary, depth.saturating_add(1))
        }
        Object::Stream(stream) => {
            dictionary_is_passive(document, &stream.dict, depth.saturating_add(1))
        }
        Object::Null
        | Object::Boolean(_)
        | Object::Integer(_)
        | Object::Real(_)
        | Object::Name(_)
        | Object::String(_, _)
        | Object::Reference(_) => true,
    }
}

fn dictionary_is_passive(document: &PdfDocument, dictionary: &Dictionary, depth: usize) -> bool {
    if depth > 128
        || dictionary
            .iter()
            .any(|(key, _)| FORBIDDEN_KEYS.contains(&key.as_slice()))
        || dictionary_kind_is_forbidden(dictionary, b"Type")
        || dictionary_kind_is_forbidden(dictionary, b"Subtype")
        || !action_dictionary_is_safe(dictionary)
        || dictionary
            .get(b"A")
            .is_ok_and(|entry| !a_entry_is_safe(document, dictionary, entry, depth))
    {
        return false;
    }
    dictionary
        .iter()
        .all(|(_, value)| object_is_passive(document, value, depth.saturating_add(1)))
}

fn a_entry_is_safe(
    document: &PdfDocument,
    owner: &Dictionary,
    entry: &Object,
    depth: usize,
) -> bool {
    if owner.has_type(b"StructElem") {
        true
    } else {
        action_object_is_safe(document, entry, &mut BTreeSet::new(), depth)
    }
}

fn dictionary_kind_is_forbidden(dictionary: &Dictionary, key: &[u8]) -> bool {
    dictionary
        .get(key)
        .and_then(Object::as_name)
        .is_ok_and(|kind| FORBIDDEN_DICTIONARY_KINDS.contains(&kind))
}

fn action_dictionary_is_safe(dictionary: &Dictionary) -> bool {
    let action_kind = dictionary.get(b"S").and_then(Object::as_name).ok();
    match action_kind {
        Some(b"GoTo") => true,
        Some(b"URI") => dictionary
            .get(b"URI")
            .and_then(Object::as_str)
            .is_ok_and(pdf_uri_is_safe),
        Some(kind) if FORBIDDEN_ACTIONS.contains(&kind) => false,
        Some(_) => !dictionary.has_type(b"Action"),
        None => !dictionary.has_type(b"Action"),
    }
}

fn action_object_is_safe(
    document: &PdfDocument,
    object: &Object,
    resolving: &mut BTreeSet<ObjectId>,
    depth: usize,
) -> bool {
    if depth > 128 {
        return false;
    }
    match object {
        Object::Reference(id) => {
            if !resolving.insert(*id) {
                return false;
            }
            let valid = document.get_object(*id).is_ok_and(|action| {
                action_object_is_safe(document, action, resolving, depth.saturating_add(1))
            });
            resolving.remove(id);
            valid
        }
        Object::Array(actions) => actions.iter().all(|action| {
            action_object_is_safe(document, action, resolving, depth.saturating_add(1))
        }),
        Object::Dictionary(action) => {
            let Some(kind) = action
                .get(b"S")
                .ok()
                .and_then(|kind| resolved_name(document, kind, depth.saturating_add(1)))
            else {
                return false;
            };
            let primary_is_safe = match kind.as_slice() {
                b"GoTo" => true,
                b"URI" => action
                    .get(b"URI")
                    .ok()
                    .and_then(|uri| resolved_string(document, uri, depth.saturating_add(1)))
                    .is_some_and(|uri| pdf_uri_is_safe(&uri)),
                _ => false,
            };
            let next_is_safe = match action.get(b"Next") {
                Ok(next) => {
                    action_object_is_safe(document, next, resolving, depth.saturating_add(1))
                }
                Err(_) => true,
            };
            primary_is_safe
                && dictionary_is_passive(document, action, depth.saturating_add(1))
                && next_is_safe
        }
        Object::Boolean(_)
        | Object::Integer(_)
        | Object::Name(_)
        | Object::Null
        | Object::Real(_)
        | Object::Stream(_)
        | Object::String(_, _) => false,
    }
}

fn resolved_name(document: &PdfDocument, object: &Object, depth: usize) -> Option<Vec<u8>> {
    if depth > 128 {
        return None;
    }
    match object {
        Object::Name(name) => Some(name.clone()),
        Object::Reference(id) => document
            .get_object(*id)
            .ok()
            .and_then(|value| resolved_name(document, value, depth.saturating_add(1))),
        _ => None,
    }
}

fn resolved_string(document: &PdfDocument, object: &Object, depth: usize) -> Option<Vec<u8>> {
    if depth > 128 {
        return None;
    }
    match object {
        Object::String(value, _) => Some(value.clone()),
        Object::Reference(id) => document
            .get_object(*id)
            .ok()
            .and_then(|value| resolved_string(document, value, depth.saturating_add(1))),
        _ => None,
    }
}

fn pdf_uri_is_safe(uri: &[u8]) -> bool {
    let Ok(uri) = std::str::from_utf8(uri) else {
        return false;
    };
    if uri
        .chars()
        .any(|character| character.is_ascii_control() || character.is_ascii_whitespace())
    {
        return false;
    }
    let Some((scheme, _)) = uri.split_once(':') else {
        return false;
    };
    matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "mailto" | "tel"
    )
}

fn page_content_streams_parse(document: &PdfDocument, page_id: ObjectId) -> bool {
    let Ok(page) = document.get_dictionary(page_id) else {
        return false;
    };
    let Ok(contents) = page.get(b"Contents") else {
        return true;
    };
    let mut remaining = maximum_decoded_bytes();
    let mut resolving = BTreeSet::new();
    parse_content_object(document, contents, &mut resolving, &mut remaining)
}

fn parse_content_object(
    document: &PdfDocument,
    object: &Object,
    resolving: &mut BTreeSet<ObjectId>,
    remaining: &mut usize,
) -> bool {
    match object {
        Object::Null => true,
        Object::Reference(id) => {
            if !resolving.insert(*id) {
                return false;
            }
            let valid = document.get_object(*id).is_ok_and(|resolved| {
                parse_content_object(document, resolved, resolving, remaining)
            });
            resolving.remove(id);
            valid
        }
        Object::Array(contents) => contents
            .iter()
            .all(|content| parse_content_object(document, content, resolving, remaining)),
        Object::Stream(stream) => {
            let Ok(decoded) = stream.decompressed_content_with_limit(*remaining) else {
                return false;
            };
            *remaining = remaining.saturating_sub(decoded.len());
            Content::<Vec<Operation>>::decode_strict(&decoded).is_ok()
        }
        Object::Boolean(_)
        | Object::Dictionary(_)
        | Object::Integer(_)
        | Object::Name(_)
        | Object::Real(_)
        | Object::String(_, _) => false,
    }
}

#[cfg(test)]
mod tests {
    use offprint_model::Result;

    use super::verify_pdf;

    fn pdf_with_objects(objects: &[&[u8]]) -> Vec<u8> {
        let mut bytes = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n", index.saturating_add(1)).as_bytes());
            bytes.extend_from_slice(object);
            bytes.extend_from_slice(b"\nendobj\n");
        }
        let xref = bytes.len();
        let mut trailer = format!(
            "xref\n0 {}\n0000000000 65535 f \n",
            objects.len().saturating_add(1)
        );
        for offset in offsets {
            trailer.push_str(&format!("{offset:010} 00000 n \n"));
        }
        trailer.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len().saturating_add(1)
        ));
        bytes.extend_from_slice(trailer.as_bytes());
        bytes
    }

    fn passive_pdf(page: &[u8], extra_objects: &[&[u8]]) -> Vec<u8> {
        let mut objects = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".as_slice(),
            b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".as_slice(),
            page,
        ];
        objects.extend_from_slice(extra_objects);
        pdf_with_objects(&objects)
    }

    #[test]
    fn pdf_verifier_parses_the_catalog_and_page_tree() -> Result<()> {
        let pdf = passive_pdf(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>",
            &[],
        );

        assert!(verify_pdf(&pdf).is_ok());
        assert!(verify_pdf(b"%PDF-1.4\n/Type /Page\nxref\nstartxref\n20\n%%EOF\n").is_err());
        Ok(())
    }

    #[test]
    fn pdf_verifier_rejects_automatic_actions() {
        let cases = [
            (
                b"<< /Type /Catalog /Pages 2 0 R /OpenAction 4 0 R >>".as_slice(),
                b"<< /Type /Action /S /JavaScript /JS (app.alert) >>".as_slice(),
            ),
            (
                b"<< /Type /Catalog /Pages 2 0 R /AA << /WC 4 0 R >> >>".as_slice(),
                b"<< /Type /Action /S /Launch /F (payload) >>".as_slice(),
            ),
        ];

        for (catalog, action) in cases {
            let pdf = pdf_with_objects(&[
                catalog,
                b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>",
                action,
            ]);
            assert!(verify_pdf(&pdf).is_err());
        }
    }

    #[test]
    fn pdf_verifier_rejects_executable_action_dictionaries() {
        let page = b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Annots [4 0 R] >>";
        let cases = [
            passive_pdf(
                page,
                &[
                    b"<< /Type /Annot /Subtype /Link /A 5 0 R >>",
                    b"<< /S /JavaScript /JS (app.alert) >>",
                ],
            ),
            passive_pdf(
                page,
                &[
                    b"<< /Type /Annot /Subtype /Link /A 5 0 R >>",
                    b"<< /S /Launch /F (payload) >>",
                ],
            ),
            passive_pdf(
                page,
                &[
                    b"<< /Type /Annot /Subtype /Link /A 5 0 R >>",
                    b"<< /S 6 0 R >>",
                    b"/JavaScript",
                ],
            ),
            passive_pdf(
                page,
                &[
                    b"<< /Type /Annot /Subtype /Link /A 5 0 R >>",
                    b"<< /Type /Action /S /ExtensionAction >>",
                ],
            ),
            passive_pdf(
                b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>",
                &[b"<< /Type /XObject /Subtype /PS >>"],
            ),
        ];

        for pdf in cases {
            assert!(verify_pdf(&pdf).is_err());
        }
    }

    #[test]
    fn pdf_verifier_rejects_embedded_files_and_rich_media() {
        let embedded = passive_pdf(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] >>",
            &[
                b"<< /Type /Filespec /EF << /F 5 0 R >> >>",
                b"<< /Type /EmbeddedFile /Length 3 >>\nstream\nabc\nendstream",
            ],
        );
        let rich_media = passive_pdf(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Annots [4 0 R] >>",
            &[
                b"<< /Type /Annot /Subtype /RichMedia /RichMediaContent 5 0 R >>",
                b"<< >>",
            ],
        );

        assert!(verify_pdf(&embedded).is_err());
        assert!(verify_pdf(&rich_media).is_err());
    }

    #[test]
    fn pdf_verifier_parses_every_referenced_page_content_stream() {
        let valid = passive_pdf(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents [4 0 R 5 0 R] >>",
            &[
                b"<< /Length 1 >>\nstream\nq\nendstream",
                b"<< /Length 1 >>\nstream\nQ\nendstream",
            ],
        );
        let malformed = passive_pdf(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents [4 0 R 5 0 R] >>",
            &[
                b"<< /Length 1 >>\nstream\nq\nendstream",
                b"<< /Length 1 >>\nstream\n(\nendstream",
            ],
        );

        assert!(verify_pdf(&valid).is_ok());
        assert!(verify_pdf(&malformed).is_err());
    }

    #[test]
    fn pdf_verifier_allows_explicit_safe_uri_navigation() {
        let pdf = passive_pdf(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Annots [4 0 R] >>",
            &[
                b"<< /Type /Annot /Subtype /Link /A 5 0 R >>",
                b"<< /Type /Action /S /URI /URI (https://example.test/) >>",
            ],
        );

        assert!(verify_pdf(&pdf).is_ok());
    }

    #[test]
    fn pdf_verifier_allows_tagged_structure_attributes() {
        let pdf = passive_pdf(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /StructParents 0 >>",
            &[
                b"<< /Type /StructElem /S /Form /Pg 3 0 R /A [5 0 R] >>",
                b"<< /O /PrintField /Role /pb >>",
            ],
        );

        assert!(verify_pdf(&pdf).is_ok());
    }

    #[test]
    fn pdf_verifier_rejects_local_file_navigation() {
        let pdf = passive_pdf(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Annots [4 0 R] >>",
            &[
                b"<< /Type /Annot /Subtype /Link /A 5 0 R >>",
                b"<< /Type /Action /S /URI /URI (file:///private/capture.html) >>",
            ],
        );

        assert!(verify_pdf(&pdf).is_err());
    }
}
