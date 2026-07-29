use std::io::Write;
use std::path::Path;

use pageknot::{
    ArtifactExportRequest, ArtifactVariant, CaptureResult, ConflictPolicy, ErrorStage, PageKnot,
    PageKnotError, PortablePath, Result, portable_file_stem,
};

use super::plan::{CaptureDestination, CapturePlan, DerivedCapturePlan, HtmlCapturePlan};
use crate::command::OutputOptions;
use crate::output::{
    DiagnosticTone, output_error, render_capture_result, write_diagnostic, write_json,
};
use crate::runner::scheduler::drive_scheduler;

pub(super) async fn finish_capture(
    pageknot: &PageKnot,
    result: CaptureResult,
    plan: &CapturePlan,
    options: &OutputOptions,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    color: bool,
) -> Result<()> {
    match plan {
        CapturePlan::Html(plan) => {
            finish_html_capture(pageknot, result, plan, options, output, diagnostics, color)
        }
        CapturePlan::Derived(plan) => {
            finish_derived_capture(pageknot, result, plan, options, output, diagnostics, color)
                .await
        }
    }
}

fn finish_html_capture(
    pageknot: &PageKnot,
    mut result: CaptureResult,
    plan: &HtmlCapturePlan,
    options: &OutputOptions,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    color: bool,
) -> Result<()> {
    if plan.destination == CaptureDestination::Default {
        let artifacts = pageknot.artifacts();
        let file_name = artifacts.suggested_capture_file_name(&result, "html")?;
        result = artifacts.commit_capture(result, file_name, ConflictPolicy::Replace)?;
    }
    render_capture_result(
        &result,
        plan.destination.requested_output(),
        options,
        output,
    )?;
    if !options.quiet {
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

async fn finish_derived_capture(
    pageknot: &PageKnot,
    result: CaptureResult,
    plan: &DerivedCapturePlan,
    options: &OutputOptions,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    color: bool,
) -> Result<()> {
    let (format_name, extension) = derived_format(&plan.variant);
    let temporary = (plan.destination == CaptureDestination::Stdout)
        .then(tempfile::tempdir)
        .transpose()
        .map_err(output_error)?;
    let output_path = match (&plan.destination, &temporary) {
        (_, Some(directory)) => directory.path().join(format!("capture.{extension}")),
        (CaptureDestination::File(path), None) => Path::new(path).to_owned(),
        (CaptureDestination::Default, None) => Path::new(
            &pageknot
                .artifacts()
                .suggested_capture_file_name(&result, extension)?,
        )
        .to_owned(),
        (CaptureDestination::Stdout, None) => {
            return Err(PageKnotError::new(
                "pageknot.internal.panic",
                ErrorStage::Internal,
                "derived stdout capture has no temporary destination",
            ));
        }
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
                format!("{format_name} capture output requires a file name"),
            )
        })?;
    if portable_file_stem(base_name) != base_name {
        return Err(PageKnotError::new(
            "pageknot.input.output",
            ErrorStage::Validation,
            format!("{format_name} capture output requires a portable file name"),
        ));
    }

    let export = drive_scheduler(
        pageknot.artifacts().export_capture(
            &result,
            ArtifactExportRequest {
                output_directory: PortablePath::from_path_buf(output_directory.to_owned())?,
                base_name: base_name.to_owned(),
                variants: vec![plan.variant],
                conflict: ConflictPolicy::Replace,
            },
        ),
        tokio::signal::ctrl_c(),
        &format!("{format_name} capture"),
    )
    .await?;
    let representation = export.variants.first().ok_or_else(|| {
        PageKnotError::new(
            "pageknot.internal.panic",
            ErrorStage::Internal,
            format!("{format_name} capture returned no committed representation"),
        )
    })?;
    if temporary.is_some() {
        let mut file = std::fs::File::open(representation.entrypoint.as_utf8_path().as_std_path())
            .map_err(output_error)?;
        std::io::copy(&mut file, output).map_err(output_error)?;
    } else if options.json {
        write_json(&export, output)?;
    } else {
        writeln!(output, "{}", representation.entrypoint).map_err(output_error)?;
    }
    if !options.quiet {
        write_diagnostic(
            diagnostics,
            color,
            DiagnosticTone::Success,
            "verified",
            format_args!(
                "{format_name}, {} resources, {} bytes",
                result.resources.embedded, representation.bytes
            ),
        )?;
    }
    Ok(())
}

fn derived_format(variant: &ArtifactVariant) -> (&'static str, &'static str) {
    match variant {
        ArtifactVariant::Pdf(_) => ("PDF", "pdf"),
        ArtifactVariant::Markdown(_) => ("Markdown", "md"),
        ArtifactVariant::Zip => ("ZIP", "zip"),
        ArtifactVariant::SelfExtracting => ("self-extracting HTML", "html"),
        ArtifactVariant::Mhtml => ("MHTML", "mhtml"),
    }
}
