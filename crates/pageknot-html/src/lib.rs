//! PageKnot HTML encoding, manifest inspection, and static verification.
//!
//! The encoder combines a transformed document with its provenance manifest.
//! The verifier checks artifact structure, policy, resources, and digest
//! without launching Chromium.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

mod csp;
mod encode;
mod fallback;
mod repair;
mod sandbox;
mod state;
mod verify;

pub use csp::content_security_policy;
pub use encode::{encode_html, encode_html_to};
pub use fallback::empty_resource_data_url;
pub use repair::{
    CSP_ELEMENT_ID, REPAIR_DATA_ELEMENT_ID, REPAIR_MARKER_ATTRIBUTE, REPAIR_MEDIA_TYPE,
    REPAIR_SCRIPT_ELEMENT_ID, RESTORATION_SCRIPT, apply_structural_repair,
    structural_repair_script_digest, validate_repair_tree,
};
pub use sandbox::encode_sandboxed_html;
pub use state::{
    STATE_RESTORATION_SCRIPT, STATE_SCRIPT_ELEMENT_ID, apply_state_restoration,
    state_restoration_script_digest,
};
pub use verify::{
    inspect_html, verify_static, verify_static_sandboxed, verify_static_with_manifest,
};

pub const MANIFEST_ELEMENT_ID: &str = "pageknot-manifest";
pub const MANIFEST_MEDIA_TYPE: &str = "application/vnd.pageknot.manifest+json";
