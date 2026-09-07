use std::io::{self, Write};

use html5ever::{Attribute, QualName};
use markup5ever::{local_name, ns};
use offprint_model::NodeId;

use crate::{Document, NodeData};

pub fn serialize_document(document: &Document) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    serialize_document_to(document, &mut output)?;
    Ok(output)
}

/// Serializes the document into `output` in deterministic tree order.
///
/// Writer errors stop traversal at the node or text segment being emitted.
pub fn serialize_document_to(
    document: &Document,
    output: &mut (impl Write + ?Sized),
) -> io::Result<()> {
    serialize_children(document, document.root(), None, output)
}

pub fn serialize_subtree(document: &Document, root: NodeId) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let parent_name = document
        .node(root)
        .and_then(|node| node.parent)
        .and_then(|parent| document.node(parent))
        .and_then(|parent| match &parent.data {
            NodeData::Element { name, .. } => Some(name),
            _ => None,
        });
    serialize_node(document, root, parent_name, &mut output)?;
    Ok(output)
}

fn serialize_children(
    document: &Document,
    parent: NodeId,
    parent_name: Option<&QualName>,
    output: &mut (impl Write + ?Sized),
) -> io::Result<()> {
    let children = document
        .node(parent)
        .map(|node| node.children.clone())
        .unwrap_or_default();
    for child in children {
        serialize_node(document, child, parent_name, output)?;
    }
    Ok(())
}

fn serialize_node(
    document: &Document,
    id: NodeId,
    parent_name: Option<&QualName>,
    output: &mut (impl Write + ?Sized),
) -> io::Result<()> {
    let Some(node) = document.node(id) else {
        return Ok(());
    };
    match &node.data {
        NodeData::Document => serialize_children(document, id, parent_name, output),
        NodeData::Doctype {
            name,
            public_id,
            system_id,
        } => write_doctype(output, name, public_id, system_id),
        NodeData::Text { contents } => write_text(output, contents, parent_name),
        NodeData::Comment { contents } => {
            output.write_all(b"<!--")?;
            output.write_all(contents.as_bytes())?;
            output.write_all(b"-->")
        }
        NodeData::Element {
            name,
            attrs,
            template_contents,
            ..
        } => {
            output.write_all(b"<")?;
            output.write_all(name.local.as_bytes())?;
            for attribute in attrs {
                write_attribute(output, attribute)?;
            }
            output.write_all(b">")?;

            if !is_void_element(name) {
                if name.ns == ns!(html)
                    && name.local == local_name!("template")
                    && let Some(template_contents) = template_contents
                {
                    serialize_children(document, *template_contents, Some(name), output)?;
                } else {
                    serialize_children(document, id, Some(name), output)?;
                }
                output.write_all(b"</")?;
                output.write_all(name.local.as_bytes())?;
                output.write_all(b">")?;
            }
            Ok(())
        }
        NodeData::ProcessingInstruction { target, contents } => {
            output.write_all(b"<?")?;
            output.write_all(target.as_bytes())?;
            output.write_all(b" ")?;
            output.write_all(contents.as_bytes())?;
            output.write_all(b">")
        }
    }
}

fn write_doctype(
    output: &mut (impl Write + ?Sized),
    name: &str,
    public_id: &str,
    system_id: &str,
) -> io::Result<()> {
    output.write_all(b"<!DOCTYPE ")?;
    output.write_all(name.as_bytes())?;
    if !public_id.is_empty() {
        output.write_all(b" PUBLIC \"")?;
        write_escaped_attribute(output, public_id)?;
        output.write_all(b"\"")?;
        if !system_id.is_empty() {
            output.write_all(b" \"")?;
            write_escaped_attribute(output, system_id)?;
            output.write_all(b"\"")?;
        }
    } else if !system_id.is_empty() {
        output.write_all(b" SYSTEM \"")?;
        write_escaped_attribute(output, system_id)?;
        output.write_all(b"\"")?;
    }
    output.write_all(b">")
}

fn write_attribute(output: &mut (impl Write + ?Sized), attribute: &Attribute) -> io::Result<()> {
    output.write_all(b" ")?;
    match attribute.name.ns {
        ns!() => {}
        ns!(xml) => output.write_all(b"xml:")?,
        ns!(xmlns) if attribute.name.local != local_name!("xmlns") => {
            output.write_all(b"xmlns:")?;
        }
        ns!(xmlns) => {}
        ns!(xlink) => output.write_all(b"xlink:")?,
        _ => output.write_all(b"unknown_namespace:")?,
    }
    output.write_all(attribute.name.local.as_bytes())?;
    output.write_all(b"=\"")?;
    write_escaped_attribute(output, &attribute.value)?;
    output.write_all(b"\"")
}

fn write_escaped_attribute(output: &mut (impl Write + ?Sized), value: &str) -> io::Result<()> {
    write_escaped(output, value, |character| match character {
        '&' => Some("&amp;"),
        '"' => Some("&quot;"),
        '\u{00a0}' => Some("&nbsp;"),
        '<' => Some("&lt;"),
        '>' => Some("&gt;"),
        _ => None,
    })
}

