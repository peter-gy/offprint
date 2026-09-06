use std::io::Write;

use offprint::{
    CaptureRequest, CrawlRequest, ErrorStage, Offprint, OffprintError, PortablePath, Result,
    ResumeOptions, VerificationMode,
};

use super::arguments::require_local_browser;
use super::scheduler::{drive_scheduler, finish_runtime};
use crate::command::CrawlArguments;
use crate::config::ResolvedConfig;
use crate::output::{output_error, write_json};

pub(super) async fn execute_crawl(arguments: CrawlArguments, output: &mut dyn Write) -> Result<()> {
    let mut resolved =
        ResolvedConfig::load(arguments.config.as_deref(), arguments.profile.as_deref())?;
    if let Some(path) = arguments.browser_path {
        resolved.apply_browser_path_flag(path);
    }
    require_local_browser(&resolved, "crawl")?;
    let mut seed = CaptureRequest::builder(&arguments.url)?.build()?;
    resolved.apply_to_request(&mut seed);
    seed.verification = VerificationMode::Offline;
    let mut builder = resolved.apply_to_builder(Offprint::builder());
    if arguments.headed {
        builder = builder.headed(true);
    }
    let request = CrawlRequest {
        schema_version: offprint::PUBLIC_SCHEMA_VERSION,
        seed,
        output_directory: PortablePath::new(arguments.output),
        maximum_pages: arguments.max_pages,
        maximum_depth: arguments.max_depth,
        concurrency: arguments.concurrency,
        same_origin: !arguments.allow_cross_origin,
        resume: arguments.resume.map(|manifest| ResumeOptions {
            manifest: PortablePath::new(manifest),
            retry_failed: arguments.retry_failed,
        }),
    };
    let offprint = builder.build()?;
    let operation = async {
        let result = drive_scheduler(
            offprint.captures().crawl(request),
            tokio::signal::ctrl_c(),
            "crawl",
        )
        .await?;
        if arguments.output_options.json {
            write_json(&result, output)?;
        } else {
            writeln!(
                output,
                "{} pages succeeded, {} failed, {} resumed",
                result.succeeded, result.failed, result.resumed
            )
            .map_err(output_error)?;
        }
        if result.failed > 0 {
            return Err(OffprintError::new(
                "offprint.scheduler.failed",
                ErrorStage::Internal,
                format!("{} crawl captures failed", result.failed),
            )
            .with_detail("failed", result.failed)
            .with_detail("succeeded", result.succeeded));
        }
        Ok(())
    }
    .await;
    finish_runtime(&offprint, operation).await
}
