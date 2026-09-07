use std::io::{Read, Write};

use offprint::{CaptureRequest, Offprint, Result};

use super::finish::finish_capture;
use super::plan::CapturePlan;
use crate::command::CaptureArguments;
use crate::config::ResolvedConfig;
use crate::credentials::{read_credentials, validate_credential_streams};
use crate::output::{DiagnosticTone, color_enabled, write_diagnostic};
use crate::runner::arguments::apply_capture_arguments;
use crate::runner::scheduler::{drive_capture, finish_runtime};

pub(in crate::runner) async fn execute_capture(
    arguments: CaptureArguments,
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    diagnostics_terminal: bool,
) -> Result<()> {
    let plan = CapturePlan::from_arguments(&arguments)?;
    let mut resolved =
        ResolvedConfig::load(arguments.config.as_deref(), arguments.profile.as_deref())?;
    if let Some(path) = &arguments.browser_path {
        resolved.apply_browser_path_flag(path.clone());
    }
    if let Some(endpoint) = &arguments.cdp_url {
        resolved.apply_cdp_url_flag(endpoint)?;
    }
    let mut request = CaptureRequest::builder(&arguments.url)?.build()?;
    resolved.apply_to_request(&mut request);
    apply_capture_arguments(&mut request, &arguments)?;
    plan.apply_to_request(&mut request);

    let headers_path = arguments
        .headers
        .as_deref()
        .or_else(|| resolved.headers_path());
    let cookies_path = arguments
        .cookies
        .as_deref()
        .or_else(|| resolved.cookies_path());
    validate_credential_streams(Some(plan.requested_output()), headers_path, cookies_path)?;
    request.credentials = read_credentials(headers_path, cookies_path, input)?;
    request.validate()?;

    let offprint = resolved.apply_to_builder(Offprint::builder()).build()?;
    let color = color_enabled(arguments.output_options.color, diagnostics_terminal);
    let operation = async {
        if !arguments.output_options.quiet && !arguments.output_options.json {
            let source = offprint::RedactedUrl::from_url(
                &request.url,
                &offprint::RedactionPolicy::default(),
            );
            write_diagnostic(
                diagnostics,
                color,
                DiagnosticTone::Stage,
                "capture",
                format_args!("{source}"),
            )?;
        }
        let job = offprint.captures().start(request).await?;
        let result = drive_capture(
            &job,
            &arguments.output_options,
            diagnostics,
            color,
            tokio::signal::ctrl_c(),
        )
        .await?;
        finish_capture(
            &result,
            &plan,
            &arguments.output_options,
            output,
            diagnostics,
            color,
        )
    }
    .await;
    finish_runtime(&offprint, operation).await
}
