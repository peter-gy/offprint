use std::io::Write;
use std::path::Path;

use camino::Utf8PathBuf;
use pageknot::{
    ArtifactResult, CaptureEvent, ConflictPolicy, ErrorStage, PageKnotError, Result,
    VerificationPolicy,
};
use pageknot_artifact::FileArtifactWriter;

use crate::output::{DiagnosticTone, output_error, write_diagnostic};

#[derive(Debug, Default)]
pub(super) struct CaptureProgress {
    last_resource_completed: u32,
}

impl CaptureProgress {
    pub(super) fn render(
        &mut self,
        event: &CaptureEvent,
        diagnostics: &mut dyn Write,
        color: bool,
    ) -> Result<()> {
        match event {
            CaptureEvent::CaptureStarted { .. } => write_diagnostic(
                diagnostics,
                color,
                DiagnosticTone::Stage,
                "stage",
                format_args!("validating"),
            ),
            CaptureEvent::BrowserReady { browser, .. } => write_diagnostic(
                diagnostics,
                color,
                DiagnosticTone::Stage,
                "browser",
                format_args!(
                    "{:?} {} ({:?})",
                    browser.product, browser.version, browser.source
                ),
            ),
            CaptureEvent::NavigationStarted { url, .. } => write_diagnostic(
                diagnostics,
                color,
                DiagnosticTone::Stage,
                "stage",
                format_args!("navigating {url}"),
            ),
            CaptureEvent::NavigationRedirected {
                from, to, status, ..
            } => write_diagnostic(
                diagnostics,
                color,
                DiagnosticTone::Stage,
                "redirect",
                format_args!("{status} {from} -> {to}"),
            ),
            CaptureEvent::ReadinessChanged {
                milestone, elapsed, ..
            } => write_diagnostic(
                diagnostics,
                color,
                DiagnosticTone::Stage,
                "readiness",
                format_args!("{milestone} after {} ms", elapsed.get()),
            ),
            CaptureEvent::ResourceProgress {
                completed,
                discovered,
                bytes,
                ..
            } => {
                let delta = completed.saturating_sub(self.last_resource_completed);
                if *completed <= 20 || delta >= 100 {
                    self.last_resource_completed = *completed;
                    write_diagnostic(
                        diagnostics,
                        color,
                        DiagnosticTone::Stage,
                        "resources",
                        format_args!("{completed}/{discovered}, {bytes} bytes"),
                    )
                } else {
                    Ok(())
                }
            }
            CaptureEvent::TransformStarted { .. } => write_diagnostic(
                diagnostics,
                color,
                DiagnosticTone::Stage,
                "stage",
                format_args!("transforming captured document"),
            ),
            CaptureEvent::ArtifactEncoding { bytes, .. } => write_diagnostic(
                diagnostics,
                color,
                DiagnosticTone::Stage,
                "artifact",
                format_args!("encoded {bytes} bytes"),
            ),
            CaptureEvent::VerificationStarted { .. } => write_diagnostic(
                diagnostics,
                color,
                DiagnosticTone::Stage,
                "stage",
                format_args!("verifying with network denied"),
            ),
            CaptureEvent::Warning { warning, .. } => write_diagnostic(
                diagnostics,
                color,
                DiagnosticTone::Warning,
                "warning",
                format_args!(
                    "{}{}{}",
                    warning.code,
                    warning
                        .frame_id
                        .map(|frame| format!(" frame={}", frame.get()))
                        .unwrap_or_default(),
                    warning
                        .resource_id
                        .map(|resource| format!(" resource={}", resource.get()))
                        .unwrap_or_default()
                ),
            ),
            CaptureEvent::CaptureSucceeded { verification, .. } => write_diagnostic(
                diagnostics,
                color,
                DiagnosticTone::Success,
                "verification",
                format_args!(
                    "{:?}, {} network requests",
                    verification.level, verification.network_requests
                ),
            ),
            CaptureEvent::CaptureCancelled { .. } => write_diagnostic(
                diagnostics,
                color,
                DiagnosticTone::Warning,
                "stage",
                format_args!("cancelled"),
            ),
            CaptureEvent::CaptureFailed { .. }
            | CaptureEvent::FrameCollected { .. }
            | CaptureEvent::ResourceDiscovered { .. } => Ok(()),
        }
    }
}

