use std::io::Write;

use offprint::{BrowserInstallRequest, Offprint, Result};

use super::scheduler::finish_runtime;
use crate::command::BrowserCommand;
use crate::config::ResolvedConfig;
use crate::output::{output_error, render_browser_operation, write_json};

pub(super) async fn execute_browser(command: BrowserCommand, output: &mut dyn Write) -> Result<()> {
    match command {
        BrowserCommand::List(arguments) => {
            let resolved = ResolvedConfig::load(None, None)?;
            let mut builder = resolved.apply_to_builder(Offprint::builder());
            if let Some(cache_dir) = arguments.cache_dir {
                builder = builder.cache_dir(cache_dir);
            }
            let offprint = builder.build()?;
            let operation = async {
                let result = offprint.browsers().list().await?;
                if arguments.output_options.json {
                    write_json(&result, output)?;
                } else {
                    for candidate in &result.candidates {
                        writeln!(
                            output,
                            "{:?} {:?} {} {}",
                            candidate.state,
                            candidate.browser.product,
                            candidate.browser.version,
                            candidate
                                .browser
                                .executable_path
                                .as_deref()
                                .unwrap_or(camino::Utf8Path::new("<remote>"))
                        )
                        .map_err(output_error)?;
                    }
                }
                Ok(())
            }
            .await;
            finish_runtime(&offprint, operation).await
        }
        BrowserCommand::Install(arguments) => {
            let resolved = ResolvedConfig::load(None, None)?;
            let mut builder = resolved.apply_to_builder(Offprint::builder());
            if let Some(cache_dir) = &arguments.cache_dir {
                builder = builder.cache_dir(cache_dir);
            }
            let offprint = builder.build()?;
            let operation = async {
                let result = offprint
                    .browsers()
                    .install(BrowserInstallRequest {
                        revision: arguments.revision,
                    })
                    .await?;
                render_browser_operation(&result, &arguments.output_options, output)
            }
            .await;
            finish_runtime(&offprint, operation).await
        }
        BrowserCommand::Remove(arguments) => {
            let resolved = ResolvedConfig::load(None, None)?;
            let mut builder = resolved.apply_to_builder(Offprint::builder());
            if let Some(cache_dir) = &arguments.cache_dir {
                builder = builder.cache_dir(cache_dir);
            }
            let offprint = builder.build()?;
            let operation = async {
                let result = offprint
                    .browsers()
                    .remove(&arguments.revision, arguments.force)
                    .await?;
                render_browser_operation(&result, &arguments.output_options, output)
            }
            .await;
            finish_runtime(&offprint, operation).await
        }
    }
}
