//! Capture request validation, lifecycle state, cancellation, and resource
//! budgets.
//!
//! This crate owns browser-independent capture invariants used by the PageKnot
//! service pipeline.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

mod budget;
mod cancellation;
mod content_store;
mod state;
mod validated;

pub use budget::{BudgetCounter, CaptureBudget};
pub use cancellation::CaptureCancellation;
pub use content_store::{ContentInsertion, ContentStore, StoredContent};
pub use state::CaptureStateMachine;
pub use validated::ValidatedCaptureRequest;
