use std::collections::{BTreeMap, BTreeSet};

use data_url::DataUrl;
use pageknot_document::{Document, NodeData};
use pageknot_model::{
    ArtifactManifest, ArtifactVariantKind, ContentDigest, MarkdownOptions, Result,
};
use pulldown_cmark::{Event as MarkdownEvent, Parser as MarkdownParser, Tag as MarkdownTag};

use crate::support::{ensure_verified, export_error};
use crate::{MarkdownBundle, VariantEvidence};

/// Converts a verified HTML artifact into Markdown and relative assets.
pub fn encode_markdown(
    html: &[u8],
    manifest: &ArtifactManifest,
    options: MarkdownOptions,
) -> Result<MarkdownBundle> {
    let document = Document::parse(html);
    let mut encoder = MarkdownEncoder {
        document: &document,
        assets: BTreeMap::new(),
    };
    let mut markdown = String::new();
    if options.front_matter {
        markdown.push_str("---\n");
        markdown.push_str("source: \"");
        markdown.push_str(&yaml_string(manifest.source.final_url.as_str()));
        markdown.push_str("\"\n");
        markdown.push_str("captured_at: \"");
        markdown.push_str(&manifest.captured_at.to_rfc3339());
        markdown.push_str("\"\n");
        markdown.push_str("policy_sha256: \"");
        markdown.push_str(&manifest.policy_sha256.to_hex());
        markdown.push_str("\"\n---\n\n");
    }
    let body = document
        .find_html_element("body")
        .unwrap_or_else(|| document.root());
    encoder.render_children(body, &mut markdown, RenderMode::Block)?;
    normalize_markdown_end(&mut markdown);
    let bundle = MarkdownBundle {
        markdown: markdown.into_bytes(),
        assets: encoder.assets,
    };
    verify_markdown(&bundle)?;
    Ok(bundle)
}

/// Verifies Markdown syntax, navigation links, and content-addressed image assets.
pub fn verify_markdown(bundle: &MarkdownBundle) -> Result<VariantEvidence> {
    let markdown = std::str::from_utf8(&bundle.markdown).map_err(|error| {
        export_error(
            "pageknot.export.verify",
            format!("Markdown output is not valid UTF-8: {error}"),
        )
    })?;
    let mut image_references = BTreeSet::new();
    let mut contains_raw_html = false;
    let mut destinations_valid = true;
    for event in MarkdownParser::new(markdown) {
        match event {
            MarkdownEvent::Html(_) | MarkdownEvent::InlineHtml(_) => contains_raw_html = true,
            MarkdownEvent::Start(MarkdownTag::Image { dest_url, .. }) => {
                let destination = dest_url.as_ref();
                destinations_valid &= image_destination_is_valid(destination, &bundle.assets);
                if bundle.assets.contains_key(destination) {
                    image_references.insert(destination.to_owned());
                }
            }
            MarkdownEvent::Start(MarkdownTag::Link { dest_url, .. }) => {
                destinations_valid &= navigation_destination_is_safe(dest_url.as_ref());
            }
            _ => {}
        }
    }
    let structure_valid = !contains_raw_html
        && destinations_valid
        && bundle
            .assets
            .iter()
            .all(|(path, bytes)| markdown_asset_is_valid(path, bytes));
    let content_valid = !markdown.trim().is_empty()
        && image_references
            .iter()
            .all(|reference| bundle.assets.contains_key(reference))
        && bundle
            .assets
            .keys()
            .all(|asset| image_references.contains(asset));
    ensure_verified(
        ArtifactVariantKind::Markdown,
        structure_valid,
        content_valid,
    )
}

struct MarkdownEncoder<'a> {
    document: &'a Document,
    assets: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Copy)]
enum RenderMode {
    Block,
    Inline,
    Preformatted,
}

