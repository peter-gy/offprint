use pageknot_model::{CaptureId, ErrorStage, FrameId, PageKnotError, Result};
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
                "pageknot.collector.chunk_sequence",
                "collector chunk sequence is outside the declared range",
            ));
        }
        if self.payload_length != u64::try_from(self.payload.len()).unwrap_or(u64::MAX) {
            return Err(protocol_error(
                "pageknot.collector.chunk_length",
                "collector chunk payload length does not match its envelope",
            ));
        }
        if self.payload_length > maximum_chunk_bytes {
            return Err(protocol_error(
                "pageknot.collector.chunk_limit",
                "collector chunk exceeds the negotiated byte limit",
            ));
        }
        if self.payload_crc32 != crc32fast::hash(&self.payload) {
            return Err(protocol_error(
                "pageknot.collector.chunk_checksum",
                "collector chunk checksum does not match its payload",
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct ChunkAssembler {
    capture_id: CaptureId,
    frame_id: FrameId,
    maximum_chunk_bytes: u64,
    maximum_payload_bytes: u64,
    total: Option<u32>,
    next_sequence: u32,
    payload: Vec<u8>,
}

impl ChunkAssembler {
    #[must_use]
    pub fn new(
        capture_id: CaptureId,
        frame_id: FrameId,
        maximum_chunk_bytes: u64,
        maximum_payload_bytes: u64,
    ) -> Self {
        Self {
            capture_id,
            frame_id,
            maximum_chunk_bytes,
            maximum_payload_bytes,
            total: None,
            next_sequence: 0,
            payload: Vec::new(),
        }
    }

    pub fn push(&mut self, chunk: ChunkEnvelope) -> Result<Option<Vec<u8>>> {
        chunk.validate(self.maximum_chunk_bytes)?;
        if chunk.capture_id != self.capture_id || chunk.frame_id != self.frame_id {
            return Err(protocol_error(
                "pageknot.collector.chunk_owner",
                "collector chunk belongs to another capture or frame",
            ));
        }
        if chunk.sequence != self.next_sequence {
            return Err(protocol_error(
                "pageknot.collector.chunk_sequence",
                "collector chunks must arrive in sequence",
            ));
        }
        match self.total {
            Some(total) if total != chunk.total => {
                return Err(protocol_error(
                    "pageknot.collector.chunk_total",
                    "collector changed the declared chunk count",
                ));
            }
            None => self.total = Some(chunk.total),
            Some(_) => {}
        }

        let next_bytes = u64::try_from(self.payload.len())
            .unwrap_or(u64::MAX)
            .saturating_add(chunk.payload_length);
        if next_bytes > self.maximum_payload_bytes {
            return Err(protocol_error(
                "pageknot.collector.payload_limit",
                "collector payload exceeds the configured frame limit",
            ));
        }

        self.payload.extend_from_slice(&chunk.payload);
        self.next_sequence += 1;

        if Some(self.next_sequence) == self.total {
            self.total = None;
            self.next_sequence = 0;
            return Ok(Some(std::mem::take(&mut self.payload)));
        }
        Ok(None)
    }
}

fn protocol_error(code: &'static str, message: &'static str) -> PageKnotError {
    PageKnotError::new(code, ErrorStage::Collection, message)
}

#[cfg(test)]
mod tests {
    use pageknot_model::{CaptureId, FrameId};
    use proptest::prelude::*;

    use super::{ChunkAssembler, ChunkEnvelope};

    #[test]
    fn assembler_reconstructs_a_bounded_payload_in_sequence() {
        let capture_id = CaptureId::new();
        let frame_id = FrameId::new(1);
        let mut assembler = ChunkAssembler::new(capture_id.clone(), frame_id, 4, 8);

        let first = assembler.push(ChunkEnvelope::new(
            capture_id.clone(),
            frame_id,
            0,
            2,
            b"page".to_vec(),
        ));
        let second = assembler.push(ChunkEnvelope::new(
            capture_id,
            frame_id,
            1,
            2,
            b"knot".to_vec(),
        ));

        assert_eq!(first, Ok(None));
        assert_eq!(second, Ok(Some(b"pageknot".to_vec())));
    }

    #[test]
    fn assembler_rejects_a_payload_before_unbounded_growth() {
        let capture_id = CaptureId::new();
        let frame_id = FrameId::new(1);
        let mut assembler = ChunkAssembler::new(capture_id.clone(), frame_id, 8, 3);
        let result = assembler.push(ChunkEnvelope::new(
            capture_id,
            frame_id,
            0,
            1,
            b"four".to_vec(),
        ));

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.collector.payload_limit")
        );
    }

    proptest! {
        #![proptest_config({
            let mut config = ProptestConfig::default();
            if std::env::var_os("PROPTEST_CASES").is_none() {
                config.cases = 128;
            }
            config
        })]

        #[test]
        fn assembler_round_trips_arbitrary_bounded_payloads(
            payload in proptest::collection::vec(any::<u8>(), 0..4096),
            chunk_bytes in 1_usize..128,
        ) {
            let capture_id = CaptureId::new();
            let frame_id = FrameId::new(1);
            let chunks = if payload.is_empty() {
                vec![&payload[..]]
            } else {
                payload.chunks(chunk_bytes).collect::<Vec<_>>()
            };
            let total = u32::try_from(chunks.len())
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let maximum_payload_bytes = u64::try_from(payload.len())
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let mut assembler = ChunkAssembler::new(
                capture_id.clone(),
                frame_id,
                u64::try_from(chunk_bytes)
                    .map_err(|error| TestCaseError::fail(error.to_string()))?,
                maximum_payload_bytes,
            );
            let mut completed = None;
            for (sequence, chunk) in chunks.into_iter().enumerate() {
                let result = assembler.push(ChunkEnvelope::new(
                    capture_id.clone(),
                    frame_id,
                    u32::try_from(sequence)
                        .map_err(|error| TestCaseError::fail(error.to_string()))?,
                    total,
                    chunk.to_vec(),
                ));
                prop_assert!(result.is_ok(), "{result:?}");
                if let Ok(Some(value)) = result {
                    completed = Some(value);
                }
            }

            prop_assert_eq!(completed, Some(payload));
        }
    }
}
