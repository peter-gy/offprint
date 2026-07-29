use std::io::Write;

use pageknot::{BrowserInstallRequest, PageKnot, Result};

use super::scheduler::finish_runtime;
use crate::command::BrowserCommand;
use crate::output::{output_error, render_browser_operation, write_json};

pub(super) async fn execute_browser(command: BrowserCommand, output: &mut dyn Write) -> Result<()> {
    match command {
        BrowserCommand::List(arguments) => {
            let mut builder = PageKnot::builder();
            if let Some(cache_dir) = arguments.cache_dir {
                builder = builder.cache_dir(cache_dir);
            }
            let pageknot = builder.build()?;
            let operation = async {
                let result = pageknot.browsers().list_operation().await?;
                if arguments.output_options.json {
                    write_json(&result, output)?;
                } else if !arguments.output_options.quiet {
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
            finish_runtime(&pageknot, operation).await
        }
        BrowserCommand::Install(arguments) => {
            let mut builder = PageKnot::builder();
            if let Some(cache_dir) = &arguments.cache_dir {
                builder = builder.cache_dir(cache_dir);
            }
            let pageknot = builder.build()?;
            let operation = async {
                let result = pageknot
                    .browsers()
                    .install_operation(BrowserInstallRequest {
                        revision: arguments.revision,
                        cache_dir: arguments.cache_dir.map(Into::into),
                    })
                    .await?;
                render_browser_operation(&result, &arguments.output_options, output)
            }
            .await;
            finish_runtime(&pageknot, operation).await
        }
        BrowserCommand::Remove(arguments) => {
            let mut builder = PageKnot::builder();
            if let Some(cache_dir) = &arguments.cache_dir {
                builder = builder.cache_dir(cache_dir);
            }
            let pageknot = builder.build()?;
            let operation = async {
                let result = pageknot
                    .browsers()
                    .remove(&arguments.revision, arguments.force)
                    .await?;
                render_browser_operation(&result, &arguments.output_options, output)
            }
            .await;
            finish_runtime(&pageknot, operation).await
        }
    }
}