impl MarkdownEncoder<'_> {
    fn render_children(
        &mut self,
        id: pageknot_model::NodeId,
        output: &mut String,
        mode: RenderMode,
    ) -> Result<()> {
        let children = self
            .document
            .node(id)
            .map(|node| node.children.clone())
            .unwrap_or_default();
        for child in children {
            self.render_node(child, output, mode)?;
        }
        Ok(())
    }

    fn render_node(
        &mut self,
        id: pageknot_model::NodeId,
        output: &mut String,
        mode: RenderMode,
    ) -> Result<()> {
        let Some(node) = self.document.node(id) else {
            return Ok(());
        };
        match &node.data {
            NodeData::Text { contents } => {
                if matches!(mode, RenderMode::Preformatted) {
                    output.push_str(contents);
                } else {
                    push_collapsed_text(output, contents);
                }
            }
            NodeData::Element { name, attrs, .. } => {
                let tag = name.local.as_ref();
                match tag {
                    "head" | "script" | "style" | "template" | "meta" | "link" => {}
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        ensure_blank_line(output);
                        let level = tag
                            .as_bytes()
                            .get(1)
                            .copied()
                            .unwrap_or(b'1')
                            .saturating_sub(b'0');
                        output.push_str(&"#".repeat(usize::from(level)));
                        output.push(' ');
                        self.render_children(id, output, RenderMode::Inline)?;
                        ensure_blank_line(output);
                    }
                    "p" | "article" | "section" | "main" | "header" | "footer" | "nav"
                    | "aside" | "div" => {
                        if matches!(mode, RenderMode::Block) {
                            ensure_blank_line(output);
                        }
                        self.render_children(id, output, mode)?;
                        if matches!(mode, RenderMode::Block) {
                            ensure_blank_line(output);
                        }
                    }
                    "br" => output.push_str("  \n"),
                    "hr" => {
                        ensure_blank_line(output);
                        output.push_str("---");
                        ensure_blank_line(output);
                    }
                    "strong" | "b" => {
                        output.push_str("**");
                        self.render_children(id, output, RenderMode::Inline)?;
                        output.push_str("**");
                    }
                    "em" | "i" => {
                        output.push('*');
                        self.render_children(id, output, RenderMode::Inline)?;
                        output.push('*');
                    }
                    "del" | "s" => {
                        output.push_str("~~");
                        self.render_children(id, output, RenderMode::Inline)?;
                        output.push_str("~~");
                    }
                    "code" if !matches!(mode, RenderMode::Preformatted) => {
                        let mut code = String::new();
                        self.render_children(id, &mut code, RenderMode::Preformatted)?;
                        push_inline_code(output, &code);
                    }
                    "pre" => {
                        let mut code = String::new();
                        self.render_children(id, &mut code, RenderMode::Preformatted)?;
                        ensure_blank_line(output);
                        push_fenced_code(output, &code);
                        ensure_blank_line(output);
                    }
                    "a" => {
                        let href = attribute(attrs, "href").unwrap_or_default();
                        output.push('[');
                        self.render_children(id, output, RenderMode::Inline)?;
                        output.push_str("](");
                        output.push_str(&markdown_destination(href));
                        output.push(')');
                    }
                    "img" => self.render_image(attrs, output)?,
                    "blockquote" => {
                        let mut quote = String::new();
                        self.render_children(id, &mut quote, RenderMode::Block)?;
                        ensure_blank_line(output);
                        for line in quote.trim().lines() {
                            output.push_str("> ");
                            output.push_str(line);
                            output.push('\n');
                        }
                        ensure_blank_line(output);
                    }
                    "ul" => self.render_list(id, output, false)?,
                    "ol" => self.render_list(id, output, true)?,
                    "table" => self.render_table(id, output)?,
                    "iframe" | "frame" => {
                        if let Some(srcdoc) = attribute(attrs, "srcdoc") {
                            let frame = Document::parse(srcdoc.as_bytes());
                            let body = frame
                                .find_html_element("body")
                                .unwrap_or_else(|| frame.root());
                            let mut nested = MarkdownEncoder {
                                document: &frame,
                                assets: std::mem::take(&mut self.assets),
                            };
                            ensure_blank_line(output);
                            nested.render_children(body, output, RenderMode::Block)?;
                            self.assets = nested.assets;
                            ensure_blank_line(output);
                        }
                    }
                    _ => self.render_children(id, output, mode)?,
                }
            }
            NodeData::Document => self.render_children(id, output, mode)?,
            NodeData::Doctype { .. }
            | NodeData::Comment { .. }
            | NodeData::ProcessingInstruction { .. } => {}
        }
        Ok(())
    }

    fn render_image(&mut self, attrs: &[html5ever::Attribute], output: &mut String) -> Result<()> {
        let alt = markdown_text(attribute(attrs, "alt").unwrap_or_default());
        let source = attribute(attrs, "src").unwrap_or_default();
        let destination = if source.starts_with("data:") {
            let parsed = DataUrl::process(source).map_err(|error| {
                export_error(
                    "pageknot.export.markdown_asset",
                    format!("embedded Markdown image is malformed: {error}"),
                )
            })?;
            let media_type = parsed.mime_type().to_string();
            let (bytes, _) = parsed.decode_to_vec().map_err(|error| {
                export_error(
                    "pageknot.export.markdown_asset",
                    format!("embedded Markdown image body is malformed: {error}"),
                )
            })?;
            let digest = ContentDigest::sha256(&bytes).to_hex();
            let extension = media_extension(&media_type);
            let path = format!("assets/{digest}.{extension}");
            self.assets.entry(path.clone()).or_insert(bytes);
            path
        } else {
            markdown_destination(source)
        };
        output.push_str("![");
        output.push_str(&alt);
        output.push_str("](");
        output.push_str(&destination);
        output.push(')');
        Ok(())
    }

    fn render_list(
        &mut self,
        id: pageknot_model::NodeId,
        output: &mut String,
        ordered: bool,
    ) -> Result<()> {
        ensure_blank_line(output);
        let items = self
            .document
            .node(id)
            .map(|node| node.children.clone())
            .unwrap_or_default();
        let mut index = 1_u64;
        for item in items {
            let is_item = self.document.node(item).is_some_and(|node| {
                matches!(
                    &node.data,
                    NodeData::Element { name, .. } if name.local.as_ref() == "li"
                )
            });
            if !is_item {
                continue;
            }
            if ordered {
                output.push_str(&format!("{index}. "));
            } else {
                output.push_str("- ");
            }
            self.render_children(item, output, RenderMode::Inline)?;
            output.push('\n');
            index = index.saturating_add(1);
        }
        ensure_blank_line(output);
        Ok(())
    }

    fn render_table(&mut self, id: pageknot_model::NodeId, output: &mut String) -> Result<()> {
        let rows = descendant_elements(self.document, id, "tr");
        if rows.is_empty() {
            return Ok(());
        }
        let mut rendered = Vec::new();
        for row in rows {
            let cells = self
                .document
                .node(row)
                .map(|node| node.children.clone())
                .unwrap_or_default()
                .into_iter()
                .filter(|cell| {
                    self.document.node(*cell).is_some_and(|node| {
                        matches!(
                            &node.data,
                            NodeData::Element { name, .. }
                                if matches!(name.local.as_ref(), "th" | "td")
                        )
                    })
                })
                .collect::<Vec<_>>();
            let mut values = Vec::new();
            for cell in cells {
                let mut value = String::new();
                self.render_children(cell, &mut value, RenderMode::Inline)?;
                values.push(value.trim().to_owned());
            }
            if !values.is_empty() {
                rendered.push(values);
            }
        }
        let Some(columns) = rendered.iter().map(Vec::len).max() else {
            return Ok(());
        };
        ensure_blank_line(output);
        for (row_index, row) in rendered.iter().enumerate() {
            output.push('|');
            for column in 0..columns {
                output.push(' ');
                output.push_str(row.get(column).map(String::as_str).unwrap_or_default());
                output.push_str(" |");
            }
            output.push('\n');
            if row_index == 0 {
                output.push('|');
                for _ in 0..columns {
                    output.push_str(" --- |");
                }
                output.push('\n');
            }
        }
        ensure_blank_line(output);
        Ok(())
    }
}

