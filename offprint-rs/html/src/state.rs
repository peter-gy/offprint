use html5ever::tendril::StrTendril;
use html5ever::{Attribute, QualName};
use markup5ever::{local_name, ns};
use offprint_document::{Document, NodeData};
use offprint_model::{ContentDigest, ErrorStage, MAXIMUM_CAPTURE_NODES, OffprintError, Result};

pub const STATE_SCRIPT_ELEMENT_ID: &str = "offprint-state-script";

const _: () = assert!(MAXIMUM_CAPTURE_NODES == 1_000_000);

pub const STATE_RESTORATION_SCRIPT: &str = r#"(() => {
  "use strict";
  const limit = 1000000000;
  const limits = { documents: 10000, nodes: 1000000 };
  const ownedIds = new Set(["offprint-csp", "offprint-manifest", "offprint-repair-data", "offprint-repair-script", "offprint-state-script"]);
  const number = (value) => {
    const parsed = Number(value);
    return Number.isFinite(parsed) && Math.abs(parsed) <= limit ? parsed : 0;
  };
  const manifestView = () => {
    try {
      const manifest = JSON.parse(document.getElementById("offprint-manifest")?.textContent ?? "");
      return manifest.viewState ?? {};
    } catch {
      return {};
    }
  };
  const restoreDocument = (doc, view, budget) => {
    if (budget.documents >= limits.documents) return;
    budget.documents += 1;
    const roots = [doc];
    for (let index = 0; index < roots.length; index += 1) {
      for (const element of roots[index].querySelectorAll("*")) {
        if (!ownedIds.has(element.id)) {
          if (budget.nodes >= limits.nodes) return;
          budget.nodes += 1;
        }
        if (element.hasAttribute("data-offprint-scroll-left") || element.hasAttribute("data-offprint-scroll-top")) {
          element.scrollLeft = number(element.getAttribute("data-offprint-scroll-left"));
          element.scrollTop = number(element.getAttribute("data-offprint-scroll-top"));
        }
        if (element.shadowRoot) roots.push(element.shadowRoot);
        if (!element.matches("iframe,frame")) continue;
        try {
          const child = element.contentDocument;
          if (!child?.documentElement) continue;
          restoreDocument(child, {
            scrollX: child.documentElement.getAttribute("data-offprint-scroll-x"),
            scrollY: child.documentElement.getAttribute("data-offprint-scroll-y")
          }, budget);
        } catch {
          continue;
        }
      }
    }
    doc.defaultView?.scrollTo(number(view.scrollX), number(view.scrollY));
  };
  const restore = () => {
    const view = manifestView();
    restoreDocument(document, view, { documents: 0, nodes: 0 });
    requestAnimationFrame(() =>
      restoreDocument(document, view, { documents: 0, nodes: 0 })
    );
  };
  if ("scrollRestoration" in history) history.scrollRestoration = "manual";
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", restore, { once: true });
    addEventListener("load", restore, { once: true });
  } else {
    restore();
  }
})();"#;

#[must_use]
pub fn state_restoration_script_digest() -> ContentDigest {
    ContentDigest::sha256(STATE_RESTORATION_SCRIPT.as_bytes())
}

pub fn apply_state_restoration(document: &mut Document) -> Result<()> {
    for script in matching_scripts(document) {
        document.detach(script);
    }
    let head = document.find_html_element("head").ok_or_else(|| {
        OffprintError::new(
            "offprint.artifact.head",
            ErrorStage::Encoding,
            "captured document has no HTML head element",
        )
    })?;
    let script = document.create_node(NodeData::Element {
        name: QualName::new(None, ns!(html), local_name!("script")),
        attrs: vec![Attribute {
            name: QualName::new(None, ns!(), local_name!("id")),
            value: StrTendril::from(STATE_SCRIPT_ELEMENT_ID),
        }],
        template_contents: None,
        mathml_annotation_xml_integration_point: false,
    })?;
    let text = document.create_node(NodeData::Text {
        contents: STATE_RESTORATION_SCRIPT.into(),
    })?;
    document.append_child(script, text)?;
    document.append_child(head, script)
}

fn matching_scripts(document: &Document) -> Vec<offprint_model::NodeId> {
    document
        .walk()
        .filter(|id| {
            matches!(
                document.node(*id).map(|node| &node.data),
                Some(NodeData::Element { name, attrs, .. })
                    if name.ns == ns!(html)
                        && name.local == local_name!("script")
                        && attrs.iter().any(|attribute| {
                            attribute.name.ns == ns!()
                                && attribute.name.local == local_name!("id")
                                && attribute.value.as_ref() == STATE_SCRIPT_ELEMENT_ID
                        })
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use offprint_document::{Document, serialize_document};

    use super::{STATE_RESTORATION_SCRIPT, STATE_SCRIPT_ELEMENT_ID, apply_state_restoration};

    #[test]
    fn state_restoration_program_is_injected_once() {
        let mut document = Document::parse(
            format!(
                r#"<html><head><script id="{STATE_SCRIPT_ELEMENT_ID}">old</script></head><body></body></html>"#
            )
            .as_bytes(),
        );

        let first = apply_state_restoration(&mut document);
        let second = apply_state_restoration(&mut document);
        let html = serialize_document(&document)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_default();

        assert!(first.is_ok());
        assert!(second.is_ok());
        assert_eq!(
            html.matches(&format!(r#"id="{STATE_SCRIPT_ELEMENT_ID}""#))
                .count(),
            1
        );
        assert!(html.contains(STATE_RESTORATION_SCRIPT));
    }

    #[test]
    fn state_restoration_stops_after_one_million_capture_nodes() {
        assert!(STATE_RESTORATION_SCRIPT.contains("nodes: 1000000"));
    }
}
