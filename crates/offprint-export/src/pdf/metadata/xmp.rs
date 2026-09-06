use std::io;

use offprint_model::{
    BrowserProduct, ContentDigest, ErrorStage, OffprintError, ResourceSummary, Result,
    VerificationMode,
};
use quick_xml::escape::unescape;
use quick_xml::events::{BytesEnd, BytesPI, BytesStart, BytesText, Event};
use quick_xml::name::{QName, ResolveResult};
use quick_xml::{NsReader, Writer};

use super::model::{
    PdfMetadata, boolean_text, browser_product, parse_boolean, parse_browser_product,
    parse_verification_mode, verification_mode,
};
use super::source::SourceMetadata;
use crate::pdf::semantics::PdfSemantics;
use crate::support::export_io_error;

const XMP_META_NAMESPACE: &[u8] = b"adobe:ns:meta/";
const RDF_NAMESPACE: &[u8] = b"http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const DC_NAMESPACE: &[u8] = b"http://purl.org/dc/elements/1.1/";
const XMP_NAMESPACE: &[u8] = b"http://ns.adobe.com/xap/1.0/";
const PDF_NAMESPACE: &[u8] = b"http://ns.adobe.com/pdf/1.3/";
const OFFPRINT_NAMESPACE: &[u8] = b"https://github.com/peter-gy/offprint/xmp/pdf/1.0/";
const MAXIMUM_XMP_NODES: usize = 512;
const MAXIMUM_XMP_DEPTH: usize = 128;

