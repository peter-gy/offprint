use std::collections::BTreeMap;

use data_url::DataUrl;
use pageknot_document::{Document, discover_css_resources, discover_document_resources};
use pageknot_model::{ContentDigest, Result};
use url::Url;

use super::active_content::validate_active_content;
use super::verification_error;

pub(super) fn validate_embedded_resource(
    url: &Url,
    base_url: &Url,
    resource_references: &mut u32,
    embedded_resources: &mut BTreeMap<(ContentDigest, String, u64), u32>,
    nested_depth: u16,
) -> Result<()> {
    if url.scheme() != "data" {
        return Err(verification_error(
            "pageknot.verification.external_reference",
            "artifact contains a render-fetch reference that is not embedded",
        ));
    }
    let mut data_url = url.clone();
    data_url.set_fragment(None);
    let parsed = DataUrl::process(data_url.as_str()).map_err(|error| {
        verification_error(
            "pageknot.verification.embedded_resource",
            format!("artifact contains a malformed data resource: {error}"),
        )
    })?;
    let media_type = parsed.mime_type().to_string();
    let (bytes, _) = parsed.decode_to_vec().map_err(|error| {
        verification_error(
            "pageknot.verification.embedded_resource",
            format!("artifact contains a malformed data resource body: {error}"),
        )
    })?;
    let bytes_length = u64::try_from(bytes.len()).map_err(|error| {
        verification_error(
            "pageknot.verification.embedded_resource",
            format!("embedded resource byte count exceeds the supported range: {error}"),
        )
    })?;
    let key = (
        ContentDigest::sha256(&bytes),
        normalize_media_type(&media_type),
        bytes_length,
    );
    *embedded_resources.entry(key).or_default() += 1;
    let media_type = normalize_media_type(&media_type);
    if media_type != "text/css" && media_type != "image/svg+xml" {
        return Ok(());
    }
    if nested_depth >= 64 {
        return Err(verification_error(
            "pageknot.verification.embedded_resource",
            "embedded resource nesting exceeds the verification limit",
        ));
    }
    let nested_urls = if media_type == "text/css" {
        let css = String::from_utf8(bytes).map_err(|error| {
            verification_error(
                "pageknot.verification.embedded_resource",
                format!("embedded stylesheet is not UTF-8: {error}"),
            )
        })?;
        discover_css_resources(&css, base_url)
            .map_err(|error| {
                verification_error(
                    "pageknot.verification.reference",
                    "embedded stylesheet contains an invalid resource reference",
                )
                .with_source(error)
            })?
            .resources()
            .iter()
            .map(|resource| resource.resolved_url.clone())
            .collect::<Vec<_>>()
    } else {
        let document = Document::parse(&bytes);
        if document.find_svg_root().is_none() {
            return Err(verification_error(
                "pageknot.verification.embedded_resource",
                "embedded SVG has no root SVG element",
            ));
        }
        validate_active_content(&document)?;
        discover_document_resources(&document, base_url)
            .map_err(|error| {
                verification_error(
                    "pageknot.verification.reference",
                    "embedded SVG contains an invalid resource reference",
                )
                .with_source(error)
            })?
            .resources()
            .iter()
            .map(|resource| resource.resolved_url.clone())
            .collect::<Vec<_>>()
    };
    for nested_url in nested_urls {
        *resource_references = resource_references.checked_add(1).ok_or_else(|| {
            verification_error(
                "pageknot.verification.resource_summary",
                "artifact resource reference count exceeds the supported range",
            )
        })?;
        validate_embedded_resource(
            &nested_url,
            base_url,
            resource_references,
            embedded_resources,
            nested_depth + 1,
        )?;
    }
    Ok(())
}

pub(super) fn normalize_media_type(media_type: &str) -> String {
    media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}
