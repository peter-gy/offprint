use crate::{Document, NodeData, SafeStaticPolicy};
use html5ever::{Attribute, QualName};
use markup5ever::{local_name, ns};
use offprint_model::{ErrorStage, OffprintError, Result};

const FREEZE_ATTRIBUTE: &str = "data-offprint-freeze";
const FREEZE_CSS: &str = "*,*::before,*::after{animation-play-state:paused!important;transition:none!important;caret-color:transparent!important}";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SanitizationReport {
    pub removed_elements: u64,
    pub removed_event_attributes: u64,
    pub removed_javascript_urls: u64,
}

pub fn sanitize_safe_static(document: &mut Document) -> SanitizationReport {
    let ids = document.walk().collect::<Vec<_>>();
    let mut report = SanitizationReport::default();
    for id in ids {
        let should_remove = document
            .node(id)
            .is_some_and(|node| should_remove_element(&node.data));
        if should_remove {
            document.detach(id);
            report.removed_elements += 1;
            continue;
        }
        let Some(node) = document.node_mut(id) else {
            continue;
        };
        let NodeData::Element { attrs, .. } = &mut node.data else {
            continue;
        };
        sanitize_attributes(attrs, &mut report);
    }
    report
}

pub fn ensure_render_freeze_styles(document: &mut Document) -> Result<()> {
    let parent = document
        .find_html_element("head")
        .or_else(|| document.find_html_element("html"))
        .ok_or_else(|| {
            OffprintError::new(
                "offprint.transform.document",
                ErrorStage::Transform,
                "captured document has no HTML root for rendering freeze rules",
            )
        })?;
    if !has_direct_freeze_style(document, parent) {
        append_freeze_style(document, parent)?;
    }

    let shadow_roots = document
        .walk()
        .filter_map(|id| {
            let NodeData::Element {
                name,
                attrs,
                template_contents,
                ..
            } = &document.node(id)?.data
            else {
                return None;
            };
            (name.ns == ns!(html)
                && name.local == local_name!("template")
                && attribute_value(attrs, "shadowrootmode").is_some())
            .then_some(*template_contents)
            .flatten()
        })
        .collect::<Vec<_>>();
    for root in shadow_roots {
        if !has_direct_freeze_style(document, root) {
            append_freeze_style(document, root)?;
        }
    }
    Ok(())
}

fn has_direct_freeze_style(document: &Document, parent: offprint_model::NodeId) -> bool {
    document
        .node(parent)
        .into_iter()
        .flat_map(|node| node.children.iter())
        .any(|id| {
            matches!(
                document.node(*id).map(|node| &node.data),
                Some(NodeData::Element { name, attrs, .. })
                    if name.ns == ns!(html)
                        && name.local == local_name!("style")
                        && attribute_value(attrs, FREEZE_ATTRIBUTE).is_some()
            )
        })
}

fn append_freeze_style(document: &mut Document, parent: offprint_model::NodeId) -> Result<()> {
    let style = document.create_node(NodeData::Element {
        name: QualName::new(None, ns!(html), local_name!("style")),
        attrs: vec![Attribute {
            name: QualName::new(None, ns!(), FREEZE_ATTRIBUTE.into()),
            value: String::new().into(),
        }],
        template_contents: None,
        mathml_annotation_xml_integration_point: false,
    })?;
    let text = document.create_node(NodeData::Text {
        contents: FREEZE_CSS.into(),
    })?;
    document.append_child(style, text)?;
    document.append_child(parent, style)
}

fn should_remove_element(data: &NodeData) -> bool {
    let NodeData::Element { name, attrs, .. } = data else {
        return false;
    };
    if SafeStaticPolicy::is_script_element(name) {
        return name.ns == ns!(svg) || !SafeStaticPolicy::is_structured_metadata_script(attrs);
    }
    if SafeStaticPolicy::is_base_element(name) {
        return true;
    }
    if SafeStaticPolicy::is_meta_refresh(name, attrs) {
        return true;
    }
    if name.ns == ns!(html)
        && name.local == local_name!("meta")
        && attribute_value(attrs, "http-equiv")
            .is_some_and(|value| value.eq_ignore_ascii_case("content-security-policy"))
    {
        return true;
    }
    SafeStaticPolicy::is_request_triggering_link(name, attrs)
}

fn sanitize_attributes(attrs: &mut Vec<Attribute>, report: &mut SanitizationReport) {
    attrs.retain(|attribute| {
        if SafeStaticPolicy::is_event_attribute(attribute) {
            report.removed_event_attributes += 1;
            return false;
        }
        if SafeStaticPolicy::is_javascript_url_attribute(attribute) {
            report.removed_javascript_urls += 1;
            return false;
        }
        true
    });
}

