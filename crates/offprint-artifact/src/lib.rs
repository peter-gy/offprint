//! Bounded artifact outputs and transactional filesystem commit.
//!
//! [`FileArtifactWriter`] stages output beside its destination.
//! [`StagedFileArtifact::commit`] provides the atomic commit boundary after
//! encoding and verification complete.
//! [`ArtifactTransaction`] stages a verified set of file and directory
//! payloads and records enough recovery state to roll an interrupted set back
//! before the next transaction enters the output directory.

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
