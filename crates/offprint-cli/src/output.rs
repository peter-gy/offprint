use std::fmt;
use std::io::Write;

use offprint::{
    ArtifactManifest, BrowserAction, BrowserOperationResult, CaptureArtifact, CaptureReceipt,
    ErrorStage, OffprintError, Result,
};

use crate::command::{
    ArtifactCommand, BrowserCommand, Cli, ColorOutput, Command, CompletionShell, OutputOptions,
};
use crate::runner::CommandExit;

pub(crate) fn render_capture_result(
    result: &CaptureReceipt,
    requested_output: Option<&str>,
    options: &OutputOptions,
    output: &mut dyn Write,
) -> Result<()> {
    if requested_output == Some("-") {
        let CaptureArtifact::Bytes { content, .. } = &result.artifact else {
            return Err(OffprintError::new(
                "offprint.output.stdout",
                ErrorStage::Commit,
                "stdout capture has no in-memory artifact",
            ));
        };
        return output.write_all(content).map_err(output_error);
    }
    if options.json {
        return write_json(result, output);
    }
    let CaptureArtifact::File { path, .. } = &result.artifact else {
        return Err(OffprintError::new(
            "offprint.output.path",
            ErrorStage::Commit,
            "capture result has no committed file path",
        ));
    };
    writeln!(output, "{path}").map_err(output_error)
}

pub(crate) fn render_artifact_manifest(
    manifest: &ArtifactManifest,
    output: &mut dyn Write,
) -> Result<()> {
    writeln!(
        output,
        "format: Offprint HTML v{} (schema {})",
        manifest.format_version, manifest.schema_version
    )
    .map_err(output_error)?;
    writeln!(output, "source: {}", manifest.source.final_url).map_err(output_error)?;
    writeln!(output, "captured: {}", manifest.captured_at.to_rfc3339()).map_err(output_error)?;
    writeln!(
        output,
        "generator: {} {}",
        manifest.generator.name, manifest.generator.version
    )
    .map_err(output_error)?;
    writeln!(
        output,
        "browser: {:?} {} ({:?}, CDP {})",
        manifest.browser.product,
        manifest.browser.version,
        manifest.browser.source,
        manifest.browser.protocol_version
    )
    .map_err(output_error)?;
    writeln!(
        output,
        "environment: {}x{}@{}, {}, {}, {:?}, {:?}",
        manifest.environment.viewport.width,
        manifest.environment.viewport.height,
        manifest.environment.viewport.scale,
        manifest.environment.locale,
        manifest.environment.timezone,
        manifest.environment.color_scheme,
        manifest.environment.reduced_motion
    )
    .map_err(output_error)?;
    writeln!(output, "policy sha256: {}", manifest.capture_policy_sha256).map_err(output_error)?;
    writeln!(output, "frames: {}", manifest.frames).map_err(output_error)?;
    writeln!(
        output,
        "resources: {} discovered, {} embedded ({} bytes), {} external, {} omitted, {} failed",
        manifest.resources.discovered,
        manifest.resources.embedded,
        manifest.resources.embedded_bytes,
        manifest.resources.external,
        manifest.resources.omitted,
        manifest.resources.failed
    )
    .map_err(output_error)?;
    if manifest.warning_codes.is_empty() {
        writeln!(output, "warnings: none").map_err(output_error)?;
    } else {
        writeln!(output, "warnings: {}", manifest.warning_codes.join(", "))
            .map_err(output_error)?;
    }
    match manifest.structural_repair.script_sha256 {
        Some(sha256) => writeln!(
            output,
            "structural repair: applied={} sha256:{}",
            manifest.structural_repair.applied, sha256
        )
        .map_err(output_error)?,
        None => writeln!(
            output,
            "structural repair: applied={}",
            manifest.structural_repair.applied
        )
        .map_err(output_error)?,
    }
    writeln!(
        output,
        "verification mode: {:?}",
        manifest.verification_mode
    )
    .map_err(output_error)
}