fn write_text(
    output: &mut (impl Write + ?Sized),
    value: &str,
    parent_name: Option<&QualName>,
) -> io::Result<()> {
    let raw = parent_name.is_some_and(|name| {
        name.ns == ns!(html)
            && matches!(
                name.local,
                local_name!("style")
                    | local_name!("script")
                    | local_name!("xmp")
                    | local_name!("iframe")
                    | local_name!("noembed")
                    | local_name!("noframes")
                    | local_name!("plaintext")
            )
    });
    if raw {
        output.write_all(value.as_bytes())
    } else {
        write_escaped(output, value, |character| match character {
            '&' => Some("&amp;"),
            '\u{00a0}' => Some("&nbsp;"),
            '<' => Some("&lt;"),
            '>' => Some("&gt;"),
            _ => None,
        })
    }
}

fn write_escaped(
    output: &mut (impl Write + ?Sized),
    value: &str,
    replacement: impl Fn(char) -> Option<&'static str>,
) -> io::Result<()> {
    let mut unescaped_start = 0;
    let bytes = value.as_bytes();
    for (index, character) in value.char_indices() {
        let Some(replacement) = replacement(character) else {
            continue;
        };
        output.write_all(&bytes[unescaped_start..index])?;
        output.write_all(replacement.as_bytes())?;
        unescaped_start = index + character.len_utf8();
    }
    output.write_all(&bytes[unescaped_start..])
}

fn is_void_element(name: &QualName) -> bool {
    name.ns == ns!(html)
        && matches!(
            name.local,
            local_name!("area")
                | local_name!("base")
                | local_name!("basefont")
                | local_name!("bgsound")
                | local_name!("br")
                | local_name!("col")
                | local_name!("embed")
                | local_name!("frame")
                | local_name!("hr")
                | local_name!("img")
                | local_name!("input")
                | local_name!("keygen")
                | local_name!("link")
                | local_name!("meta")
                | local_name!("param")
                | local_name!("source")
                | local_name!("track")
                | local_name!("wbr")
        )
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use markup5ever::ns;
    use proptest::prelude::*;

    use crate::{Document, NodeData};

    use super::{serialize_document, serialize_document_to, serialize_subtree};

    #[test]
    fn serialization_escapes_text_and_attributes_without_changing_unicode()
    -> Result<(), Box<dyn std::error::Error>> {
        let document = Document::parse(
            "<html><body><p title='A &amp; &quot; &lt; &gt;\u{00a0}café'>A &amp; &lt; &gt;\u{00a0}café</p></body></html>"
                .as_bytes(),
        );
        let serialized = String::from_utf8(serialize_document(&document)?)?;

        assert!(serialized.contains("title=\"A &amp; &quot; &lt; &gt;&nbsp;café\""));
        assert!(serialized.contains(">A &amp; &lt; &gt;&nbsp;café</p>"));
        Ok(())
    }

    #[test]
    fn subtree_serialization_keeps_an_svg_root() {
        let document = Document::parse(
            br#"<svg xmlns="http://www.w3.org/2000/svg"><image href="pixel.png"/></svg>"#,
        );
        let svg = document.walk().find(|id| {
            matches!(
                document.node(*id).map(|node| &node.data),
                Some(NodeData::Element { name, .. })
                    if name.ns == ns!(svg) && name.local.as_ref() == "svg"
            )
        });
        let serialized = svg.and_then(|svg| serialize_subtree(&document, svg).ok());

        assert!(serialized.as_ref().is_some_and(|bytes| {
            let svg = String::from_utf8_lossy(bytes);
            svg.starts_with("<svg") && svg.contains("<image href=\"pixel.png\"></image>")
        }));
    }

    #[test]
    fn streaming_serialization_stops_before_crossing_the_writer_limit() {
        #[derive(Debug)]
        struct LimitedWriter {
            bytes: usize,
            limit: usize,
        }

        impl Write for LimitedWriter {
            fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
                if self.bytes.saturating_add(buffer.len()) > self.limit {
                    return Err(io::Error::new(
                        io::ErrorKind::FileTooLarge,
                        "test output limit reached",
                    ));
                }
                self.bytes += buffer.len();
                Ok(buffer.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let document = Document::parse(
            b"<html><head></head><body><p>bounded serialization output</p></body></html>",
        );
        let mut writer = LimitedWriter {
            bytes: 0,
            limit: 32,
        };

        let result = serialize_document_to(&document, &mut writer);

        assert_eq!(
            result.as_ref().map_err(std::io::Error::kind),
            Err(std::io::ErrorKind::FileTooLarge)
        );
        assert!(writer.bytes <= writer.limit);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn parsed_documents_reach_a_stable_serialized_form(
            input in proptest::collection::vec(any::<char>(), 0..256)
                .prop_map(|characters| characters.into_iter().collect::<String>()),
        ) {
            let document = Document::parse(input.as_bytes());
            let first = serialize_document(&document)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let reparsed = Document::parse(&first);
            let second = serialize_document(&reparsed)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;

            prop_assert_eq!(first, second);
        }
    }
}
