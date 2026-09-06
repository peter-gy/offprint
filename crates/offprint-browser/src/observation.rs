use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// Viewport and scroll state observed for one rendered frame.
pub struct ObservationViewport {
    /// Viewport width in CSS pixels.
    pub width: u32,
    /// Viewport height in CSS pixels.
    pub height: u32,
    /// Device pixel ratio as an exact decimal string.
    pub device_scale_factor: String,
    /// Horizontal scroll offset as an exact decimal string.
    pub scroll_x: String,
    /// Vertical scroll offset as an exact decimal string.
    pub scroll_y: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// One warning emitted while observing browser state.
pub struct ObservationWarning {
    /// Stable warning identifier.
    pub code: String,
    /// Human-readable warning detail.
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
/// Browser state that requires a pixel fallback.
pub enum VisualFallbackKind {
    /// Canvas pixels captured as an image.
    Canvas,
    /// A media frame captured as an image.
    Video,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// The browser-space rectangle for one pixel fallback.
pub struct VisualFallback {
    /// Observation-local target identifier.
    pub id: String,
    /// Browser state represented by the fallback.
    pub kind: VisualFallbackKind,
    /// Horizontal offset in CSS pixels.
    pub x: String,
    /// Vertical offset in CSS pixels.
    pub y: String,
    /// Width in CSS pixels.
    pub width: String,
    /// Height in CSS pixels.
    pub height: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// Selection state retained by one observed frame.
pub struct SelectionObservation {
    /// Number of retained selection ranges.
    pub ranges: u32,
    /// Number of nodes touched by the retained ranges.
    pub nodes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// Original and retained paths for an embedded frame owner.
pub struct FrameOwnerObservation {
    /// Owner path before page state was filtered.
    pub original_path: Vec<u32>,
    /// Owner path in the retained document.
    pub retained_path: Vec<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
/// Provider-neutral state observed from one rendered browser frame.
pub struct FrameObservation {
    /// Serialized document type declaration.
    pub doctype: String,
    /// Serialized rendered document element.
    pub html: String,
    /// Requested frame URL.
    pub requested_url: String,
    /// Final frame URL after redirects.
    pub final_url: String,
    /// Base URL used to resolve document references.
    pub base_url: String,
    /// Rendered document title.
    pub title: String,
    /// Browser-reported document encoding.
    pub encoding: String,
    /// Viewport and scroll state.
    pub viewport: ObservationViewport,
    /// Number of frames contained in this observation.
    pub frames: u32,
    /// Number of retained nodes in this frame.
    pub nodes: u64,
    /// Number of retained nodes including inline descendants.
    pub subtree_nodes: u64,
    #[serde(default)]
    /// Warnings produced while observing this frame.
    pub warnings: Vec<ObservationWarning>,
    #[serde(default)]
    /// Pixel fallbacks required by browser-only rendering state.
    pub visual_fallbacks: Vec<VisualFallback>,
    #[serde(default)]
    /// Retained selection state.
    pub selection: SelectionObservation,
    #[serde(default)]
    /// Embedded frame owner path mappings.
    pub frame_owners: Vec<FrameOwnerObservation>,
}
