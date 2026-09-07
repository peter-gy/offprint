use std::io::{Read, Write};

use offprint::{ErrorStage, Offprint, OffprintError, PortablePath, Result};

use super::arguments::{artifact_format, parse_duration, require_local_browser, verification_mode};
use super::diagnostics::VerificationDiagnostics;
use super::input::{ensure_format_input_readable, read_artifact};
use super::scheduler::{drive_verification, finish_runtime};
use crate::command::{FormatSpec, InspectArguments, VerificationModeArg, VerifyArguments};
use crate::config::ResolvedConfig;
use crate::output::{output_error, render_artifact_manifest, write_json};

pub(super) async fn execute_inspect(
    arguments: InspectArguments,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<()> {
    let offprint = Offprint::builder().build()?;
    let operation = async {
        let artifact = read_artifact(arguments.artifact.0, input)?;
        let manifest = offprint.artifacts().inspect(artifact).await?;
        if arguments.output_options.json {
            write_json(&manifest, output)?;
        } else {
            render_artifact_manifest(&manifest, output)?;
        }
        Ok(())
    }
    .await;
    finish_runtime(&offprint, operation).await
}

pub(super) async fn execute_verify(
    arguments: VerifyArguments,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<()> {
    if let Some(format) = arguments.format {
        return execute_format_verification(arguments, format, output).await;
    }
    let level = arguments
        .verification
        .unwrap_or(VerificationModeArg::Offline);
    if level == VerificationModeArg::Static && arguments.browser_path.is_some() {
        return Err(OffprintError::new(
            "offprint.input.browser_selection",
            ErrorStage::Validation,
            "browser selection applies to offline verification",
        ));
    }
    let timeout = arguments
        .timeout
        .as_deref()
        .map(parse_duration)
        .transpose()?;
    let diagnostics_policy = VerificationDiagnostics::prepare(
        arguments.diagnostics.as_deref(),
        verification_mode(level),
    )?;
    let mut resolved = ResolvedConfig::load(arguments.config.as_deref(), None)?;
    if let Some(path) = arguments.browser_path {
        resolved.apply_browser_path_flag(path);
    }
    if level == VerificationModeArg::Offline {
        require_local_browser(&resolved, "artifact verification")?;
    }
    let builder = resolved.apply_to_builder(Offprint::builder());
    let offprint = builder.build()?;
    let operation = async {
        let artifact_name = arguments.artifact.0.clone();
        let artifact = read_artifact(arguments.artifact.0, input)?;
        let policy = verification_mode(level);
        let artifacts = offprint.artifacts();
        let verification = Box::pin(artifacts.verify(artifact, policy));
        let result = drive_verification(verification, timeout, tokio::signal::ctrl_c()).await?;
        if arguments.output_options.json {
            write_json(&offprint::ArtifactVerification::html(result), output)?;
        } else {
            writeln!(
                output,
                "verified {:?} artifact {} ({} bytes, sha256:{}) with {} network requests",
                result.mode,
                artifact_name,
                result.bytes,
                result.artifact_sha256,
                result.network_requests
            )
            .map_err(output_error)?;
        }
        Ok(())
    }
    .await;
    let result = finish_runtime(&offprint, operation).await;
    VerificationDiagnostics::attach(diagnostics_policy, result)
}

async fn execute_format_verification(
    arguments: VerifyArguments,
    format: FormatSpec,
    output: &mut dyn Write,
) -> Result<()> {
    if arguments.artifact.0 == "-" {
        return Err(OffprintError::new(
            "offprint.input.artifact",
            ErrorStage::Validation,
            "exported artifact verification requires a filesystem path",
        ));
    }
    if arguments.verification.is_some()
        || arguments.config.is_some()
        || arguments.browser_path.is_some()
        || arguments.timeout.is_some()
        || arguments.diagnostics.is_some()
    {
        return Err(OffprintError::new(
            "offprint.input.verification_option",
            ErrorStage::Validation,
            "HTML verification options apply when --format is omitted",
        ));
    }
    ensure_format_input_readable(&arguments.artifact.0)?;
    let offprint = Offprint::builder().build()?;
    let operation = async {
        let artifact_name = arguments.artifact.0.clone();
        let result = offprint
            .artifacts()
            .verify_format(
                PortablePath::new(arguments.artifact.0),
                artifact_format(format),
            )
            .await?;
        if arguments.output_options.json {
            write_json(&offprint::ArtifactVerification::format(result), output)?;
        } else {
            writeln!(
                output,
                "verified {:?} artifact {} ({} bytes, sha256:{})",
                result.format, artifact_name, result.bytes, result.sha256
            )
            .map_err(output_error)?;
        }
        Ok(())
    }
    .await;
    finish_runtime(&offprint, operation).await
}
