use std::collections::{BTreeMap, BTreeSet};

use base64::Engine as _;
use pageknot_model::{ArtifactManifest, ArtifactVariantKind, ContentDigest, Result};

use crate::VariantEvidence;
use crate::support::{
    MAXIMUM_DECODED_HTML_BYTES, MAXIMUM_DECODED_MANIFEST_BYTES, ensure_verified, export_error,
    export_serialize_error, manifest_encoding_matches,
};

/// Encodes the verified HTML and manifest as a deterministic MHTML message.
pub fn encode_mhtml(html: &[u8], manifest: &ArtifactManifest) -> Result<Vec<u8>> {
    let html = pageknot_html::encode_sandboxed_html(html)?;
    let digest = ContentDigest::sha256(&html);
    let boundary = format!("pageknot-{digest}");
    let html_body = mime_base64(&html);
    let manifest_bytes = serde_json::to_vec_pretty(manifest).map_err(export_serialize_error)?;
    let manifest_body = mime_base64(&manifest_bytes);
    let source = manifest.source.final_url.as_str();
    let message = format!(
        "From: <Saved by PageKnot>\r\n\
Subject: PageKnot capture\r\n\
MIME-Version: 1.0\r\n\
Snapshot-Content-Location: {source}\r\n\
Content-Type: multipart/related; type=\"text/html\"; boundary=\"{boundary}\"\r\n\
\r\n\
--{boundary}\r\n\
Content-Type: text/html; charset=\"utf-8\"\r\n\
Content-Transfer-Encoding: base64\r\n\
Content-Location: {source}\r\n\
\r\n\
{html_body}\r\n\
--{boundary}\r\n\
Content-Type: application/vnd.pageknot.manifest+json\r\n\
Content-Transfer-Encoding: base64\r\n\
Content-Location: pageknot-manifest.json\r\n\
\r\n\
{manifest_body}\r\n\
--{boundary}--\r\n"
    );
    let bytes = message.into_bytes();
    verify_mhtml(&bytes)?;
    Ok(bytes)
}

/// Verifies MHTML framing, exact parts, and the contained PageKnot manifest.
pub fn verify_mhtml(bytes: &[u8]) -> Result<VariantEvidence> {
    let source = std::str::from_utf8(bytes).map_err(|error| {
        export_error(
            "pageknot.export.verify",
            format!("MHTML output is not valid UTF-8: {error}"),
        )
    })?;
    let Some((header_source, body)) = source.split_once("\r\n\r\n") else {
        return ensure_verified(ArtifactVariantKind::Mhtml, false, false);
    };
    let Some(headers) = parse_mime_headers(header_source) else {
        return ensure_verified(ArtifactVariantKind::Mhtml, false, false);
    };
    let expected_header_names = BTreeSet::from([
        "content-type".to_owned(),
        "from".to_owned(),
        "mime-version".to_owned(),
        "snapshot-content-location".to_owned(),
        "subject".to_owned(),
    ]);
    let Some(content_type) = headers.get("content-type") else {
        return ensure_verified(ArtifactVariantKind::Mhtml, false, false);
    };
    let boundary_prefix = "multipart/related; type=\"text/html\"; boundary=\"";
    let Some(boundary) = content_type
        .strip_prefix(boundary_prefix)
        .and_then(|value| value.strip_suffix('"'))
    else {
        return ensure_verified(ArtifactVariantKind::Mhtml, false, false);
    };
    let Some(parts) = parse_multipart_parts(body, boundary) else {
        return ensure_verified(ArtifactVariantKind::Mhtml, false, false);
    };
    if headers.keys().cloned().collect::<BTreeSet<_>>() != expected_header_names
        || headers.get("from").map(String::as_str) != Some("<Saved by PageKnot>")
        || headers.get("subject").map(String::as_str) != Some("PageKnot capture")
        || headers.get("mime-version").map(String::as_str) != Some("1.0")
        || parts.len() != 2
    {
        return ensure_verified(ArtifactVariantKind::Mhtml, false, false);
    }
    let Some(html) = decode_mhtml_part(
        &parts[0],
        "text/html; charset=\"utf-8\"",
        MAXIMUM_DECODED_HTML_BYTES,
    )?
    else {
        return ensure_verified(ArtifactVariantKind::Mhtml, false, false);
    };
    let Some(manifest_bytes) = decode_mhtml_part(
        &parts[1],
        "application/vnd.pageknot.manifest+json",
        MAXIMUM_DECODED_MANIFEST_BYTES,
    )?
    else {
        return ensure_verified(ArtifactVariantKind::Mhtml, false, false);
    };
    let embedded_manifest = pageknot_html::inspect_html(&html).ok();
    let sidecar_manifest = serde_json::from_slice::<ArtifactManifest>(&manifest_bytes).ok();
    let manifest_matches = embedded_manifest
        .as_ref()
        .zip(sidecar_manifest.as_ref())
        .is_some_and(|(embedded, sidecar)| embedded == sidecar);
    let expected_boundary = format!("pageknot-{}", ContentDigest::sha256(&html));
    let source_location = headers.get("snapshot-content-location").map(String::as_str);
    let structure_valid = boundary == expected_boundary
        && parts[0].headers.get("content-location").map(String::as_str) == source_location
        && parts[1].headers.get("content-location").map(String::as_str)
            == Some("pageknot-manifest.json");
    let content_valid = pageknot_html::verify_static_sandboxed(&html).is_ok()
        && manifest_matches
        && sidecar_manifest
            .as_ref()
            .is_some_and(|manifest| manifest_encoding_matches(&manifest_bytes, manifest, false))
        && sidecar_manifest
            .as_ref()
            .is_some_and(|manifest| Some(manifest.source.final_url.as_str()) == source_location);
    ensure_verified(ArtifactVariantKind::Mhtml, structure_valid, content_valid)
}

