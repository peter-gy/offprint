use std::io::{Read, Write};

use pageknot::{BatchRequest, ErrorStage, PageKnot, PageKnotError, Result};

use super::arguments::parse_cdp_url;
use super::input::read_json_input;
use super::scheduler::{drive_scheduler, finish_runtime};
use crate::command::BatchArguments;
use crate::config::ResolvedConfig;
use crate::output::{output_error, write_json};

pub(super) async fn execute_batch(
    arguments: BatchArguments,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<()> {
    let request = read_json_input::<BatchRequest>(&arguments.manifest, input, "batch manifest")?;
    let resolved = ResolvedConfig::load(arguments.config.as_deref(), None)?;
    let mut builder = resolved.apply_to_builder(PageKnot::builder());
    if let Some(path) = arguments.browser_path {
        builder = builder.browser_path(path);
    }
    if let Some(endpoint) = arguments.cdp_url {
        builder = builder.cdp_url(parse_cdp_url(&endpoint)?);
    }
    let pageknot = builder.build()?;
    let operation = async {
        let result = drive_scheduler(
            pageknot.captures().batch(request),
            tokio::signal::ctrl_c(),
            "batch capture",
        )
        .await?;
        if arguments.output_options.json {
            write_json(&result, output)?;
        } else if !arguments.output_options.quiet {
            writeln!(
                output,
                "{} succeeded, {} failed, {} resumed",
                result.succeeded, result.failed, result.resumed
            )
            .map_err(output_error)?;
        }
        if result.failed > 0 {
            return Err(PageKnotError::new(
                "pageknot.scheduler.failed",
                ErrorStage::Internal,
                format!("{} batch captures failed", result.failed),
            )
            .with_detail("failed", result.failed)
            .with_detail("succeeded", result.succeeded));
        }
        Ok(())
    }
    .await;
    finish_runtime(&pageknot, operation).await
}