fn descendant_elements(
    document: &Document,
    root: pageknot_model::NodeId,
    name: &str,
) -> Vec<pageknot_model::NodeId> {
    let mut output = Vec::new();
    let mut pending = document
        .node(root)
        .map(|node| node.children.iter().rev().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    while let Some(id) = pending.pop() {
        let Some(node) = document.node(id) else {
            continue;
        };
        pending.extend(node.children.iter().rev().copied());
        if matches!(&node.data, NodeData::Element { name: element, .. } if element.local.as_ref() == name)
        {
            output.push(id);
        }
    }
    output
}

fn attribute<'a>(attrs: &'a [html5ever::Attribute], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|attribute| attribute.name.local.as_ref() == name)
        .map(|attribute| attribute.value.as_ref())
}

fn push_collapsed_text(output: &mut String, value: &str) {
    let leading = value.chars().next().is_some_and(char::is_whitespace);
    let trailing = value.chars().next_back().is_some_and(char::is_whitespace);
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        if leading && !output.ends_with(char::is_whitespace) {
            output.push(' ');
        }
        return;
    }
    if leading && !output.is_empty() && !output.ends_with(char::is_whitespace) {
        output.push(' ');
    }
    output.push_str(&markdown_text(&collapsed));
    if trailing && !output.ends_with(char::is_whitespace) {
        output.push(' ');
    }
}

