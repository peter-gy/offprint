use std::collections::BTreeSet;

use pageknot_model::{CaptureId, ContentDigest, ErrorStage, PageKnotError, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum CollectorCapability {
    AdoptedStylesheets,
    CanvasPixels,
    ClosedShadowRoots,
    Cssom,
    FormState,
    FrameOwnerMapping,
    MediaState,
    OpenShadowRoots,
    ResponsiveImages,
    SelectionCapture,
    SelectorCapture,
    UnusedCssRemoval,
    UnusedFontRemoval,
    HiddenElementRemoval,
}

impl CollectorCapability {
    pub const ALL: [Self; 14] = [
        Self::AdoptedStylesheets,
        Self::CanvasPixels,
        Self::ClosedShadowRoots,
        Self::Cssom,
        Self::FormState,
        Self::FrameOwnerMapping,
        Self::MediaState,
        Self::OpenShadowRoots,
        Self::ResponsiveImages,
        Self::SelectionCapture,
        Self::SelectorCapture,
        Self::UnusedCssRemoval,
        Self::UnusedFontRemoval,
        Self::HiddenElementRemoval,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AdoptedStylesheets => "adopted-stylesheets",
            Self::CanvasPixels => "canvas-pixels",
            Self::ClosedShadowRoots => "closed-shadow-roots",
            Self::Cssom => "cssom",
            Self::FormState => "form-state",
            Self::FrameOwnerMapping => "frame-owner-mapping",
            Self::MediaState => "media-state",
            Self::OpenShadowRoots => "open-shadow-roots",
            Self::ResponsiveImages => "responsive-images",
            Self::SelectionCapture => "selection-capture",
            Self::SelectorCapture => "selector-capture",
            Self::UnusedCssRemoval => "unused-css-removal",
            Self::UnusedFontRemoval => "unused-font-removal",
            Self::HiddenElementRemoval => "hidden-element-removal",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectorHandshake {
    pub protocol: ProtocolVersion,
    pub capture_id: CaptureId,
    pub host_build_sha256: ContentDigest,
    pub collector_build_sha256: ContentDigest,
    pub requested_capabilities: BTreeSet<CollectorCapability>,
    pub available_capabilities: BTreeSet<CollectorCapability>,
    pub maximum_chunk_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NegotiatedProtocol {
    pub version: ProtocolVersion,
    pub capture_id: CaptureId,
    pub capabilities: BTreeSet<CollectorCapability>,
    pub chunk_bytes: u64,
}

pub fn negotiate_protocol(
    handshake: &CollectorHandshake,
    host_version: ProtocolVersion,
    host_maximum_chunk_bytes: u64,
) -> Result<NegotiatedProtocol> {
    if handshake.protocol.major != host_version.major {
        return Err(PageKnotError::new(
            "pageknot.collector.protocol_major",
            ErrorStage::Collection,
            format!(
                "collector protocol major {} is incompatible with host major {}",
                handshake.protocol.major, host_version.major
            ),
        ));
    }

    let missing = handshake
        .requested_capabilities
        .difference(&handshake.available_capabilities)
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(PageKnotError::new(
            "pageknot.collector.capability",
            ErrorStage::Collection,
            "collector is missing a requested capability",
        )
        .with_detail(
            "missing",
            missing
                .iter()
                .map(|capability| format!("{capability:?}"))
                .collect::<Vec<_>>()
                .join(","),
        ));
    }

    let chunk_bytes = handshake.maximum_chunk_bytes.min(host_maximum_chunk_bytes);
    if chunk_bytes == 0 {
        return Err(PageKnotError::new(
            "pageknot.collector.chunk_limit",
            ErrorStage::Collection,
            "collector chunk limit must be greater than zero",
        ));
    }

    Ok(NegotiatedProtocol {
        version: ProtocolVersion {
            major: host_version.major,
            minor: host_version.min(handshake.protocol).minor,
        },
        capture_id: handshake.capture_id.clone(),
        capabilities: handshake.requested_capabilities.clone(),
        chunk_bytes,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use pageknot_model::{CaptureId, ContentDigest};

    use super::{CollectorCapability, CollectorHandshake, ProtocolVersion, negotiate_protocol};

    fn handshake() -> CollectorHandshake {
        let capabilities = BTreeSet::from([
            CollectorCapability::CanvasPixels,
            CollectorCapability::FormState,
        ]);
        CollectorHandshake {
            protocol: ProtocolVersion { major: 1, minor: 1 },
            capture_id: CaptureId::new(),
            host_build_sha256: ContentDigest::sha256(b"host"),
            collector_build_sha256: ContentDigest::sha256(b"collector"),
            requested_capabilities: capabilities.clone(),
            available_capabilities: capabilities,
            maximum_chunk_bytes: 1024,
        }
    }

    #[test]
    fn negotiation_uses_the_lower_minor_and_chunk_limit() {
        let negotiated =
            negotiate_protocol(&handshake(), ProtocolVersion { major: 1, minor: 0 }, 512);

        assert_eq!(
            negotiated.as_ref().map(|value| value.version),
            Ok(ProtocolVersion { major: 1, minor: 0 })
        );
        assert_eq!(negotiated.as_ref().map(|value| value.chunk_bytes), Ok(512));
    }

    #[test]
    fn major_mismatch_fails_before_collection() {
        let mut handshake = handshake();
        handshake.protocol.major = 2;

        let result = negotiate_protocol(&handshake, ProtocolVersion { major: 1, minor: 0 }, 512);

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.collector.protocol_major")
        );
    }

    #[test]
    fn capability_inventory_matches_the_wire_contract() {
        assert_eq!(
            CollectorCapability::ALL.map(CollectorCapability::as_str),
            [
                "adopted-stylesheets",
                "canvas-pixels",
                "closed-shadow-roots",
                "cssom",
                "form-state",
                "frame-owner-mapping",
                "media-state",
                "open-shadow-roots",
                "responsive-images",
                "selection-capture",
                "selector-capture",
                "unused-css-removal",
                "unused-font-removal",
                "hidden-element-removal",
            ]
        );
    }
}
