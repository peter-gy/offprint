//! Alternate artifact encoders and format-specific structural verification.
//!
//! Every encoder consumes an already verified PageKnot HTML artifact. The
//! resulting formats preserve the source manifest and resource report while
//! exposing a representation suited to printing, text workflows, packaging,
//! compressed transport, or browser-native archive import.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

use std::collections::BTreeMap;

mod markdown;
mod mhtml;
mod pdf;
mod self_extracting;
mod support;
mod zip;

pub use markdown::{encode_markdown, verify_markdown};
pub use mhtml::{encode_mhtml, verify_mhtml};
pub use pdf::verify_pdf;
pub use self_extracting::{encode_self_extracting, verify_self_extracting};
pub use zip::{encode_zip, verify_zip};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Format-specific verification evidence before filesystem commit.
pub struct VariantEvidence {
    pub structure_valid: bool,
    pub content_valid: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// Markdown entrypoint and its content-addressed relative assets.
pub struct MarkdownBundle {
    pub markdown: Vec<u8>,
    pub assets: BTreeMap<String, Vec<u8>>,
}