#[derive(Debug)]
pub(super) struct VerificationDiagnostics {
    destination: Utf8PathBuf,
    level: VerificationPolicy,
}

impl VerificationDiagnostics {
    pub(super) fn prepare(
        directory: Option<&str>,
        level: VerificationPolicy,
    ) -> Result<Option<Self>> {
        let Some(directory) = directory else {
            return Ok(None);
        };
        let directory = Utf8PathBuf::from(directory);
        prepare_diagnostic_directory(directory.as_std_path())?;
        Ok(Some(Self {
            destination: directory.join("verification.diagnostics.json"),
            level,
        }))
    }

    pub(super) fn attach(policy: Option<Self>, result: Result<()>) -> Result<()> {
        let Err(error) = result else {
            return Ok(());
        };
        let Some(policy) = policy else {
            return Err(error);
        };
        match policy.write(&error) {
            Ok(path) => Err(error.with_diagnostics_path(path)),
            Err(diagnostics_error) => {
                Err(error.with_detail("diagnosticsError", diagnostics_error.code.to_string()))
            }
        }
    }

    fn write(self, error: &PageKnotError) -> Result<pageknot::PortablePath> {
        let payload = serde_json::json!({
            "schemaVersion": pageknot::PUBLIC_SCHEMA_VERSION,
            "operation": "verify",
            "verificationLevel": self.level,
            "failure": sanitized_failure(error),
        });
        let mut bytes = serde_json::to_vec_pretty(&payload).map_err(|encode_error| {
            PageKnotError::new(
                "pageknot.output.diagnostics",
                ErrorStage::Commit,
                format!("failed to encode verification diagnostics: {encode_error}"),
            )
        })?;
        bytes.push(b'\n');
        let mut writer = FileArtifactWriter::create(self.destination, ConflictPolicy::Uniquify)?;
        writer.write_all(&bytes).map_err(output_error)?;
        let artifact = writer.finish()?.commit()?;
        let ArtifactResult::File { path, .. } = artifact else {
            return Err(PageKnotError::new(
                "pageknot.output.diagnostics",
                ErrorStage::Commit,
                "verification diagnostics did not produce a file",
            ));
        };
        Ok(path)
    }
}

fn prepare_diagnostic_directory(directory: &Path) -> Result<()> {
    let created = match std::fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(PageKnotError::new(
                "pageknot.output.diagnostics",
                ErrorStage::Validation,
                "diagnostic destination must be a directory",
            ));
        }
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(directory).map_err(|error| {
                PageKnotError::new(
                    "pageknot.output.diagnostics",
                    ErrorStage::Validation,
                    format!("failed to create diagnostic directory: {error}"),
                )
            })?;
            true
        }
        Err(error) => {
            return Err(PageKnotError::new(
                "pageknot.output.diagnostics",
                ErrorStage::Validation,
                format!("failed to inspect diagnostic directory: {error}"),
            ));
        }
    };
    if created {
        protect_diagnostic_directory(directory)?;
    }
    Ok(())
}

#[cfg(unix)]
fn protect_diagnostic_directory(directory: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).map_err(|error| {
        PageKnotError::new(
            "pageknot.output.diagnostics",
            ErrorStage::Validation,
            format!("failed to protect diagnostic directory permissions: {error}"),
        )
    })
}

#[cfg(not(unix))]
fn protect_diagnostic_directory(_directory: &Path) -> Result<()> {
    Ok(())
}

fn sanitized_failure(error: &PageKnotError) -> serde_json::Value {
    let mut chain = Vec::new();
    let mut current = Some(error);
    while let Some(error) = current.take() {
        chain.push(serde_json::json!({
            "code": error.code,
            "stage": error.stage,
            "retryable": error.retryable,
        }));
        if chain.len() == 16 {
            break;
        }
        current = error.source.as_deref();
    }
    serde_json::json!({
        "code": error.code,
        "stage": error.stage,
        "retryable": error.retryable,
        "chain": chain,
    })
}
