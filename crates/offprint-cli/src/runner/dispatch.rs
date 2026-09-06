use std::io::{Read, Write};

use clap::CommandFactory as _;
use offprint::{ErrorStage, Offprint, OffprintError, Result};

use super::artifacts::{execute_inspect, execute_verify};
use super::batch::execute_batch;
use super::browser::execute_browser;
use super::capture::execute_capture;
use super::crawl::execute_crawl;
use super::export::execute_export;
use super::scheduler::finish_runtime;
use crate::command::{ArtifactCommand, Cli, Command};
use crate::config::ResolvedConfig;
use crate::output::{completion_shell, render_json_or_doctor};

pub(super) async fn execute(
    cli: Cli,
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    diagnostics_terminal: bool,
) -> Result<()> {
    match cli.command {
        Command::Doctor(arguments) => {
            let mut resolved = ResolvedConfig::load(arguments.config.as_deref(), None)?;
            if let Some(path) = arguments.browser_path {
                resolved.apply_browser_path_flag(path);
            }
            if let Some(endpoint) = arguments.cdp_url {
                resolved.apply_cdp_url_flag(&endpoint)?;
            }
            let builder = resolved.apply_to_builder(Offprint::builder());
            let offprint = builder.build()?;
            let operation = async {
                let report = offprint.browsers().doctor().await;
                render_json_or_doctor(&report, &arguments.output_options, output)?;
                if report.ready {
                    Ok(())
                } else {
                    Err(OffprintError::new(
                        "offprint.browser.unavailable",
                        ErrorStage::Browser,
                        "Offprint doctor found no ready browser configuration",
                    ))
                }
            }
            .await;
            finish_runtime(&offprint, operation).await
        }
        Command::Artifact(arguments) => match arguments.command {
            ArtifactCommand::Inspect(arguments) => execute_inspect(arguments, input, output).await,
            ArtifactCommand::Verify(arguments) => execute_verify(arguments, input, output).await,
            ArtifactCommand::Export(arguments) => execute_export(arguments, input, output).await,
        },
        Command::Completion(arguments) => {
            let shell = completion_shell(arguments.shell);
            let mut command = Cli::command();
            let name = command.get_name().to_owned();
            clap_complete::generate(shell, &mut command, name, output);
            Ok(())
        }
        Command::Browser(arguments) => execute_browser(arguments.command, output).await,
        Command::Capture(arguments) => {
            execute_capture(*arguments, input, output, diagnostics, diagnostics_terminal).await
        }
        Command::Batch(arguments) => execute_batch(arguments, input, output).await,
        Command::Crawl(arguments) => execute_crawl(arguments, output).await,
    }
}
