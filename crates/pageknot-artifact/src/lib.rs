//! Bounded artifact outputs and transactional filesystem commit.
//!
//! [`FileArtifactWriter`] stages output beside its destination.
//! [`StagedFileArtifact::commit`] provides the atomic commit boundary after
//! encoding and verification complete.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

mod filename;
mod memory;
mod transaction;

pub use filename::portable_file_stem;
pub use memory::{MemoryArtifact, MemoryArtifactWriter};
pub use transaction::{FileArtifactWriter, StagedFileArtifact};
