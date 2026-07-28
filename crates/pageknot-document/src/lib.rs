//! Arena-backed document processing and safe-static transformations.
//!
//! This crate discovers render-affecting resources, rewrites embedded content,
//! materializes browser state, strips executable page code, and serializes the
//! transformed document deterministically.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

mod discover;
mod document;
mod policy;
mod resource;
mod sanitize;
mod serialize;
mod state;

pub use discover::{
    CssParseMode, CssResources, DiscoveredCssResource, DiscoveredDocumentResource,
    DocumentResources, discover_css_resources, discover_css_resources_bounded,
    discover_document_resources, discover_document_resources_bounded,
};
pub use document::{Document, DocumentParse, InlineFrame, Node, NodeData};
pub use policy::{LinkRelPolicy, SafeStaticPolicy};
pub use resource::{
    RenderingRole, ResourceGraph, ResourceGraphRecord, ResourceLocationKind, ResourceReference,
};
pub use sanitize::{SanitizationReport, ensure_render_freeze_styles, sanitize_safe_static};
pub use serialize::{serialize_document, serialize_document_to, serialize_subtree};
pub use state::{
    DOCUMENT_SCROLL_X_ATTRIBUTE, DOCUMENT_SCROLL_Y_ATTRIBUTE, ELEMENT_SCROLL_LEFT_ATTRIBUTE,
    ELEMENT_SCROLL_TOP_ATTRIBUTE, set_document_scroll_state,
};
