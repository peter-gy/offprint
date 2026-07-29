//! Command parsing and execution for the `pageknot` binary.
//!
//! The CLI maps configuration and arguments into the same [`pageknot`] service
//! API used by language bindings. Machine results use versioned model records
//! on stdout while progress and diagnostics use stderr.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

mod command;
mod config;
mod credentials;
mod output;
mod runner;

pub use command::{
    ArtifactArgument, ArtifactVariant, BrowserCommand, CaptureArguments, CaptureFormat, Cli,
    Command, CompletionShell, ExportArguments, InspectArguments, VerificationLevel,
    VerifyArguments,
};
pub use runner::{CommandExit, run, run_with_terminal_diagnostics};
