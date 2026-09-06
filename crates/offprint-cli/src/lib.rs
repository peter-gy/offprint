//! Command parsing and execution for the `offprint` binary.
//!
//! The CLI maps configuration and arguments into the same [`offprint`] service
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
    ArtifactArgument, ArtifactCommand, BrowserCommand, CaptureArguments, Cli, Command,
    CompletionShell, ExportArguments, FormatSpec, InspectArguments, VerificationModeArg,
    VerifyArguments,
};
pub use runner::{CommandExit, run, run_with_terminal_diagnostics};
