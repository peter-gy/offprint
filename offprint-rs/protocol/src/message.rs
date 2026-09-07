use std::collections::BTreeMap;

use offprint_model::{CaptureId, CaptureScope, ContentDigest, FrameId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ChunkEnvelope, CollectorHandshake};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationDescriptor {
    pub capture_id: CaptureId,
    pub frame_id: FrameId,
    pub chunks: u32,
    pub encoded_bytes: u64,
    pub payload_sha256: ContentDigest,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum CollectorCommand {
    Prepare {
        capture_id: CaptureId,
        frame_id: FrameId,
        frame_depth: u16,
        maximum_chunk_bytes: u64,
        maximum_frame_depth: u16,
        maximum_frames: u32,
        maximum_nodes: u64,
        maximum_payload_bytes: u64,
        preserve_password_values: bool,
        capture_scope: CaptureScope,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        remove_unused_css: bool,
        remove_unused_fonts: bool,
        remove_hidden_elements: bool,
    },
    Describe {
        capture_id: CaptureId,
        frame_id: FrameId,
    },
    Read {
        capture_id: CaptureId,
        frame_id: FrameId,
        sequence: u32,
    },
    Acknowledge {
        capture_id: CaptureId,
        frame_id: FrameId,
        sequence: u32,
    },
    Release {
        capture_id: CaptureId,
        frame_id: FrameId,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorRelease {
    pub capture_id: CaptureId,
    pub frame_id: FrameId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorErrorPayload {
    pub capture_id: CaptureId,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "kebab-case")]
pub enum CollectorErrorResponse {
    Error(CollectorErrorPayload),
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(untagged)]
pub enum CollectorMessage {
    Handshake(CollectorHandshake),
    Description(ObservationDescriptor),
    Chunk(ChunkEnvelope),
    Released(CollectorRelease),
    Acknowledged(bool),
    Error(CollectorErrorResponse),
}

#[cfg(test)]
mod tests {
    use offprint_model::{CaptureId, CaptureScope, FrameId};
    use serde_json::json;

    use super::{CollectorCommand, CollectorErrorResponse, CollectorMessage};

    #[test]
    fn prepare_command_carries_collector_allocation_limits() {
        let capture_id = CaptureId::new();
        let command = CollectorCommand::Prepare {
            capture_id: capture_id.clone(),
            frame_id: FrameId::new(7),
            frame_depth: 2,
            maximum_chunk_bytes: 1024,
            maximum_frame_depth: 8,
            maximum_frames: 16,
            maximum_nodes: 2048,
            maximum_payload_bytes: 4096,
            preserve_password_values: false,
            capture_scope: CaptureScope::Page,
            selector: Some("main article".to_owned()),
            remove_unused_css: false,
            remove_unused_fonts: false,
            remove_hidden_elements: false,
        };

        let encoded = serde_json::to_value(command);
        let expected = json!({
            "type": "prepare",
            "captureId": capture_id,
            "frameId": 7,
            "frameDepth": 2,
            "maximumChunkBytes": 1024,
            "maximumFrameDepth": 8,
            "maximumFrames": 16,
            "maximumNodes": 2048,
            "maximumPayloadBytes": 4096,
            "preservePasswordValues": false,
            "captureScope": "page",
            "selector": "main article",
            "removeUnusedCss": false,
            "removeUnusedFonts": false,
            "removeHiddenElements": false,
        });

        assert_eq!(encoded.as_ref().ok(), Some(&expected));
    }

    #[test]
    fn collector_response_fixture_round_trips_the_live_wire_shapes()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/collector-responses.json"))?;
        let Some(responses) = fixture.as_object() else {
            return Err(
                std::io::Error::other("collector response fixture must be an object").into(),
            );
        };

        for (name, value) in responses {
            let decoded: CollectorMessage =
                serde_json::from_value(value.clone()).map_err(|error| {
                    std::io::Error::other(format!("{name} response did not decode: {error}"))
                })?;
            let encoded = serde_json::to_value(&decoded).map_err(|error| {
                std::io::Error::other(format!("{name} response did not encode: {error}"))
            })?;
            assert_eq!(&encoded, value, "{name}");
        }

        assert!(matches!(
            serde_json::from_value::<CollectorMessage>(responses["error"].clone()),
            Ok(CollectorMessage::Error(CollectorErrorResponse::Error(_)))
        ));
        Ok(())
    }
}
