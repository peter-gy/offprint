use std::collections::BTreeMap;
use std::ops::Range;

use html5ever::Attribute;
use markup5ever::ns;
use offprint_model::{ErrorStage, NodeId, OffprintError, ResourceId, Result};
use url::Url;

use crate::{Document, NodeData, RenderingRole};

mod css;
mod html;
mod svg;

pub use css::{
    CssParseMode, CssResources, DiscoveredCssResource, discover_css_resources,
    discover_css_resources_bounded,
};
use css::{css_references_bounded, escape_css_string};

const CSS_BASE_ATTRIBUTE: &str = "data-offprint-css-base";

#[derive(Clone, Debug)]
pub struct DiscoveredDocumentResource {
    pub id: ResourceId,
    pub node_id: NodeId,
    pub original: String,
    pub resolved_url: Url,
    pub role: RenderingRole,
}

#[derive(Clone, Debug)]
pub struct DocumentResources {
    resources: Vec<DiscoveredDocumentResource>,
    targets: BTreeMap<ResourceId, RewriteTarget>,
}

impl DocumentResources {
    #[must_use]
    pub fn resources(&self) -> &[DiscoveredDocumentResource] {
        &self.resources
    }

    pub fn rewrite(
        &self,
        document: &mut Document,
        replacements: &BTreeMap<ResourceId, String>,
    ) -> Result<()> {
        let mut ranged = BTreeMap::<TextContainer, Vec<(Range<usize>, String)>>::new();
        for (id, replacement) in replacements {
            let target = self.targets.get(id).ok_or_else(|| {
                resource_error(
                    "offprint.resource.identifier",
                    format!("resource {} has no document target", id.get()),
                )
            })?;
            match target {
                RewriteTarget::Attribute {
                    node_id,
                    attribute_index,
                } => {
                    let attribute = attribute_mut(document, *node_id, *attribute_index)?;
                    attribute.value = replacement.as_str().into();
                }
                RewriteTarget::Range {
                    container,
                    range,
                    syntax,
                } => {
                    let replacement = match syntax {
                        ReplacementSyntax::CssUrl => {
                            format!("url(\"{}\")", escape_css_string(replacement))
                        }
                        ReplacementSyntax::Srcset => replacement.clone(),
                    };
                    ranged
                        .entry(*container)
                        .or_default()
                        .push((range.clone(), replacement));
                }
            }
        }
        for (container, mut changes) in ranged {
            changes.sort_by_key(|change| std::cmp::Reverse(change.0.start));
            let source = container_text(document, container)?.to_owned();
            let mut rewritten = source;
            for (range, replacement) in changes {
                if range.start > range.end
                    || range.end > rewritten.len()
                    || !rewritten.is_char_boundary(range.start)
                    || !rewritten.is_char_boundary(range.end)
                {
                    return Err(resource_error(
                        "offprint.resource.rewrite",
                        "resource rewrite range is outside its source text",
                    ));
                }
                rewritten.replace_range(range, &replacement);
            }
            set_container_text(document, container, rewritten)?;
        }
        remove_css_base_attributes(document);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum TextContainer {
    Attribute {
        node_id: NodeId,
        attribute_index: usize,
    },
    Text(NodeId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReplacementSyntax {
    CssUrl,
    Srcset,
}

#[derive(Clone, Debug)]
enum RewriteTarget {
    Attribute {
        node_id: NodeId,
        attribute_index: usize,
    },
    Range {
        container: TextContainer,
        range: Range<usize>,
        syntax: ReplacementSyntax,
    },
}

#[derive(Clone, Debug)]
pub(super) struct Reference {
    value: String,
    range: Range<usize>,
}

pub fn discover_document_resources(
    document: &Document,
    base_url: &Url,
) -> Result<DocumentResources> {
    discover_document_resources_bounded(document, base_url, usize::MAX)
}

pub fn discover_document_resources_bounded(
    document: &Document,
    base_url: &Url,
    maximum: usize,
) -> Result<DocumentResources> {
    let mut discovery = DiscoveryContext::new(maximum);
    for node_id in document.walk() {
        let Some(node) = document.node(node_id) else {
            continue;
        };
        let NodeData::Element { name, attrs, .. } = &node.data else {
            continue;
        };
        let element = name.local.as_ref();
        for (attribute_index, attribute) in attrs.iter().enumerate() {
            let attribute_name = attribute.name.local.as_ref();
            if attribute.name.ns == ns!() && attribute_name == "style" {
                discovery.add_css_references(
                    node_id,
                    TextContainer::Attribute {
                        node_id,
                        attribute_index,
                    },
                    attribute.value.as_ref(),
                    RenderingRole::Other,
                    base_url,
                )?;
                continue;
            }
            if name.ns == ns!(svg)
                && attribute.name.ns == ns!()
                && let Some(role) = svg::presentation_resource_role(attribute_name)
            {
                discovery.add_css_references(
                    node_id,
                    TextContainer::Attribute {
                        node_id,
                        attribute_index,
                    },
                    attribute.value.as_ref(),
                    role,
                    base_url,
                )?;
                continue;
            }
            if attribute.name.ns == ns!() && attribute_name == "srcset" {
                let remaining = discovery.remaining();
                for reference in
                    html::srcset_references_bounded(attribute.value.as_ref(), remaining)?
                {
                    discovery.add_reference(
                        node_id,
                        reference.value,
                        RenderingRole::Image,
                        RewriteTarget::Range {
                            container: TextContainer::Attribute {
                                node_id,
                                attribute_index,
                            },
                            range: reference.range,
                            syntax: ReplacementSyntax::Srcset,
                        },
                        false,
                        base_url,
                    )?;
                }
                continue;
            }
            let role = if name.ns == ns!(svg) {
                svg::direct_resource_role(element, attribute)
            } else {
                html::direct_resource_role(element, attrs, attribute)
            };
            if let Some(role) = role {
                let same_document_fragment_is_local = name.ns == ns!(svg);
                discovery.add_reference(
                    node_id,
                    attribute.value.to_string(),
                    role,
                    RewriteTarget::Attribute {
                        node_id,
                        attribute_index,
                    },
                    same_document_fragment_is_local,
                    base_url,
                )?;
            }
        }
        if matches!(name.ns, ns!(html) | ns!(svg)) && element == "style" {
            let css_base = stylesheet_base_url(attrs)?.unwrap_or_else(|| base_url.clone());
            for child_id in &node.children {
                let Some(NodeData::Text { contents }) =
                    document.node(*child_id).map(|child| &child.data)
                else {
                    continue;
                };
                discovery.add_css_references(
                    node_id,
                    TextContainer::Text(*child_id),
                    contents,
                    RenderingRole::Other,
                    &css_base,
                )?;
            }
        }
    }
    Ok(discovery.finish())
}

struct DiscoveryContext {
    resources: Vec<DiscoveredDocumentResource>,
    targets: BTreeMap<ResourceId, RewriteTarget>,
    maximum: usize,
}

impl DiscoveryContext {
    fn new(maximum: usize) -> Self {
        Self {
            resources: Vec::new(),
            targets: BTreeMap::new(),
            maximum,
        }
    }

    fn remaining(&self) -> usize {
        self.maximum.saturating_sub(self.resources.len())
    }

    fn finish(self) -> DocumentResources {
        DocumentResources {
            resources: self.resources,
            targets: self.targets,
        }
    }

    fn add_css_references(
        &mut self,
        node_id: NodeId,
        container: TextContainer,
        css: &str,
        role: RenderingRole,
        base_url: &Url,
    ) -> Result<()> {
        for reference in css_references_bounded(css, self.remaining())? {
            self.add_reference(
                node_id,
                reference.value,
                role,
                RewriteTarget::Range {
                    container,
                    range: reference.range,
                    syntax: ReplacementSyntax::CssUrl,
                },
                true,
                base_url,
            )?;
        }
        Ok(())
    }

    fn add_reference(
        &mut self,
        node_id: NodeId,
        original: String,
        role: RenderingRole,
        target: RewriteTarget,
        same_document_fragment_is_local: bool,
        base_url: &Url,
    ) -> Result<()> {
        let trimmed = original.trim();
        if trimmed.is_empty() || (same_document_fragment_is_local && trimmed.starts_with('#')) {
            return Ok(());
        }
        if self.resources.len() >= self.maximum {
            return Err(resource_error(
                "offprint.resource.limit",
                "document resource count exceeds the configured limit",
            ));
        }
        let resolved_url = base_url.join(trimmed).map_err(|error| {
            resource_error(
                "offprint.resource.url",
                format!("resource URL `{trimmed}` cannot be resolved: {error}"),
            )
        })?;
        let index = u32::try_from(self.resources.len()).map_err(|error| {
            resource_error(
                "offprint.resource.limit",
                format!("resource count exceeds the identifier range: {error}"),
            )
        })?;
        let id = ResourceId::new(index);
        self.resources.push(DiscoveredDocumentResource {
            id,
            node_id,
            original,
            resolved_url,
            role,
        });
        self.targets.insert(id, target);
        Ok(())
    }
}

fn stylesheet_base_url(attrs: &[Attribute]) -> Result<Option<Url>> {
    let Some(value) = attrs.iter().find_map(|attribute| {
        (attribute.name.ns == ns!() && attribute.name.local.as_ref() == CSS_BASE_ATTRIBUTE)
            .then_some(attribute.value.as_ref())
    }) else {
        return Ok(None);
    };
    Url::parse(value).map(Some).map_err(|error| {
        resource_error(
            "offprint.resource.url",
            format!("stylesheet base URL `{value}` is invalid: {error}"),
        )
    })
}

fn remove_css_base_attributes(document: &mut Document) {
    let node_ids = document.walk().collect::<Vec<_>>();
    for node_id in node_ids {
        let Some(NodeData::Element { attrs, .. }) =
            document.node_mut(node_id).map(|node| &mut node.data)
        else {
            continue;
        };
        attrs.retain(|attribute| {
            attribute.name.ns != ns!() || attribute.name.local.as_ref() != CSS_BASE_ATTRIBUTE
        });
    }
}

fn attribute_mut(
    document: &mut Document,
    node_id: NodeId,
    attribute_index: usize,
) -> Result<&mut Attribute> {
    let Some(NodeData::Element { attrs, .. }) =
        document.node_mut(node_id).map(|node| &mut node.data)
    else {
        return Err(resource_error(
            "offprint.resource.rewrite",
            "resource attribute owner is unavailable",
        ));
    };
    attrs.get_mut(attribute_index).ok_or_else(|| {
        resource_error(
            "offprint.resource.rewrite",
            "resource attribute is unavailable",
        )
    })
}

fn container_text(document: &Document, container: TextContainer) -> Result<&str> {
    match container {
        TextContainer::Attribute {
            node_id,
            attribute_index,
        } => {
            let Some(NodeData::Element { attrs, .. }) =
                document.node(node_id).map(|node| &node.data)
            else {
                return Err(resource_error(
                    "offprint.resource.rewrite",
                    "resource attribute owner is unavailable",
                ));
            };
            attrs
                .get(attribute_index)
                .map(|attribute| attribute.value.as_ref())
                .ok_or_else(|| {
                    resource_error(
                        "offprint.resource.rewrite",
                        "resource attribute is unavailable",
                    )
                })
        }
        TextContainer::Text(node_id) => {
            let Some(NodeData::Text { contents }) = document.node(node_id).map(|node| &node.data)
            else {
                return Err(resource_error(
                    "offprint.resource.rewrite",
                    "resource CSS text is unavailable",
                ));
            };
            Ok(contents)
        }
    }
}

fn set_container_text(
    document: &mut Document,
    container: TextContainer,
    text: String,
) -> Result<()> {
    match container {
        TextContainer::Attribute {
            node_id,
            attribute_index,
        } => {
            attribute_mut(document, node_id, attribute_index)?.value = text.into();
            Ok(())
        }
        TextContainer::Text(node_id) => {
            let Some(NodeData::Text { contents }) =
                document.node_mut(node_id).map(|node| &mut node.data)
            else {
                return Err(resource_error(
                    "offprint.resource.rewrite",
                    "resource CSS text is unavailable",
                ));
            };
            *contents = text.into();
            Ok(())
        }
    }
}

pub(super) fn resource_error(code: &'static str, message: impl Into<String>) -> OffprintError {
    OffprintError::new(code, ErrorStage::Resource, message)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use url::Url;

    use crate::{
        Document, RenderingRole, discover_document_resources, discover_document_resources_bounded,
        serialize_document,
    };

    #[test]
    fn discovers_and_rewrites_html_css_and_srcset_references() {
        let mut document = Document::parse(
            br#"<html><head><style>@import "theme.css";.x{background:url(icons/a.svg)}</style></head>
                <body><img src="a.png" srcset="small.png 1x, large.png 2x"
                style="cursor: url('cursor.cur'), auto"></body></html>"#,
        );
        let base = Url::parse("https://example.com/docs/").ok();
        let discovered = base
            .as_ref()
            .map(|base| discover_document_resources(&document, base));
        let Some(Ok(discovered)) = discovered else {
            return;
        };
        assert_eq!(discovered.resources().len(), 6);
        let replacements = discovered
            .resources()
            .iter()
            .map(|resource| {
                (
                    resource.id,
                    format!("data:application/octet-stream;base64,{}", resource.id.get()),
                )
            })
            .collect::<BTreeMap<_, _>>();

        assert!(discovered.rewrite(&mut document, &replacements).is_ok());
        let html = serialize_document(&document)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        assert!(
            html.as_ref()
                .is_some_and(|html| !html.contains("https://example.com"))
        );
        assert!(
            html.as_ref()
                .is_some_and(|html| html.matches("data:application").count() == 6)
        );
    }

    #[test]
    fn resolves_materialized_css_from_its_stylesheet_base() {
        let document = Document::parse(
            br#"<html><head><style data-offprint-css-base="https://cdn.example/assets/theme.css">
                @font-face{src:url("font.woff2")}main{background:url("image.svg")}
                </style></head><body><main></main></body></html>"#,
        );
        let base = Url::parse("https://example.com/pages/index.html").ok();
        let discovered = base
            .as_ref()
            .map(|base| discover_document_resources(&document, base));
        let Some(Ok(discovered)) = discovered else {
            return;
        };
        let resolved = discovered
            .resources()
            .iter()
            .map(|resource| resource.resolved_url.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            resolved,
            [
                "https://cdn.example/assets/font.woff2",
                "https://cdn.example/assets/image.svg"
            ]
        );
    }

    #[test]
    fn discovers_and_rewrites_document_icon_links() {
        let mut document = Document::parse(
            br##"<html><head>
                <link rel="shortcut&#xA;ICON" href="/favicon.ico">
                <link rel="apple-touch-icon" href="/touch.png">
                <link rel="mask-icon" href="/mask.svg" color="#123456">
                <link rel="alternate&#x9;StyleSheet" href="/theme.css">
                <link rel="MANIFEST stylesheet" href="/app.webmanifest">
                <link rel="preLOAD" href="/font.woff2">
            </head><body></body></html>"##,
        );
        let base = Url::parse("https://example.com/docs/").ok();
        let discovered = base
            .as_ref()
            .map(|base| discover_document_resources(&document, base));
        let Some(Ok(discovered)) = discovered else {
            return;
        };
        assert_eq!(discovered.resources().len(), 4);
        assert_eq!(
            discovered
                .resources()
                .iter()
                .filter(|resource| resource.role == RenderingRole::Image)
                .count(),
            3
        );
        let replacements = discovered
            .resources()
            .iter()
            .map(|resource| {
                (
                    resource.id,
                    format!("data:application/octet-stream;base64,{}", resource.id.get()),
                )
            })
            .collect::<BTreeMap<_, _>>();

        assert!(discovered.rewrite(&mut document, &replacements).is_ok());
        let html = serialize_document(&document)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        assert!(
            html.as_ref()
                .is_some_and(|html| html.matches("data:application").count() == 4)
        );
    }

    #[test]
    fn discovers_and_rewrites_svg_presentation_and_href_resources() {
        let mut document = Document::parse(
            br##"<html><head></head><body>
            <svg xmlns="http://www.w3.org/2000/svg"
                xmlns:xlink="http://www.w3.org/1999/xlink">
              <defs>
                <linearGradient id="local" href="#parent"/>
                <linearGradient href="gradients.svg#source"/>
                <pattern xlink:href="patterns.svg#tile"/>
                <filter id="effects">
                  <feImage xlink:href="texture.png"/>
                </filter>
              </defs>
              <path fill="#fff" stroke="url(#local) #000"
                filter="url(filters.svg#blur)"
                clip-path="url(clips.svg#clip)"
                mask="url(masks.svg#mask)"
                marker-start="url(markers.svg#start)"
                marker-mid="url(#mid)"
                marker-end="url(markers.svg#end)"
                cursor="url(cursors.svg#cursor), auto"/>
              <use href="#local"/>
              <use xlink:href="symbols.svg#icon"/>
              <image href="photo.png"/>
            </svg>
            </body></html>"##,
        );
        let base = Url::parse("https://example.test/assets/").ok();
        let discovered = base
            .as_ref()
            .map(|base| discover_document_resources(&document, base));
        let Some(Ok(discovered)) = discovered else {
            return;
        };

        assert_eq!(discovered.resources().len(), 11);
        assert_eq!(
            discovered
                .resources()
                .iter()
                .filter(|resource| resource.role == RenderingRole::Image)
                .count(),
            2
        );
        assert_eq!(
            discovered
                .resources()
                .iter()
                .filter(|resource| resource.role == RenderingRole::Cursor)
                .count(),
            1
        );
        assert_eq!(
            discovered
                .resources()
                .iter()
                .filter(|resource| resource.role == RenderingRole::Svg)
                .count(),
            8
        );
        assert!(
            discovered
                .resources()
                .iter()
                .all(|resource| !resource.original.trim_start().starts_with('#'))
        );
        let replacements = discovered
            .resources()
            .iter()
            .map(|resource| {
                (
                    resource.id,
                    format!("data:image/svg+xml;base64,{}", resource.id.get()),
                )
            })
            .collect::<BTreeMap<_, _>>();

        assert!(discovered.rewrite(&mut document, &replacements).is_ok());
        let html = serialize_document(&document)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());

        assert!(
            html.as_ref()
                .is_some_and(|html| html.matches("data:image/svg+xml").count() == 11)
        );
        assert!(
            html.as_ref()
                .is_some_and(|html| html.contains("fill=\"#fff\""))
        );
        assert!(
            html.as_ref()
                .is_some_and(|html| html.contains("stroke=\"url(#local) #000\""))
        );
        assert!(
            html.as_ref()
                .is_some_and(|html| html.contains("marker-mid=\"url(#mid)\""))
        );
        assert!(
            html.as_ref()
                .is_some_and(|html| html.contains("href=\"#local\""))
        );
    }

    #[test]
    fn svg_colors_and_same_document_fragments_are_not_resources() {
        let document = Document::parse(
            br##"<html><head></head><body>
            <svg xmlns="http://www.w3.org/2000/svg"
                xmlns:xlink="http://www.w3.org/1999/xlink">
              <defs>
                <linearGradient id="paint" href="#base"/>
                <pattern id="pattern" xlink:href="#tile"/>
              </defs>
              <path fill="#abcdef" stroke="rgb(1 2 3)"
                filter="none" clip-path="url(#clip)" mask="url(#mask)"
                marker="url(#marker)" marker-start="none"
                marker-mid="url(#mid)" marker-end="url(#end)"
                cursor="url(#cursor), auto"/>
              <use href="#symbol"/>
              <image xlink:href="#image"/>
            </svg>
            </body></html>"##,
        );
        let base = Url::parse("https://example.test/assets/page.svg").ok();
        let discovered = base
            .as_ref()
            .map(|base| discover_document_resources(&document, base));

        assert_eq!(
            discovered
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .map(|resources| resources.resources().len()),
            Some(0)
        );
    }

    #[test]
    fn html_fragment_urls_that_trigger_requests_are_resources() {
        let document = Document::parse(
            br##"<html><head><link rel="stylesheet" href="#theme"></head>
            <body><img src="#image"><svg><use href="#symbol"/></svg></body></html>"##,
        );
        let base = Url::parse("https://example.test/page.html").ok();
        let discovered = base
            .as_ref()
            .map(|base| discover_document_resources(&document, base));

        assert_eq!(
            discovered
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .map(|resources| resources.resources().len()),
            Some(2)
        );
    }

    #[test]
    fn bounded_document_discovery_stops_on_the_first_excess_reference() {
        let document = Document::parse(
            br#"<html><head></head><body><img src="one.png"><img src="two.png"></body></html>"#,
        );
        let base = Url::parse("https://example.com/").ok();
        let discovered = base
            .as_ref()
            .map(|base| discover_document_resources_bounded(&document, base, 1));

        assert_eq!(
            discovered
                .and_then(std::result::Result::err)
                .map(|error| error.code.as_str().to_owned()),
            Some("offprint.resource.limit".to_owned())
        );
    }
}
