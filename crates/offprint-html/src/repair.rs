use std::collections::BTreeSet;

use html5ever::tendril::StrTendril;
use html5ever::{Attribute, QualName};
use markup5ever::{local_name, ns};
use offprint_document::{Document, NodeData};
use offprint_model::{
    ContentDigest, ErrorStage, MAXIMUM_CAPTURE_NODES, OffprintError, RepairNode, Result,
    StructuralRepairTree,
};

pub const CSP_ELEMENT_ID: &str = "offprint-csp";
pub const REPAIR_DATA_ELEMENT_ID: &str = "offprint-repair-data";
pub const REPAIR_SCRIPT_ELEMENT_ID: &str = "offprint-repair-script";
pub const REPAIR_MEDIA_TYPE: &str = "application/vnd.offprint.repair+json";
pub const REPAIR_MARKER_ATTRIBUTE: &str = "data-offprint-node";

pub const RESTORATION_SCRIPT: &str = r#"(() => {
  "use strict";
  const data = document.getElementById("offprint-repair-data");
  if (!data) return;
  const ownedIds = new Set(["offprint-csp", "offprint-manifest", "offprint-repair-data", "offprint-repair-script", "offprint-state-script"]);
  const rawNames = new Set(["iframe", "noembed", "noframes", "plaintext", "script", "style", "textarea", "title", "xmp"]);
  const nodes = new Map();
  const index = (root) => {
    for (const node of root.childNodes) {
      if (node.nodeType !== Node.ELEMENT_NODE) continue;
      const marker = node.getAttribute("data-offprint-node");
      if (marker !== null && !nodes.has(marker)) nodes.set(marker, node);
      if (node.localName === "template") index(node.content);
      index(node);
      if (node.shadowRoot) index(node.shadowRoot);
    }
  };
  const restoreShadow = (host, spec) => {
    const root = host.shadowRoot;
    if (!root) return false;
    const children = spec.templateContent.map(build).filter((node) => node !== null);
    root.replaceChildren(...children);
    return true;
  };
  const buildChildren = (element, specs) => {
    const children = [];
    for (const spec of specs) {
      if (spec.kind === "element" && spec.shadowMode && restoreShadow(element, spec)) continue;
      const child = build(spec);
      if (child !== null) children.push(child);
    }
    return children;
  };
  const build = (spec) => {
    if (spec.kind === "text") return document.createTextNode(spec.value);
    if (spec.kind === "comment") return document.createComment(spec.value);
    const element = nodes.get(spec.marker);
    if (!element) return null;
    if (spec.shadowMode) {
      const children = spec.templateContent.map(build).filter((node) => node !== null);
      element.content.replaceChildren(...children);
      return element;
    }
    if (!rawNames.has(spec.name)) {
      const container = spec.name === "template" ? element.content : element;
      const source = spec.name === "template" ? spec.templateContent : spec.children;
      const children = buildChildren(element, source);
      if (spec.name === "head") {
        children.push(...Array.from(element.children).filter((child) => ownedIds.has(child.id)));
      }
      container.replaceChildren(...children);
    }
    return element;
  };
  const clearMarkers = (root) => {
    for (const node of root.childNodes) {
      if (node.nodeType !== Node.ELEMENT_NODE) continue;
      node.removeAttribute("data-offprint-node");
      if (node.localName === "template") clearMarkers(node.content);
      clearMarkers(node);
      if (node.shadowRoot) clearMarkers(node.shadowRoot);
    }
  };
  const restore = () => {
    let repair;
    try {
      repair = JSON.parse(data.textContent);
    } catch {
      return;
    }
    index(document);
    const root = build(repair.documentElement);
    if (root && root !== document.documentElement) document.documentElement.replaceWith(root);
    document.documentElement.removeAttribute("data-offprint-node");
    clearMarkers(document.documentElement);
  };
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", restore, { once: true });
  } else {
    restore();
  }
})();"#;

#[must_use]
pub fn structural_repair_script_digest() -> ContentDigest {
    ContentDigest::sha256(RESTORATION_SCRIPT.as_bytes())
}

pub fn apply_structural_repair(document: &mut Document) -> Result<bool> {
    let data_nodes = matching_scripts(document, REPAIR_DATA_ELEMENT_ID, Some(REPAIR_MEDIA_TYPE));
    if data_nodes.is_empty() {
        return Ok(false);
    }
    if data_nodes.len() != 1 {
        return Err(repair_error(
            "a captured document contains more than one structural repair record",
        ));
    }
    let data_node = data_nodes[0];
    let json = text_contents(document, data_node);
    let repair: StructuralRepairTree = serde_json::from_str(&json)
        .map_err(|error| repair_error(format!("structural repair data is malformed: {error}")))?;
    validate_repair_tree(&repair)?;

    for script in matching_scripts(document, REPAIR_SCRIPT_ELEMENT_ID, None) {
        document.detach(script);
    }
    let head = document
        .find_html_element("head")
        .ok_or_else(|| repair_error("structural repair requires an HTML head element"))?;
    let script = document.create_node(NodeData::Element {
        name: html_name(local_name!("script")),
        attrs: vec![html_attribute(local_name!("id"), REPAIR_SCRIPT_ELEMENT_ID)],
        template_contents: None,
        mathml_annotation_xml_integration_point: false,
    })?;
    let script_text = document.create_node(NodeData::Text {
        contents: RESTORATION_SCRIPT.into(),
    })?;
    document.append_child(script, script_text)?;
    document.append_child(head, script)?;
    Ok(true)
}