pub(crate) fn render_browser_operation(
    result: &BrowserOperationResult,
    options: &OutputOptions,
    output: &mut dyn Write,
) -> Result<()> {
    if options.json {
        return write_json(result, output);
    }
    if result.action == BrowserAction::List {
        for candidate in &result.candidates {
            let location = candidate
                .browser
                .executable_path
                .as_ref()
                .map_or_else(|| "<remote>".to_owned(), ToString::to_string);
            writeln!(
                output,
                "{}\t{:?}\t{:?} {}\t{}\t{} leases\t{}",
                candidate.priority,
                candidate.state,
                candidate.browser.product,
                candidate.browser.version,
                location,
                candidate.active_leases,
                candidate.reason_code
            )
            .map_err(output_error)?;
        }
        return Ok(());
    }
    let action = match result.action {
        BrowserAction::Install => "installed",
        BrowserAction::List => "listed",
        BrowserAction::Remove => "removed",
    };
    if let Some(revision) = &result.revision {
        writeln!(output, "{action} managed Chromium revision {revision}").map_err(output_error)
    } else {
        writeln!(
            output,
            "{action} managed Chromium cache {}",
            result.cache_dir
        )
        .map_err(output_error)
    }
}

pub(crate) fn render_json_or_doctor(
    report: &offprint::BrowserDoctorReport,
    options: &OutputOptions,
    output: &mut dyn Write,
) -> Result<()> {
    if options.json {
        write_json(report, output)
    } else {
        writeln!(output, "ready: {}", report.ready).map_err(output_error)?;
        if let Some(browser) = &report.selected {
            writeln!(
                output,
                "browser: {:?} {} ({:?})",
                browser.product, browser.version, browser.source
            )
            .map_err(output_error)?;
        }
        writeln!(
            output,
            "collector: compatible={} host={} missing={}",
            report.collector.compatible,
            report.collector.host_version,
            report.collector.missing_capabilities.join(",")
        )
        .map_err(output_error)?;
        writeln!(
            output,
            "output: {} writable={} create={} replace={}",
            report.output.directory,
            report.output.writable,
            report.output.atomic_create,
            report.output.atomic_replace
        )
        .map_err(output_error)?;
        writeln!(
            output,
            "network: {} loopback={} private={} redirects={}",
            report.network.profile,
            report.network.permits_loopback_initial_origin,
            report.network.permits_private_addresses,
            report.network.revalidates_redirects
        )
        .map_err(output_error)?;
        for recovery in &report.recovery {
            if recovery.command.is_empty() {
                writeln!(
                    output,
                    "recovery: {}: {}",
                    recovery.code, recovery.description
                )
                .map_err(output_error)?;
            } else {
                writeln!(
                    output,
                    "recovery: {} {}",
                    recovery.command,
                    recovery.arguments.join(" ")
                )
                .map_err(output_error)?;
            }
        }
        Ok(())
    }
}

pub(crate) fn write_json(value: &impl serde::Serialize, output: &mut dyn Write) -> Result<()> {
    serde_json::to_writer(&mut *output, value).map_err(|error| {
        OffprintError::new(
            "offprint.output.stdout",
            ErrorStage::Commit,
            format!("failed to write JSON output: {error}"),
        )
    })?;
    output.write_all(b"\n").map_err(output_error)
}

