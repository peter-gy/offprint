use html5ever::Attribute;
use markup5ever::{local_name, ns};
use pageknot_document::{NodeData, serialize_document};
use pageknot_model::{ErrorStage, PageKnotError, Result};

use crate::csp::sandboxed_content_security_policy;
use crate::{CSP_ELEMENT_ID, STATE_SCRIPT_ELEMENT_ID, verify_html, verify_static_sandboxed};

pub fn encode_sandboxed_html(bytes: &[u8]) -> Result<Vec<u8>> {
    let verified = verify_html(bytes)?;
    let manifest = verified.manifest();
    if manifest.structural_repair.applied {
        return Err(PageKnotError::new(
            "pageknot.artifact.sandbox",
            ErrorStage::Encoding,
            "sandboxed HTML cannot apply the artifact structural repair program",
        ));
    }

    let mut document = verified.document().clone();
    let state_scripts = document
        .walk()
        .filter(|id| {
            matches!(
                document.node(*id).map(|node| &node.data),
                Some(NodeData::Element { name, attrs, .. })
                    if name.ns == ns!(html)
                        && name.local == local_name!("script")
                        && attribute_value(attrs, "id") == Some(STATE_SCRIPT_ELEMENT_ID)
            )
        })
        .collect::<Vec<_>>();
    for script in state_scripts {
        document.detach(script);
    }

    let policy = sandboxed_content_security_policy(manifest);
    let csp = document
        .walk()
        .find(|id| {
            matches!(
                document.node(*id).map(|node| &node.data),
                Some(NodeData::Element { name, attrs, .. })
                    if name.ns == ns!(html)
                        && name.local == local_name!("meta")
                        && attribute_value(attrs, "id") == Some(CSP_ELEMENT_ID)
            )
        })
        .ok_or_else(|| {
            PageKnotError::new(
                "pageknot.artifact.sandbox",
                ErrorStage::Encoding,
                "sandboxed HTML has no PageKnot content security policy",
            )
        })?;
    let Some(NodeData::Element { attrs, .. }) = document.node_mut(csp).map(|node| &mut node.data)
    else {
        return Err(PageKnotError::new(
            "pageknot.artifact.sandbox",
            ErrorStage::Encoding,
            "sandboxed HTML content security policy is malformed",
        ));
    };
    let Some(content) = attrs.iter_mut().find(|attribute| {
        attribute.name.ns == ns!() && attribute.name.local == local_name!("content")
    }) else {
        return Err(PageKnotError::new(
            "pageknot.artifact.sandbox",
            ErrorStage::Encoding,
            "sandboxed HTML content security policy has no content",
        ));
    };
    content.value = policy.into();

    let encoded = serialize_document(&document).map_err(|error| {
        PageKnotError::new(
            "pageknot.artifact.serialize",
            ErrorStage::Encoding,
            format!("failed to serialize sandboxed HTML: {error}"),
        )
    })?;
    verify_static_sandboxed(&encoded)?;
    Ok(encoded)
}

fn attribute_value<'a>(attrs: &'a [Attribute], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|attribute| attribute.name.ns == ns!() && attribute.name.local.as_ref() == name)
        .map(|attribute| attribute.value.as_ref())
}

#[cfg(test)]
mod tests {
    use pageknot_document::Document;

    use crate::{STATE_SCRIPT_ELEMENT_ID, encode_html, inspect_html, verify_static_sandboxed};

    use super::encode_sandboxed_html;

    #[test]
    fn sandboxed_html_keeps_view_state_without_an_executable_restorer() {
        let document = Document::parse(b"<html><head></head><body></body></html>");
        let mut manifest = crate::verify::test_manifest();
        manifest.view_state.scroll_x = "8".to_owned();
        manifest.view_state.scroll_y = "144".to_owned();
        let sandboxed =
            encode_html(&document, &manifest).and_then(|bytes| encode_sandboxed_html(&bytes));
        let html = sandboxed
            .as_ref()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned());

        assert!(
            html.as_ref()
                .is_ok_and(|html| !html.contains(STATE_SCRIPT_ELEMENT_ID))
        );
        assert!(
            html.as_ref()
                .is_ok_and(|html| html.contains("script-src 'none'"))
        );
        assert!(
            html.as_ref()
                .is_ok_and(|html| html.contains(r#"data-pageknot-scroll-x="8""#))
        );
        assert!(
            html.as_ref()
                .is_ok_and(|html| html.contains(r#"data-pageknot-scroll-y="144""#))
        );
        assert!(
            sandboxed
                .as_deref()
                .is_ok_and(|bytes| verify_static_sandboxed(bytes).is_ok())
        );
        assert!(
            sandboxed
                .as_deref()
                .is_ok_and(|bytes| inspect_html(bytes).is_ok_and(|decoded| {
                    decoded.view_state.scroll_x == "8" && decoded.view_state.scroll_y == "144"
                }))
        );
    }
}