pub fn validate_repair_tree(repair: &StructuralRepairTree) -> Result<()> {
    let RepairNode::Element {
        marker,
        namespace,
        name,
        ..
    } = &repair.document_element
    else {
        return Err(repair_error(
            "structural repair root must be an HTML element",
        ));
    };
    if marker != "0" || namespace != "http://www.w3.org/1999/xhtml" || name != "html" {
        return Err(repair_error(
            "structural repair root does not identify the captured HTML element",
        ));
    }
    let mut markers = BTreeSet::new();
    let mut pending = vec![&repair.document_element];
    let mut nodes = 0_u64;
    while let Some(node) = pending.pop() {
        nodes = nodes.saturating_add(1);
        if nodes > MAXIMUM_CAPTURE_NODES {
            return Err(
                repair_error("structural repair data exceeds the node limit")
                    .with_detail("attempted", nodes)
                    .with_detail("limit", MAXIMUM_CAPTURE_NODES),
            );
        }
        if let RepairNode::Element {
            marker,
            namespace,
            name,
            children,
            template_content,
            shadow_mode,
        } = node
        {
            if marker.is_empty()
                || marker.len() > 20
                || !marker.bytes().all(|byte| byte.is_ascii_digit())
                || !markers.insert(marker.as_str())
            {
                return Err(repair_error(
                    "structural repair markers must be unique decimal identifiers",
                ));
            }
            if namespace.is_empty() || namespace.len() > 256 || name.is_empty() || name.len() > 256
            {
                return Err(repair_error(
                    "structural repair element identity is invalid",
                ));
            }
            if shadow_mode
                .as_deref()
                .is_some_and(|mode| !matches!(mode, "open" | "closed"))
            {
                return Err(repair_error("structural repair shadow mode is invalid"));
            }
            pending.extend(children);
            pending.extend(template_content);
        }
    }
    Ok(())
}

fn matching_scripts(
    document: &Document,
    id: &str,
    media_type: Option<&str>,
) -> Vec<offprint_model::NodeId> {
    document
        .walk()
        .filter(|node_id| {
            let Some(NodeData::Element { name, attrs, .. }) =
                document.node(*node_id).map(|node| &node.data)
            else {
                return false;
            };
            name.ns == ns!(html)
                && name.local == local_name!("script")
                && attribute_value(attrs, "id") == Some(id)
                && media_type
                    .is_none_or(|expected| attribute_value(attrs, "type") == Some(expected))
        })
        .collect()
}

fn text_contents(document: &Document, parent: offprint_model::NodeId) -> String {
    document
        .node(parent)
        .into_iter()
        .flat_map(|node| &node.children)
        .filter_map(|id| document.node(*id))
        .filter_map(|node| match &node.data {
            NodeData::Text { contents } => Some(contents.as_ref()),
            _ => None,
        })
        .collect()
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

fn repair_error(message: impl Into<String>) -> OffprintError {
    OffprintError::new(
        "offprint.transform.structural_repair",
        ErrorStage::Transform,
        message,
    )
}

#[cfg(test)]
mod tests {
    use offprint_document::{Document, serialize_document};

    use super::{
        REPAIR_DATA_ELEMENT_ID, REPAIR_MEDIA_TYPE, REPAIR_SCRIPT_ELEMENT_ID,
        apply_structural_repair,
    };

    #[test]
    fn valid_repair_data_adds_the_owned_script() {
        let source = format!(
            r#"<html data-offprint-node="0"><head data-offprint-node="1">
            <script id="{REPAIR_DATA_ELEMENT_ID}" type="{REPAIR_MEDIA_TYPE}">{{
              "documentElement": {{
                "kind": "element",
                "marker": "0",
                "namespace": "http://www.w3.org/1999/xhtml",
                "name": "html",
                "children": [],
                "templateContent": []
              }}
            }}</script></head><body data-offprint-node="2"></body></html>"#
        );
        let mut document = Document::parse(source.as_bytes());

        assert_eq!(apply_structural_repair(&mut document), Ok(true));
        let html = serialize_document(&document)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        assert!(
            html.as_ref()
                .is_some_and(|html| html.contains(REPAIR_SCRIPT_ELEMENT_ID))
        );
    }
}
