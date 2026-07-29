use std::collections::BTreeSet;
use std::io;

use html5ever::Attribute;
use lopdf::{
    Dictionary, Document as PdfDocument, LoadOptions as PdfLoadOptions, Object, Stream,
    decode_text_string, dictionary, text_string,
};
use pageknot_document::{Document as HtmlDocument, NodeData};
use pageknot_model::{
    ArtifactManifest, ArtifactVariantKind, BrowserProduct, ContentDigest, ErrorStage,
    PageKnotError, Result, VerificationPolicy,
};
use quick_xml::Reader;
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesPI, BytesStart, BytesText, Event};

use super::{maximum_decoded_bytes, verify_pdf};
use crate::VariantEvidence;
use crate::support::{
    MAXIMUM_DECODED_MANIFEST_BYTES, ensure_verified, export_error, export_io_error,
};

const MAXIMUM_FIELD_CHARACTERS: usize = 16 * 1024;
const MAXIMUM_LIST_ITEMS: usize = 128;
const MAXIMUM_LIST_ITEM_CHARACTERS: usize = 1024;
const PAGEKNOT_XMP_NAMESPACE: &str = "https://github.com/peter-gy/pageknot/xmp/pdf/1.0/";

/// Adds source metadata and PageKnot provenance to a passive tagged PDF.
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
        return Err(PageKnotError::new(
            "pageknot.export.source",
            ErrorStage::Encoding,
            "PDF metadata source digest does not match the verified HTML artifact",
        ));
    }
    verify_pdf(pdf)?;
    let mut document = load_pdf(pdf, ErrorStage::Encoding)?;
    let semantics = PdfSemantics::inspect(&document, ErrorStage::Encoding)?;
    if !semantics.tagged {
        return Err(PageKnotError::new(
            "pageknot.export.pdf_semantics",
            ErrorStage::Encoding,
            "Chromium PDF output has no tagged document structure",
        ));
    }

    let source = SourceMetadata::from_html(html, manifest);
    let embedded = EmbeddedMetadata::new(
        &source,
        manifest,
        source_artifact_sha256,
        semantics,
        document.version.clone(),
    )?;
    let xmp = write_xmp(&source, manifest, &embedded)?;
    let expected_info = apply_metadata(&mut document, &source, manifest, &embedded, xmp.clone())?;

    let mut encoded = Vec::with_capacity(pdf.len().saturating_add(xmp.len()).saturating_add(4096));
    document.save_to(&mut encoded).map_err(export_io_error)?;
    if u64::try_from(encoded.len()).unwrap_or(u64::MAX) > maximum_bytes {
        return Err(PageKnotError::new(
            "pageknot.export.size",
            ErrorStage::Encoding,
            format!("PDF output exceeds the {maximum_bytes}-byte export limit"),
        ));
    }

    verify_pageknot_pdf(&encoded)?;
    let finalized = load_pdf(&encoded, ErrorStage::Verification)?;
    if !embedded_metadata_matches(
        &finalized,
        &expected_info,
        &source.language,
        &xmp,
        &embedded,
    ) {
        return Err(export_error(
            "pageknot.export.verify",
            "PDF metadata did not survive file serialization",
        ));
    }
    Ok(encoded)
}

