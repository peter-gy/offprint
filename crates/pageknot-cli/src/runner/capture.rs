use std::io::{Read, Write};
use std::path::Path;

use pageknot::{
    ArtifactExportRequest, ArtifactResult, ArtifactVariant, CaptureRequest, CaptureResult,
    ConflictPolicy, ErrorStage, PageKnot, PageKnotError, PdfOptions, PortablePath, Result,
    VerificationPolicy,
};
use pageknot_artifact::{FileArtifactWriter, portable_file_stem};
use pageknot_document::{Document, NodeData};
use url::Url;

use super::arguments::{
    apply_capture_arguments, require_local_browser, validate_capture_combinations,
};
use super::scheduler::{drive_capture, drive_scheduler, finish_runtime};
use crate::command::{CaptureArguments, CaptureFormat};
use crate::config::ResolvedConfig;
use crate::credentials::{read_credentials, validate_credential_streams};
use crate::output::{
    DiagnosticTone, color_enabled, output_error, render_capture_result, write_diagnostic,
    write_json,
};

pub(super) async fn execute_capture(
    arguments: CaptureArguments,
    input: &mut dyn Read,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    diagnostics_terminal: bool,
) -> Result<()> {
    validate_capture_combinations(&arguments)?;
    let mut resolved =
        ResolvedConfig::load(arguments.config.as_deref(), arguments.profile.as_deref())?;
    if let Some(path) = &arguments.browser_path {
        resolved.apply_browser_path_flag(path.clone());
    }
    if let Some(endpoint) = &arguments.cdp_url {
        resolved.apply_cdp_url_flag(endpoint)?;
    }
    if arguments.format == CaptureFormat::Pdf {
        require_local_browser(&resolved, "PDF capture")?;
    }
    let mut request = CaptureRequest::builder(&arguments.url)?.build()?;
    resolved.apply_to_request(&mut request);
    apply_capture_arguments(&mut request, &arguments)?;
    if arguments.format == CaptureFormat::Pdf && request.verification != VerificationPolicy::Offline
    {
        return Err(PageKnotError::new(
            "pageknot.input.export_option",
            ErrorStage::Validation,
            "PDF capture requires `--verify offline`",
        ));
    }
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
        let result = drive_capture(
            &job,
            &arguments.output_options,
            diagnostics,
            color,
            tokio::signal::ctrl_c(),
        )
        .await?;
        match arguments.format {
            CaptureFormat::Html => {
                finish_html_capture(result, &arguments, output, diagnostics, color)?;
            }
            CaptureFormat::Pdf => {
                finish_pdf_capture(&pageknot, result, &arguments, output, diagnostics, color)
                    .await?;
            }
        }
        Ok(())
    }
    .await;
    finish_runtime(&pageknot, operation).await
}

fn finish_html_capture(
    mut result: CaptureResult,
    arguments: &CaptureArguments,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    color: bool,
) -> Result<()> {
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

async fn finish_pdf_capture(
    pageknot: &PageKnot,
    result: CaptureResult,
    arguments: &CaptureArguments,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    color: bool,
) -> Result<()> {
    let html = capture_bytes(&result)?;
    let temporary = (arguments.output.as_deref() == Some("-"))
        .then(tempfile::tempdir)
        .transpose()
        .map_err(output_error)?;
    let output_path = match (&arguments.output, &temporary) {
        (_, Some(directory)) => directory.path().join("capture.pdf"),
        (Some(path), None) => Path::new(path).to_owned(),
        (None, None) => Path::new(&default_file_name(
            html,
            result.source.final_url.as_str(),
            "pdf",
        ))
        .to_owned(),
    };
    let output_directory = output_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let base_name = output_path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(|| {
            PageKnotError::new(
                "pageknot.input.output",
                ErrorStage::Validation,
                "PDF capture output requires a file name",
            )
        })?;
    if portable_file_stem(base_name) != base_name {
        return Err(PageKnotError::new(
            "pageknot.input.output",
            ErrorStage::Validation,
            "PDF capture output requires a portable file name",
        ));
    }
    let export = drive_scheduler(
        pageknot.artifacts().export_capture(
            &result,
            ArtifactExportRequest {
                output_directory: PortablePath::from_path_buf(output_directory.to_owned())?,
                base_name: base_name.to_owned(),
                variants: vec![ArtifactVariant::Pdf(PdfOptions {
                    landscape: arguments.landscape,
                    prefer_css_page_size: arguments.prefer_css_page_size,
                })],
                conflict: ConflictPolicy::Replace,
            },
        ),
        tokio::signal::ctrl_c(),
        "PDF capture",
    )
    .await?;
    let pdf = export.variants.first().ok_or_else(|| {
        PageKnotError::new(
            "pageknot.internal.panic",
            ErrorStage::Internal,
            "PDF capture returned no committed representation",
        )
    })?;
    if temporary.is_some() {
        let mut file = std::fs::File::open(pdf.entrypoint.as_utf8_path().as_std_path())
            .map_err(output_error)?;
        std::io::copy(&mut file, output).map_err(output_error)?;
    } else if arguments.output_options.json {
        write_json(&export, output)?;
    } else {
        writeln!(output, "{}", pdf.entrypoint).map_err(output_error)?;
    }
    if !arguments.output_options.quiet {
        write_diagnostic(
            diagnostics,
            color,
            DiagnosticTone::Success,
            "verified",
            format_args!(
                "PDF, {} resources, {} bytes",
                result.resources.embedded, pdf.bytes
            ),
        )?;
    }
    Ok(())
}

fn capture_bytes(result: &CaptureResult) -> Result<&[u8]> {
    match &result.artifact {
        ArtifactResult::Bytes { content, .. } => Ok(content),
        ArtifactResult::File { .. } => Err(PageKnotError::new(
            "pageknot.internal.panic",
            ErrorStage::Internal,
            "PDF capture requires an in-memory HTML representation",
        )),
    }
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
    let file_name = default_file_name(content, result.source.final_url.as_str(), "html");
    let mut writer = FileArtifactWriter::create(file_name, ConflictPolicy::Replace)?;
    writer.write_all(content).map_err(output_error)?;
    result.artifact = writer.finish()?.commit()?;
    Ok(result)
}

fn default_file_name(content: &[u8], fallback_url: &str, extension: &str) -> String {
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
    format!("{stem}.{extension}")
}