fn attribute_value<'a>(attrs: &'a [Attribute], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|attribute| attribute.name.ns == ns!() && attribute.name.local.as_ref() == name)
        .map(|attribute| attribute.value.as_ref())
}

#[cfg(test)]
mod tests {
    use crate::{Document, ensure_render_freeze_styles, sanitize_safe_static, serialize_document};

    #[test]
    fn safe_static_sanitization_removes_active_content_and_preserves_json_metadata() {
        let mut document = Document::parse(
            br#"<html><head>
                <base href="https://example.com/">
                <link rel="preload" href="font.woff2">
                <script>alert(1)</script>
                <script type="application/ld+json">{"name":"Offprint"}</script>
                </head><body onload="go()"><a href=" JAVASCRIPT:go()">go</a></body></html>"#,
        );

        let report = sanitize_safe_static(&mut document);
        let html = serialize_document(&document);
        let html = html
            .as_ref()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned());

        assert_eq!(report.removed_elements, 3);
        assert_eq!(report.removed_event_attributes, 1);
        assert_eq!(report.removed_javascript_urls, 1);
        assert!(
            html.as_ref()
                .is_ok_and(|html| html.contains("application/ld+json"))
        );
        assert!(html.as_ref().is_ok_and(|html| !html.contains("alert(1)")));
    }

    #[test]
    fn removing_a_script_preserves_siblings_with_declarative_shadow_content() {
        let mut document = Document::parse(
            br#"<html><head></head><body><article><input value="after">
            <template shadowrootmode="closed"><strong>shadow</strong></template></article>
            <script>remove()</script></body></html>"#,
        );

        sanitize_safe_static(&mut document);
        let html = serialize_document(&document)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());

        assert!(
            html.as_ref()
                .is_some_and(|html| html.contains("value=\"after\""))
        );
        assert!(
            html.as_ref()
                .is_some_and(|html| html.contains("<strong>shadow</strong>"))
        );
    }

    #[test]
    fn safe_static_sanitization_removes_svg_scripts() {
        let mut document = Document::parse(
            br#"<html><head></head><body>
            <svg xmlns="http://www.w3.org/2000/svg">
              <script href="https://example.test/active.js">run()</script>
              <rect width="10" height="10"/>
            </svg>
            </body></html>"#,
        );

        let report = sanitize_safe_static(&mut document);
        let html = serialize_document(&document)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());

        assert_eq!(report.removed_elements, 1);
        assert!(html.as_ref().is_some_and(|html| !html.contains("run()")));
        assert!(
            html.as_ref()
                .is_some_and(|html| html.contains("<rect width=\"10\" height=\"10\"></rect>"))
        );
    }

    #[test]
    fn safe_static_sanitization_removes_request_triggering_links() {
        let mut document = Document::parse(
            br#"<html><head>
            <link rel="  MANIFEST  " href="/app.webmanifest">
            <link rel="alternate&#x9;PreLoad&#xA;" href="/next.html">
            <link rel="stylesheet" href="data:text/css,body{}">
            <link rel="icon" href="data:image/png;base64,">
            </head><body></body></html>"#,
        );

        let report = sanitize_safe_static(&mut document);
        let html = serialize_document(&document)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());

        assert_eq!(report.removed_elements, 2);
        assert!(
            html.as_ref()
                .is_some_and(|html| !html.contains("app.webmanifest"))
        );
        assert!(
            html.as_ref()
                .is_some_and(|html| !html.contains("next.html"))
        );
        assert!(
            html.as_ref()
                .is_some_and(|html| html.contains("rel=\"stylesheet\""))
        );
        assert!(
            html.as_ref()
                .is_some_and(|html| html.contains("rel=\"icon\""))
        );
    }

    #[test]
    fn rendering_freeze_rules_cover_the_document_and_declarative_shadow_roots() {
        let mut document = Document::parse(
            br#"<html><head></head><body><template shadowrootmode="closed">
            <span>shadow</span></template></body></html>"#,
        );

        let first = ensure_render_freeze_styles(&mut document);
        let second = ensure_render_freeze_styles(&mut document);
        let html = serialize_document(&document)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());

        assert!(first.is_ok(), "{first:?}");
        assert!(second.is_ok(), "{second:?}");
        assert_eq!(
            html.as_deref()
                .map(|html| html.matches("data-offprint-freeze").count()),
            Some(2)
        );
    }
}
