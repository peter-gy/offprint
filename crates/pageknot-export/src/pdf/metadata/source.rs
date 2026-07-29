use html5ever::{Attribute, ns};
use pageknot_document::{Document as HtmlDocument, NodeData};
use pageknot_model::{ArtifactManifest, NodeId};

pub(super) const MAXIMUM_FIELD_CHARACTERS: usize = 16 * 1024;
pub(super) const MAXIMUM_LIST_ITEMS: usize = 128;
pub(super) const MAXIMUM_LIST_ITEM_CHARACTERS: usize = 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SourceMetadata {
    pub(super) title: String,
    pub(super) authors: Vec<String>,
    pub(super) description: Option<String>,
    pub(super) keywords: Vec<String>,
    pub(super) language: String,
    pub(super) rights: Option<String>,
    pub(super) site_name: Option<String>,
    pub(super) source_generator: Option<String>,
    pub(super) published_at: Option<String>,
    pub(super) modified_at: Option<String>,
}

impl SourceMetadata {
    pub(super) fn from_html(html: &[u8], manifest: &ArtifactManifest) -> Self {
        let document = HtmlDocument::parse(html);
        let mut head_title = None;
        let mut first_title = None;
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
            if name.ns != ns!(html) {
                continue;
            }
            match name.local.as_ref() {
                "html" => {
                    language = language.or_else(|| {
                        attribute(attrs, "lang")
                            .filter(|value| language_tag_is_valid(value.trim()))
                            .map(|value| value.trim().to_owned())
                    });
                }
                "title" => {
                    let title = normalized_optional(
                        &descendant_text(&document, id),
                        MAXIMUM_FIELD_CHARACTERS,
                    );
                    first_title = first_title.or_else(|| title.clone());
                    if head_title.is_none() && has_html_head_ancestor(&document, id) {
                        head_title = title;
                    }
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
            title: head_title
                .or(first_title)
                .or(social_title)
                .unwrap_or(fallback_title),
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

fn has_html_head_ancestor(document: &HtmlDocument, id: NodeId) -> bool {
    let mut parent = document.node(id).and_then(|node| node.parent);
    while let Some(id) = parent {
        let Some(node) = document.node(id) else {
            return false;
        };
        if matches!(
            &node.data,
            NodeData::Element { name, .. }
                if name.ns == ns!(html) && name.local.as_ref() == "head"
        ) {
            return true;
        }
        parent = node.parent;
    }
    false
}

fn attribute<'a>(attributes: &'a [Attribute], local_name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|attribute| attribute.name.ns == ns!() && attribute.name.local.as_ref() == local_name)
        .map(|attribute| attribute.value.as_ref())
}

fn descendant_text(document: &HtmlDocument, root: NodeId) -> String {
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

pub(super) fn language_tag_is_valid(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty()
        && value.len() <= 64
        && value.split('-').all(|part| {
            !part.is_empty()
                && part.len() <= 8
                && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}