pub(super) fn encode_xmp(metadata: &PdfMetadata) -> Result<Vec<u8>> {
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
                ("x:xmptk", metadata.creator_tool().as_str()),
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
                (
                    "xmlns:offprint",
                    "https://github.com/peter-gy/offprint/xmp/pdf/1.0/",
                ),
            ],
        )?;

        write_alt(
            &mut writer,
            "dc:title",
            &metadata.source.title,
            &metadata.source.language,
        )?;
        if !metadata.source.authors.is_empty() {
            write_array(
                &mut writer,
                "dc:creator",
                "rdf:Seq",
                &metadata.source.authors,
            )?;
        }
        if let Some(description) = &metadata.source.description {
            write_alt(
                &mut writer,
                "dc:description",
                description,
                &metadata.source.language,
            )?;
        }
        if !metadata.source.keywords.is_empty() {
            write_array(
                &mut writer,
                "dc:subject",
                "rdf:Bag",
                &metadata.source.keywords,
            )?;
        }
        write_text_element(&mut writer, "dc:format", "application/pdf")?;
        write_text_element(&mut writer, "dc:identifier", &metadata.identifier())?;
        write_text_element(&mut writer, "dc:source", &metadata.final_url)?;
        write_array(
            &mut writer,
            "dc:language",
            "rdf:Bag",
            std::slice::from_ref(&metadata.source.language),
        )?;
        if let Some(rights) = &metadata.source.rights {
            write_alt(&mut writer, "dc:rights", rights, &metadata.source.language)?;
        }

        write_text_element(&mut writer, "xmp:CreateDate", &metadata.captured_at)?;
        write_text_element(&mut writer, "xmp:ModifyDate", &metadata.captured_at)?;
        write_text_element(&mut writer, "xmp:MetadataDate", &metadata.captured_at)?;
        write_text_element(&mut writer, "xmp:CreatorTool", &metadata.creator_tool())?;
        write_text_element(&mut writer, "pdf:Producer", &metadata.producer())?;
        write_text_element(&mut writer, "pdf:PDFVersion", &metadata.pdf_version)?;
        if !metadata.source.keywords.is_empty() {
            write_text_element(
                &mut writer,
                "pdf:Keywords",
                &metadata.source.keywords.join(", "),
            )?;
        }

        write_text_element(
            &mut writer,
            "offprint:SourceRequestedURL",
            &metadata.requested_url,
        )?;
        write_text_element(&mut writer, "offprint:SourceFinalURL", &metadata.final_url)?;
        write_text_element(
            &mut writer,
            "offprint:SourceRequestedSHA256",
            &metadata.requested_url_sha256.to_hex(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:SourceFinalSHA256",
            &metadata.final_url_sha256.to_hex(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:SourceArtifactSHA256",
            &metadata.source_artifact_sha256.to_hex(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:CapturePolicySHA256",
            &metadata.capture_policy_sha256.to_hex(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:CaptureTimestamp",
            &metadata.captured_at,
        )?;
        write_text_element(
            &mut writer,
            "offprint:GeneratorName",
            &metadata.generator_name,
        )?;
        write_text_element(
            &mut writer,
            "offprint:GeneratorVersion",
            &metadata.generator_version,
        )?;
        write_text_element(
            &mut writer,
            "offprint:ManifestSchemaVersion",
            &metadata.schema_version.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:ArtifactFormatVersion",
            &metadata.artifact_format_version.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:BrowserProduct",
            browser_product(metadata.browser_product),
        )?;
        write_text_element(
            &mut writer,
            "offprint:BrowserVersion",
            &metadata.browser_version,
        )?;
        if let Some(revision) = &metadata.browser_revision {
            write_text_element(&mut writer, "offprint:BrowserRevision", revision)?;
        }
        write_text_element(
            &mut writer,
            "offprint:ProtocolVersion",
            &metadata.protocol_version,
        )?;
        write_text_element(&mut writer, "offprint:CaptureLocale", &metadata.locale)?;
        write_text_element(&mut writer, "offprint:CaptureTimezone", &metadata.timezone)?;
        write_text_element(
            &mut writer,
            "offprint:VerificationModeArg",
            verification_mode(metadata.verification_mode),
        )?;
        write_text_element(
            &mut writer,
            "offprint:FrameCount",
            &metadata.frames.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:ResourceCount",
            &metadata.resources.discovered.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:EmbeddedResourceCount",
            &metadata.resources.embedded.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:ExternalResourceCount",
            &metadata.resources.external.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:OmittedResourceCount",
            &metadata.resources.omitted.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:FailedResourceCount",
            &metadata.resources.failed.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:EmbeddedResourceBytes",
            &metadata.resources.embedded_bytes.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:PageCount",
            &metadata.semantics.pages.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:Tagged",
            boolean_text(metadata.semantics.tagged),
        )?;
        write_text_element(
            &mut writer,
            "offprint:HasDocumentOutline",
            boolean_text(metadata.semantics.has_document_outline),
        )?;
        write_text_element(
            &mut writer,
            "offprint:StructureElementCount",
            &metadata.semantics.structure_elements.to_string(),
        )?;
        write_text_element(
            &mut writer,
            "offprint:HasTextStructure",
            boolean_text(metadata.semantics.has_text_structure),
        )?;
        write_text_element(
            &mut writer,
            "offprint:StructuralRepairApplied",
            boolean_text(metadata.structural_repair_applied),
        )?;
        if !metadata.warning_codes.is_empty() {
            write_array(
                &mut writer,
                "offprint:WarningCodes",
                "rdf:Bag",
                &metadata.warning_codes,
            )?;
        }
        write_optional_text(
            &mut writer,
            "offprint:SourceSiteName",
            metadata.source.site_name.as_deref(),
        )?;
        write_optional_text(
            &mut writer,
            "offprint:SourceGenerator",
            metadata.source.source_generator.as_deref(),
        )?;
        write_optional_text(
            &mut writer,
            "offprint:SourcePublishedDate",
            metadata.source.published_at.as_deref(),
        )?;
        write_optional_text(
            &mut writer,
            "offprint:SourceModifiedDate",
            metadata.source.modified_at.as_deref(),
        )?;

        write_end(&mut writer, "rdf:Description")?;
        write_end(&mut writer, "rdf:RDF")?;
        write_end(&mut writer, "x:xmpmeta")?;
        writer.write_event(Event::PI(BytesPI::new("xpacket end=\"w\"")))?;
        Ok(())
    })();
    write.map_err(export_io_error)?;
    Ok(writer.into_inner())
}

pub(super) fn decode_xmp(bytes: &[u8]) -> Result<PdfMetadata> {
    let root = parse_xml(bytes)?;
    let rdf = required_child(&root, RDF_NAMESPACE, b"RDF")?;
    let description = required_child(rdf, RDF_NAMESPACE, b"Description")?;

    let title = required_alt(description, DC_NAMESPACE, b"title")?;
    let authors =
        optional_array(description, DC_NAMESPACE, b"creator", b"Seq")?.unwrap_or_default();
    let source_description = optional_alt(description, DC_NAMESPACE, b"description")?;
    let keywords =
        optional_array(description, DC_NAMESPACE, b"subject", b"Bag")?.unwrap_or_default();
    let dc_format = required_text(description, DC_NAMESPACE, b"format")?;
    let dc_identifier = required_text(description, DC_NAMESPACE, b"identifier")?;
    let dc_source = required_text(description, DC_NAMESPACE, b"source")?;
    let languages = required_array(description, DC_NAMESPACE, b"language", b"Bag")?;
    if languages.len() != 1 {
        return Err(xmp_error("XMP language array must contain one value"));
    }
    let language = languages[0].clone();
    let rights = optional_alt(description, DC_NAMESPACE, b"rights")?;

    let create_date = required_text(description, XMP_NAMESPACE, b"CreateDate")?;
    let modify_date = required_text(description, XMP_NAMESPACE, b"ModifyDate")?;
    let metadata_date = required_text(description, XMP_NAMESPACE, b"MetadataDate")?;
    let creator_tool = required_text(description, XMP_NAMESPACE, b"CreatorTool")?;
    let producer = required_text(description, PDF_NAMESPACE, b"Producer")?;
    let pdf_version = required_text(description, PDF_NAMESPACE, b"PDFVersion")?;
    let pdf_keywords = optional_text(description, PDF_NAMESPACE, b"Keywords")?;

    let requested_url = required_text(description, OFFPRINT_NAMESPACE, b"SourceRequestedURL")?;
    let final_url = required_text(description, OFFPRINT_NAMESPACE, b"SourceFinalURL")?;
    let requested_url_sha256 = required_digest(description, b"SourceRequestedSHA256")?;
    let final_url_sha256 = required_digest(description, b"SourceFinalSHA256")?;
    let source_artifact_sha256 = required_digest(description, b"SourceArtifactSHA256")?;
    let capture_policy_sha256 = required_digest(description, b"CapturePolicySHA256")?;
    let captured_at = required_text(description, OFFPRINT_NAMESPACE, b"CaptureTimestamp")?;
    let generator_name = required_text(description, OFFPRINT_NAMESPACE, b"GeneratorName")?;
    let generator_version = required_text(description, OFFPRINT_NAMESPACE, b"GeneratorVersion")?;
    let schema_version = required_u32(description, b"ManifestSchemaVersion")?;
    let artifact_format_version = required_u32(description, b"ArtifactFormatVersion")?;
    let browser_product = required_browser_product(description)?;
    let browser_version = required_text(description, OFFPRINT_NAMESPACE, b"BrowserVersion")?;
    let browser_revision = optional_text(description, OFFPRINT_NAMESPACE, b"BrowserRevision")?;
    let protocol_version = required_text(description, OFFPRINT_NAMESPACE, b"ProtocolVersion")?;
    let locale = required_text(description, OFFPRINT_NAMESPACE, b"CaptureLocale")?;
    let timezone = required_text(description, OFFPRINT_NAMESPACE, b"CaptureTimezone")?;
    let verification_mode = required_verification_mode(description)?;
    let frames = required_u32(description, b"FrameCount")?;
    let resources = ResourceSummary {
        discovered: required_u32(description, b"ResourceCount")?,
        embedded: required_u32(description, b"EmbeddedResourceCount")?,
        external: required_u32(description, b"ExternalResourceCount")?,
        omitted: required_u32(description, b"OmittedResourceCount")?,
        failed: required_u32(description, b"FailedResourceCount")?,
        embedded_bytes: required_u64(description, b"EmbeddedResourceBytes")?,
    };
    let semantics = PdfSemantics {
        pages: required_u64(description, b"PageCount")?,
        tagged: required_boolean(description, b"Tagged")?,
        has_document_outline: required_boolean(description, b"HasDocumentOutline")?,
        structure_elements: required_u64(description, b"StructureElementCount")?,
        has_text_structure: required_boolean(description, b"HasTextStructure")?,
    };
    let structural_repair_applied = required_boolean(description, b"StructuralRepairApplied")?;
    let warning_codes = optional_array(description, OFFPRINT_NAMESPACE, b"WarningCodes", b"Bag")?
        .unwrap_or_default();
    let source = SourceMetadata {
        title,
        authors,
        description: source_description,
        keywords,
        language,
        rights,
        site_name: optional_text(description, OFFPRINT_NAMESPACE, b"SourceSiteName")?,
        source_generator: optional_text(description, OFFPRINT_NAMESPACE, b"SourceGenerator")?,
        published_at: optional_text(description, OFFPRINT_NAMESPACE, b"SourcePublishedDate")?,
        modified_at: optional_text(description, OFFPRINT_NAMESPACE, b"SourceModifiedDate")?,
    };
    let metadata = PdfMetadata {
        source,
        requested_url,
        final_url,
        requested_url_sha256,
        final_url_sha256,
        source_artifact_sha256,
        capture_policy_sha256,
        captured_at,
        generator_name,
        generator_version,
        schema_version,
        artifact_format_version,
        browser_product,
        browser_version,
        browser_revision,
        protocol_version,
        locale,
        timezone,
        verification_mode,
        frames,
        resources,
        semantics,
        structural_repair_applied,
        warning_codes,
        pdf_version,
    };
    metadata.validate(ErrorStage::Verification)?;

    let expected_keywords =
        (!metadata.source.keywords.is_empty()).then(|| metadata.source.keywords.join(", "));
    if dc_format != "application/pdf"
        || dc_identifier != metadata.identifier()
        || dc_source != metadata.final_url
        || create_date != metadata.captured_at
        || modify_date != metadata.captured_at
        || metadata_date != metadata.captured_at
        || creator_tool != metadata.creator_tool()
        || producer != metadata.producer()
        || pdf_keywords != expected_keywords
        || encode_xmp(&metadata)? != bytes
    {
        return Err(xmp_error("XMP metadata fields are inconsistent"));
    }
    Ok(metadata)
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

fn write_optional_text(
    writer: &mut Writer<Vec<u8>>,
    name: &str,
    value: Option<&str>,
) -> io::Result<()> {
    if let Some(value) = value {
        write_text_element(writer, name, value)?;
    }
    Ok(())
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

#[derive(Debug, Eq, PartialEq)]
struct XmlName {
    namespace: Vec<u8>,
    local: Vec<u8>,
}

#[derive(Debug)]
struct XmlNode {
    name: XmlName,
    text: String,
    children: Vec<Self>,
}

impl XmlNode {
    fn matches(&self, namespace: &[u8], local: &[u8]) -> bool {
        self.name.namespace == namespace && self.name.local == local
    }
}

fn parse_xml(bytes: &[u8]) -> Result<XmlNode> {
    let mut reader = NsReader::from_reader(bytes);
    let mut buffer = Vec::new();
    let mut stack = Vec::<XmlNode>::new();
    let mut root = None;
    let mut nodes = 0usize;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element)) => {
                nodes = nodes.saturating_add(1);
                if nodes > MAXIMUM_XMP_NODES || stack.len() >= MAXIMUM_XMP_DEPTH {
                    return Err(xmp_error("XMP document exceeds structural limits"));
                }
                let name = expanded_name(&reader, element.name())?;
                stack.push(XmlNode {
                    name,
                    text: String::new(),
                    children: Vec::new(),
                });
            }
            Ok(Event::Empty(element)) => {
                nodes = nodes.saturating_add(1);
                if nodes > MAXIMUM_XMP_NODES {
                    return Err(xmp_error("XMP document exceeds structural limits"));
                }
                let node = XmlNode {
                    name: expanded_name(&reader, element.name())?,
                    text: String::new(),
                    children: Vec::new(),
                };
                attach_node(&mut stack, &mut root, node)?;
            }
            Ok(Event::Text(text)) => {
                let Some(node) = stack.last_mut() else {
                    let raw: &[u8] = text.as_ref();
                    if !raw.iter().all(u8::is_ascii_whitespace) {
                        return Err(xmp_error("XMP text appears outside the root element"));
                    }
                    buffer.clear();
                    continue;
                };
                let decoded = text
                    .decode()
                    .map_err(|error| xmp_error(format!("XMP text is invalid: {error}")))?;
                let decoded = unescape(&decoded)
                    .map_err(|error| xmp_error(format!("XMP text is invalid: {error}")))?;
                node.text.push_str(&decoded);
            }
            Ok(Event::End(element)) => {
                let Some(node) = stack.pop() else {
                    return Err(xmp_error("XMP element nesting is invalid"));
                };
                if node.name != expanded_name(&reader, element.name())? {
                    return Err(xmp_error("XMP element nesting is invalid"));
                }
                attach_node(&mut stack, &mut root, node)?;
            }
            Ok(Event::GeneralRef(reference)) => {
                let Some(node) = stack.last_mut() else {
                    return Err(xmp_error("XMP reference appears outside the root element"));
                };
                let reference = reference
                    .decode()
                    .map_err(|error| xmp_error(format!("XMP reference is invalid: {error}")))?;
                let escaped = format!("&{reference};");
                let decoded = unescape(&escaped)
                    .map_err(|error| xmp_error(format!("XMP reference is invalid: {error}")))?;
                node.text.push_str(&decoded);
            }
            Ok(Event::Eof) => break,
            Ok(Event::CData(_) | Event::DocType(_)) => {
                return Err(xmp_error("XMP contains an unsupported XML construct"));
            }
            Ok(_) => {}
            Err(error) => return Err(xmp_error(format!("XMP could not be parsed: {error}"))),
        }
        buffer.clear();
    }
    if !stack.is_empty() {
        return Err(xmp_error("XMP element nesting is incomplete"));
    }
    let root = root.ok_or_else(|| xmp_error("XMP root element is missing"))?;
    if !root.matches(XMP_META_NAMESPACE, b"xmpmeta") {
        return Err(xmp_error("XMP root element has the wrong namespace"));
    }
    Ok(root)
}

