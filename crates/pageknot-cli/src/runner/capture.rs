use std::io::{Read, Write};

use pageknot::{ArtifactResult, CaptureRequest, CaptureResult, ConflictPolicy, PageKnot, Result};
use pageknot_artifact::{FileArtifactWriter, portable_file_stem};
use pageknot_document::{Document, NodeData};
use url::Url;

use super::arguments::{apply_capture_arguments, parse_cdp_url, validate_capture_combinations};
use super::scheduler::{drive_capture, finish_runtime};
use crate::command::CaptureArguments;
use crate::config::ResolvedConfig;
use crate::credentials::{read_credentials, validate_credential_streams};
use crate::output::{
    DiagnosticTone, color_enabled, output_error, render_capture_result, write_diagnostic,
};

pub(super) async fn execute_capture(
    arguments: CaptureArguments,
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    diagnostics_terminal: bool,
) -> Result<()> {
    validate_capture_combinations(&arguments)?;
    let resolved = ResolvedConfig::load(arguments.config.as_deref(), arguments.profile.as_deref())?;
    let mut request = CaptureRequest::builder(&arguments.url)?.build()?;
    resolved.apply_to_request(&mut request);
    apply_capture_arguments(&mut request, &arguments)?;
    let headers_path = arguments
        .headers
        .as_deref()
        .or_else(|| resolved.headers_path());
    let cookies_path = arguments
        .cookies
        .as_deref()
        .or_else(|| resolved.cookies_path());
    validate_credential_streams(arguments.output.as_deref(), headers_path, cookies_path)?;
    request.credentials = read_credentials(headers_path, cookies_path, input)?;
    request.validate()?;
    let mut builder = resolved.apply_to_builder(PageKnot::builder());
    if arguments.headed {
        builder = builder.headed(true);
    }
    if let Some(path) = &arguments.browser_path {
        builder = builder.browser_path(path);
    }
    if let Some(endpoint) = &arguments.cdp_url {
        builder = builder.cdp_url(parse_cdp_url(endpoint)?);
    }
    let pageknot = builder.build()?;
    let color = color_enabled(arguments.output_options.color, diagnostics_terminal);
    let operation = async {
        if !arguments.output_options.quiet {
            let source = pageknot::RedactedUrl::from_url(
                &request.url,
                &pageknot::RedactionPolicy::default(),
            );
            write_diagnostic(
                diagnostics,
                color,
                DiagnosticTone::Stage,
                "capture",
                format_args!("{source}"),
            )?;
        }
        let job = pageknot.captures().start(request).await?;
        let mut result = drive_capture(
            &job,
            &arguments.output_options,
            diagnostics,
            color,
            tokio::signal::ctrl_c(),
        )
        .await?;
        if arguments.output.is_none() {
            result = commit_default_output(result)?;
        }
        render_capture_result(
            &result,
            arguments.output.as_deref(),
            &arguments.output_options,
            output,
        )?;
        if !arguments.output_options.quiet {
            write_diagnostic(
                diagnostics,
                color,
                DiagnosticTone::Success,
                "verified",
                format_args!(
                    "{:?}, {} resources, {} bytes",
                    result.verification.level,
                    result.resources.embedded,
                    result.artifact.bytes()
                ),
            )?;
        }
        Ok(())
    }
    .await;
    finish_runtime(&pageknot, operation).await
}

fn commit_default_output(mut result: CaptureResult) -> Result<CaptureResult> {
    let ArtifactResult::Bytes {
        content,
        bytes: _,
        sha256: _,
    } = &result.artifact
    else {
        return Ok(result);
    };
    let file_name = default_file_name(content, result.source.final_url.as_str());
    let mut writer = FileArtifactWriter::create(file_name, ConflictPolicy::Replace)?;
    writer.write_all(content).map_err(output_error)?;
    result.artifact = writer.finish()?.commit()?;
    Ok(result)
}

fn default_file_name(content: &[u8], fallback_url: &str) -> String {
    let document = Document::parse(content);
    let title = document.find_html_element("title").map(|title| {
        document
            .node(title)
            .into_iter()
            .flat_map(|node| &node.children)
            .filter_map(|id| document.node(*id))
            .filter_map(|node| match &node.data {
                NodeData::Text { contents } => Some(contents.as_ref()),
                _ => None,
            })
            .collect::<String>()
    });
    let fallback = Url::parse(fallback_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .unwrap_or_else(|| "capture".to_owned());
    let stem = portable_file_stem(
        title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or(&fallback),
    );
    format!("{stem}.html")
}