#[derive(Debug)]
struct MimePart<'a> {
    headers: BTreeMap<String, String>,
    body: &'a str,
}

fn parse_mime_headers(source: &str) -> Option<BTreeMap<String, String>> {
    let mut headers = BTreeMap::new();
    for line in source.split("\r\n") {
        let (name, value) = line.split_once(':')?;
        if name.is_empty() || name.chars().any(char::is_whitespace) || value.contains(['\r', '\n'])
        {
            return None;
        }
        let key = name.to_ascii_lowercase();
        if headers.insert(key, value.trim().to_owned()).is_some() {
            return None;
        }
    }
    Some(headers)
}

fn parse_multipart_parts<'a>(body: &'a str, boundary: &str) -> Option<Vec<MimePart<'a>>> {
    if boundary.is_empty()
        || !boundary
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return None;
    }
    let opening = format!("--{boundary}\r\n");
    let marker = format!("\r\n--{boundary}");
    let mut remainder = body.strip_prefix(&opening)?;
    let mut parts = Vec::new();
    loop {
        let boundary_start = remainder.find(&marker)?;
        let part_source = &remainder[..boundary_start];
        let (header_source, part_body) = part_source.split_once("\r\n\r\n")?;
        parts.push(MimePart {
            headers: parse_mime_headers(header_source)?,
            body: part_body,
        });
        let after_boundary = &remainder[boundary_start.saturating_add(marker.len())..];
        if after_boundary == "--\r\n" {
            return Some(parts);
        }
        remainder = after_boundary.strip_prefix("\r\n")?;
        if parts.len() > 2 {
            return None;
        }
    }
}

fn decode_mhtml_part(
    part: &MimePart<'_>,
    expected_content_type: &str,
    maximum_bytes: u64,
) -> Result<Option<Vec<u8>>> {
    let expected_headers = BTreeSet::from([
        "content-location".to_owned(),
        "content-transfer-encoding".to_owned(),
        "content-type".to_owned(),
    ]);
    if part.headers.keys().cloned().collect::<BTreeSet<_>>() != expected_headers
        || part.headers.get("content-type").map(String::as_str) != Some(expected_content_type)
        || part
            .headers
            .get("content-transfer-encoding")
            .map(String::as_str)
            != Some("base64")
        || u64::try_from(part.body.len()).unwrap_or(u64::MAX) > maximum_bytes.saturating_mul(2)
    {
        return Ok(None);
    }
    let encoded = part.body.split_ascii_whitespace().collect::<String>();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| {
            export_error(
                "pageknot.export.verify",
                format!("MHTML part is not valid base64: {error}"),
            )
        })?;
    if u64::try_from(decoded.len()).unwrap_or(u64::MAX) > maximum_bytes
        || mime_base64(&decoded) != part.body
    {
        return Ok(None);
    }
    Ok(Some(decoded))
}

fn mime_base64(bytes: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    encoded
        .as_bytes()
        .chunks(76)
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\r\n")
}

#[cfg(test)]
mod tests {
    use pageknot_document::Document;
    use pageknot_model::{ArtifactManifest, ErrorStage, PageKnotError, Result};

    use super::{encode_mhtml, verify_mhtml};

    fn fixture() -> Result<(Vec<u8>, ArtifactManifest)> {
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
        let document = Document::parse(
            br#"<html><head><title>Export fixture</title></head>
            <body><h1>Export fixture</h1></body></html>"#,
        );
        Ok((pageknot_html::encode_html(&document, &manifest)?, manifest))
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn test_error(message: &'static str) -> PageKnotError {
        PageKnotError::new("pageknot.export.test", ErrorStage::Encoding, message)
    }

    #[test]
    fn mhtml_verifier_rejects_manifest_mutation_and_extra_parts() -> Result<()> {
        let (html, manifest) = fixture()?;
        let mhtml = encode_mhtml(&html, &manifest)?;
        let mut changed_manifest = mhtml.clone();
        let manifest_marker = b"Content-Location: pageknot-manifest.json\r\n\r\n";
        let Some(manifest_start) = find_bytes(&changed_manifest, manifest_marker)
            .map(|index| index.saturating_add(manifest_marker.len()))
        else {
            return Err(test_error(
                "MHTML fixture did not contain its manifest part",
            ));
        };
        changed_manifest[manifest_start] = if changed_manifest[manifest_start] == b'A' {
            b'B'
        } else {
            b'A'
        };

        let sandboxed = pageknot_html::encode_sandboxed_html(&html)?;
        let boundary = format!(
            "pageknot-{}",
            pageknot_model::ContentDigest::sha256(&sandboxed)
        );
        let closing = format!("--{boundary}--\r\n");
        let Some(closing_start) = find_bytes(&mhtml, closing.as_bytes()) else {
            return Err(test_error(
                "MHTML fixture did not contain its closing boundary",
            ));
        };
        let extra = format!(
            "--{boundary}\r\nContent-Type: text/plain\r\nContent-Transfer-Encoding: \
             base64\r\nContent-Location: extra.txt\r\n\r\nZXh0cmE=\r\n"
        );
        let mut extra_part = mhtml;
        drop(extra_part.splice(closing_start..closing_start, extra.bytes()));

        assert!(verify_mhtml(&changed_manifest).is_err());
        assert!(verify_mhtml(&extra_part).is_err());
        Ok(())
    }
}
