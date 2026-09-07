use std::ops::Range;

use offprint_model::{CaptureId, ContentDigest, ErrorStage, FrameId, OffprintError, Result};

use crate::{ChunkEnvelope, ObservationDescriptor};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferPlan {
    capture_id: CaptureId,
    frame_id: FrameId,
    chunks: u32,
    encoded_bytes: u64,
    payload_sha256: ContentDigest,
    maximum_chunk_bytes: u64,
}

impl TransferPlan {
    pub fn new(
        descriptor: ObservationDescriptor,
        expected_capture_id: &CaptureId,
        expected_frame_id: FrameId,
        maximum_chunk_bytes: u64,
        maximum_payload_bytes: u64,
    ) -> Result<Self> {
        if descriptor.capture_id != *expected_capture_id || descriptor.frame_id != expected_frame_id
        {
            return Err(protocol_error(
                "offprint.collector.protocol_shape",
                "collector descriptor belongs to another capture or frame",
            ));
        }
        if maximum_chunk_bytes == 0 {
            return Err(protocol_error(
                "offprint.collector.chunk_limit",
                "collector chunk limit must be greater than zero",
            ));
        }
        if descriptor.encoded_bytes > maximum_payload_bytes {
            return Err(protocol_error(
                "offprint.collector.payload_limit",
                "collector descriptor exceeds the remaining capture limit",
            )
            .with_detail("encodedBytes", descriptor.encoded_bytes)
            .with_detail("limit", maximum_payload_bytes));
        }

        let expected_chunks = descriptor
            .encoded_bytes
            .div_ceil(maximum_chunk_bytes)
            .max(1);
        let expected_chunks = u32::try_from(expected_chunks).map_err(|_| {
            protocol_error(
                "offprint.collector.chunk_total",
                "collector payload requires more chunks than the protocol can address",
            )
            .with_detail("declared", descriptor.chunks)
            .with_detail("expected", expected_chunks)
        })?;
        if descriptor.chunks != expected_chunks {
            return Err(protocol_error(
                "offprint.collector.chunk_total",
                "collector descriptor chunk count does not match its encoded bytes",
            )
            .with_detail("declared", descriptor.chunks)
            .with_detail("expected", expected_chunks));
        }

        Ok(Self {
            capture_id: descriptor.capture_id,
            frame_id: descriptor.frame_id,
            chunks: descriptor.chunks,
            encoded_bytes: descriptor.encoded_bytes,
            payload_sha256: descriptor.payload_sha256,
            maximum_chunk_bytes,
        })
    }

    #[must_use]
    pub const fn encoded_bytes(&self) -> u64 {
        self.encoded_bytes
    }

    #[must_use]
    pub fn sequences(&self) -> Range<u32> {
        0..self.chunks
    }

    fn expected_chunk_bytes(&self, sequence: u32) -> u64 {
        if sequence + 1 < self.chunks {
            self.maximum_chunk_bytes
        } else {
            self.encoded_bytes - self.maximum_chunk_bytes * u64::from(self.chunks - 1)
        }
    }
}

#[derive(Debug)]
pub struct ChunkAssembler {
    plan: TransferPlan,
    next_sequence: u32,
    payload: Vec<u8>,
    complete: bool,
}

impl ChunkAssembler {
    #[must_use]
    pub fn new(plan: TransferPlan) -> Self {
        Self {
            plan,
            next_sequence: 0,
            payload: Vec::new(),
            complete: false,
        }
    }

    pub fn push(&mut self, chunk: ChunkEnvelope) -> Result<Option<Vec<u8>>> {
        if chunk.capture_id != self.plan.capture_id || chunk.frame_id != self.plan.frame_id {
            return Err(protocol_error(
                "offprint.collector.chunk_owner",
                "collector chunk belongs to another capture or frame",
            ));
        }
        if chunk.total != self.plan.chunks {
            return Err(protocol_error(
                "offprint.collector.chunk_total",
                "collector chunk count does not match the validated transfer plan",
            )
            .with_detail("declared", chunk.total)
            .with_detail("expected", self.plan.chunks));
        }
        chunk.validate(self.plan.maximum_chunk_bytes)?;
        if self.complete || chunk.sequence != self.next_sequence {
            return Err(protocol_error(
                "offprint.collector.chunk_sequence",
                "collector chunks must arrive in sequence",
            ));
        }

        let expected_bytes = self.plan.expected_chunk_bytes(chunk.sequence);
        if chunk.payload_length != expected_bytes {
            return Err(protocol_error(
                "offprint.collector.chunk_length",
                "collector chunk length does not match the validated transfer plan",
            )
            .with_detail("actual", chunk.payload_length)
            .with_detail("expected", expected_bytes));
        }
        let next_bytes = u64::try_from(self.payload.len())
            .ok()
            .and_then(|length| length.checked_add(chunk.payload_length))
            .ok_or_else(|| {
                protocol_error(
                    "offprint.collector.payload_limit",
                    "collector payload length exceeds the host address space",
                )
            })?;
        if next_bytes > self.plan.encoded_bytes {
            return Err(protocol_error(
                "offprint.collector.payload_limit",
                "collector payload exceeds the validated transfer plan",
            )
            .with_detail("attempted", next_bytes)
            .with_detail("limit", self.plan.encoded_bytes));
        }

        self.payload.extend_from_slice(&chunk.payload);
        self.next_sequence += 1;

        if self.next_sequence != self.plan.chunks {
            return Ok(None);
        }
        if next_bytes != self.plan.encoded_bytes {
            return Err(protocol_error(
                "offprint.collector.chunk_length",
                "collector payload length does not match the validated transfer plan",
            )
            .with_detail("actual", next_bytes)
            .with_detail("expected", self.plan.encoded_bytes));
        }
        if ContentDigest::sha256(&self.payload) != self.plan.payload_sha256 {
            return Err(protocol_error(
                "offprint.collector.payload_checksum",
                "collector payload digest does not match its descriptor",
            ));
        }

        self.complete = true;
        Ok(Some(std::mem::take(&mut self.payload)))
    }
}

