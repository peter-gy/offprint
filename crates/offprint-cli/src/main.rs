use std::io;
use std::io::IsTerminal as _;
use std::process::ExitCode;

use clap::Parser as _;
use offprint_cli::{Cli, run_with_terminal_diagnostics};

#[tokio::main]
async fn main() -> ExitCode {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    let json = arguments.iter().any(|argument| argument == "--json");
    let cli = match Cli::try_parse_from(&arguments) {
        Ok(cli) => cli,
        Err(error) if json => {
            let record = offprint::OffprintError::new(
                "offprint.input.arguments",
                offprint::ErrorStage::Validation,
                error.to_string(),
            );
            let mut diagnostics = io::stderr().lock();
            let _ignored = serde_json::to_writer(&mut diagnostics, &record);
            let _ignored = std::io::Write::write_all(&mut diagnostics, b"\n");
            return ExitCode::from(2);
        }
        Err(error) => {
            let _ignored = error.print();
            return ExitCode::from(2);
        }
    };
    let diagnostics_terminal = io::stderr().is_terminal();
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut diagnostics = io::stderr().lock();
    let exit = run_with_terminal_diagnostics(
        cli,
        &mut input,
        &mut output,
        &mut diagnostics,
        diagnostics_terminal,
    )
    .await;
    ExitCode::from(exit as u8)
}
