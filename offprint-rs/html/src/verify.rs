use html5ever::Attribute;
use markup5ever::ns;
use offprint_document::{Document, NodeData};
use offprint_model::{
    ContentDigest, ErrorStage, OffprintError, Result, VerificationMode, VerificationReport,
};

use crate::VerifiedHtml;

#[cfg(test)]
use offprint_model::ArtifactManifest;

mod active_content;
mod manifest;
mod resource;
mod structure;

pub use manifest::inspect_html;
#[cfg(test)]
use resource::validate_embedded_resource;
use structure::{validate_artifact_documents, validate_csp};

pub fn verify_static(bytes: &[u8]) -> Result<VerificationReport> {
    verify_html(bytes).map(|verified| verified.into_verification_and_manifest().0)
}

/// Parses and statically verifies bytes as one proof-bearing HTML value.
pub fn verify_html<B: AsRef<[u8]>>(bytes: B) -> Result<VerifiedHtml<B>> {
    verify_html_with_state_restoration(bytes, true)
}

/// Verifies a safe-static HTML artifact and returns its validated manifest.
pub fn verify_static_with_manifest(
    bytes: &[u8],
) -> Result<(VerificationReport, offprint_model::ArtifactManifest)> {
    verify_html(bytes).map(VerifiedHtml::into_verification_and_manifest)
}

pub fn verify_static_sandboxed(bytes: &[u8]) -> Result<VerificationReport> {
    verify_html_with_state_restoration(bytes, false)
        .map(|verified| verified.into_verification_and_manifest().0)
}

fn verify_html_with_state_restoration<B: AsRef<[u8]>>(
    bytes: B,
    restore_state: bool,
) -> Result<VerifiedHtml<B>> {
    let document = Document::parse(bytes.as_ref());
    let manifest = manifest::inspect_document(&document)?;
    if !restore_state && manifest.structural_repair.applied {
        return Err(verification_error(
            "offprint.verification.structural_repair",
            "sandboxed artifacts cannot apply structural repair",
        ));
    }
    validate_csp(&document, &manifest, restore_state)?;
    validate_artifact_documents(&document, &manifest, restore_state)?;
    let byte_count = u64::try_from(bytes.as_ref().len()).map_err(|error| {
        verification_error(
            "offprint.verification.size",
            format!("artifact byte count exceeds the supported range: {error}"),
        )
    })?;
    let sha256 = ContentDigest::sha256(bytes.as_ref());
    let verification = VerificationReport {
        schema_version: offprint_model::PUBLIC_SCHEMA_VERSION,
        mode: VerificationMode::Static,
        artifact_sha256: sha256,
        bytes: byte_count,
        network_requests: 0,
    };
    Ok(VerifiedHtml::new(
        bytes,
        document,
        manifest,
        sha256,
        verification,
    ))
}

fn text_contents(document: &Document, parent: offprint_model::NodeId) -> String {
    document
        .node(parent)
        .into_iter()
        .flat_map(|node| &node.children)
        .filter_map(|id| document.node(*id))
        .filter_map(|node| match &node.data {
            NodeData::Text { contents } => Some(contents.as_ref()),
            _ => None,
        })
        .collect()
}

fn attribute_value<'a>(attrs: &'a [Attribute], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|attribute| attribute.name.ns == ns!() && attribute.name.local.as_ref() == name)
        .map(|attribute| attribute.value.as_ref())
}

fn verification_error(code: &'static str, message: impl Into<String>) -> OffprintError {
    OffprintError::new(code, ErrorStage::Verification, message)
}

