use std::collections::{BTreeMap, BTreeSet};

use markup5ever::{local_name, ns};
use offprint_document::{
    DOCUMENT_SCROLL_X_ATTRIBUTE, DOCUMENT_SCROLL_Y_ATTRIBUTE, Document,
    ELEMENT_SCROLL_LEFT_ATTRIBUTE, ELEMENT_SCROLL_TOP_ATTRIBUTE, NodeData,
    discover_document_resources,
};
use offprint_model::{
    ArtifactManifest, ContentDigest, ResourceOutcome, Result, StructuralRepairTree, ViewState,
};
use url::Url;

use crate::csp::sandboxed_content_security_policy;
use crate::{
    REPAIR_DATA_ELEMENT_ID, REPAIR_MARKER_ATTRIBUTE, REPAIR_MEDIA_TYPE, REPAIR_SCRIPT_ELEMENT_ID,
    RESTORATION_SCRIPT, STATE_RESTORATION_SCRIPT, STATE_SCRIPT_ELEMENT_ID, content_security_policy,
    state_restoration_script_digest, structural_repair_script_digest, validate_repair_tree,
};

use super::active_content::validate_active_content;
use super::manifest::parse_scroll_coordinate;
use super::resource::{normalize_media_type, validate_embedded_resource};
use super::{attribute_value, text_contents, verification_error};

const FRAME_ID_ATTRIBUTE: &str = "data-offprint-frame-id";
const FRAME_BASE_ATTRIBUTE: &str = "data-offprint-frame-base";
const CSS_BASE_ATTRIBUTE: &str = "data-offprint-css-base";

