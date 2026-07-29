//! Bounded artifact outputs and transactional filesystem commit.
//!
//! [`FileArtifactWriter`] stages output beside its destination.
//! [`StagedFileArtifact::commit`] provides the atomic commit boundary after
//! encoding and verification complete.
//! [`ArtifactTransaction`] stages a verified set of file and directory
//! payloads, records recovery state, and commits the set together.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

mod filename;
mod memory;
mod multi_output;
mod transaction;

pub use filename::portable_file_stem;
pub use memory::{MemoryArtifact, MemoryArtifactWriter};
pub use multi_output::{
    ArtifactDirectory, ArtifactTransaction, ArtifactTransactionLimits, PreparedArtifact,
};
pub use transaction::{FileArtifactWriter, StagedFileArtifact};
