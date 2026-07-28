use std::io;
use std::io::IsTerminal as _;
use std::process::ExitCode;

use clap::Parser as _;
use pageknot_cli::{Cli, run_with_terminal_diagnostics};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
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