fn markdown_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_punctuation() {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn markdown_destination(value: &str) -> String {
    let active_scheme = navigation_scheme(value)
        .is_some_and(|scheme| !matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel"));
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let safe = byte.is_ascii_alphanumeric()
            || matches!(
                *byte,
                b'-' | b'.'
                    | b'_'
                    | b'~'
                    | b'/'
                    | b'?'
                    | b'#'
                    | b'['
                    | b']'
                    | b'@'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b'%'
            )
            || (*byte == b':' && !active_scheme);
        if safe {
            encoded.push(char::from(*byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn push_inline_code(output: &mut String, value: &str) {
    let delimiter = "`".repeat(longest_run(value, '`').saturating_add(1));
    let needs_padding = value.starts_with(['`', ' ']) || value.ends_with(['`', ' ']);
    output.push_str(&delimiter);
    if needs_padding {
        output.push(' ');
    }
    output.push_str(value);
    if needs_padding {
        output.push(' ');
    }
    output.push_str(&delimiter);
}

fn push_fenced_code(output: &mut String, value: &str) {
    let fence = "`".repeat(longest_run(value, '`').saturating_add(1).max(3));
    output.push_str(&fence);
    output.push('\n');
    output.push_str(value);
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(&fence);
}

fn longest_run(value: &str, needle: char) -> usize {
    value
        .chars()
        .fold((0_usize, 0_usize), |(longest, current), character| {
            if character == needle {
                let current = current.saturating_add(1);
                (longest.max(current), current)
            } else {
                (longest, 0)
            }
        })
        .0
}

fn ensure_blank_line(output: &mut String) {
    while output.ends_with(' ') {
        output.pop();
    }
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    if !output.is_empty() && !output.ends_with("\n\n") {
        output.push('\n');
    }
}

fn normalize_markdown_end(output: &mut String) {
    let trimmed = output.trim_end().len();
    output.truncate(trimmed);
    output.push('\n');
}

fn media_extension(media_type: &str) -> &'static str {
    match media_type.split(';').next().unwrap_or_default() {
        "image/avif" => "avif",
        "image/gif" => "gif",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/svg+xml" => "svg",
        "image/webp" => "webp",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        "font/woff" => "woff",
        "font/woff2" => "woff2",
        _ => "bin",
    }
}

fn markdown_asset_is_valid(path: &str, bytes: &[u8]) -> bool {
    let Some(name) = path.strip_prefix("assets/") else {
        return false;
    };
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return false;
    }
    let Some((digest, extension)) = name.rsplit_once('.') else {
        return false;
    };
    !extension.is_empty()
        && extension.len() <= 8
        && extension
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && digest == ContentDigest::sha256(bytes).to_hex()
}

fn image_destination_is_valid(destination: &str, assets: &BTreeMap<String, Vec<u8>>) -> bool {
    !destination.is_empty()
        && !destination.starts_with('/')
        && !destination.starts_with('\\')
        && assets
            .get(destination)
            .is_some_and(|bytes| markdown_asset_is_valid(destination, bytes))
}

fn navigation_destination_is_safe(destination: &str) -> bool {
    if destination
        .chars()
        .any(|character| character.is_ascii_control() || character.is_ascii_whitespace())
        || destination.contains(['<', '>', '\\'])
    {
        return false;
    }
    navigation_scheme(destination)
        .is_none_or(|scheme| matches!(scheme.as_str(), "http" | "https" | "mailto" | "tel"))
}

fn navigation_scheme(destination: &str) -> Option<String> {
    let colon = destination.find(':')?;
    let prefix = &destination[..colon];
    if prefix.is_empty()
        || prefix.contains(['/', '?', '#'])
        || !prefix.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_alphabetic()
                || (index > 0 && (byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')))
        })
    {
        return None;
    }
    Some(prefix.to_ascii_lowercase())
}

fn yaml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pageknot_document::Document;
    use pageknot_model::{
        ArtifactManifest, ContentDigest, ErrorStage, MarkdownOptions, PageKnotError, Result,
    };
    use pulldown_cmark::{Event as MarkdownEvent, Parser as MarkdownParser};

    use super::{encode_markdown, verify_markdown};
    use crate::MarkdownBundle;

    fn fixture_from_html(source: &[u8]) -> Result<(Vec<u8>, ArtifactManifest)> {
        let manifest = serde_json::from_slice::<ArtifactManifest>(include_bytes!(
            "../../../schemas/examples/artifact-manifest.json"
        ))
        .map_err(|error| {
            PageKnotError::new(
                "pageknot.export.encode",
                ErrorStage::Encoding,
                error.to_string(),
            )
        })?;
        let document = Document::parse(source);
        let html = pageknot_html::encode_html(&document, &manifest)?;
        Ok((html, manifest))
    }

    fn test_error(message: &'static str) -> PageKnotError {
        PageKnotError::new("pageknot.export.test", ErrorStage::Encoding, message)
    }

    fn asset_bundle(markdown: String, bytes: Vec<u8>) -> MarkdownBundle {
        let path = format!("assets/{}.png", ContentDigest::sha256(&bytes));
        MarkdownBundle {
            markdown: markdown.replace("{asset}", &path).into_bytes(),
            assets: BTreeMap::from([(path, bytes)]),
        }
    }

    #[test]
    fn markdown_assets_are_bound_to_their_content_digest() -> Result<()> {
        let (html, manifest) = fixture_from_html(
            br#"<html><body><p>Asset</p>
            <img src="data:image/png;base64,AQID" alt="asset">
            </body></html>"#,
        )?;
        let bundle = encode_markdown(&html, &manifest, MarkdownOptions::default())?;
        let mut changed = bundle.clone();
        let Some(asset) = changed.assets.values_mut().next() else {
            return Err(test_error("Markdown fixture did not produce an asset"));
        };
        asset.push(4);

        assert!(verify_markdown(&bundle).is_ok());
        assert!(verify_markdown(&changed).is_err());
        Ok(())
    }

    #[test]
    fn markdown_escapes_raw_html_code_fences_and_destinations() -> Result<()> {
        let (html, manifest) = fixture_from_html(
            br#"<html><body>
            <p>&lt;script&gt;alert("text")&lt;/script&gt;</p>
            <p><code>`&lt;img src=x onerror=alert(1)&gt;</code></p>
            <pre>```&lt;script&gt;alert("fence")&lt;/script&gt;</pre>
            <a href="https://example.test/a)&#10;&lt;script&gt;">safe link</a>
            <a href="javascript:alert(1)">active link</a>
            </body></html>"#,
        )?;
        let bundle = encode_markdown(&html, &manifest, MarkdownOptions::default())?;
        let rendered = std::str::from_utf8(&bundle.markdown)
            .map_err(|_| test_error("Markdown fixture was not UTF-8"))?;
        let has_raw_html = MarkdownParser::new(rendered)
            .any(|event| matches!(event, MarkdownEvent::Html(_) | MarkdownEvent::InlineHtml(_)));

        assert!(!has_raw_html);
        assert!(rendered.contains("%29%0A%3Cscript%3E"));
        assert!(rendered.contains("javascript%3Aalert%281%29"));
        assert!(!rendered.contains("](javascript:"));
        assert!(verify_markdown(&bundle).is_ok());
        Ok(())
    }

    #[test]
    fn markdown_verifier_distinguishes_images_from_navigation_links() {
        let bytes = vec![1, 2, 3];
        let image = asset_bundle("![asset]({asset})\n".to_owned(), bytes.clone());
        let link = asset_bundle("[asset]({asset})\n".to_owned(), bytes);
        let navigation = MarkdownBundle {
            markdown: b"[web](https://example.test/)\n[mail](mailto:test@example.test)\n".to_vec(),
            assets: BTreeMap::new(),
        };

        assert!(verify_markdown(&image).is_ok());
        assert!(verify_markdown(&link).is_err());
        assert!(verify_markdown(&navigation).is_ok());
    }

    #[test]
    fn markdown_verifier_rejects_nonlocal_image_destinations() {
        let bytes = vec![1, 2, 3];
        let digest = ContentDigest::sha256(&bytes);
        let invalid = [
            format!("/assets/{digest}.png"),
            format!("//example.test/{digest}.png"),
            format!("http://example.test/{digest}.png"),
            format!("https://example.test/{digest}.png"),
            "data:image/png;base64,AQID".to_owned(),
            format!("file:///tmp/{digest}.png"),
            format!("assets/../{digest}.png"),
        ];

        for destination in invalid {
            let bundle = MarkdownBundle {
                markdown: format!("![asset]({destination})\n").into_bytes(),
                assets: BTreeMap::from([(destination.clone(), bytes.clone())]),
            };
            assert!(verify_markdown(&bundle).is_err(), "{destination}");
        }

        let missing = MarkdownBundle {
            markdown: format!("![asset](assets/{digest}.png)\n").into_bytes(),
            assets: BTreeMap::new(),
        };
        let empty = MarkdownBundle {
            markdown: b"![asset]()\n".to_vec(),
            assets: BTreeMap::new(),
        };
        assert!(verify_markdown(&missing).is_err());
        assert!(verify_markdown(&empty).is_err());
    }

    #[test]
    fn markdown_verifier_rejects_raw_html_and_active_navigation() {
        let raw_html = MarkdownBundle {
            markdown: b"<script>alert(1)</script>\n".to_vec(),
            assets: BTreeMap::new(),
        };
        let active_link = MarkdownBundle {
            markdown: b"[open](javascript:alert%281%29)\n".to_vec(),
            assets: BTreeMap::new(),
        };

        assert!(verify_markdown(&raw_html).is_err());
        assert!(verify_markdown(&active_link).is_err());
    }
}
