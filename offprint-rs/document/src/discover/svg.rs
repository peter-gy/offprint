use html5ever::Attribute;
use markup5ever::ns;

use crate::RenderingRole;

pub(super) fn presentation_resource_role(attribute: &str) -> Option<RenderingRole> {
    match attribute {
        "clip-path" | "fill" | "filter" | "marker" | "marker-end" | "marker-mid"
        | "marker-start" | "mask" | "stroke" => Some(RenderingRole::Svg),
        "cursor" => Some(RenderingRole::Cursor),
        _ => None,
    }
}

pub(super) fn direct_resource_role(element: &str, attribute: &Attribute) -> Option<RenderingRole> {
    if attribute.name.local.as_ref() != "href" || !matches!(attribute.name.ns, ns!() | ns!(xlink)) {
        return None;
    }
    match element {
        "feImage" | "image" => Some(RenderingRole::Image),
        "cursor" => Some(RenderingRole::Cursor),
        "altGlyph" | "color-profile" | "filter" | "font-face-uri" | "glyphRef"
        | "linearGradient" | "mpath" | "pattern" | "radialGradient" | "textPath" | "tref"
        | "use" => Some(RenderingRole::Svg),
        _ => None,
    }
}