/// Verifies the passive file structure and semantic metadata of a PageKnot PDF.
pub fn verify_pageknot_pdf(bytes: &[u8]) -> Result<VariantEvidence> {
    let evidence = verify_pdf(bytes)?;
    let semantic_valid = load_pdf(bytes, ErrorStage::Verification)
        .ok()
        .is_some_and(|document| pageknot_pdf_semantics_are_valid(&document));
    ensure_verified(
        ArtifactVariantKind::Pdf,
        evidence.structure_valid,
        evidence.content_valid && semantic_valid,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PdfSemantics {
    pages: u64,
    structure_elements: u64,
    tagged: bool,
    has_document_outline: bool,
    has_text_structure: bool,
}

impl PdfSemantics {
    fn inspect(document: &PdfDocument, stage: ErrorStage) -> Result<Self> {
        let pages = document.get_pages();
        let catalog = document.catalog().map_err(|error| {
            PageKnotError::new(
                "pageknot.export.pdf_semantics",
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceMetadata {
    title: String,
    authors: Vec<String>,
    description: Option<String>,
    keywords: Vec<String>,
    language: String,
    rights: Option<String>,
    site_name: Option<String>,
    source_generator: Option<String>,
    published_at: Option<String>,
    modified_at: Option<String>,
}

impl SourceMetadata {
    fn from_html(html: &[u8], manifest: &ArtifactManifest) -> Self {
        let document = HtmlDocument::parse(html);
        let mut title_element = None;
        let mut social_title = None;
        let mut authors = Vec::new();
        let mut description = None;
        let mut social_description = None;
        let mut keywords = Vec::new();
        let mut language = None;
        let mut rights = None;
        let mut site_name = None;
        let mut source_generator = None;
        let mut published_at = None;
        let mut modified_at = None;

        for id in document.walk() {
            let Some(node) = document.node(id) else {
                continue;
            };
            let NodeData::Element { name, attrs, .. } = &node.data else {
                continue;
            };
            match name.local.as_ref() {
                "html" => {
                    language = language.or_else(|| {
                        attribute(attrs, "lang")
                            .filter(|value| language_tag_is_valid(value.trim()))
                            .map(|value| value.trim().to_owned())
                    });
                }
                "title" => {
                    title_element = normalized_optional(
                        &descendant_text(&document, id),
                        MAXIMUM_FIELD_CHARACTERS,
                    );
                }
                "meta" => {
                    let Some(content) = attribute(attrs, "content") else {
                        continue;
                    };
                    let key = attribute(attrs, "name")
                        .or_else(|| attribute(attrs, "property"))
                        .map(str::trim)
                        .map(str::to_ascii_lowercase);
                    let Some(key) = key else {
                        continue;
                    };
                    match key.as_str() {
                        "author" | "article:author" | "citation_author" | "dc.creator"
                        | "dcterms.creator" => {
                            push_unique(&mut authors, content, MAXIMUM_LIST_ITEM_CHARACTERS);
                        }
                        "description" | "dc.description" | "dcterms.description" => {
                            description = description
                                .or_else(|| normalized_optional(content, MAXIMUM_FIELD_CHARACTERS));
                        }
                        "og:description" | "twitter:description" => {
                            social_description = social_description
                                .or_else(|| normalized_optional(content, MAXIMUM_FIELD_CHARACTERS));
                        }
                        "keywords" | "citation_keywords" | "news_keywords" => {
                            for keyword in content.split([',', ';']) {
                                push_unique(&mut keywords, keyword, MAXIMUM_LIST_ITEM_CHARACTERS);
                            }
                        }
                        "article:tag" | "dc.subject" | "dcterms.subject" => {
                            push_unique(&mut keywords, content, MAXIMUM_LIST_ITEM_CHARACTERS);
                        }
                        "og:title" | "twitter:title" | "citation_title" | "dc.title"
                        | "dcterms.title" => {
                            social_title = social_title
                                .or_else(|| normalized_optional(content, MAXIMUM_FIELD_CHARACTERS));
                        }
                        "copyright" | "dc.rights" | "dcterms.rights" => {
                            rights = rights
                                .or_else(|| normalized_optional(content, MAXIMUM_FIELD_CHARACTERS));
                        }
                        "og:site_name" => {
                            site_name = site_name.or_else(|| {
                                normalized_optional(content, MAXIMUM_LIST_ITEM_CHARACTERS)
                            });
                        }
                        "generator" => {
                            source_generator = source_generator.or_else(|| {
                                normalized_optional(content, MAXIMUM_LIST_ITEM_CHARACTERS)
                            });
                        }
                        "article:published_time"
                        | "citation_date"
                        | "citation_publication_date"
                        | "date"
                        | "dc.date"
                        | "dcterms.created"
                        | "dcterms.issued" => {
                            published_at = published_at.or_else(|| {
                                normalized_optional(content, MAXIMUM_LIST_ITEM_CHARACTERS)
                            });
                        }
                        "article:modified_time" | "dcterms.modified" => {
                            modified_at = modified_at.or_else(|| {
                                normalized_optional(content, MAXIMUM_LIST_ITEM_CHARACTERS)
                            });
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        let fallback_title =
            normalize_text(manifest.source.final_url.as_str(), MAXIMUM_FIELD_CHARACTERS);
        let language = language
            .or_else(|| {
                language_tag_is_valid(manifest.environment.locale.trim())
                    .then(|| manifest.environment.locale.trim().to_owned())
            })
            .unwrap_or_else(|| "und".to_owned());
        Self {
            title: title_element.or(social_title).unwrap_or(fallback_title),
            authors,
            description: description.or(social_description),
            keywords,
            language,
            rights,
            site_name,
            source_generator,
            published_at,
            modified_at,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EmbeddedMetadata {
    creator_tool: String,
    producer: String,
    captured_at_xmp: String,
    captured_at_pdf: String,
    source_artifact_sha256: String,
    pdf_version: String,
    semantics: PdfSemantics,
}

impl EmbeddedMetadata {
    fn new(
        source: &SourceMetadata,
        manifest: &ArtifactManifest,
        source_artifact_sha256: ContentDigest,
        semantics: PdfSemantics,
        pdf_version: String,
    ) -> Result<Self> {
        if source.title.is_empty() || source.language.is_empty() || pdf_version.is_empty() {
            return Err(PageKnotError::new(
                "pageknot.export.pdf_metadata",
                ErrorStage::Encoding,
                "PDF metadata requires a title, language, and format version",
            ));
        }
        let creator_tool = format!(
            "{} {}",
            manifest.generator.name.trim(),
            manifest.generator.version.trim()
        );
        let producer = format!(
            "{} with {} {}",
            creator_tool,
            browser_product(manifest.browser.product),
            manifest.browser.version.trim()
        );
        Ok(Self {
            creator_tool,
            producer,
            captured_at_xmp: manifest.captured_at.to_rfc3339(),
            captured_at_pdf: format!("D:{}", manifest.captured_at.format("%Y%m%d%H%M%SZ")),
            source_artifact_sha256: source_artifact_sha256.to_hex(),
            pdf_version,
            semantics,
        })
    }
}

fn apply_metadata(
    document: &mut PdfDocument,
    source: &SourceMetadata,
    manifest: &ArtifactManifest,
    embedded: &EmbeddedMetadata,
    xmp: Vec<u8>,
) -> Result<Dictionary> {
    let mut info = existing_info_dictionary(document);
    for key in [
        b"Title".as_slice(),
        b"Author",
        b"Subject",
        b"Keywords",
        b"Creator",
        b"Producer",
        b"CreationDate",
        b"ModDate",
        b"Source",
        b"PageKnotSourceArtifactSHA256",
    ] {
        info.remove(key);
    }
    info.set("Title", text_string(&source.title));
    if !source.authors.is_empty() {
        info.set("Author", text_string(&source.authors.join(", ")));
    }
    if let Some(description) = &source.description {
        info.set("Subject", text_string(description));
    }
    if !source.keywords.is_empty() {
        info.set("Keywords", text_string(&source.keywords.join(", ")));
    }
    info.set("Creator", text_string(&embedded.creator_tool));
    info.set("Producer", text_string(&embedded.producer));
    info.set("CreationDate", text_string(&embedded.captured_at_pdf));
    info.set("ModDate", text_string(&embedded.captured_at_pdf));
    info.set("Source", text_string(manifest.source.final_url.as_str()));
    info.set(
        "PageKnotSourceArtifactSHA256",
        text_string(&embedded.source_artifact_sha256),
    );

    let info_id = document.add_object(info.clone());
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
        PageKnotError::new(
            "pageknot.export.pdf_metadata",
            ErrorStage::Encoding,
            format!("PDF catalog could not be updated: {error}"),
        )
    })?;
    catalog.set("Metadata", metadata_id);
    catalog.set("Lang", text_string(&source.language));
    catalog.set("ViewerPreferences", viewer_preferences_id);
    Ok(info)
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

fn write_xmp(
    source: &SourceMetadata,
    manifest: &ArtifactManifest,
    embedded: &EmbeddedMetadata,
) -> Result<Vec<u8>> {
    let mut writer = Writer::new(Vec::new());
    let write = (|| -> io::Result<()> {
        writer.write_event(Event::PI(BytesPI::new(
            "xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"",
        )))?;
        write_start(
            &mut writer,
            "x:xmpmeta",
            &[
                ("xmlns:x", "adobe:ns:meta/"),
                ("x:xmptk", embedded.creator_tool.as_str()),
            ],
        )?;
        write_start(
            &mut writer,
            "rdf:RDF",
            &[("xmlns:rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#")],
        )?;
        write_start(
            &mut writer,
            "rdf:Description",
            &[
                ("rdf:about", ""),
                ("xmlns:dc", "http://purl.org/dc/elements/1.1/"),
                ("xmlns:xmp", "http://ns.adobe.com/xap/1.0/"),
                ("xmlns:pdf", "http://ns.adobe.com/pdf/1.3/"),
                ("xmlns:pageknot", PAGEKNOT_XMP_NAMESPACE),
            ],
        )?;

        write_alt(&mut writer, "dc:title", &source.title, &source.language)?;
        if !source.authors.is_empty() {
            write_array(&mut writer, "dc:creator", "rdf:Seq", &source.authors)?;
        }
        if let Some(description) = &source.description {
            write_alt(&mut writer, "dc:description", description, &source.language)?;
        }
        if !source.keywords.is_empty() {
            write_array(&mut writer, "dc:subject", "rdf:Bag", &source.keywords)?;
        }
        write_text_element(&mut writer, "dc:format", "application/pdf")?;
        write_text_element(
            &mut writer,
            "dc:identifier",
            &format!(
                "urn:pageknot:source-artifact:sha256:{}",
                embedded.source_artifact_sha256
            ),
        )?;
        write_text_element(&mut writer, "dc:source", manifest.source.final_url.as_str())?;
        write_array(
            &mut writer,
            "dc:language",
            "rdf:Bag",
            std::slice::from_ref(&source.language),
        )?;
        if let Some(rights) = &source.rights {
            write_alt(&mut writer, "dc:rights", rights, &source.language)?;
        }

        write_text_element(&mut writer, "xmp:CreateDate", &embedded.captured_at_xmp)?;
        write_text_element(&mut writer, "xmp:ModifyDate", &embedded.captured_at_xmp)?;
        write_text_element(&mut writer, "xmp:MetadataDate", &embedded.captured_at_xmp)?;
        write_text_element(&mut writer, "xmp:CreatorTool", &embedded.creator_tool)?;
        write_text_element(&mut writer, "pdf:Producer", &embedded.producer)?;
        write_text_element(&mut writer, "pdf:PDFVersion", &embedded.pdf_version)?;
        if !source.keywords.is_empty() {
            write_text_element(&mut writer, "pdf:Keywords", &source.keywords.join(", "))?;
        }

        write_text_element(
            &mut writer,
            "pageknot:SourceRequestedURL",
            manifest.source.requested_url.as_str(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:SourceFinalURL",
            manifest.source.final_url.as_str(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:SourceRequestedSHA256",
            &manifest.source.requested_url_sha256.to_hex(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:SourceFinalSHA256",
            &manifest.source.final_url_sha256.to_hex(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:SourceArtifactSHA256",
            &embedded.source_artifact_sha256,
        )?;
        write_text_element(
            &mut writer,
            "pageknot:PolicySHA256",
            &manifest.policy_sha256.to_hex(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:CaptureTimestamp",
            &embedded.captured_at_xmp,
        )?;
        write_text_element(
            &mut writer,
            "pageknot:GeneratorName",
            &manifest.generator.name,
        )?;
        write_text_element(
            &mut writer,
            "pageknot:GeneratorVersion",
            &manifest.generator.version,
        )?;
        write_text_element(
            &mut writer,
            "pageknot:ManifestSchemaVersion",
            &manifest.schema_version.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:ArtifactFormatVersion",
            &manifest.format.version.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:BrowserProduct",
            browser_product(manifest.browser.product),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:BrowserVersion",
            &manifest.browser.version,
        )?;
        if let Some(revision) = &manifest.browser.revision {
            write_text_element(&mut writer, "pageknot:BrowserRevision", revision)?;
        }
        write_text_element(
            &mut writer,
            "pageknot:ProtocolVersion",
            &manifest.browser.protocol_version,
        )?;
        write_text_element(
            &mut writer,
            "pageknot:CaptureLocale",
            &manifest.environment.locale,
        )?;
        write_text_element(
            &mut writer,
            "pageknot:CaptureTimezone",
            &manifest.environment.timezone,
        )?;
        write_text_element(
            &mut writer,
            "pageknot:VerificationLevel",
            verification_level(manifest.verification.level),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:FrameCount",
            &manifest.frames.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:ResourceCount",
            &manifest.resources.discovered.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:EmbeddedResourceCount",
            &manifest.resources.embedded.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:ExternalResourceCount",
            &manifest.resources.external.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:OmittedResourceCount",
            &manifest.resources.omitted.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:FailedResourceCount",
            &manifest.resources.failed.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:EmbeddedResourceBytes",
            &manifest.resources.embedded_bytes.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:PageCount",
            &embedded.semantics.pages.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:Tagged",
            if embedded.semantics.tagged {
                "true"
            } else {
                "false"
            },
        )?;
        write_text_element(
            &mut writer,
            "pageknot:HasDocumentOutline",
            if embedded.semantics.has_document_outline {
                "true"
            } else {
                "false"
            },
        )?;
        write_text_element(
            &mut writer,
            "pageknot:StructureElementCount",
            &embedded.semantics.structure_elements.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "pageknot:HasTextStructure",
            if embedded.semantics.has_text_structure {
                "true"
            } else {
                "false"
            },
        )?;
        write_text_element(
            &mut writer,
            "pageknot:StructuralRepairApplied",
            if manifest.structural_repair.applied {
                "true"
            } else {
                "false"
            },
        )?;
        if !manifest.warning_codes.is_empty() {
            write_array(
                &mut writer,
                "pageknot:WarningCodes",
                "rdf:Bag",
                &manifest.warning_codes,
            )?;
        }
        if let Some(site_name) = &source.site_name {
            write_text_element(&mut writer, "pageknot:SourceSiteName", site_name)?;
        }
        if let Some(generator) = &source.source_generator {
            write_text_element(&mut writer, "pageknot:SourceGenerator", generator)?;
        }
        if let Some(published_at) = &source.published_at {
            write_text_element(&mut writer, "pageknot:SourcePublishedDate", published_at)?;
        }
        if let Some(modified_at) = &source.modified_at {
            write_text_element(&mut writer, "pageknot:SourceModifiedDate", modified_at)?;
        }

        write_end(&mut writer, "rdf:Description")?;
        write_end(&mut writer, "rdf:RDF")?;
        write_end(&mut writer, "x:xmpmeta")?;
        writer.write_event(Event::PI(BytesPI::new("xpacket end=\"w\"")))?;
        Ok(())
    })();
    write.map_err(export_io_error)?;
    Ok(writer.into_inner())
}

fn write_start(
    writer: &mut Writer<Vec<u8>>,
    name: &str,
    attributes: &[(&str, &str)],
) -> io::Result<()> {
    let mut element = BytesStart::new(name);
    for (key, value) in attributes {
        element.push_attribute((*key, *value));
    }
    writer.write_event(Event::Start(element))
}

fn write_end(writer: &mut Writer<Vec<u8>>, name: &str) -> io::Result<()> {
    writer.write_event(Event::End(BytesEnd::new(name)))
}

fn write_text_element(writer: &mut Writer<Vec<u8>>, name: &str, value: &str) -> io::Result<()> {
    write_start(writer, name, &[])?;
    writer.write_event(Event::Text(BytesText::new(value)))?;
    write_end(writer, name)
}

fn write_alt(
    writer: &mut Writer<Vec<u8>>,
    name: &str,
    value: &str,
    language: &str,
) -> io::Result<()> {
    write_start(writer, name, &[])?;
    write_start(writer, "rdf:Alt", &[])?;
    write_start(writer, "rdf:li", &[("xml:lang", "x-default")])?;
    writer.write_event(Event::Text(BytesText::new(value)))?;
    write_end(writer, "rdf:li")?;
    if language != "und" {
        write_start(writer, "rdf:li", &[("xml:lang", language)])?;
        writer.write_event(Event::Text(BytesText::new(value)))?;
        write_end(writer, "rdf:li")?;
    }
    write_end(writer, "rdf:Alt")?;
    write_end(writer, name)
}

fn write_array(
    writer: &mut Writer<Vec<u8>>,
    name: &str,
    container: &str,
    values: &[String],
) -> io::Result<()> {
    write_start(writer, name, &[])?;
    write_start(writer, container, &[])?;
    for value in values {
        write_text_element(writer, "rdf:li", value)?;
    }
    write_end(writer, container)?;
    write_end(writer, name)
}

fn pageknot_pdf_semantics_are_valid(document: &PdfDocument) -> bool {
    let Ok(semantics) = PdfSemantics::inspect(document, ErrorStage::Verification) else {
        return false;
    };
    let Ok(catalog) = document.catalog() else {
        return false;
    };
    let Some(metadata) = catalog
        .get(b"Metadata")
        .ok()
        .and_then(|object| resolved_object(document, object))
        .and_then(|object| object.as_stream().ok())
    else {
        return false;
    };
    let xmp = metadata
        .dict
        .has_type(b"Metadata")
        .then(|| metadata.dict.get(b"Subtype").ok())
        .flatten()
        .and_then(|value| value.as_name().ok())
        .filter(|value| *value == b"XML")
        .and_then(|_| {
            metadata
                .get_plain_content_with_limit(
                    usize::try_from(MAXIMUM_DECODED_MANIFEST_BYTES).unwrap_or(usize::MAX),
                )
                .ok()
        });
    let language_valid = catalog
        .get(b"Lang")
        .ok()
        .and_then(|value| resolved_object(document, value))
        .and_then(|value| decode_text_string(value).ok())
        .is_some_and(|value| language_tag_is_valid(&value));
    let display_title = catalog
        .get(b"ViewerPreferences")
        .ok()
        .and_then(|value| resolved_dictionary(document, value))
        .and_then(|preferences| preferences.get(b"DisplayDocTitle").ok())
        .and_then(|value| value.as_bool().ok())
        == Some(true);
    let info_valid = document
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|value| resolved_dictionary(document, value))
        .is_some_and(info_dictionary_is_valid);

    semantics.tagged
        && language_valid
        && display_title
        && info_valid
        && xmp.is_some_and(|bytes| xmp_packet_is_valid(&bytes, semantics))
}

fn info_dictionary_is_valid(info: &Dictionary) -> bool {
    [
        b"Title".as_slice(),
        b"Creator",
        b"Producer",
        b"CreationDate",
        b"ModDate",
        b"Source",
        b"PageKnotSourceArtifactSHA256",
    ]
    .iter()
    .all(|key| {
        info.get(key)
            .ok()
            .and_then(|value| decode_text_string(value).ok())
            .is_some_and(|value| !value.trim().is_empty())
    })
}

fn xmp_packet_is_valid(bytes: &[u8], semantics: PdfSemantics) -> bool {
    let mut reader = Reader::from_reader(bytes);
    let required = [
        b"x:xmpmeta".as_slice(),
        b"rdf:RDF",
        b"rdf:Description",
        b"dc:title",
        b"dc:format",
        b"dc:identifier",
        b"dc:source",
        b"xmp:CreateDate",
        b"xmp:ModifyDate",
        b"xmp:MetadataDate",
        b"xmp:CreatorTool",
        b"pdf:Producer",
        b"pdf:PDFVersion",
        b"pageknot:SourceFinalURL",
        b"pageknot:SourceArtifactSHA256",
        b"pageknot:PolicySHA256",
        b"pageknot:PageCount",
        b"pageknot:Tagged",
        b"pageknot:HasDocumentOutline",
        b"pageknot:StructureElementCount",
        b"pageknot:HasTextStructure",
    ];
    let mut seen = BTreeSet::new();
    let mut stack = Vec::new();
    let mut source_digest = String::new();
    let mut format = String::new();
    let mut tagged = String::new();
    let mut page_count = String::new();
    let mut has_document_outline = String::new();
    let mut structure_elements = String::new();
    let mut has_text_structure = String::new();
    let mut namespace_valid = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) => {
                let name = element.name().as_ref().to_vec();
                if name == b"rdf:Description" {
                    namespace_valid = element
                        .attributes()
                        .filter_map(std::result::Result::ok)
                        .any(|attribute| {
                            attribute.key.as_ref() == b"xmlns:pageknot"
                                && attribute.value.as_ref() == PAGEKNOT_XMP_NAMESPACE.as_bytes()
                        });
                }
                seen.insert(name.clone());
                stack.push(name);
            }
            Ok(Event::Empty(element)) => {
                seen.insert(element.name().as_ref().to_vec());
            }
            Ok(Event::Text(text)) => {
                let Some(name) = stack.last() else {
                    continue;
                };
                let Ok(value) = text.decode() else {
                    return false;
                };
                if name.as_slice() == b"pageknot:SourceArtifactSHA256" {
                    source_digest.push_str(value.trim());
                } else if name.as_slice() == b"dc:format" {
                    format.push_str(value.trim());
                } else if name.as_slice() == b"pageknot:Tagged" {
                    tagged.push_str(value.trim());
                } else if name.as_slice() == b"pageknot:PageCount" {
                    page_count.push_str(value.trim());
                } else if name.as_slice() == b"pageknot:HasDocumentOutline" {
                    has_document_outline.push_str(value.trim());
                } else if name.as_slice() == b"pageknot:StructureElementCount" {
                    structure_elements.push_str(value.trim());
                } else if name.as_slice() == b"pageknot:HasTextStructure" {
                    has_text_structure.push_str(value.trim());
                }
            }
            Ok(Event::End(_)) => {
                stack.pop();
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    namespace_valid
        && required.iter().all(|name| seen.contains(*name))
        && format == "application/pdf"
        && tagged == boolean_text(semantics.tagged)
        && page_count.parse::<u64>() == Ok(semantics.pages)
        && has_document_outline == boolean_text(semantics.has_document_outline)
        && structure_elements.parse::<u64>() == Ok(semantics.structure_elements)
        && has_text_structure == boolean_text(semantics.has_text_structure)
        && source_digest.len() == ContentDigest::HEX_LENGTH
        && source_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn embedded_metadata_matches(
    document: &PdfDocument,
    expected_info: &Dictionary,
    expected_language: &str,
    expected_xmp: &[u8],
    embedded: &EmbeddedMetadata,
) -> bool {
    let info_matches = document
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|value| resolved_dictionary(document, value))
        .is_some_and(|actual| {
            expected_info
                .iter()
                .all(|(key, value)| actual.get(key).ok() == Some(value))
        });
    let Ok(catalog) = document.catalog() else {
        return false;
    };
    let language_matches = catalog
        .get(b"Lang")
        .ok()
        .and_then(|value| resolved_object(document, value))
        .and_then(|value| decode_text_string(value).ok())
        .is_some_and(|value| value == expected_language);
    let xmp_matches = catalog
        .get(b"Metadata")
        .ok()
        .and_then(|value| resolved_object(document, value))
        .and_then(|value| value.as_stream().ok())
        .and_then(|stream| {
            stream
                .get_plain_content_with_limit(
                    usize::try_from(MAXIMUM_DECODED_MANIFEST_BYTES).unwrap_or(usize::MAX),
                )
                .ok()
        })
        .is_some_and(|actual| actual == expected_xmp);
    let semantics_match = PdfSemantics::inspect(document, ErrorStage::Verification)
        .is_ok_and(|actual| actual == embedded.semantics);
    info_matches && language_matches && xmp_matches && semantics_match
}

fn resolved_object<'a>(document: &'a PdfDocument, object: &'a Object) -> Option<&'a Object> {
    document.dereference(object).ok().map(|(_, value)| value)
}

fn resolved_dictionary<'a>(
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

fn load_pdf(bytes: &[u8], stage: ErrorStage) -> Result<PdfDocument> {
    PdfDocument::load_mem_with_options(
        bytes,
        PdfLoadOptions {
            strict: true,
            max_decompressed_size: Some(maximum_decoded_bytes()),
            ..PdfLoadOptions::default()
        },
    )
    .map_err(|error| {
        PageKnotError::new(
            "pageknot.export.pdf",
            stage,
            format!("PDF document could not be parsed: {error}"),
        )
    })
}

fn attribute<'a>(attributes: &'a [Attribute], local_name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|attribute| attribute.name.local.as_ref() == local_name)
        .map(|attribute| attribute.value.as_ref())
}

fn descendant_text(document: &HtmlDocument, root: pageknot_model::NodeId) -> String {
    let mut normalized = NormalizedText::default();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if normalized.is_full(MAXIMUM_FIELD_CHARACTERS) {
            break;
        }
        let Some(node) = document.node(id) else {
            continue;
        };
        match &node.data {
            NodeData::Text { contents } => {
                normalized.push(contents.as_ref(), MAXIMUM_FIELD_CHARACTERS);
            }
            _ => stack.extend(node.children.iter().rev().copied()),
        }
    }
    normalized.finish()
}

fn normalize_text(value: &str, maximum_characters: usize) -> String {
    let mut normalized = NormalizedText::default();
    normalized.push(value, maximum_characters);
    normalized.finish()
}

#[derive(Debug, Default)]
struct NormalizedText {
    value: String,
    characters: usize,
    pending_space: bool,
}

impl NormalizedText {
    fn push(&mut self, input: &str, maximum_characters: usize) {
        for character in input.chars() {
            if character.is_whitespace() {
                self.pending_space = !self.value.is_empty();
                continue;
            }
            let required = 1usize.saturating_add(usize::from(self.pending_space));
            if self.characters.saturating_add(required) > maximum_characters {
                return;
            }
            if self.pending_space {
                self.value.push(' ');
                self.characters = self.characters.saturating_add(1);
                self.pending_space = false;
            }
            self.value.push(character);
            self.characters = self.characters.saturating_add(1);
        }
    }

    const fn is_full(&self, maximum_characters: usize) -> bool {
        self.characters >= maximum_characters
    }

    fn finish(self) -> String {
        self.value
    }
}

fn normalized_optional(value: &str, maximum_characters: usize) -> Option<String> {
    let value = normalize_text(value, maximum_characters);
    (!value.is_empty()).then_some(value)
}

fn push_unique(values: &mut Vec<String>, value: &str, maximum_characters: usize) {
    if values.len() >= MAXIMUM_LIST_ITEMS {
        return;
    }
    let Some(value) = normalized_optional(value, maximum_characters) else {
        return;
    };
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn language_tag_is_valid(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 64
        && value.split('-').all(|part| {
            !part.is_empty()
                && part.len() <= 8
                && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}

const fn browser_product(product: BrowserProduct) -> &'static str {
    match product {
        BrowserProduct::Chrome => "chrome",
        BrowserProduct::Chromium => "chromium",
        BrowserProduct::Edge => "edge",
    }
}

const fn verification_level(level: VerificationPolicy) -> &'static str {
    match level {
        VerificationPolicy::Static => "static",
        VerificationPolicy::Offline => "offline",
    }
}

const fn boolean_text(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use lopdf::{Dictionary, Document as PdfDocument, Object, Stream, dictionary};
    use pageknot_model::{ArtifactManifest, ContentDigest};

    use super::{
        SourceMetadata, embed_pdf_metadata, load_pdf, pageknot_pdf_semantics_are_valid,
        verify_pageknot_pdf,
    };

    type TestResult<T = ()> = std::result::Result<T, Box<dyn Error>>;

    fn manifest() -> TestResult<ArtifactManifest> {
        Ok(serde_json::from_str(include_str!(
            "../../../../schemas/examples/artifact-manifest.json"
        ))?)
    }

    fn tagged_pdf() -> TestResult<Vec<u8>> {
        let mut document = PdfDocument::with_version("1.7");
        let pages_id = document.new_object_id();
        let page_id = document.new_object_id();
        let content_id = document.add_object(Stream::new(Dictionary::new(), b"BT ET".to_vec()));
        let structure_id = document.add_object(dictionary! {
            "Type" => "StructTreeRoot",
            "K" => Object::Array(Vec::new()),
        });
        let outlines_id = document.add_object(dictionary! {
            "Type" => "Outlines",
            "Count" => 0,
        });
        document.objects.insert(
            page_id,
            Object::Dictionary(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
                "Contents" => content_id,
                "StructParents" => 0,
            }),
        );
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Count" => 1,
                "Kids" => vec![page_id.into()],
            }),
        );
        let catalog_id = document.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
            "StructTreeRoot" => structure_id,
            "MarkInfo" => dictionary! { "Marked" => true },
            "Outlines" => outlines_id,
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes)?;
        Ok(bytes)
    }

    #[test]
    fn source_metadata_uses_html_semantics_and_manifest_fallbacks() -> TestResult {
        let manifest = manifest()?;
        let source = SourceMetadata::from_html(
            r#"<!doctype html><html lang="de-AT"><head>
            <title>Die Übersicht &amp; Daten</title>
            <meta name="author" content="Ada Lovelace">
            <meta name="description" content="  Eine   Zusammenfassung. ">
            <meta name="keywords" content="charts, data; accessibility">
            <meta property="article:tag" content="PDF">
            <meta property="og:site_name" content="Example Research">
            <meta name="generator" content="StaticPress 2">
            <meta property="article:published_time" content="2026-07-29">
            </head><body><h1>Übersicht</h1></body></html>"#
                .as_bytes(),
            &manifest,
        );

        assert_eq!(source.title, "Die Übersicht & Daten");
        assert_eq!(source.authors, ["Ada Lovelace"]);
        assert_eq!(source.description.as_deref(), Some("Eine Zusammenfassung."));
        assert_eq!(source.keywords, ["charts", "data", "accessibility", "PDF"]);
        assert_eq!(source.language, "de-AT");
        assert_eq!(source.site_name.as_deref(), Some("Example Research"));
        assert_eq!(source.source_generator.as_deref(), Some("StaticPress 2"));
        assert_eq!(source.published_at.as_deref(), Some("2026-07-29"));
        Ok(())
    }

    #[test]
    fn enriched_pdf_has_pageknot_semantics_and_xmp() -> TestResult {
        let raw = tagged_pdf()?;
        let html = br#"<!doctype html><html lang="en"><head>
            <title>Semantic capture</title>
            <meta name="author" content="Grace Hopper">
            <meta name="description" content="A tagged PDF fixture">
            <meta name="keywords" content="metadata, semantics">
            </head><body><h1>Semantic capture</h1></body></html>"#;
        let digest = ContentDigest::sha256(html);
        let enriched = embed_pdf_metadata(&raw, html, &manifest()?, digest, 4 * 1024 * 1024)?;

        verify_pageknot_pdf(&enriched)?;
        let document = load_pdf(&enriched, pageknot_model::ErrorStage::Verification)?;
        assert!(pageknot_pdf_semantics_are_valid(&document));
        let metadata = document
            .catalog()?
            .get(b"Metadata")
            .and_then(Object::as_reference)
            .and_then(|id| document.get_object(id))
            .and_then(Object::as_stream)
            .and_then(Stream::get_plain_content)?;
        let metadata = String::from_utf8(metadata)?;
        assert!(metadata.contains("<dc:title>"));
        assert!(metadata.contains("Semantic capture"));
        assert!(metadata.contains("Grace Hopper"));
        assert!(metadata.contains("<pageknot:SourceArtifactSHA256>"));
        assert!(metadata.contains("urn:pageknot:source-artifact:sha256:"));
        assert!(metadata.contains(&digest.to_hex()));
        Ok(())
    }

    #[test]
    fn pageknot_pdf_verifier_requires_the_xmp_packet() -> TestResult {
        let raw = tagged_pdf()?;
        let html =
            br#"<!doctype html><html lang="en"><title>Metadata</title><body>text</body></html>"#;
        let digest = ContentDigest::sha256(html);
        let enriched = embed_pdf_metadata(&raw, html, &manifest()?, digest, 4 * 1024 * 1024)?;
        let mut document = load_pdf(&enriched, pageknot_model::ErrorStage::Verification)?;
        document.catalog_mut()?.remove(b"Metadata");
        let mut stripped = Vec::new();
        document.save_to(&mut stripped)?;

        assert!(verify_pageknot_pdf(&stripped).is_err());
        Ok(())
    }
}
