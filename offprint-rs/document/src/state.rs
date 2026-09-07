use html5ever::{Attribute, QualName};
use markup5ever::ns;
use offprint_model::{ErrorStage, OffprintError, Result};

use crate::{Document, NodeData};

pub const DOCUMENT_SCROLL_X_ATTRIBUTE: &str = "data-offprint-scroll-x";
pub const DOCUMENT_SCROLL_Y_ATTRIBUTE: &str = "data-offprint-scroll-y";
pub const ELEMENT_SCROLL_LEFT_ATTRIBUTE: &str = "data-offprint-scroll-left";
pub const ELEMENT_SCROLL_TOP_ATTRIBUTE: &str = "data-offprint-scroll-top";

pub fn set_document_scroll_state(document: &mut Document, x: &str, y: &str) -> Result<()> {
    let root = document.find_html_element("html").ok_or_else(|| {
        OffprintError::new(
            "offprint.transform.document",
            ErrorStage::Transform,
            "captured document has no HTML root for scroll state",
        )
    })?;
    let Some(NodeData::Element { attrs, .. }) = document.node_mut(root).map(|node| &mut node.data)
    else {
        return Err(OffprintError::new(
            "offprint.transform.document",
            ErrorStage::Transform,
            "captured document root is not an HTML element",
        ));
    };
    set_attribute(attrs, DOCUMENT_SCROLL_X_ATTRIBUTE, x);
    set_attribute(attrs, DOCUMENT_SCROLL_Y_ATTRIBUTE, y);
    Ok(())
}

fn set_attribute(attrs: &mut Vec<Attribute>, name: &str, value: &str) {
    attrs.retain(|attribute| attribute.name.ns != ns!() || attribute.name.local.as_ref() != name);
    attrs.push(Attribute {
        name: QualName::new(None, ns!(), name.into()),
        value: value.into(),
    });
}

#[cfg(test)]
mod tests {
    use crate::{Document, serialize_document, set_document_scroll_state};

    #[test]
    fn document_scroll_state_is_replaced_on_the_html_root() {
        let mut document = Document::parse(
            br#"<html data-offprint-scroll-x="old"><head></head><body></body></html>"#,
        );

        let result = set_document_scroll_state(&mut document, "12.5", "48");
        let html = serialize_document(&document).map(String::from_utf8);

        assert!(result.is_ok());
        assert!(html.is_ok_and(|html| {
            html.is_ok_and(|html| {
                html.contains(r#"data-offprint-scroll-x="12.5""#)
                    && html.contains(r#"data-offprint-scroll-y="48""#)
                    && !html.contains(r#"data-offprint-scroll-x="old""#)
            })
        }));
    }
}
