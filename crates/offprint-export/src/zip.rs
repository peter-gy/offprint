use std::io::{Cursor, Read as _, Write as _};

use offprint_model::{ArtifactFormat, ArtifactManifest, ErrorStage, OffprintError, Result};
use zip::write::SimpleFileOptions;

use crate::FormatEvidence;
use crate::support::{
    MAXIMUM_DECODED_HTML_BYTES, MAXIMUM_DECODED_MANIFEST_BYTES, ensure_verified, export_error,
    export_io_error, export_serialize_error, export_verify_io_error, manifest_encoding_matches,
    rfind_bytes,
};

/// Packages the verified HTML and manifest into a deterministic ZIP archive.
pub fn encode_zip(html: &[u8], manifest: &ArtifactManifest) -> Result<Vec<u8>> {
    let mut output = Cursor::new(Vec::new());
    {
        let mut archive = zip::ZipWriter::new(&mut output);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        archive
            .start_file("index.html", options)
            .map_err(zip_error)?;
        archive.write_all(html).map_err(export_io_error)?;
        archive
            .start_file("offprint-manifest.json", options)
            .map_err(zip_error)?;
        let mut manifest = serde_json::to_vec_pretty(manifest).map_err(export_serialize_error)?;
        manifest.push(b'\n');
        archive.write_all(&manifest).map_err(export_io_error)?;
        archive.finish().map_err(zip_error)?;
    }
    let bytes = output.into_inner();
    verify_zip(&bytes)?;
    Ok(bytes)
}

/// Verifies exact ZIP members and the contained Offprint HTML artifact.
pub fn verify_zip(bytes: &[u8]) -> Result<FormatEvidence> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(zip_verify_error)?;
    let expected_names = ["index.html", "offprint-manifest.json"];
    let structure_valid = zip_declared_member_count(bytes) == Some(expected_names.len())
        && archive.len() == expected_names.len()
        && archive.offset() == 0
        && archive.comment().is_empty()
        && expected_names.iter().enumerate().all(|(index, expected)| {
            archive.by_index(index).is_ok_and(|entry| {
                entry.name() == *expected
                    && entry.is_file()
                    && !entry.encrypted()
                    && entry.comment().is_empty()
                    && entry.compression() == zip::CompressionMethod::Deflated
            })
        });
    if !structure_valid {
        return ensure_verified(ArtifactFormat::Zip, false, false);
    }
    let html = read_zip_member(&mut archive, 0, MAXIMUM_DECODED_HTML_BYTES)?;
    let manifest_bytes = read_zip_member(&mut archive, 1, MAXIMUM_DECODED_MANIFEST_BYTES)?;
    let embedded_manifest = offprint_html::inspect_html(&html).ok();
    let sidecar_manifest = serde_json::from_slice::<ArtifactManifest>(&manifest_bytes).ok();
    let content_valid = offprint_html::verify_static(&html).is_ok()
        && embedded_manifest.is_some()
        && embedded_manifest == sidecar_manifest
        && sidecar_manifest
            .as_ref()
            .is_some_and(|manifest| manifest_encoding_matches(&manifest_bytes, manifest, true));
    ensure_verified(ArtifactFormat::Zip, structure_valid, content_valid)
}

