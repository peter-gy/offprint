use std::io::{Read, Write};

use crate::command::{ArtifactCommand, Cli, Command};
use crate::output::{
    DiagnosticTone, diagnostic_color, exit_for_error, json_output, write_diagnostic,
};

mod arguments;
mod artifacts;
mod batch;
mod browser;
mod capture;
mod crawl;
mod diagnostics;
mod dispatch;
mod export;
mod input;
mod scheduler;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandExit {
    Success = 0,
    RuntimeFailure = 1,
    InvalidInput = 2,
    VerificationFailure = 3,
    Interrupted = 130,
}

pub async fn run(
    cli: Cli,
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> CommandExit {
    run_with_terminal_diagnostics(cli, input, output, diagnostics, false).await
}

pub async fn run_with_terminal_diagnostics(
    cli: Cli,
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    diagnostics_terminal: bool,
) -> CommandExit {
    let color = diagnostic_color(&cli, diagnostics_terminal);
    let json = json_output(&cli);
    let verification_failure = matches!(
        &cli.command,
        Command::Capture(_)
            | Command::Artifact(crate::command::ArtifactArguments {
                command: ArtifactCommand::Export(_) | ArtifactCommand::Verify(_),
            })
    );
    // Keep the command dispatcher off the caller's stack. Browser operations
    // carry large async state even when an earlier CLI validation ends the run.
    let result = async {
        crate::config::validate_environment()?;
        Box::pin(dispatch::execute(
            cli,
            input,
            output,
            diagnostics,
            diagnostics_terminal,
        ))
        .await
    }
    .await;
    match result {
        Ok(()) => CommandExit::Success,
        Err(error) => {
            if json {
                let _ignored = serde_json::to_writer(&mut *diagnostics, &error);
                let _ignored = diagnostics.write_all(b"\n");
            } else {
                let _ignored = write_diagnostic(
                    diagnostics,
                    color,
                    DiagnosticTone::Error,
                    "error",
                    format_args!("{}: {}", error.code, error.message),
                );
                if let Some(path) = &error.diagnostics_path {
                    let _ignored = write_diagnostic(
                        diagnostics,
                        color,
                        DiagnosticTone::Stage,
                        "diagnostics",
                        format_args!("{path}"),
                    );
                }
            }
            exit_for_error(&error, verification_failure)
        }
    }
}

#[cfg(test)]
use crate::credentials::read_credentials;
#[cfg(test)]
use scheduler::{drive_capture, drive_verification};

#[cfg(test)]
mod tests;