fn attach_node(stack: &mut [XmlNode], root: &mut Option<XmlNode>, node: XmlNode) -> Result<()> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.replace(node).is_some() {
        return Err(xmp_error("XMP contains multiple root elements"));
    }
    Ok(())
}

fn expanded_name(reader: &NsReader<&[u8]>, name: QName<'_>) -> Result<XmlName> {
    let (namespace, local) = reader.resolver().resolve_element(name);
    let namespace = match namespace {
        ResolveResult::Bound(namespace) => namespace.as_ref().to_vec(),
        ResolveResult::Unbound => Vec::new(),
        ResolveResult::Unknown(prefix) => {
            return Err(xmp_error(format!(
                "XMP namespace prefix is unbound: {}",
                String::from_utf8_lossy(&prefix)
            )));
        }
    };
    Ok(XmlName {
        namespace,
        local: local.as_ref().to_vec(),
    })
}

fn required_child<'a>(node: &'a XmlNode, namespace: &[u8], local: &[u8]) -> Result<&'a XmlNode> {
    optional_child(node, namespace, local)?
        .ok_or_else(|| xmp_error(format!("XMP field {} is missing", display_name(local))))
}

fn optional_child<'a>(
    node: &'a XmlNode,
    namespace: &[u8],
    local: &[u8],
) -> Result<Option<&'a XmlNode>> {
    let mut matches = node
        .children
        .iter()
        .filter(|child| child.matches(namespace, local));
    let first = matches.next();
    if matches.next().is_some() {
        return Err(xmp_error(format!(
            "XMP field {} is duplicated",
            display_name(local)
        )));
    }
    Ok(first)
}

