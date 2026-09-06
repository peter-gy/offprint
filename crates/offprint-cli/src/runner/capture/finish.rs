use std::io::Write;

use offprint::{CaptureReceipt, Result};

use super::plan::CapturePlan;
use crate::command::OutputOptions;
use crate::output::{DiagnosticTone, render_capture_result, write_diagnostic};

pub(super) fn finish_capture(
    result: &CaptureReceipt,
    plan: &CapturePlan,
    options: &OutputOptions,
    output: &mut dyn Write,
    diagnostics: &mut dyn Write,
    color: bool,
) -> Result<()> {
    render_capture_result(result, Some(plan.requested_output()), options, output)?;
    if !options.quiet && !options.json {
        write_diagnostic(
            diagnostics,
            color,
            DiagnosticTone::Success,
            "verified",
            format_args!(
                "{:?}, {} resources, {} bytes",
                result.verification.mode,
                result.resources.embedded,
                result.artifact.bytes()
            ),
        )?;
    }
    Ok(())
}
