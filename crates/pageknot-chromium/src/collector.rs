use std::collections::BTreeSet;

use pageknot_browser::{CollectedPageObservation, CollectorLimits};
use pageknot_model::{
    CaptureId, CapturePolicy, ContentDigest, ErrorCode, ErrorStage, FrameId, MAXIMUM_CAPTURE_NODES,
    PageKnotError, Result,
};
use pageknot_protocol::{
    COLLECTOR_PROTOCOL_VERSION, COLLECTOR_PROTOCOL_VERSION_STRING, ChunkAssembler, ChunkEnvelope,
    CollectorCapability, CollectorCommand, CollectorErrorResponse, CollectorHandshake,
    CollectorMessage, FrameOwnerObservation, ObservationDescriptor, PageObservation, TransferPlan,
    negotiate_protocol,
};
use serde_json::{Value, json};

use crate::ChromiumPage;

pub const COLLECTOR_BUNDLE: &str = include_str!("../generated/collector.js");

pub async fn collect_page_observation(
    page: &ChromiumPage,
    capture_id: &CaptureId,
    limits: CollectorLimits,
    capture_policy: &CapturePolicy,
) -> Result<PageObservation> {
    Ok(collect_frame_observation_with_metadata(
        page,
        page.session_id(),
        FrameId::new(1),
        capture_id,
        limits,
        capture_policy,
    )
    .await?
    .observation)
}

pub async fn collect_frame_observation(
    page: &ChromiumPage,
    session_id: &str,
    frame_id: FrameId,
    capture_id: &CaptureId,
    limits: CollectorLimits,
    capture_policy: &CapturePolicy,
) -> Result<PageObservation> {
    Ok(collect_frame_observation_with_metadata(
        page,
        session_id,
        frame_id,
        capture_id,
        limits,
        capture_policy,
    )
    .await?
    .observation)
}