fn required_text(node: &XmlNode, namespace: &[u8], local: &[u8]) -> Result<String> {
    leaf_text(required_child(node, namespace, local)?)
}

fn optional_text(node: &XmlNode, namespace: &[u8], local: &[u8]) -> Result<Option<String>> {
    optional_child(node, namespace, local)?
        .map(leaf_text)
        .transpose()
}

fn leaf_text(node: &XmlNode) -> Result<String> {
    if !node.children.is_empty() {
        return Err(xmp_error("XMP scalar field contains nested elements"));
    }
    Ok(node.text.clone())
}

fn required_alt(node: &XmlNode, namespace: &[u8], local: &[u8]) -> Result<String> {
    optional_alt(node, namespace, local)?
        .ok_or_else(|| xmp_error(format!("XMP field {} is missing", display_name(local))))
}

fn optional_alt(node: &XmlNode, namespace: &[u8], local: &[u8]) -> Result<Option<String>> {
    let Some(field) = optional_child(node, namespace, local)? else {
        return Ok(None);
    };
    let alt = required_child(field, RDF_NAMESPACE, b"Alt")?;
    let values = list_items(alt)?;
    values
        .into_iter()
        .next()
        .map(Some)
        .ok_or_else(|| xmp_error("XMP language alternative is empty"))
}

