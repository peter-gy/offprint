use std::io::{Read as _, Write as _};

use base64::Engine as _;
use flate2::Compression;
use flate2::read::GzDecoder;
use offprint_model::{ArtifactFormat, ContentDigest, Result};

use crate::FormatEvidence;
use crate::support::{
    MAXIMUM_DECODED_HTML_BYTES, ensure_verified, export_error, export_io_error,
    export_verify_io_error,
};

/// Encodes HTML into a browser-readable shell with a deterministic gzip body.
pub fn encode_self_extracting(html: &[u8]) -> Result<Vec<u8>> {
    let bytes = render_self_extracting(html)?;
    verify_self_extracting(&bytes)?;
    Ok(bytes)
}

fn render_self_extracting(html: &[u8]) -> Result<Vec<u8>> {
    let mut compressor = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::best());
    compressor.write_all(html).map_err(export_io_error)?;
    let compressed = compressor.finish().map_err(export_io_error)?;
    let payload = base64::engine::general_purpose::STANDARD.encode(compressed);
    let digest = ContentDigest::sha256(html);
    let shell = format!(
        r#"<!doctype html><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; base-uri 'none'; connect-src 'none'; font-src data:; form-action 'none'; frame-src data: blob:; img-src data: blob:; media-src data: blob:; object-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline' data:">
<title>Opening Offprint capture</title>
<script id="offprint-compressed" type="application/octet-stream" data-sha256="{digest}">{payload}</script>
<script>
(() => {{
  const encoded = document.getElementById("offprint-compressed").textContent.trim();
  const binary = atob(encoded);
  const bytes = Uint8Array.from(binary, character => character.charCodeAt(0));
  new Response(new Blob([bytes]).stream().pipeThrough(new DecompressionStream("gzip")))
    .text()
    .then(html => {{
      document.open();
      document.write(html);
      document.close();
    }});
}})();
</script>"#
    );
    Ok(shell.into_bytes())
}

/// Verifies and decompresses a self-extracting Offprint shell.
pub fn verify_self_extracting(bytes: &[u8]) -> Result<FormatEvidence> {
    let source = std::str::from_utf8(bytes).map_err(|error| {
        export_error(
            "offprint.export.verify",
            format!("self-extracting output is not valid UTF-8: {error}"),
        )
    })?;
    let marker =
        r#"<script id="offprint-compressed" type="application/octet-stream" data-sha256=""#;
    let Some(start) = source.find(marker).map(|index| index + marker.len()) else {
        return ensure_verified(ArtifactFormat::SelfExtractingHtml, false, false);
    };
    let Some(digest_end) = source[start..].find('"').map(|index| start + index) else {
        return ensure_verified(ArtifactFormat::SelfExtractingHtml, false, false);
    };
    let digest = &source[start..digest_end];
    let Some(payload_start) = source[digest_end..]
        .find('>')
        .map(|index| digest_end + index + 1)
    else {
        return ensure_verified(ArtifactFormat::SelfExtractingHtml, false, false);
    };
    let Some(payload_end) = source[payload_start..]
        .find("</script>")
        .map(|index| payload_start + index)
    else {
        return ensure_verified(ArtifactFormat::SelfExtractingHtml, false, false);
    };
    if u64::try_from(payload_end.saturating_sub(payload_start)).unwrap_or(u64::MAX)
        > MAXIMUM_DECODED_HTML_BYTES.saturating_mul(2)
    {
        return ensure_verified(ArtifactFormat::SelfExtractingHtml, false, false);
    }
    let compressed = base64::engine::general_purpose::STANDARD
        .decode(source[payload_start..payload_end].trim())
        .map_err(|error| {
            export_error(
                "offprint.export.verify",
                format!("self-extracting payload is not valid base64: {error}"),
            )
        })?;
    let mut html = Vec::new();
    GzDecoder::new(compressed.as_slice())
        .take(MAXIMUM_DECODED_HTML_BYTES.saturating_add(1))
        .read_to_end(&mut html)
        .map_err(export_verify_io_error)?;
    let canonical = if u64::try_from(html.len()).unwrap_or(u64::MAX) <= MAXIMUM_DECODED_HTML_BYTES {
        render_self_extracting(&html)?
    } else {
        Vec::new()
    };
    let structure_valid = digest.parse::<ContentDigest>().is_ok() && canonical == bytes;
    let content_valid = u64::try_from(html.len()).unwrap_or(u64::MAX) <= MAXIMUM_DECODED_HTML_BYTES
        && ContentDigest::sha256(&html).to_hex() == digest
        && offprint_html::verify_static(&html).is_ok();
    ensure_verified(
        ArtifactFormat::SelfExtractingHtml,
        structure_valid,
        content_valid,
    )
}

#[cfg(test)]
mod tests {
    use offprint_document::Document;
    use offprint_model::{ArtifactManifest, ErrorStage, OffprintError, Result};

    use super::{encode_self_extracting, verify_self_extracting};

    fn fixture() -> Result<Vec<u8>> {
        let manifest = serde_json::from_slice::<ArtifactManifest>(include_bytes!(
            "../../../schemas/examples/artifact-manifest.json"
        ))
        .map_err(|error| {
            OffprintError::new(
                "offprint.export.encode",
                ErrorStage::Encoding,
                error.to_string(),
            )
        })?;
        let document = Document::parse(
            br#"<html><head><title>Export fixture</title></head>
            <body><h1>Export fixture</h1></body></html>"#,
        );
        offprint_html::encode_html(&document, &manifest)
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn test_error(message: &'static str) -> OffprintError {
        OffprintError::new("offprint.export.test", ErrorStage::Encoding, message)
    }

    #[test]
    fn self_extracting_encoder_and_verifier_agree() -> Result<()> {
        let html = fixture()?;
        assert!(verify_self_extracting(&encode_self_extracting(&html)?).is_ok());
        Ok(())
    }

    #[test]
    fn self_extracting_verifier_rejects_shell_mutation_and_extra_scripts() -> Result<()> {
        let html = fixture()?;
        let shell = encode_self_extracting(&html)?;
        let mut changed_loader = shell.clone();
        let Some(loader) = find_bytes(&changed_loader, b"DecompressionStream") else {
            return Err(test_error(
                "self-extracting fixture did not contain its loader",
            ));
        };
        changed_loader[loader] = b'X';
        let mut extra_script = shell;
        extra_script.extend_from_slice(b"\n<script>alert(1)</script>");

        assert!(verify_self_extracting(&changed_loader).is_err());
        assert!(verify_self_extracting(&extra_script).is_err());
        Ok(())
    }
}
