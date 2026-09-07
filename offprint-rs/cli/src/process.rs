use std::io;
use std::io::{IsTerminal as _, Write as _};

use crate::{Cli, run_with_terminal_diagnostics};
use clap::Parser as _;

/// Runs a command with process streams and returns its shell exit status.
///
/// `arguments` includes the program name. Call once from a synchronous process
/// entrypoint so the command owns its runtime and signal handlers.
pub fn run_process(arguments: Vec<std::ffi::OsString>) -> u8 {
    let json = arguments.iter().any(|argument| argument == "--json");
    let cli = match Cli::try_parse_from(&arguments) {
        Ok(cli) => cli,
        Err(error) if json && error.use_stderr() => {
            let exit = error.exit_code() as u8;
            let record = offprint::OffprintError::new(
                "offprint.input.arguments",
                offprint::ErrorStage::Validation,
                error.to_string(),
            );
            let mut diagnostics = io::stderr().lock();
            let _ignored = serde_json::to_writer(&mut diagnostics, &record);
            let _ignored = std::io::Write::write_all(&mut diagnostics, b"\n");
            return exit;
        }
        Err(error) => {
            let exit = error.exit_code() as u8;
            let _ignored = error.print();
            return exit;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let record = offprint::OffprintError::new(
                "offprint.runtime.start",
                offprint::ErrorStage::Internal,
                format!("failed to start Offprint runtime: {error}"),
            );
            let mut diagnostics = io::stderr().lock();
            if json {
                let _ignored = serde_json::to_writer(&mut diagnostics, &record);
                let _ignored = diagnostics.write_all(b"\n");
            } else {
                let _ignored = writeln!(diagnostics, "{}: {}", record.code, record.message);
            }
            return 1;
        }
    };
    let diagnostics_terminal = io::stderr().is_terminal();
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut diagnostics = io::stderr().lock();
    let exit = runtime.block_on(run_with_terminal_diagnostics(
        cli,
        &mut input,
        &mut output,
        &mut diagnostics,
        diagnostics_terminal,
    ));
    exit as u8
}
