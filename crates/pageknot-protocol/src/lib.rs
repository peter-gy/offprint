//! Versioned protocol between the Rust browser host and the observational
//! TypeScript collector.
//!
//! Payloads are pulled in bounded chunks with per-chunk checksums and explicit
//! acknowledgement. [`ProtocolVersion`] rejects incompatible major versions
//! before collection.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

mod chunk;
mod handshake;
mod message;

pub use chunk::{ChunkAssembler, ChunkEnvelope};
pub use handshake::{
    CollectorCapability, CollectorHandshake, NegotiatedProtocol, ProtocolVersion,
    negotiate_protocol,
};
pub use message::{
    CollectorCommand, CollectorErrorPayload, CollectorErrorResponse, CollectorMessage,
    CollectorRelease, CollectorWarning, FrameOwnerObservation, ObservationDescriptor,
    ObservationViewport, PageObservation, SelectionObservation, VisualFallback, VisualFallbackKind,
};

pub const COLLECTOR_PROTOCOL_VERSION_STRING: &str = "1.5";
pub const COLLECTOR_PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 5 };

#[cfg(test)]
mod tests {
    use super::{COLLECTOR_PROTOCOL_VERSION, COLLECTOR_PROTOCOL_VERSION_STRING};

    #[test]
    fn protocol_version_string_matches_the_structured_version() {
        assert_eq!(
            COLLECTOR_PROTOCOL_VERSION_STRING,
            format!(
                "{}.{}",
                COLLECTOR_PROTOCOL_VERSION.major, COLLECTOR_PROTOCOL_VERSION.minor
            )
        );
    }
}
