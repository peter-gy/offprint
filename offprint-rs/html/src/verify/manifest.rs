use std::collections::BTreeMap;

use markup5ever::{local_name, ns};
use offprint_document::{Document, NodeData};
use offprint_model::{
    ArtifactManifest, ContentDigest, ResourceOutcome, ResourceSummary, Result, ViewState,
};

use crate::{MANIFEST_ELEMENT_ID, MANIFEST_MEDIA_TYPE, structural_repair_script_digest};

use super::{attribute_value, text_contents, verification_error};

const MAXIMUM_SCROLL_OFFSET: f64 = 1_000_000_000.0;

pub fn inspect_html(bytes: &[u8]) -> Result<ArtifactManifest> {
    let document = Document::parse(bytes);
    inspect_document(&document)
}

pub(super) fn inspect_document(document: &Document) -> Result<ArtifactManifest> {
    let manifest_nodes = document
        .walk()
        .filter(|id| {
            matches!(
                document.node(*id).map(|node| &node.data),
                Some(NodeData::Element { name, attrs, .. })
                    if name.ns == ns!(html)
                        && name.local == local_name!("script")
                        && attribute_value(attrs, "id") == Some(MANIFEST_ELEMENT_ID)
                        && attribute_value(attrs, "type") == Some(MANIFEST_MEDIA_TYPE)
            )
        })
        .collect::<Vec<_>>();
    if manifest_nodes.len() != 1 {
        return Err(verification_error(
            "offprint.verification.manifest",
            "artifact must contain one Offprint manifest",
        ));
    }
    let head = document.find_html_element("head");
    if head.is_none()
        || document
            .node(manifest_nodes[0])
            .and_then(|node| node.parent)
            != head
        || !document.node(manifest_nodes[0]).is_some_and(
            |node| matches!(&node.data, NodeData::Element { attrs, .. } if attrs.len() == 2),
        )
    {
        return Err(verification_error(
            "offprint.verification.manifest",
            "artifact manifest must be a direct child of the document head",
        ));
    }
    let json = text_contents(document, manifest_nodes[0]);
    let manifest = serde_json::from_str::<ArtifactManifest>(&json).map_err(|error| {
        verification_error(
            "offprint.verification.manifest",
            format!("artifact manifest is malformed: {error}"),
        )
    })?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &ArtifactManifest) -> Result<()> {
    if manifest.schema_version != offprint_model::PUBLIC_SCHEMA_VERSION
        || manifest.format_version != offprint_model::ARTIFACT_FORMAT_VERSION
    {
        return Err(verification_error(
            "offprint.verification.format",
            "artifact format or schema version is incompatible",
        ));
    }
    if !manifest.resources.is_complete() {
        return Err(verification_error(
            "offprint.verification.resource_summary",
            "manifest does not account for every discovered resource",
        ));
    }
    validate_resource_records(manifest)?;
    if manifest.resources.external != 0 {
        return Err(verification_error(
            "offprint.verification.resource_summary",
            "safe-static artifacts cannot retain external rendering resources",
        ));
    }
    if manifest.frames == 0 {
        return Err(verification_error(
            "offprint.verification.frames",
            "artifact manifest must account for the top frame",
        ));
    }
    validate_view_state(&manifest.view_state)?;
    if manifest.structural_repair.applied != manifest.structural_repair.script_sha256.is_some() {
        return Err(verification_error(
            "offprint.verification.structural_repair",
            "structural repair state and script digest are inconsistent",
        ));
    }
    if manifest.structural_repair.applied
        && manifest.structural_repair.script_sha256 != Some(structural_repair_script_digest())
    {
        return Err(verification_error(
            "offprint.verification.structural_repair",
            "structural repair script digest does not match the artifact format",
        ));
    }
    Ok(())
}

fn validate_resource_records(manifest: &ArtifactManifest) -> Result<()> {
    let record_count = u32::try_from(manifest.resource_records.len()).map_err(|error| {
        verification_error(
            "offprint.verification.resource_summary",
            format!("manifest resource record count exceeds the supported range: {error}"),
        )
    })?;
    if record_count != manifest.resources.discovered {
        return Err(verification_error(
            "offprint.verification.resource_summary",
            "manifest resource record count does not match discovered resources",
        ));
    }
    let mut summary = ResourceSummary {
        discovered: record_count,
        ..ResourceSummary::default()
    };
    let mut unique_embedded = BTreeMap::<ContentDigest, u64>::new();
    for (index, record) in manifest.resource_records.iter().enumerate() {
        let expected_id = u32::try_from(index).map_err(|error| {
            verification_error(
                "offprint.verification.resource_summary",
                format!("manifest resource identifier exceeds the supported range: {error}"),
            )
        })?;
        if record.id.get() != expected_id
            || record.frame_id.get() == 0
            || record.frame_id.get() > u64::from(manifest.frames)
        {
            return Err(verification_error(
                "offprint.verification.resource_summary",
                "manifest resource identifiers or frame ownership are inconsistent",
            ));
        }
        match &record.outcome {
            ResourceOutcome::Embedded {
                digest,
                media_type: _,
                bytes,
            } => {
                if record.provenance.is_none() {
                    return Err(verification_error(
                        "offprint.verification.resource_provenance",
                        "embedded resource record has no retrieval provenance",
                    ));
                }
                summary.embedded = summary.embedded.saturating_add(1);
                if let Some(previous) = unique_embedded.insert(*digest, *bytes)
                    && previous != *bytes
                {
                    return Err(verification_error(
                        "offprint.verification.embedded_resource",
                        "one resource digest is associated with conflicting byte lengths",
                    ));
                }
            }
            ResourceOutcome::External { .. } => {
                summary.external = summary.external.saturating_add(1);
            }
            ResourceOutcome::Omitted { .. } => {
                summary.omitted = summary.omitted.saturating_add(1);
            }
            ResourceOutcome::Failed { .. } => {
                summary.failed = summary.failed.saturating_add(1);
            }
        }
    }
    summary.embedded_bytes = unique_embedded
        .values()
        .try_fold(0_u64, |total, bytes| total.checked_add(*bytes))
        .ok_or_else(|| {
            verification_error(
                "offprint.verification.resource_summary",
                "manifest embedded resource byte count overflowed",
            )
        })?;
    if summary != manifest.resources {
        return Err(verification_error(
            "offprint.verification.resource_summary",
            "manifest resource records do not match the resource summary",
        ));
    }
    Ok(())
}

fn validate_view_state(view_state: &ViewState) -> Result<()> {
    parse_scroll_coordinate(&view_state.scroll_x)?;
    parse_scroll_coordinate(&view_state.scroll_y)?;
    Ok(())
}

pub(super) fn parse_scroll_coordinate(value: &str) -> Result<f64> {
    let parsed = value.parse::<f64>().map_err(|error| {
        verification_error(
            "offprint.verification.manifest",
            format!("captured scroll state is invalid: {error}"),
        )
    })?;
    if !parsed.is_finite() || parsed.abs() > MAXIMUM_SCROLL_OFFSET {
        return Err(verification_error(
            "offprint.verification.manifest",
            "captured scroll state exceeds the supported range",
        ));
    }
    Ok(parsed)
}
