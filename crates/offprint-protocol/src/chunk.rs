use offprint_model::{CaptureId, ErrorStage, FrameId, OffprintError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkEnvelope {
    pub capture_id: CaptureId,
    pub frame_id: FrameId,
    pub sequence: u32,
    pub total: u32,
    pub payload_length: u64,
    pub payload_crc32: u32,
    pub payload: Vec<u8>,
}

impl ChunkEnvelope {
    #[must_use]
    pub fn new(
        capture_id: CaptureId,
        frame_id: FrameId,
        sequence: u32,
        total: u32,
        payload: Vec<u8>,
    ) -> Self {
        let payload_length = u64::try_from(payload.len()).unwrap_or(u64::MAX);
        let payload_crc32 = crc32fast::hash(&payload);
        Self {
            capture_id,
            frame_id,
            sequence,
            total,
            payload_length,
            payload_crc32,
            payload,
        }
    }

    pub fn validate(&self, maximum_chunk_bytes: u64) -> Result<()> {
        if self.total == 0 || self.sequence >= self.total {
            return Err(protocol_error(
                "offprint.collector.chunk_sequence",
                "collector chunk sequence is outside the declared range",
            ));
        }
        if self.payload_length != u64::try_from(self.payload.len()).unwrap_or(u64::MAX) {
            return Err(protocol_error(
                "offprint.collector.chunk_length",
                "collector chunk payload length does not match its envelope",
            ));
        }
        if self.payload_length > maximum_chunk_bytes {
            return Err(protocol_error(
                "offprint.collector.chunk_limit",
                "collector chunk exceeds the negotiated byte limit",
            ));
        }
        if self.payload_crc32 != crc32fast::hash(&self.payload) {
            return Err(protocol_error(
                "offprint.collector.chunk_checksum",
                "collector chunk checksum does not match its payload",
            ));
        }
        Ok(())
    }
}

fn protocol_error(code: &'static str, message: &'static str) -> OffprintError {
    OffprintError::new(code, ErrorStage::Collection, message)
}

#[cfg(test)]
mod tests {
    use offprint_model::{CaptureId, FrameId};

    use super::ChunkEnvelope;

    #[test]
    fn envelope_validates_its_declared_length_and_checksum() {
        let capture_id = CaptureId::new();
        let frame_id = FrameId::new(1);
        let mut envelope = ChunkEnvelope::new(capture_id, frame_id, 0, 1, b"offprint".to_vec());

        assert!(envelope.validate(8).is_ok());

        envelope.payload[0] = b'P';
        assert_eq!(
            envelope
                .validate(8)
                .as_ref()
                .map_err(|error| error.code.as_str()),
            Err("offprint.collector.chunk_checksum")
        );
    }

    #[test]
    fn envelope_rejects_bytes_above_the_negotiated_limit() {
        let capture_id = CaptureId::new();
        let frame_id = FrameId::new(1);
        let envelope = ChunkEnvelope::new(capture_id, frame_id, 0, 1, b"offprint".to_vec());
        let result = envelope.validate(7);

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.collector.chunk_limit")
        );
    }
}