pub(super) fn validate_csp(
    document: &Document,
    manifest: &ArtifactManifest,
    restore_state: bool,
) -> Result<()> {
    let policies = document
        .walk()
        .filter_map(|id| {
            let node = document.node(id)?;
            let NodeData::Element { name, attrs, .. } = &node.data else {
                return None;
            };
            if name.ns == ns!(html)
                && name.local == local_name!("meta")
                && attribute_value(attrs, "id") == Some(crate::CSP_ELEMENT_ID)
                && attribute_value(attrs, "http-equiv")
                    .is_some_and(|value| value.eq_ignore_ascii_case("content-security-policy"))
                && attrs.len() == 3
            {
                attribute_value(attrs, "content").map(|content| (id, content.to_owned()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let expected = if restore_state {
        content_security_policy(manifest)
    } else {
        sandboxed_content_security_policy(manifest)
    };
    let head = document.find_html_element("head");
    let canonical = match (head, policies.as_slice()) {
        (Some(head), [(policy, content)]) => {
            document.node(*policy).and_then(|node| node.parent) == Some(head)
                && document
                    .node(head)
                    .and_then(|node| node.children.first())
                    .copied()
                    == Some(*policy)
                && content == &expected
        }
        _ => false,
    };
    if !canonical {
        return Err(verification_error(
            "offprint.verification.csp",
            "artifact content security policy is missing, misplaced, or inconsistent",
        ));
    }
    Ok(())
}

pub(super) fn validate_artifact_documents(
    document: &Document,
    manifest: &ArtifactManifest,
    restore_state: bool,
) -> Result<()> {
    let base_url = Url::parse("file:///offprint-artifact.html").map_err(|error| {
        verification_error(
            "offprint.verification.reference",
            format!("verification base URL is invalid: {error}"),
        )
    })?;
    let mut pending = vec![document.clone()];
    let mut frame_count = 1_u32;
    let mut frame_ids = BTreeSet::from([1_u64]);
    let mut resource_references = 0_u32;
    let mut embedded_resources = BTreeMap::<(ContentDigest, String, u64), u32>::new();
    let mut repaired_documents = 0_u32;
    let mut state_scripts = 0_u32;
    let mut top_document = true;

    while let Some(document) = pending.pop() {
        validate_no_css_base_metadata(&document)?;
        if validate_structural_repair(&document, manifest)? {
            repaired_documents = repaired_documents.saturating_add(1);
        }
        let has_state_restoration = validate_state_restoration(&document)?;
        let expects_state_restoration = top_document && restore_state;
        if expects_state_restoration != has_state_restoration {
            return Err(verification_error(
                "offprint.verification.active_content",
                "state restoration must be owned by the top artifact document",
            ));
        }
        if has_state_restoration {
            state_scripts = state_scripts.saturating_add(1);
        }
        validate_scroll_attributes(&document, top_document.then_some(&manifest.view_state))?;
        top_document = false;
        validate_active_content(&document)?;
        collect_frames(
            &document,
            manifest.frames,
            &mut frame_count,
            &mut frame_ids,
            &mut pending,
        )?;
        let resources = discover_document_resources(&document, &base_url).map_err(|error| {
            verification_error(
                "offprint.verification.reference",
                "artifact contains an invalid rendering resource reference",
            )
            .with_source(error)
        })?;
        for resource in resources.resources() {
            resource_references = resource_references.checked_add(1).ok_or_else(|| {
                verification_error(
                    "offprint.verification.resource_summary",
                    "artifact resource reference count exceeds the supported range",
                )
            })?;
            validate_embedded_resource(
                &resource.resolved_url,
                &base_url,
                &mut resource_references,
                &mut embedded_resources,
                0,
            )?;
        }
    }

    if frame_count != manifest.frames {
        return Err(verification_error(
            "offprint.verification.frames",
            "artifact frame count does not match its manifest",
        ));
    }
    let expected_frame_ids = (1..=u64::from(manifest.frames)).collect::<BTreeSet<_>>();
    if frame_ids != expected_frame_ids {
        return Err(verification_error(
            "offprint.verification.frames",
            "artifact frame identifiers are incomplete or duplicated",
        ));
    }
    if manifest.structural_repair.applied != (repaired_documents > 0) {
        return Err(verification_error(
            "offprint.verification.structural_repair",
            "artifact structural repair records do not match the manifest",
        ));
    }
    if state_scripts != u32::from(restore_state) {
        return Err(verification_error(
            "offprint.verification.active_content",
            "artifact state restoration does not match its execution policy",
        ));
    }
    if resource_references != manifest.resources.discovered {
        return Err(verification_error(
            "offprint.verification.resource_summary",
            "artifact resource reference count does not match its manifest",
        ));
    }
    let mut expected_embedded = BTreeMap::<(ContentDigest, String, u64), u32>::new();
    for record in &manifest.resource_records {
        if let ResourceOutcome::Embedded {
            digest,
            media_type,
            bytes,
        } = &record.outcome
        {
            let key = (*digest, normalize_media_type(media_type), *bytes);
            *expected_embedded.entry(key).or_default() += 1;
        }
    }
    for (key, expected) in expected_embedded {
        let observed = embedded_resources.get_mut(&key).ok_or_else(|| {
            verification_error(
                "offprint.verification.embedded_resource",
                "embedded resource bytes do not match their manifest digest records",
            )
        })?;
        if *observed < expected {
            return Err(verification_error(
                "offprint.verification.embedded_resource",
                "embedded resource bytes do not match their manifest digest records",
            ));
        }
        *observed -= expected;
    }
    embedded_resources.retain(|_, count| *count > 0);
    let fallback_count = embedded_resources.iter().try_fold(
        0_u32,
        |count, ((digest, media_type, bytes), occurrences)| {
            if !crate::fallback::is_empty_resource_fallback(*digest, media_type, *bytes) {
                return Err(verification_error(
                    "offprint.verification.embedded_resource",
                    "artifact contains embedded bytes with no resource outcome",
                ));
            }
            count.checked_add(*occurrences).ok_or_else(|| {
                verification_error(
                    "offprint.verification.embedded_resource",
                    "embedded fallback count exceeds the supported range",
                )
            })
        },
    )?;
    if fallback_count != manifest.resources.failed {
        return Err(verification_error(
            "offprint.verification.embedded_resource",
            "embedded fallbacks do not match failed resource outcomes",
        ));
    }
    Ok(())
}

fn validate_no_css_base_metadata(document: &Document) -> Result<()> {
    for id in document.walk() {
        let Some(NodeData::Element { attrs, .. }) = document.node(id).map(|node| &node.data) else {
            continue;
        };
        if attribute_value(attrs, CSS_BASE_ATTRIBUTE).is_some() {
            return Err(verification_error(
                "offprint.verification.reference",
                "artifact contains transient stylesheet collection metadata",
            ));
        }
    }
    Ok(())
}

fn collect_frames(
    document: &Document,
    manifest_frames: u32,
    frame_count: &mut u32,
    frame_ids: &mut BTreeSet<u64>,
    pending: &mut Vec<Document>,
) -> Result<()> {
    for id in document.walk() {
        let Some(NodeData::Element { name, attrs, .. }) = document.node(id).map(|node| &node.data)
        else {
            continue;
        };
        if name.ns != ns!(html) || !matches!(name.local.as_ref(), "iframe" | "frame") {
            continue;
        }
        if attribute_value(attrs, FRAME_BASE_ATTRIBUTE).is_some() {
            return Err(verification_error(
                "offprint.verification.frames",
                "artifact contains transient frame collection metadata",
            ));
        }
        if attribute_value(attrs, "src").is_some() {
            return Err(verification_error(
                "offprint.verification.frames",
                "captured frames must use embedded document content",
            )
            .with_detail(
                "hasFrameId",
                attribute_value(attrs, FRAME_ID_ATTRIBUTE).is_some(),
            )
            .with_detail("hasSrcdoc", attribute_value(attrs, "srcdoc").is_some()));
        }
        let source = attribute_value(attrs, "srcdoc").ok_or_else(|| {
            verification_error(
                "offprint.verification.frames",
                "captured frame has no embedded document content",
            )
        })?;
        let frame_id = attribute_value(attrs, FRAME_ID_ATTRIBUTE)
            .ok_or_else(|| {
                verification_error(
                    "offprint.verification.frames",
                    "captured frame has no stable frame identifier",
                )
            })?
            .parse::<u64>()
            .map_err(|error| {
                verification_error(
                    "offprint.verification.frames",
                    format!("captured frame identifier is invalid: {error}"),
                )
            })?;
        if frame_id < 2 || !frame_ids.insert(frame_id) {
            return Err(verification_error(
                "offprint.verification.frames",
                "captured frame identifier is invalid or duplicated",
            ));
        }
        *frame_count = frame_count.checked_add(1).ok_or_else(|| {
            verification_error(
                "offprint.verification.frames",
                "artifact frame count exceeds the supported range",
            )
        })?;
        if *frame_count > manifest_frames {
            return Err(verification_error(
                "offprint.verification.frames",
                "artifact contains more frames than its manifest records",
            ));
        }
        pending.push(Document::parse(source.as_bytes()));
    }
    Ok(())
}

fn validate_scroll_attributes(document: &Document, expected: Option<&ViewState>) -> Result<()> {
    for id in document.walk() {
        let Some(NodeData::Element { attrs, .. }) = document.node(id).map(|node| &node.data) else {
            continue;
        };
        for attribute in attrs {
            if attribute.name.ns == ns!()
                && matches!(
                    attribute.name.local.as_ref(),
                    DOCUMENT_SCROLL_X_ATTRIBUTE
                        | DOCUMENT_SCROLL_Y_ATTRIBUTE
                        | ELEMENT_SCROLL_LEFT_ATTRIBUTE
                        | ELEMENT_SCROLL_TOP_ATTRIBUTE
                )
            {
                parse_scroll_coordinate(attribute.value.as_ref())?;
            }
        }
    }
    let Some(expected) = expected else {
        return Ok(());
    };
    let root = document
        .find_html_element("html")
        .and_then(|id| document.node(id))
        .and_then(|node| match &node.data {
            NodeData::Element { attrs, .. } => Some(attrs),
            _ => None,
        })
        .ok_or_else(|| {
            verification_error(
                "offprint.verification.manifest",
                "artifact has no HTML root for captured view state",
            )
        })?;
    let observed_x = attribute_value(root, DOCUMENT_SCROLL_X_ATTRIBUTE)
        .ok_or_else(|| {
            verification_error(
                "offprint.verification.manifest",
                "artifact root has no horizontal scroll state",
            )
        })
        .and_then(parse_scroll_coordinate)?;
    let observed_y = attribute_value(root, DOCUMENT_SCROLL_Y_ATTRIBUTE)
        .ok_or_else(|| {
            verification_error(
                "offprint.verification.manifest",
                "artifact root has no vertical scroll state",
            )
        })
        .and_then(parse_scroll_coordinate)?;
    if observed_x != parse_scroll_coordinate(&expected.scroll_x)?
        || observed_y != parse_scroll_coordinate(&expected.scroll_y)?
    {
        return Err(verification_error(
            "offprint.verification.manifest",
            "artifact root scroll state differs from its manifest",
        ));
    }
    Ok(())
}

fn validate_state_restoration(document: &Document) -> Result<bool> {
    let scripts = document
        .walk()
        .filter(|id| {
            matches!(
                document.node(*id).map(|node| &node.data),
                Some(NodeData::Element { name, attrs, .. })
                    if name.ns == ns!(html)
                        && name.local == local_name!("script")
                        && attribute_value(attrs, "id") == Some(STATE_SCRIPT_ELEMENT_ID)
            )
        })
        .collect::<Vec<_>>();
    if scripts.is_empty() {
        return Ok(false);
    }
    if scripts.len() != 1 {
        return Err(verification_error(
            "offprint.verification.active_content",
            "artifact contains duplicate state restoration programs",
        ));
    }
    let canonical = document.node(scripts[0]).is_some_and(
        |node| matches!(&node.data, NodeData::Element { attrs, .. } if attrs.len() == 1),
    );
    if !canonical || !is_direct_head_child(document, scripts[0]) {
        return Err(verification_error(
            "offprint.verification.active_content",
            "artifact state restoration program is outside the document head",
        ));
    }
    let script = text_contents(document, scripts[0]);
    if script != STATE_RESTORATION_SCRIPT
        || ContentDigest::sha256(script.as_bytes()) != state_restoration_script_digest()
    {
        return Err(verification_error(
            "offprint.verification.active_content",
            "artifact state restoration program does not match the artifact format",
        ));
    }
    Ok(true)
}

fn validate_structural_repair(document: &Document, manifest: &ArtifactManifest) -> Result<bool> {
    let data = document
        .walk()
        .filter(|id| {
            matches!(
                document.node(*id).map(|node| &node.data),
                Some(NodeData::Element { name, attrs, .. })
                    if name.ns == ns!(html)
                        && name.local == local_name!("script")
                        && attribute_value(attrs, "id") == Some(REPAIR_DATA_ELEMENT_ID)
                        && attribute_value(attrs, "type") == Some(REPAIR_MEDIA_TYPE)
            )
        })
        .collect::<Vec<_>>();
    let scripts = document
        .walk()
        .filter(|id| {
            matches!(
                document.node(*id).map(|node| &node.data),
                Some(NodeData::Element { name, attrs, .. })
                    if name.ns == ns!(html)
                        && name.local == local_name!("script")
                        && attribute_value(attrs, "id") == Some(REPAIR_SCRIPT_ELEMENT_ID)
            )
        })
        .collect::<Vec<_>>();
    if data.is_empty() && scripts.is_empty() {
        if document.walk().any(|id| {
            matches!(
                document.node(id).map(|node| &node.data),
                Some(NodeData::Element { attrs, .. })
                    if attribute_value(attrs, REPAIR_MARKER_ATTRIBUTE).is_some()
            )
        }) {
            return Err(verification_error(
                "offprint.verification.structural_repair",
                "artifact contains structural markers without repair data",
            ));
        }
        return Ok(false);
    }
    if data.len() != 1 || scripts.len() != 1 || !manifest.structural_repair.applied {
        return Err(verification_error(
            "offprint.verification.structural_repair",
            "structural repair requires one data record and one owned script",
        ));
    }
    let canonical_data = document.node(data[0]).is_some_and(
        |node| matches!(&node.data, NodeData::Element { attrs, .. } if attrs.len() == 2),
    );
    let canonical_script = document.node(scripts[0]).is_some_and(
        |node| matches!(&node.data, NodeData::Element { attrs, .. } if attrs.len() == 1),
    );
    if !canonical_data
        || !canonical_script
        || !is_direct_head_child(document, data[0])
        || !is_direct_head_child(document, scripts[0])
    {
        return Err(verification_error(
            "offprint.verification.structural_repair",
            "structural repair records must be direct children of the document head",
        ));
    }
    let script = text_contents(document, scripts[0]);
    let expected_digest = structural_repair_script_digest();
    if script != RESTORATION_SCRIPT
        || ContentDigest::sha256(script.as_bytes()) != expected_digest
        || manifest.structural_repair.script_sha256 != Some(expected_digest)
    {
        return Err(verification_error(
            "offprint.verification.structural_repair",
            "structural repair script does not match its manifest digest",
        ));
    }
    let json = text_contents(document, data[0]);
    let repair = serde_json::from_str::<StructuralRepairTree>(&json).map_err(|error| {
        verification_error(
            "offprint.verification.structural_repair",
            format!("structural repair data is malformed: {error}"),
        )
    })?;
    validate_repair_tree(&repair).map_err(|error| {
        verification_error("offprint.verification.structural_repair", error.message)
    })?;
    let mut markers = BTreeSet::new();
    for id in document.walk() {
        let Some(NodeData::Element { attrs, .. }) = document.node(id).map(|node| &node.data) else {
            continue;
        };
        if let Some(marker) = attribute_value(attrs, REPAIR_MARKER_ATTRIBUTE)
            && !markers.insert(marker)
        {
            return Err(verification_error(
                "offprint.verification.structural_repair",
                "artifact structural repair markers are duplicated",
            ));
        }
    }
    if !markers.contains("0") {
        return Err(verification_error(
            "offprint.verification.structural_repair",
            "artifact structural repair root marker is missing",
        ));
    }
    Ok(true)
}

fn is_direct_head_child(document: &Document, id: offprint_model::NodeId) -> bool {
    let Some(head) = document.find_html_element("head") else {
        return false;
    };
    document.node(id).and_then(|node| node.parent) == Some(head)
}

#[cfg(test)]
mod placement_tests {
    use offprint_document::Document;

    use super::validate_csp;
    use crate::content_security_policy;
    use crate::verify::test_manifest;

    #[test]
    fn content_security_policy_must_be_the_first_head_child() {
        let manifest = test_manifest();
        let policy = content_security_policy(&manifest);
        let misplaced = Document::parse(
            format!(
                r#"<html><head><title>x</title><meta http-equiv="Content-Security-Policy" content="{policy}"></head><body></body></html>"#
            )
            .as_bytes(),
        );
        let inactive = Document::parse(
            format!(
                r#"<html><head></head><body><template><meta http-equiv="Content-Security-Policy" content="{policy}"></template></body></html>"#
            )
            .as_bytes(),
        );

        assert_eq!(
            validate_csp(&misplaced, &manifest, true)
                .err()
                .map(|error| error.code.as_str().to_owned()),
            Some("offprint.verification.csp".to_owned())
        );
        assert_eq!(
            validate_csp(&inactive, &manifest, true)
                .err()
                .map(|error| error.code.as_str().to_owned()),
            Some("offprint.verification.csp".to_owned())
        );
    }
}