#[cfg(test)]
pub(crate) fn test_manifest() -> ArtifactManifest {
    use chrono::{TimeZone as _, Utc};
    use offprint_model::{
        BrowserEnvironment, BrowserInfo, BrowserProduct, BrowserSource, ManifestGenerator,
        ManifestSource, RedactedUrl, ResourceSummary, StructuralRepair, ViewState,
    };

    ArtifactManifest {
        schema_version: offprint_model::PUBLIC_SCHEMA_VERSION,
        format_version: offprint_model::ARTIFACT_FORMAT_VERSION,
        generator: ManifestGenerator {
            name: "Offprint".to_owned(),
            version: "0.1.0".to_owned(),
        },
        source: ManifestSource {
            requested_url: RedactedUrl::from_url(
                &url::Url::parse("https://example.com").unwrap_or_else(|_| {
                    url::Url::parse("about:blank").unwrap_or_else(|_| std::process::abort())
                }),
                &offprint_model::RedactionPolicy::default(),
            ),
            requested_url_sha256: ContentDigest::sha256("https://example.com"),
            final_url: RedactedUrl::from_url(
                &url::Url::parse("https://example.com").unwrap_or_else(|_| {
                    url::Url::parse("about:blank").unwrap_or_else(|_| std::process::abort())
                }),
                &offprint_model::RedactionPolicy::default(),
            ),
            final_url_sha256: ContentDigest::sha256("https://example.com"),
        },
        captured_at: Utc
            .with_ymd_and_hms(2026, 7, 27, 0, 0, 0)
            .single()
            .unwrap_or_else(|| std::process::abort()),
        browser: BrowserInfo {
            product: BrowserProduct::Chromium,
            version: "1".to_owned(),
            source: BrowserSource::System,
            executable_path: None,
            endpoint: None,
            revision: None,
            protocol_version: "1.3".to_owned(),
        },
        environment: BrowserEnvironment::default(),
        view_state: ViewState::default(),
        capture_policy_sha256: ContentDigest::sha256(b"policy"),
        frames: 1,
        resources: ResourceSummary::default(),
        resource_records: Vec::new(),
        warning_codes: Vec::new(),
        structural_repair: StructuralRepair {
            applied: false,
            script_sha256: None,
        },
        verification_mode: VerificationMode::Offline,
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use offprint_document::Document;
    use offprint_model::{
        ContentDigest, ErrorCode, FrameId, RedactedUrl, RedactionPolicy, ResourceError,
        ResourceOutcome, ResourceProvenance, ResourceRecord, ResourceRetrievalSource,
        ResourceSummary,
    };
    use url::Url;

    use crate::{STATE_RESTORATION_SCRIPT, encode_html};

    use super::{test_manifest, validate_embedded_resource, verify_html, verify_static};

    fn attach_empty_embedded_records(
        manifest: &mut offprint_model::ArtifactManifest,
        frame_ids: impl IntoIterator<Item = u64>,
    ) {
        let url = Url::parse("data:image/png;base64,").unwrap_or_else(|_| std::process::abort());
        manifest.resource_records = frame_ids
            .into_iter()
            .enumerate()
            .map(|(index, frame_id)| ResourceRecord {
                id: offprint_model::ResourceId::new(u32::try_from(index).unwrap_or(u32::MAX)),
                frame_id: FrameId::new(frame_id),
                requested_url: RedactedUrl::from_url(&url, &RedactionPolicy::default()),
                requested_url_sha256: ContentDigest::sha256(url.as_str()),
                outcome: ResourceOutcome::Embedded {
                    digest: ContentDigest::sha256([]),
                    media_type: "image/png".to_owned(),
                    bytes: 0,
                },
                provenance: Some(ResourceProvenance {
                    source: ResourceRetrievalSource::InlineData,
                    final_url: RedactedUrl::from_url(&url, &RedactionPolicy::default()),
                    final_url_sha256: ContentDigest::sha256(url.as_str()),
                    status: None,
                    redirects: Vec::new(),
                    received_bytes: 0,
                }),
            })
            .collect();
    }

    #[test]
    fn verifier_decodes_a_fragmented_embedded_svg_url() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><g id="root"/></svg>"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(svg);
        let url = Url::parse(&format!("data:image/svg+xml;base64,{encoded}#root"))
            .unwrap_or_else(|_| std::process::abort());
        let base_url = Url::parse("file:///capture.html").unwrap_or_else(|_| std::process::abort());
        let mut references = 0;
        let mut embedded = std::collections::BTreeMap::new();

        let result = validate_embedded_resource(&url, &base_url, &mut references, &mut embedded, 0);

        assert!(result.is_ok(), "{result:?}");
        assert_eq!(references, 0);
        assert_eq!(embedded.values().copied().sum::<u32>(), 1);
    }

    #[test]
    fn encoded_safe_static_artifact_passes_static_verification() {
        let document = Document::parse(
            b"<html><head><title>x</title></head><body><img src=\"data:image/png;base64,\"></body></html>",
        );
        let mut manifest = test_manifest();
        manifest.resources = ResourceSummary {
            discovered: 1,
            embedded: 1,
            ..ResourceSummary::default()
        };
        attach_empty_embedded_records(&mut manifest, [1]);
        let encoded = encode_html(&document, &manifest);
        let verified = encoded.as_deref().map(verify_static);

        assert!(verified.is_ok_and(|result| result.is_ok()));
    }

    #[test]
    fn verified_html_binds_owned_bytes_to_one_parsed_proof() {
        let document = Document::parse(b"<html><head></head><body><p>bound</p></body></html>");
        let encoded = encode_html(&document, &test_manifest());
        let verified = encoded.and_then(verify_html);

        assert!(verified.as_ref().is_ok_and(|html| {
            html.verification().artifact_sha256 == html.sha256()
                && html.verification().bytes
                    == u64::try_from(html.bytes().len()).unwrap_or(u64::MAX)
                && html.document().node_count() > 0
                && html.manifest().format_version == offprint_model::ARTIFACT_FORMAT_VERSION
        }));
    }

    #[test]
    fn verifier_rejects_an_external_image_reference() {
        let document = Document::parse(
            b"<html><head><title>x</title></head><body><img src=\"https://example.com/a.png\"></body></html>",
        );
        let encoded = encode_html(&document, &test_manifest());
        let verified = encoded.as_deref().map(verify_static);

        assert_eq!(
            verified
                .ok()
                .and_then(std::result::Result::err)
                .map(|error| error.code),
            Some(offprint_model::ErrorCode::from_static(
                "offprint.verification.external_reference"
            ))
        );
    }

    #[test]
    fn verifier_rejects_an_unembedded_stylesheet_fragment() {
        let document = Document::parse(
            br##"<html><head><title>x</title>
            <link rel="stylesheet" href="#theme">
            </head><body></body></html>"##,
        );
        let encoded = encode_html(&document, &test_manifest());
        let verified = encoded.as_deref().map(verify_static);

        assert_eq!(
            verified
                .ok()
                .and_then(std::result::Result::err)
                .map(|error| error.code),
            Some(ErrorCode::from_static(
                "offprint.verification.external_reference"
            ))
        );
    }

    #[test]
    fn verifier_rejects_external_svg_presentation_resources() {
        for attribute in [
            r#"fill="url(https://example.test/paint.svg#fill)""#,
            r#"stroke="url(https://example.test/paint.svg#stroke)""#,
            r#"filter="url(https://example.test/effects.svg#filter)""#,
            r#"clip-path="url(https://example.test/effects.svg#clip)""#,
            r#"mask="url(https://example.test/effects.svg#mask)""#,
            r#"marker="url(https://example.test/markers.svg#marker)""#,
            r#"marker-start="url(https://example.test/markers.svg#start)""#,
            r#"marker-mid="url(https://example.test/markers.svg#mid)""#,
            r#"marker-end="url(https://example.test/markers.svg#end)""#,
            r#"cursor="url(https://example.test/cursors.svg#cursor), auto""#,
        ] {
            let document = Document::parse(
                format!(
                    "<html><head><title>x</title></head><body><svg><path {attribute}/></svg></body></html>"
                )
                .as_bytes(),
            );
            let encoded = encode_html(&document, &test_manifest());
            let verified = encoded.as_deref().map(verify_static);

            assert_eq!(
                verified
                    .ok()
                    .and_then(std::result::Result::err)
                    .map(|error| error.code),
                Some(offprint_model::ErrorCode::from_static(
                    "offprint.verification.external_reference"
                )),
                "{attribute}"
            );
        }
    }

    #[test]
    fn verifier_rejects_plain_and_xlink_svg_resource_urls() {
        for element in [
            r#"<linearGradient href="https://example.test/gradients.svg#paint"/>"#,
            r#"<pattern xmlns:xlink="http://www.w3.org/1999/xlink"
                xlink:href="https://example.test/patterns.svg#tile"/>"#,
            r#"<feImage href="https://example.test/pixel.png"/>"#,
            r#"<use xmlns:xlink="http://www.w3.org/1999/xlink"
                xlink:href="https://example.test/symbols.svg#icon"/>"#,
        ] {
            let document = Document::parse(
                format!(
                    "<html><head><title>x</title></head><body><svg>{element}</svg></body></html>"
                )
                .as_bytes(),
            );
            let encoded = encode_html(&document, &test_manifest());
            let verified = encoded.as_deref().map(verify_static);

            assert_eq!(
                verified
                    .ok()
                    .and_then(std::result::Result::err)
                    .map(|error| error.code),
                Some(offprint_model::ErrorCode::from_static(
                    "offprint.verification.external_reference"
                )),
                "{element}"
            );
        }
    }

    #[test]
    fn verifier_accepts_svg_colors_and_same_document_fragments() {
        let document = Document::parse(
            br##"<html><head><title>x</title></head><body>
            <svg xmlns:xlink="http://www.w3.org/1999/xlink">
              <defs>
                <linearGradient id="paint" href="#base"/>
                <pattern id="pattern" xlink:href="#tile"/>
              </defs>
              <path fill="#abcdef" stroke="url(#paint) #000"
                filter="none" clip-path="url(#clip)" mask="url(#mask)"
                marker="url(#marker)" marker-start="none"
                marker-mid="url(#mid)" marker-end="url(#end)"
                cursor="url(#cursor), auto"/>
              <use href="#symbol"/>
            </svg>
            </body></html>"##,
        );
        let encoded = encode_html(&document, &test_manifest());
        let verified = encoded.as_deref().map(verify_static);

        assert!(verified.is_ok_and(|result| result.is_ok()));
    }

    #[test]
    fn verifier_rejects_an_inline_svg_script() {
        let document = Document::parse(
            br#"<html><head><title>x</title></head><body>
            <svg><script href="https://example.test/active.js">run()</script></svg>
            </body></html>"#,
        );
        let encoded = encode_html(&document, &test_manifest());
        let verified = encoded.as_deref().map(verify_static);

        assert_eq!(
            verified
                .ok()
                .and_then(std::result::Result::err)
                .map(|error| error.code),
            Some(offprint_model::ErrorCode::from_static(
                "offprint.verification.active_content"
            ))
        );
    }

    #[test]
    fn verifier_rejects_a_manifest_link_contract() {
        let document = Document::parse(
            br#"<html><head><title>x</title>
            <link rel="manifest" href="data:application/manifest+json,%7B%7D">
            </head><body></body></html>"#,
        );
        let encoded = encode_html(&document, &test_manifest());
        let verified = encoded.as_deref().map(verify_static);

        assert_eq!(
            verified
                .ok()
                .and_then(std::result::Result::err)
                .map(|error| error.code),
            Some(ErrorCode::from_static(
                "offprint.verification.active_content"
            ))
        );
    }

    #[test]
    fn verifier_rejects_request_link_rel_tokens_with_ascii_case_and_whitespace() {
        for rel in [
            "  MANIFEST  ",
            "alternate&#x9;PreLoad&#xA;",
            "icon&#xD;&#xA;modulePRELOAD",
        ] {
            let document = Document::parse(
                format!(
                    "<html><head><title>x</title><link rel=\"{rel}\" \
                     href=\"data:text/plain,\"></head><body></body></html>"
                )
                .as_bytes(),
            );
            let encoded = encode_html(&document, &test_manifest());
            let verified = encoded.as_deref().map(verify_static);

            assert_eq!(
                verified
                    .ok()
                    .and_then(std::result::Result::err)
                    .map(|error| error.code),
                Some(ErrorCode::from_static(
                    "offprint.verification.active_content"
                )),
                "{rel}"
            );
        }
    }

    #[test]
    fn verifier_rejects_a_modified_state_restoration_program() -> offprint_model::Result<()> {
        let document = Document::parse(b"<html><head><title>x</title></head><body></body></html>");
        let encoded = encode_html(&document, &test_manifest())?;
        let original = String::from_utf8_lossy(&encoded);
        let modified = original.replacen(STATE_RESTORATION_SCRIPT, "invalid", 1);
        assert_ne!(modified, original);
        let verified = verify_static(modified.as_bytes());

        assert_eq!(
            verified.err().map(|error| error.code),
            Some(offprint_model::ErrorCode::from_static(
                "offprint.verification.active_content"
            ))
        );
        Ok(())
    }

    #[test]
    fn verifier_rejects_a_script_in_an_embedded_svg() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><script>run()</script></svg>"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(svg);
        let url = Url::parse(&format!("data:image/svg+xml;base64,{encoded}"))
            .unwrap_or_else(|_| std::process::abort());
        let base_url = Url::parse("file:///capture.html").unwrap_or_else(|_| std::process::abort());
        let mut references = 0;
        let mut embedded = std::collections::BTreeMap::new();

        let result = validate_embedded_resource(&url, &base_url, &mut references, &mut embedded, 0);

        assert_eq!(
            result.err().map(|error| error.code),
            Some(offprint_model::ErrorCode::from_static(
                "offprint.verification.active_content"
            ))
        );
    }

    #[test]
    fn verifier_rejects_a_script_in_an_embedded_html_resource() {
        let html = br#"<html><body><script>run()</script></body></html>"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(html);
        let url = Url::parse(&format!("data:text/html;base64,{encoded}"))
            .unwrap_or_else(|_| std::process::abort());
        let base_url = Url::parse("file:///capture.html").unwrap_or_else(|_| std::process::abort());
        let mut references = 0;
        let mut embedded = std::collections::BTreeMap::new();

        let result = validate_embedded_resource(&url, &base_url, &mut references, &mut embedded, 0);

        assert_eq!(
            result.err().map(|error| error.code),
            Some(offprint_model::ErrorCode::from_static(
                "offprint.verification.active_content"
            ))
        );
    }

    #[test]
    fn verifier_rejects_a_nested_frame_in_an_embedded_html_resource() {
        let html =
            br#"<html><body><iframe srcdoc="<script>run()</script>"></iframe></body></html>"#;
        let encoded = base64::engine::general_purpose::STANDARD.encode(html);
        let url = Url::parse(&format!("data:text/html;base64,{encoded}"))
            .unwrap_or_else(|_| std::process::abort());
        let base_url = Url::parse("file:///capture.html").unwrap_or_else(|_| std::process::abort());
        let mut references = 0;
        let mut embedded = std::collections::BTreeMap::new();

        let result = validate_embedded_resource(&url, &base_url, &mut references, &mut embedded, 0);

        assert_eq!(
            result.err().map(|error| error.code),
            Some(offprint_model::ErrorCode::from_static(
                "offprint.verification.embedded_resource"
            ))
        );
    }

    #[test]
    fn verifier_checks_embedded_frames_and_resource_counts_recursively() {
        let document = Document::parse(
            br#"<html><head><title>x</title></head><body>
            <iframe data-offprint-frame-id="2"
              srcdoc="<img src='data:image/png;base64,'>"></iframe>
            </body></html>"#,
        );
        let mut manifest = test_manifest();
        manifest.frames = 2;
        manifest.resources = ResourceSummary {
            discovered: 1,
            embedded: 1,
            ..ResourceSummary::default()
        };
        attach_empty_embedded_records(&mut manifest, [2]);
        let encoded = encode_html(&document, &manifest);
        let verified = encoded.as_deref().map(verify_static);

        assert!(verified.is_ok_and(|result| result.is_ok()));
    }

    #[test]
    fn verifier_rejects_an_external_reference_inside_srcdoc() {
        let document = Document::parse(
            br#"<html><head><title>x</title></head><body>
            <iframe data-offprint-frame-id="2"
              srcdoc="<img src='https://example.com/frame.png'>"></iframe>
            </body></html>"#,
        );
        let mut manifest = test_manifest();
        manifest.frames = 2;
        manifest.resources = ResourceSummary {
            discovered: 1,
            embedded: 1,
            ..ResourceSummary::default()
        };
        attach_empty_embedded_records(&mut manifest, [2]);
        let encoded = encode_html(&document, &manifest);
        let verified = encoded.as_deref().map(verify_static);

        assert_eq!(
            verified
                .ok()
                .and_then(std::result::Result::err)
                .map(|error| error.code),
            Some(offprint_model::ErrorCode::from_static(
                "offprint.verification.external_reference"
            ))
        );
    }

    #[test]
    fn verifier_rejects_external_references_nested_in_embedded_css() {
        let document = Document::parse(
            br#"<html><head>
            <link rel="stylesheet"
              href="data:text/css,%40import%20url%28https%3A%2F%2Fexample.com%2Fx.css%29%3B">
            </head><body></body></html>"#,
        );
        let mut manifest = test_manifest();
        manifest.resources = ResourceSummary {
            discovered: 2,
            embedded: 2,
            ..ResourceSummary::default()
        };
        attach_empty_embedded_records(&mut manifest, [1, 1]);
        let encoded = encode_html(&document, &manifest);
        let verified = encoded.as_deref().map(verify_static);

        assert_eq!(
            verified
                .ok()
                .and_then(std::result::Result::err)
                .map(|error| error.code),
            Some(offprint_model::ErrorCode::from_static(
                "offprint.verification.external_reference"
            ))
        );
    }

    #[test]
    fn verifier_rejects_frame_count_drift() {
        let document = Document::parse(
            br#"<html><head><title>x</title></head><body>
            <iframe data-offprint-frame-id="2" srcdoc="<p>child</p>"></iframe>
            </body></html>"#,
        );
        let encoded = encode_html(&document, &test_manifest());
        let verified = encoded.as_deref().map(verify_static);

        assert_eq!(
            verified
                .ok()
                .and_then(std::result::Result::err)
                .map(|error| error.code),
            Some(offprint_model::ErrorCode::from_static(
                "offprint.verification.frames"
            ))
        );
    }

    #[test]
    fn verifier_rejects_an_embedded_digest_that_differs_from_the_data_url() {
        let document = Document::parse(
            b"<html><head><title>x</title></head><body><img src=\"data:image/png;base64,\"></body></html>",
        );
        let mut manifest = test_manifest();
        manifest.resources = ResourceSummary {
            discovered: 1,
            embedded: 1,
            embedded_bytes: 1,
            ..ResourceSummary::default()
        };
        attach_empty_embedded_records(&mut manifest, [1]);
        if let ResourceOutcome::Embedded { digest, bytes, .. } =
            &mut manifest.resource_records[0].outcome
        {
            *digest = ContentDigest::sha256([1]);
            *bytes = 1;
        }
        let encoded = encode_html(&document, &manifest);
        let verified = encoded.as_deref().map(verify_static);

        assert_eq!(
            verified
                .ok()
                .and_then(std::result::Result::err)
                .map(|error| error.code),
            Some(offprint_model::ErrorCode::from_static(
                "offprint.verification.embedded_resource"
            ))
        );
    }

    #[test]
    fn verifier_rejects_unrecorded_embedded_bytes_disguised_as_a_failure_fallback() {
        let document = Document::parse(
            b"<html><head><title>x</title></head><body><img src=\"data:text/plain;base64,YXJiaXRyYXJ5\"></body></html>",
        );
        let mut manifest = test_manifest();
        manifest.resources = ResourceSummary {
            discovered: 1,
            failed: 1,
            ..ResourceSummary::default()
        };
        let source =
            Url::parse("https://example.com/missing.png").unwrap_or_else(|_| std::process::abort());
        manifest.resource_records = vec![ResourceRecord {
            id: offprint_model::ResourceId::new(0),
            frame_id: FrameId::new(1),
            requested_url: RedactedUrl::from_url(&source, &RedactionPolicy::default()),
            requested_url_sha256: ContentDigest::sha256(source.as_str()),
            outcome: ResourceOutcome::Failed {
                error: ResourceError {
                    code: ErrorCode::from_static("offprint.resource.load"),
                    message: "resource load failed".to_owned(),
                    retryable: true,
                },
            },
            provenance: None,
        }];
        let encoded = encode_html(&document, &manifest);
        let verified = encoded.as_deref().map(verify_static);

        assert_eq!(
            verified
                .ok()
                .and_then(std::result::Result::err)
                .map(|error| error.code),
            Some(ErrorCode::from_static(
                "offprint.verification.embedded_resource"
            ))
        );
    }

    #[test]
    fn verifier_accepts_the_owned_empty_fallback_for_a_failed_resource() {
        let fallback = crate::empty_resource_data_url(offprint_document::RenderingRole::Image);
        let document = Document::parse(
            format!(
                "<html><head><title>x</title></head><body><img src=\"{fallback}\"></body></html>"
            )
            .as_bytes(),
        );
        let mut manifest = test_manifest();
        manifest.resources = ResourceSummary {
            discovered: 1,
            failed: 1,
            ..ResourceSummary::default()
        };
        let source =
            Url::parse("https://example.com/missing.png").unwrap_or_else(|_| std::process::abort());
        manifest.resource_records = vec![ResourceRecord {
            id: offprint_model::ResourceId::new(0),
            frame_id: FrameId::new(1),
            requested_url: RedactedUrl::from_url(&source, &RedactionPolicy::default()),
            requested_url_sha256: ContentDigest::sha256(source.as_str()),
            outcome: ResourceOutcome::Failed {
                error: ResourceError {
                    code: ErrorCode::from_static("offprint.resource.load"),
                    message: "resource load failed".to_owned(),
                    retryable: true,
                },
            },
            provenance: None,
        }];
        let encoded = encode_html(&document, &manifest);
        let verified = encoded.as_deref().map(verify_static);

        assert!(verified.is_ok_and(|result| result.is_ok()));
    }

    #[test]
    fn verifier_rejects_an_unknown_structural_repair_program_digest() {
        let document =
            Document::parse(b"<html><head><title>x</title></head><body><p>same</p></body></html>");
        let mut manifest = test_manifest();
        manifest.structural_repair.applied = true;
        manifest.structural_repair.script_sha256 = Some(ContentDigest::sha256(b"other program"));
        let encoded = encode_html(&document, &manifest);
        let verified = encoded.as_deref().map(verify_static);

        assert_eq!(
            verified
                .ok()
                .and_then(std::result::Result::err)
                .map(|error| error.code),
            Some(ErrorCode::from_static(
                "offprint.verification.structural_repair"
            ))
        );
    }
}
