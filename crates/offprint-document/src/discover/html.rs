use std::ops::Range;

use html5ever::Attribute;
use markup5ever::ns;
use offprint_model::Result;

use crate::{LinkRelPolicy, RenderingRole, SafeStaticPolicy};

use super::{Reference, resource_error};

pub(super) fn direct_resource_role(
    element: &str,
    attributes: &[Attribute],
    attribute: &Attribute,
) -> Option<RenderingRole> {
    if attribute.name.ns != ns!() {
        return None;
    }
    let name = attribute.name.local.as_ref();
    match (element, name) {
        ("img" | "input", "src") => Some(RenderingRole::Image),
        ("video", "poster") => Some(RenderingRole::Image),
        ("video" | "audio" | "source" | "track", "src") => Some(RenderingRole::Media),
        ("link", "href") => match SafeStaticPolicy::link_rel(attributes) {
            LinkRelPolicy::Stylesheet => Some(RenderingRole::Stylesheet),
            LinkRelPolicy::Icon => Some(RenderingRole::Image),
            LinkRelPolicy::Other | LinkRelPolicy::RequestTrigger => None,
        },
        ("iframe" | "frame", "src") => Some(RenderingRole::Frame),
        ("object", "data") | ("embed", "src") => Some(RenderingRole::Other),
        (_, "background") => Some(RenderingRole::Image),
        _ => None,
    }
}

pub(super) fn srcset_references_bounded(srcset: &str, maximum: usize) -> Result<Vec<Reference>> {
    let bytes = srcset.as_bytes();
    let mut references = Vec::new();
    let mut position = 0;
    while position < bytes.len() {
        while position < bytes.len()
            && (bytes[position].is_ascii_whitespace() || bytes[position] == b',')
        {
            position += 1;
        }
        if position == bytes.len() {
            break;
        }
        let start = position;
        while position < bytes.len() && !bytes[position].is_ascii_whitespace() {
            position += 1;
        }
        let mut end = position;
        while end > start && bytes[end - 1] == b',' {
            end -= 1;
        }
        if end > start {
            let value = &srcset[start..end];
            if references.len() >= maximum {
                return Err(resource_error(
                    "offprint.resource.limit",
                    "document resource count exceeds the configured limit",
                ));
            }
            references.push(Reference {
                value: value.to_owned(),
                range: Range { start, end },
            });
        }
        let mut parentheses = 0_u32;
        while position < bytes.len() {
            match bytes[position] {
                b'(' => parentheses = parentheses.saturating_add(1),
                b')' => parentheses = parentheses.saturating_sub(1),
                b',' if parentheses == 0 => {
                    position += 1;
                    break;
                }
                _ => {}
            }
            position += 1;
        }
    }
    Ok(references)
}

#[cfg(test)]
mod tests {
    use super::srcset_references_bounded;

    #[test]
    fn data_url_comma_stays_inside_one_srcset_candidate() {
        let references =
            srcset_references_bounded("data:image/svg+xml,a,b 1x, b.png 2x", usize::MAX)
                .unwrap_or_default();

        assert_eq!(references.len(), 2);
        assert_eq!(references[0].value, "data:image/svg+xml,a,b");
        assert_eq!(references[1].value, "b.png");
    }

    #[test]
    fn bounded_srcset_discovery_stops_on_the_first_excess_candidate() {
        let error = srcset_references_bounded("one.png 1x, two.png 2x", 1).err();

        assert_eq!(
            error.map(|error| error.code.as_str().to_owned()),
            Some("offprint.resource.limit".to_owned())
        );
    }
}
