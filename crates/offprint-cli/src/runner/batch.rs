use std::io::{Read, Write};

use offprint::{BatchRequest, ErrorStage, Offprint, OffprintError, Result};

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
        let result = drive_scheduler(
            offprint.captures().batch(request),
            tokio::signal::ctrl_c(),
            "batch capture",
        )
        .await?;
        if arguments.output_options.json {
            write_json(&result, output)?;
        } else {
            writeln!(
                output,
                "{} succeeded, {} failed, {} resumed",
                result.succeeded, result.failed, result.resumed
            )
            .map_err(output_error)?;
        }
        if result.failed > 0 {
            return Err(OffprintError::new(
                "offprint.scheduler.failed",
                ErrorStage::Internal,
                format!("{} batch captures failed", result.failed),
            )
            .with_detail("failed", result.failed)
            .with_detail("succeeded", result.succeeded));
        }
        Ok(())
    }
    .await;
    finish_runtime(&offprint, operation).await
}