pub(crate) fn output_error(error: std::io::Error) -> OffprintError {
    OffprintError::new(
        "offprint.output.stdout",
        ErrorStage::Commit,
        format!("failed to write stdout: {error}"),
    )
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum DiagnosticTone {
    Stage,
    Success,
    Warning,
    Error,
}

pub(crate) fn write_diagnostic(
    diagnostics: &mut dyn Write,
    color: bool,
    tone: DiagnosticTone,
    label: &str,
    message: fmt::Arguments<'_>,
) -> Result<()> {
    if color {
        let code = match tone {
            DiagnosticTone::Stage => "36",
            DiagnosticTone::Success => "32",
            DiagnosticTone::Warning => "33",
            DiagnosticTone::Error => "31",
        };
        writeln!(diagnostics, "\u{1b}[{code}m{label}\u{1b}[0m: {message}")
            .map_err(diagnostic_output_error)
    } else {
        writeln!(diagnostics, "{label}: {message}").map_err(diagnostic_output_error)
    }
}

fn diagnostic_output_error(error: std::io::Error) -> OffprintError {
    OffprintError::new(
        "offprint.output.stderr",
        ErrorStage::Commit,
        format!("failed to write diagnostics: {error}"),
    )
}

pub(crate) fn diagnostic_color(cli: &Cli, terminal: bool) -> bool {
    let color = match &cli.command {
        Command::Capture(arguments) => Some(arguments.output_options.color),
        Command::Artifact(arguments) => Some(match &arguments.command {
            ArtifactCommand::Export(arguments) => arguments.output_options.color,
            ArtifactCommand::Verify(arguments) => arguments.output_options.color,
            ArtifactCommand::Inspect(arguments) => arguments.output_options.color,
        }),
        Command::Batch(arguments) => Some(arguments.output_options.color),
        Command::Crawl(arguments) => Some(arguments.output_options.color),
        Command::Doctor(arguments) => Some(arguments.output_options.color),
        Command::Browser(arguments) => Some(match &arguments.command {
            BrowserCommand::Install(arguments) => arguments.output_options.color,
            BrowserCommand::List(arguments) => arguments.output_options.color,
            BrowserCommand::Remove(arguments) => arguments.output_options.color,
        }),
        Command::Completion(_) => None,
    };
    color.is_some_and(|color| color_enabled(color, terminal))
}

pub(crate) fn json_output(cli: &Cli) -> bool {
    match &cli.command {
        Command::Capture(arguments) => arguments.output_options.json,
        Command::Artifact(arguments) => match &arguments.command {
            ArtifactCommand::Export(arguments) => arguments.output_options.json,
            ArtifactCommand::Verify(arguments) => arguments.output_options.json,
            ArtifactCommand::Inspect(arguments) => arguments.output_options.json,
        },
        Command::Batch(arguments) => arguments.output_options.json,
        Command::Crawl(arguments) => arguments.output_options.json,
        Command::Doctor(arguments) => arguments.output_options.json,
        Command::Browser(arguments) => match &arguments.command {
            BrowserCommand::Install(arguments) => arguments.output_options.json,
            BrowserCommand::List(arguments) => arguments.output_options.json,
            BrowserCommand::Remove(arguments) => arguments.output_options.json,
        },
        Command::Completion(_) => false,
    }
}

pub(crate) const fn color_enabled(color: ColorOutput, terminal: bool) -> bool {
    match color {
        ColorOutput::Auto => terminal,
        ColorOutput::Always => true,
        ColorOutput::Never => false,
    }
}

pub(crate) fn exit_for_error(error: &OffprintError, verification_failure: bool) -> CommandExit {
    if error.code.as_str() == "offprint.runtime.interrupted" {
        CommandExit::Interrupted
    } else if matches!(
        error.code.as_str(),
        "offprint.artifact.read" | "offprint.artifact.size"
    ) {
        CommandExit::RuntimeFailure
    } else if error.stage == ErrorStage::Verification && verification_failure {
        CommandExit::VerificationFailure
    } else if error.stage == ErrorStage::Validation {
        CommandExit::InvalidInput
    } else {
        CommandExit::RuntimeFailure
    }
}

pub(crate) fn completion_shell(shell: CompletionShell) -> clap_complete::Shell {
    match shell {
        CompletionShell::Bash => clap_complete::Shell::Bash,
        CompletionShell::Elvish => clap_complete::Shell::Elvish,
        CompletionShell::Fish => clap_complete::Shell::Fish,
        CompletionShell::Powershell => clap_complete::Shell::PowerShell,
        CompletionShell::Zsh => clap_complete::Shell::Zsh,
    }
}
