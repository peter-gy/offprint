use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(offprint_cli::run_process(std::env::args_os().collect()))
}