fn read_zip_member(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    index: usize,
    maximum_bytes: u64,
) -> Result<Vec<u8>> {
    let entry = archive.by_index(index).map_err(zip_verify_error)?;
    if entry.size() > maximum_bytes {
        return Err(export_error(
            "offprint.export.verify",
            "ZIP member exceeds the verification byte limit",
        ));
    }
    let mut bytes = Vec::new();
    entry
        .take(maximum_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(export_verify_io_error)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum_bytes {
        return Err(export_error(
            "offprint.export.verify",
            "ZIP member exceeds the verification byte limit",
        ));
    }
    Ok(bytes)
}

fn zip_declared_member_count(bytes: &[u8]) -> Option<usize> {
    const END_OF_CENTRAL_DIRECTORY: &[u8] = b"PK\x05\x06";
    const FIXED_LENGTH: usize = 22;

    let start = rfind_bytes(bytes, END_OF_CENTRAL_DIRECTORY)?;
    let record = bytes.get(start..)?;
    if record.len() < FIXED_LENGTH
        || read_u16_le(record.get(4..6)?) != 0
        || read_u16_le(record.get(6..8)?) != 0
    {
        return None;
    }
    let entries_on_disk = read_u16_le(record.get(8..10)?);
    let total_entries = read_u16_le(record.get(10..12)?);
    let comment_length = usize::from(read_u16_le(record.get(20..22)?));
    if entries_on_disk != total_entries
        || total_entries == u16::MAX
        || record.len() != FIXED_LENGTH.saturating_add(comment_length)
    {
        return None;
    }
    Some(usize::from(total_entries))
}

fn read_u16_le(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn zip_error(error: zip::result::ZipError) -> OffprintError {
    OffprintError::new(
        "offprint.export.zip",
        ErrorStage::Encoding,
        format!("ZIP artifact processing failed: {error}"),
    )
}

fn zip_verify_error(error: zip::result::ZipError) -> OffprintError {
    export_error(
        "offprint.export.verify",
        format!("ZIP artifact verification failed: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write as _};

    use offprint_document::Document;
    use offprint_model::{ArtifactManifest, ErrorStage, OffprintError, Result};
    use zip::write::SimpleFileOptions;

    use super::{encode_zip, verify_zip, zip_error};
    use crate::support::{export_io_error, export_serialize_error};

    fn fixture() -> Result<(Vec<u8>, ArtifactManifest)> {
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
        Ok((offprint_html::encode_html(&document, &manifest)?, manifest))
    }

    fn zip_with_members(members: &[(&str, &[u8])]) -> Result<Vec<u8>> {
        let mut output = Cursor::new(Vec::new());
        {
            let mut archive = zip::ZipWriter::new(&mut output);
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(0o644);
            for (name, bytes) in members {
                archive.start_file(*name, options).map_err(zip_error)?;
                archive.write_all(bytes).map_err(export_io_error)?;
            }
            archive.finish().map_err(zip_error)?;
        }
        Ok(output.into_inner())
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn replace_all_same_length(bytes: &mut [u8], needle: &[u8], replacement: &[u8]) {
        if needle.len() != replacement.len() {
            return;
        }
        let mut offset = 0;
        while let Some(index) = find_bytes(&bytes[offset..], needle) {
            let start = offset.saturating_add(index);
            let end = start.saturating_add(needle.len());
            bytes[start..end].copy_from_slice(replacement);
            offset = end;
        }
    }

    #[test]
    fn zip_encoder_and_verifier_agree() -> Result<()> {
        let (html, manifest) = fixture()?;
        assert!(verify_zip(&encode_zip(&html, &manifest)?).is_ok());
        Ok(())
    }

    #[test]
    fn zip_verifier_rejects_mismatched_duplicate_and_extra_members() -> Result<()> {
        let (html, manifest) = fixture()?;
        let mut different_manifest = manifest.clone();
        different_manifest
            .warning_codes
            .push("offprint.test.mismatch".to_owned());
        let sidecar =
            serde_json::to_vec_pretty(&different_manifest).map_err(export_serialize_error)?;
        let mismatch =
            zip_with_members(&[("index.html", &html), ("offprint-manifest.json", &sidecar)])?;
        let valid_sidecar = serde_json::to_vec_pretty(&manifest).map_err(export_serialize_error)?;
        let mut duplicate = zip_with_members(&[
            ("index.html", &html),
            ("offprint-manifest.json", &valid_sidecar),
            ("other.html", &html),
        ])?;
        replace_all_same_length(&mut duplicate, b"other.html", b"index.html");
        let extra = zip_with_members(&[
            ("index.html", &html),
            ("offprint-manifest.json", &valid_sidecar),
            ("notes.txt", b"unexpected"),
        ])?;

        assert!(verify_zip(&mismatch).is_err());
        assert!(verify_zip(&duplicate).is_err());
        assert!(verify_zip(&extra).is_err());
        Ok(())
    }
}