fn required_array(
    node: &XmlNode,
    namespace: &[u8],
    local: &[u8],
    container: &[u8],
) -> Result<Vec<String>> {
    optional_array(node, namespace, local, container)?
        .ok_or_else(|| xmp_error(format!("XMP field {} is missing", display_name(local))))
}

fn optional_array(
    node: &XmlNode,
    namespace: &[u8],
    local: &[u8],
    container: &[u8],
) -> Result<Option<Vec<String>>> {
    let Some(field) = optional_child(node, namespace, local)? else {
        return Ok(None);
    };
    let container = required_child(field, RDF_NAMESPACE, container)?;
    Ok(Some(list_items(container)?))
}

fn list_items(node: &XmlNode) -> Result<Vec<String>> {
    node.children
        .iter()
        .map(|item| {
            if !item.matches(RDF_NAMESPACE, b"li") {
                return Err(xmp_error("XMP array contains an invalid item"));
            }
            leaf_text(item)
        })
        .collect()
}

fn required_digest(node: &XmlNode, local: &[u8]) -> Result<ContentDigest> {
    required_text(node, OFFPRINT_NAMESPACE, local)?
        .parse()
        .map_err(|error| xmp_error(format!("XMP digest is invalid: {error}")))
}

fn required_u32(node: &XmlNode, local: &[u8]) -> Result<u32> {
    required_text(node, OFFPRINT_NAMESPACE, local)?
        .parse()
        .map_err(|_| xmp_error(format!("XMP field {} is invalid", display_name(local))))
}