pub(crate) async fn collect_frame_observation_with_metadata(
    page: &ChromiumPage,
    session_id: &str,
    frame_id: FrameId,
    capture_id: &CaptureId,
    limits: CollectorLimits,
    capture_policy: &CapturePolicy,
) -> Result<CollectedPageObservation> {
    validate_maximum_nodes(limits.maximum_nodes)?;
    let handshake =
        probe_collector_handshake(page, session_id, capture_id, limits.maximum_chunk_bytes).await?;
    let negotiated = negotiate_protocol(
        &handshake,
        COLLECTOR_PROTOCOL_VERSION,
        limits.maximum_chunk_bytes,
    )?;

    let prepare = CollectorCommand::Prepare {
        capture_id: capture_id.clone(),
        frame_id,
        frame_depth: limits.frame_depth,
        maximum_chunk_bytes: negotiated.chunk_bytes,
        maximum_frame_depth: limits.maximum_frame_depth,
        maximum_frames: limits.maximum_frames,
        maximum_nodes: limits.maximum_nodes,
        maximum_payload_bytes: limits.maximum_payload_bytes,
        preserve_password_values: capture_policy.preserve_password_values,
        capture_scope: capture_policy.scope,
        selector: (limits.frame_depth == 0)
            .then(|| capture_policy.selector.clone())
            .flatten(),
        remove_unused_css: capture_policy.optimizations.remove_unused_css,
        remove_unused_fonts: capture_policy.optimizations.remove_unused_fonts,
        remove_hidden_elements: capture_policy.optimizations.remove_hidden_elements,
    };
    let descriptor_value = page
        .collector_call_in_session(session_id, "prepare", json!([prepare]))
        .await?;
    if let Some(error) = collector_protocol_error(&descriptor_value, capture_id)? {
        return Err(error);
    }
    let descriptor: ObservationDescriptor =
        serde_json::from_value(descriptor_value).map_err(|error| {
            collector_shape_error(format!("collector descriptor is malformed: {error}"))
        })?;
    let transfer = TransferPlan::new(
        descriptor,
        capture_id,
        frame_id,
        negotiated.chunk_bytes,
        limits.maximum_payload_bytes,
    )?;
    let sequences = transfer.sequences();
    let encoded_bytes = transfer.encoded_bytes();
    let mut assembler = ChunkAssembler::new(transfer);
    let mut complete = None;
    let collection = async {
        for sequence in sequences {
            let chunk_value = page
                .collector_call_in_session(
                    session_id,
                    "read",
                    json!([capture_id, frame_id, sequence]),
                )
                .await?;
            let chunk: ChunkEnvelope = serde_json::from_value(chunk_value).map_err(|error| {
                collector_shape_error(format!("collector chunk is malformed: {error}"))
            })?;
            complete = assembler.push(chunk)?;
            let acknowledged = page
                .collector_call_in_session(
                    session_id,
                    "acknowledge",
                    json!([capture_id, frame_id, sequence]),
                )
                .await?;
            if acknowledged != Value::Bool(true) {
                return Err(collector_shape_error(
                    "collector did not acknowledge a consumed chunk",
                ));
            }
        }
        let payload = complete.take().ok_or_else(|| {
            collector_shape_error("collector ended before its payload was complete")
        })?;
        let observation = serde_json::from_slice::<PageObservation>(&payload).map_err(|error| {
            collector_shape_error(format!("collector observation is malformed: {error}"))
        })?;
        validate_frame_owner_paths(&observation.frame_owners)?;
        Ok((observation, encoded_bytes))
    }
    .await;
    let release = page
        .collector_call_in_session(session_id, "release", json!([capture_id, frame_id]))
        .await;
    match (collection, release) {
        (Ok((observation, encoded_bytes)), Ok(_)) => Ok(CollectedPageObservation {
            observation,
            encoded_bytes,
        }),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn validate_frame_owner_paths(owners: &[FrameOwnerObservation]) -> Result<()> {
    let mut original_paths = BTreeSet::new();
    let mut retained_paths = BTreeSet::new();
    for owner in owners {
        if owner.original_path.is_empty()
            || owner.retained_path.is_empty()
            || owner.original_path.len() != owner.retained_path.len()
        {
            return Err(collector_shape_error(
                "collector frame owner paths must be non-empty and have matching depths",
            ));
        }
        if !original_paths.insert(owner.original_path.as_slice())
            || !retained_paths.insert(owner.retained_path.as_slice())
        {
            return Err(collector_shape_error(
                "collector frame owner paths must be unique",
            ));
        }
    }
    Ok(())
}

fn validate_maximum_nodes(maximum_nodes: u64) -> Result<()> {
    if maximum_nodes <= MAXIMUM_CAPTURE_NODES {
        return Ok(());
    }
    Err(PageKnotError::new(
        "pageknot.input.limit",
        ErrorStage::Validation,
        "capture DOM node limit exceeds the supported maximum",
    )
    .with_detail("attempted", maximum_nodes)
    .with_detail("limit", MAXIMUM_CAPTURE_NODES))
}

pub async fn probe_collector_handshake(
    page: &ChromiumPage,
    session_id: &str,
    capture_id: &CaptureId,
    maximum_chunk_bytes: u64,
) -> Result<CollectorHandshake> {
    let requested_capabilities = requested_capabilities();
    let handshake_value = page
        .collector_call_in_session(
            session_id,
            "handshake",
            json!([
                capture_id,
                host_build_digest(),
                requested_capabilities,
                maximum_chunk_bytes,
            ]),
        )
        .await?;
    let handshake: CollectorHandshake =
        serde_json::from_value(handshake_value).map_err(|error| {
            collector_shape_error(format!("collector handshake is malformed: {error}"))
        })?;
    if handshake.capture_id != *capture_id {
        return Err(collector_shape_error(
            "collector handshake belongs to another capture",
        ));
    }
    if handshake.host_build_sha256 != host_build_digest() {
        return Err(collector_shape_error(
            "collector handshake did not echo the host build digest",
        ));
    }
    if handshake.requested_capabilities != requested_capabilities {
        return Err(collector_shape_error(
            "collector handshake changed the requested capabilities",
        ));
    }
    Ok(handshake)
}

fn requested_capabilities() -> BTreeSet<CollectorCapability> {
    BTreeSet::from(CollectorCapability::ALL)
}

fn host_build_digest() -> ContentDigest {
    let input = format!(
        "{}:{}:collector-protocol-{COLLECTOR_PROTOCOL_VERSION_STRING}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    );
    ContentDigest::sha256(input.as_bytes())
}

fn collector_protocol_error(
    value: &Value,
    expected_capture_id: &CaptureId,
) -> Result<Option<PageKnotError>> {
    if value.get("type").and_then(Value::as_str) != Some("error") {
        return Ok(None);
    }
    let response = serde_json::from_value::<CollectorMessage>(value.clone()).map_err(|error| {
        collector_shape_error(format!("collector error response is malformed: {error}"))
    })?;
    let CollectorMessage::Error(CollectorErrorResponse::Error(payload)) = response else {
        return Err(collector_shape_error(
            "collector returned a non-error response with an error tag",
        ));
    };
    if payload.capture_id != *expected_capture_id {
        return Err(collector_shape_error(
            "collector error response belongs to another capture",
        ));
    }
    let code = ErrorCode::new(payload.code)
        .map_err(|_| collector_shape_error("collector returned an invalid error code"))?;
    let details = payload
        .details
        .into_iter()
        .map(|(key, value)| (key, value.into()))
        .collect();
    Ok(Some(PageKnotError {
        code,
        message: payload.message,
        stage: ErrorStage::Collection,
        retryable: false,
        details,
        diagnostics_path: None,
        source: None,
    }))
}

fn collector_shape_error(message: impl Into<String>) -> PageKnotError {
    PageKnotError::new(
        "pageknot.collector.protocol_shape",
        ErrorStage::Collection,
        message,
    )
}

#[cfg(test)]
mod tests {
    use pageknot_model::{CaptureId, MAXIMUM_CAPTURE_NODES};
    use pageknot_protocol::FrameOwnerObservation;
    use serde_json::json;

    use super::{collector_protocol_error, validate_frame_owner_paths, validate_maximum_nodes};

    #[test]
    fn collector_node_limit_matches_the_public_capture_boundary() {
        assert!(validate_maximum_nodes(MAXIMUM_CAPTURE_NODES).is_ok());

        let rejection = validate_maximum_nodes(MAXIMUM_CAPTURE_NODES + 1);
        let error = rejection.as_ref().err();
        assert_eq!(
            error.map(|error| error.code.as_str()),
            Some("pageknot.input.limit")
        );
        assert_eq!(
            error.and_then(|error| error.details.get("attempted")),
            Some(&json!(MAXIMUM_CAPTURE_NODES + 1))
        );
        assert_eq!(
            error.and_then(|error| error.details.get("limit")),
            Some(&json!(MAXIMUM_CAPTURE_NODES))
        );
    }

    #[test]
    fn frame_owner_paths_are_nonempty_unique_and_depth_aligned() {
        let valid = [
            FrameOwnerObservation {
                original_path: vec![1, 2],
                retained_path: vec![0, 1],
            },
            FrameOwnerObservation {
                original_path: vec![2],
                retained_path: vec![1],
            },
        ];
        let duplicate = [
            valid[0].clone(),
            FrameOwnerObservation {
                original_path: vec![1, 2],
                retained_path: vec![1, 0],
            },
        ];
        let mismatched = [FrameOwnerObservation {
            original_path: vec![1, 2],
            retained_path: vec![0],
        }];

        assert!(validate_frame_owner_paths(&valid).is_ok());
        assert_eq!(
            validate_frame_owner_paths(&duplicate)
                .as_ref()
                .map_err(|error| error.code.as_str()),
            Err("pageknot.collector.protocol_shape")
        );
        assert_eq!(
            validate_frame_owner_paths(&mismatched)
                .as_ref()
                .map_err(|error| error.code.as_str()),
            Err("pageknot.collector.protocol_shape")
        );
    }

    #[test]
    fn structured_collector_error_preserves_the_payload_limit_code() {
        let capture_id = CaptureId::new();
        let response = json!({
            "type": "error",
            "payload": {
                "captureId": capture_id,
                "code": "pageknot.collector.payload_limit",
                "message": "collector payload exceeds the configured frame limit",
                "details": {
                    "limit": 4096,
                },
            }
        });

        let rejection = collector_protocol_error(&response, &capture_id);
        let error = rejection.as_ref().ok().and_then(Option::as_ref);

        assert_eq!(
            error.map(|error| error.code.as_str()),
            Some("pageknot.collector.payload_limit")
        );
        assert_eq!(
            error.and_then(|error| error.details.get("limit")),
            Some(&json!(4096))
        );
    }

    #[test]
    fn structured_collector_error_preserves_the_node_limit_details() {
        let capture_id = CaptureId::new();
        let response = json!({
            "type": "error",
            "payload": {
                "captureId": capture_id,
                "code": "pageknot.frame.nodes",
                "message": "captured frame exceeds the configured DOM node limit",
                "details": {
                    "attempted": 101,
                    "limit": 100,
                },
            }
        });

        let rejection = collector_protocol_error(&response, &capture_id);
        let error = rejection.as_ref().ok().and_then(Option::as_ref);

        assert_eq!(
            error.map(|error| error.code.as_str()),
            Some("pageknot.frame.nodes")
        );
        assert_eq!(
            error.and_then(|error| error.details.get("attempted")),
            Some(&json!(101))
        );
        assert_eq!(
            error.and_then(|error| error.details.get("limit")),
            Some(&json!(100))
        );
    }
}
