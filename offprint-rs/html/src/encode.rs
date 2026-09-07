use std::io::Write;

use html5ever::tendril::StrTendril;
use html5ever::{Attribute, QualName};
use markup5ever::{local_name, ns};
use offprint_document::{Document, NodeData, serialize_document_to, set_document_scroll_state};
use offprint_model::{ArtifactManifest, ErrorStage, OffprintError, Result};

use crate::{
    CSP_ELEMENT_ID, MANIFEST_ELEMENT_ID, MANIFEST_MEDIA_TYPE, apply_state_restoration,
    content_security_policy,
};

pub fn encode_html(document: &Document, manifest: &ArtifactManifest) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    encode_html_to(document, manifest, &mut output)?;
    Ok(output)
}

/// Encodes the safe-static document and manifest into `output`.
///
/// The caller controls buffering and output limits through its writer.
pub fn encode_html_to(
    document: &Document,
    manifest: &ArtifactManifest,
    output: &mut (impl Write + ?Sized),
) -> Result<()> {
    if !manifest.resources.is_complete() {
        return Err(OffprintError::new(
            "offprint.artifact.resource_summary",
            ErrorStage::Encoding,
            "manifest resource summary must account for every discovered reference",
        ));
    }
    let mut document = document.clone();
    remove_existing_artifact_metadata(&mut document);
    set_document_scroll_state(
        &mut document,
        &manifest.view_state.scroll_x,
        &manifest.view_state.scroll_y,
    )?;
    let head = document.find_html_element("head").ok_or_else(|| {
        OffprintError::new(
            "offprint.artifact.head",
            ErrorStage::Encoding,
            "captured document has no HTML head element",
        )
    })?;

    let csp = content_security_policy(manifest);
    let csp_meta = document.create_node(NodeData::Element {
        name: html_name(local_name!("meta")),
        attrs: vec![
            html_attribute(local_name!("id"), CSP_ELEMENT_ID),
            html_attribute(local_name!("http-equiv"), "Content-Security-Policy"),
            html_attribute(local_name!("content"), &csp),
        ],
        template_contents: None,
        mathml_annotation_xml_integration_point: false,
    })?;
    document.prepend_child(head, csp_meta)?;

    let manifest_json = serde_json::to_string(manifest).map_err(|error| {
        OffprintError::new(
            "offprint.artifact.manifest",
            ErrorStage::Encoding,
            format!("failed to serialize the artifact manifest: {error}"),
        )
    })?;
    let manifest_script = document.create_node(NodeData::Element {
        name: html_name(local_name!("script")),
        attrs: vec![
            html_attribute(local_name!("id"), MANIFEST_ELEMENT_ID),
            html_attribute(local_name!("type"), MANIFEST_MEDIA_TYPE),
        ],
        template_contents: None,
        mathml_annotation_xml_integration_point: false,
    })?;
    let manifest_text = document.create_node(NodeData::Text {
        contents: escape_script_data(&manifest_json).into(),
    })?;
    document.append_child(manifest_script, manifest_text)?;
    document.append_child(head, manifest_script)?;
    apply_state_restoration(&mut document)?;

    serialize_document_to(&document, output).map_err(|error| {
        let code = if error.kind() == std::io::ErrorKind::FileTooLarge {
            "offprint.artifact.size"
        } else {
            "offprint.artifact.serialize"
        };
        OffprintError::new(
            code,
            ErrorStage::Encoding,
            format!("failed to serialize the HTML artifact: {error}"),
        )
    })
}

fn remove_existing_artifact_metadata(document: &mut Document) {
    let ids = document
        .walk()
        .filter(|id| {
            let Some(node) = document.node(*id) else {
                return false;
            };
            let NodeData::Element { name, attrs, .. } = &node.data else {
                return false;
            };
            let existing_manifest = name.ns == ns!(html)
                && name.local == local_name!("script")
                && attribute_value(attrs, "id") == Some(MANIFEST_ELEMENT_ID);
            let existing_csp = name.ns == ns!(html)
                && name.local == local_name!("meta")
                && attribute_value(attrs, "http-equiv")
                    .is_some_and(|value| value.eq_ignore_ascii_case("content-security-policy"));
            existing_manifest || existing_csp
        })
        .collect::<Vec<_>>();
    for id in ids {
        document.detach(id);
    }
}

fn html_name(local: html5ever::LocalName) -> QualName {
    QualName::new(None, ns!(html), local)
}

fn html_attribute(local: html5ever::LocalName, value: &str) -> Attribute {
    Attribute {
        name: QualName::new(None, ns!(), local),
        value: StrTendril::from(value),
    }
}

fn attribute_value<'a>(attrs: &'a [Attribute], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|attribute| attribute.name.ns == ns!() && attribute.name.local.as_ref() == name)
        .map(|attribute| attribute.value.as_ref())
}

fn escape_script_data(json: &str) -> String {
    json.chars()
        .flat_map(|character| match character {
            '<' => "\\u003c".chars().collect::<Vec<_>>(),
            '>' => "\\u003e".chars().collect(),
            '&' => "\\u0026".chars().collect(),
            '\u{2028}' => "\\u2028".chars().collect(),
            '\u{2029}' => "\\u2029".chars().collect(),
            character => vec![character],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::verify::test_manifest;
    use offprint_document::Document;

    use super::encode_html;

    #[test]
    fn manifest_json_cannot_close_its_non_executable_script_element() {
        let document = Document::parse(b"<html><head><title>x</title></head><body></body></html>");
        let mut manifest = test_manifest();
        manifest
            .warning_codes
            .push("</script><script>alert(1)</script>".to_owned());

        let encoded = encode_html(&document, &manifest);
        let html = encoded
            .as_ref()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned());

        assert!(
            html.as_ref()
                .is_ok_and(|html| !html.contains("</script><script>alert(1)"))
        );
        assert!(
            html.as_ref()
                .is_ok_and(|html| html.contains("\\u003c/script\\u003e"))
        );
    }

    #[test]
    fn encoded_artifact_carries_the_manifest_view_state_on_its_root() {
        let document = Document::parse(b"<html><head></head><body></body></html>");
        let mut manifest = test_manifest();
        manifest.view_state.scroll_x = "12.5".to_owned();
        manifest.view_state.scroll_y = "320".to_owned();

        let encoded = encode_html(&document, &manifest);
        let html = encoded
            .as_ref()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned());

        assert!(html.as_ref().is_ok_and(|html| {
            html.contains(r#"data-offprint-scroll-x="12.5""#)
                && html.contains(r#"data-offprint-scroll-y="320""#)
                && html.contains(crate::STATE_SCRIPT_ELEMENT_ID)
        }));
        assert!(encoded.as_deref().is_ok_and(|bytes| {
            crate::inspect_html(bytes)
                .is_ok_and(|decoded| decoded.view_state == manifest.view_state)
        }));
    }
}