fn protocol_error(code: &'static str, message: &'static str) -> OffprintError {
    OffprintError::new(code, ErrorStage::Collection, message)
}

#[cfg(test)]
mod tests {
    use offprint_model::{CaptureId, ContentDigest, FrameId};
    use proptest::prelude::*;

    use super::{ChunkAssembler, ChunkEnvelope, ObservationDescriptor, TransferPlan};

    fn descriptor(
        capture_id: CaptureId,
        frame_id: FrameId,
        chunks: u32,
        payload: &[u8],
    ) -> ObservationDescriptor {
        ObservationDescriptor {
            capture_id,
            frame_id,
            chunks,
            encoded_bytes: u64::try_from(payload.len()).unwrap_or(u64::MAX),
            payload_sha256: ContentDigest::sha256(payload),
        }
    }

    fn plan(
        capture_id: &CaptureId,
        frame_id: FrameId,
        chunks: u32,
        payload: &[u8],
        maximum_chunk_bytes: u64,
    ) -> Result<TransferPlan, offprint_model::OffprintError> {
        TransferPlan::new(
            descriptor(capture_id.clone(), frame_id, chunks, payload),
            capture_id,
            frame_id,
            maximum_chunk_bytes,
            u64::try_from(payload.len()).unwrap_or(u64::MAX),
        )
    }

    #[test]
    fn plan_binds_chunk_count_to_encoded_bytes_and_negotiated_size() {
        let capture_id = CaptureId::new();
        let frame_id = FrameId::new(1);

        assert!(plan(&capture_id, frame_id, 3, b"offprint", 3).is_ok());
        assert_eq!(
            plan(&capture_id, frame_id, 4, b"offprint", 3)
                .as_ref()
                .map_err(|error| error.code.as_str()),
            Err("offprint.collector.chunk_total")
        );
    }

    #[test]
    fn plan_rejects_payloads_above_the_host_limit() {
        let capture_id = CaptureId::new();
        let frame_id = FrameId::new(1);
        let descriptor = descriptor(capture_id.clone(), frame_id, 1, b"offprint");
        let result = TransferPlan::new(descriptor, &capture_id, frame_id, 8, 7);

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.collector.payload_limit")
        );
    }

    #[test]
    fn assembler_reconstructs_the_validated_payload_in_sequence()
    -> Result<(), offprint_model::OffprintError> {
        let capture_id = CaptureId::new();
        let frame_id = FrameId::new(1);
        let transfer = plan(&capture_id, frame_id, 2, b"offprint", 4)?;
        let mut assembler = ChunkAssembler::new(transfer);

        let first = assembler.push(ChunkEnvelope::new(
            capture_id.clone(),
            frame_id,
            0,
            2,
            b"offp".to_vec(),
        ));
        let second = assembler.push(ChunkEnvelope::new(
            capture_id,
            frame_id,
            1,
            2,
            b"rint".to_vec(),
        ));

        assert_eq!(first, Ok(None));
        assert_eq!(second, Ok(Some(b"offprint".to_vec())));
        Ok(())
    }

    #[test]
    fn assembler_rejects_an_empty_intermediate_chunk() -> Result<(), offprint_model::OffprintError>
    {
        let capture_id = CaptureId::new();
        let frame_id = FrameId::new(1);
        let transfer = plan(&capture_id, frame_id, 2, b"offprint", 4)?;
        let mut assembler = ChunkAssembler::new(transfer);
        let result = assembler.push(ChunkEnvelope::new(capture_id, frame_id, 0, 2, Vec::new()));

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.collector.chunk_length")
        );
        Ok(())
    }

    #[test]
    fn assembler_rejects_a_chunk_total_outside_the_plan()
    -> Result<(), offprint_model::OffprintError> {
        let capture_id = CaptureId::new();
        let frame_id = FrameId::new(1);
        let transfer = plan(&capture_id, frame_id, 2, b"offprint", 4)?;
        let mut assembler = ChunkAssembler::new(transfer);
        let result = assembler.push(ChunkEnvelope::new(
            capture_id,
            frame_id,
            0,
            3,
            b"offp".to_vec(),
        ));

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.collector.chunk_total")
        );
        Ok(())
    }

    #[test]
    fn assembler_rejects_payload_bytes_outside_the_plan()
    -> Result<(), offprint_model::OffprintError> {
        let capture_id = CaptureId::new();
        let frame_id = FrameId::new(1);
        let transfer = plan(&capture_id, frame_id, 1, b"offprint", 8)?;
        let mut assembler = ChunkAssembler::new(transfer);
        let result = assembler.push(ChunkEnvelope::new(
            capture_id,
            frame_id,
            0,
            1,
            b"offprinT".to_vec(),
        ));

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.collector.payload_checksum")
        );
        Ok(())
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
            let maximum_chunk_bytes = u64::try_from(chunk_bytes)
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            let transfer = plan(
                &capture_id,
                frame_id,
                total,
                &payload,
                maximum_chunk_bytes,
            ).map_err(|error| TestCaseError::fail(error.to_string()))?;
            let mut assembler = ChunkAssembler::new(transfer);
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
