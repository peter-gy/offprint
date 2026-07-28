use std::io::{Read, Write};

use pageknot::{ErrorStage, PageKnot, PageKnotError, PortablePath, Result};

use super::arguments::{
    artifact_variant_kind, parse_duration, require_local_browser, verification_policy,
};
use super::diagnostics::VerificationDiagnostics;
use super::input::{ensure_variant_input_readable, read_artifact};
use super::scheduler::{drive_verification, finish_runtime};
use crate::command::{ArtifactVariant, InspectArguments, VerificationLevel, VerifyArguments};
use crate::config::ResolvedConfig;
use crate::output::{output_error, render_artifact_manifest, write_json};

pub(super) async fn execute_inspect(
    arguments: InspectArguments,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<()> {
    let pageknot = PageKnot::builder().build()?;
    let operation = async {
        let artifact = read_artifact(arguments.artifact.0, input)?;
        let manifest = pageknot.artifacts().inspect(artifact).await?;
        if arguments.output_options.json {
            write_json(&manifest, output)?;
        } else if !arguments.output_options.quiet {
            render_artifact_manifest(&manifest, output)?;
        }
        Ok(())
    }
    .await;
    finish_runtime(&pageknot, operation).await
}

pub(super) async fn execute_verify(
    arguments: VerifyArguments,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<()> {
    if let Some(format) = arguments.format {
        return execute_variant_verification(arguments, format, output).await;
    }
    let level = arguments.level.unwrap_or(VerificationLevel::Offline);
    if level == VerificationLevel::Static && arguments.browser_path.is_some() {
        return Err(PageKnotError::new(
            "pageknot.input.browser_selection",
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
        verification_policy(level),
    )?;
    let mut resolved = ResolvedConfig::load(arguments.config.as_deref(), None)?;
    if let Some(path) = arguments.browser_path {
        resolved.apply_browser_path_flag(path);
    }
    require_local_browser(&resolved, "artifact verification")?;
    let builder = resolved.apply_to_builder(PageKnot::builder());
    let pageknot = builder.build()?;
    let operation = async {
        let artifact_name = arguments.artifact.0.clone();
        let artifact = read_artifact(arguments.artifact.0, input)?;
        let policy = verification_policy(level);
        let artifacts = pageknot.artifacts();
        let verification = Box::pin(artifacts.verify(artifact, policy));
        let result = drive_verification(verification, timeout, tokio::signal::ctrl_c()).await?;
        if arguments.output_options.json {
            write_json(&result, output)?;
        } else if !arguments.output_options.quiet {
            writeln!(
                output,
                "verified {:?} artifact {} ({} bytes, sha256:{}) with {} network requests",
                result.level,
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
    let result = finish_runtime(&pageknot, operation).await;
    VerificationDiagnostics::attach(diagnostics_policy, result)
}

async fn execute_variant_verification(
    arguments: VerifyArguments,
    format: ArtifactVariant,
    output: &mut dyn Write,
) -> Result<()> {
    if arguments.artifact.0 == "-" {
        return Err(PageKnotError::new(
            "pageknot.input.artifact",
            ErrorStage::Validation,
            "exported artifact verification requires a filesystem path",
        ));
    }
    if arguments.level.is_some()
        || arguments.config.is_some()
        || arguments.browser_path.is_some()
        || arguments.timeout.is_some()
        || arguments.diagnostics.is_some()
    {
        return Err(PageKnotError::new(
            "pageknot.input.verification_option",
            ErrorStage::Validation,
            "HTML verification options apply when --format is omitted",
        ));
    }
    ensure_variant_input_readable(&arguments.artifact.0)?;
    let pageknot = PageKnot::builder().build()?;
    let operation = async {
        let artifact_name = arguments.artifact.0.clone();
        let result = pageknot
            .artifacts()
            .verify_variant(
                PortablePath::new(arguments.artifact.0),
                artifact_variant_kind(format),
            )
            .await?;
        if arguments.output_options.json {
            write_json(&result, output)?;
        } else if !arguments.output_options.quiet {
            writeln!(
                output,
                "verified {:?} artifact {} ({} bytes, sha256:{})",
                result.kind, artifact_name, result.bytes, result.sha256
            )
            .map_err(output_error)?;
        }
        Ok(())
    }
    .await;
    finish_runtime(&pageknot, operation).await
}