fn required_u64(node: &XmlNode, local: &[u8]) -> Result<u64> {
    required_text(node, OFFPRINT_NAMESPACE, local)?
        .parse()
        .map_err(|_| xmp_error(format!("XMP field {} is invalid", display_name(local))))
}

fn required_boolean(node: &XmlNode, local: &[u8]) -> Result<bool> {
    let value = required_text(node, OFFPRINT_NAMESPACE, local)?;
    parse_boolean(&value)
        .ok_or_else(|| xmp_error(format!("XMP field {} is invalid", display_name(local))))
}

fn required_browser_product(node: &XmlNode) -> Result<BrowserProduct> {
    let value = required_text(node, OFFPRINT_NAMESPACE, b"BrowserProduct")?;
    parse_browser_product(&value).ok_or_else(|| xmp_error("XMP browser product is invalid"))
}

fn required_verification_mode(node: &XmlNode) -> Result<VerificationMode> {
    let value = required_text(node, OFFPRINT_NAMESPACE, b"VerificationModeArg")?;
    parse_verification_mode(&value).ok_or_else(|| xmp_error("XMP verification mode is invalid"))
}

fn display_name(local: &[u8]) -> String {
    String::from_utf8_lossy(local).into_owned()
}

fn xmp_error(message: impl Into<String>) -> OffprintError {
    OffprintError::new(
        "offprint.export.pdf_metadata",
        ErrorStage::Verification,
        message,
    )
}
