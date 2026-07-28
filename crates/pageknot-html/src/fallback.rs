use base64::Engine as _;
use pageknot_document::RenderingRole;

#[must_use]
pub fn empty_resource_data_url(role: RenderingRole) -> String {
    match role {
        RenderingRole::Image | RenderingRole::Svg => data_url(
            "image/svg+xml",
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#,
        ),
        RenderingRole::Stylesheet => "data:text/css;base64,".to_owned(),
        RenderingRole::Frame => data_url("text/html", b"<!doctype html><meta charset=utf-8>"),
        RenderingRole::Font
        | RenderingRole::Media
        | RenderingRole::Cursor
        | RenderingRole::Other => "data:application/octet-stream;base64,".to_owned(),
    }
}

pub(crate) fn is_empty_resource_fallback(
    digest: pageknot_model::ContentDigest,
    expected_media_type: &str,
    bytes: u64,
) -> bool {
    [
        RenderingRole::Image,
        RenderingRole::Stylesheet,
        RenderingRole::Frame,
        RenderingRole::Other,
    ]
    .into_iter()
    .any(|role| {
        let url = empty_resource_data_url(role);
        let Some((media_type_and_encoding, encoded)) = url
            .strip_prefix("data:")
            .and_then(|value| value.split_once(','))
        else {
            return false;
        };
        let Some(candidate_media_type) = media_type_and_encoding.strip_suffix(";base64") else {
            return false;
        };
        let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
            return false;
        };
        digest == pageknot_model::ContentDigest::sha256(&decoded)
            && normalize_media_type(candidate_media_type)
                == normalize_media_type(expected_media_type)
            && u64::try_from(decoded.len()).ok() == Some(bytes)
    })
}

fn normalize_media_type(media_type: &str) -> String {
    media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

fn data_url(media_type: &str, bytes: &[u8]) -> String {
    format!(
        "data:{media_type};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}
