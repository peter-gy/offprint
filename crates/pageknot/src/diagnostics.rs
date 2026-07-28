use std::fs;
use std::io::Write as _;
use std::path::Path;

use camino::Utf8PathBuf;
use pageknot_model::{
    ArtifactSpec, ArtifactTarget, BrowserSpec, CaptureEvent, CaptureId, CaptureRequest,
    CaptureStatus, ContentDigest, ErrorStage, PageKnotError, PortablePath, RedactedUrl,
    RedactionPolicy, Result, UserAgentPolicy,
};
use serde_json::{Value, json};

const MAXIMUM_DIAGNOSTIC_BYTES: usize = 8 * 1024 * 1024;
const MAXIMUM_DIAGNOSTIC_STRING_BYTES: usize = 2 * 1024;

#[derive(Debug)]
pub(crate) struct CaptureDiagnostics {
    directory: Utf8PathBuf,
    request: Value,
}

impl CaptureDiagnostics {
    pub(crate) async fn prepare(request: &CaptureRequest) -> Result<Option<Self>> {
        let Some(directory) = request.diagnostics.directory.clone() else {
            return Ok(None);
        };
        let directory = directory.into_utf8_path_buf();
        let prepared_directory = directory.clone();
        tokio::task::spawn_blocking(move || prepare_directory(&prepared_directory))
            .await
            .map_err(|error| {
                diagnostic_error(format!(
                    "diagnostic directory preparation task failed: {error}"
                ))
            })??;
        Ok(Some(Self {
            directory,
            request: sanitized_request(request),
        }))
    }

    pub(crate) async fn write(
        &self,
        capture_id: &CaptureId,
        status: CaptureStatus,
        events: Vec<Value>,
        dropped_events: u64,
        error: &PageKnotError,
    ) -> Result<PortablePath> {
        let directory = self.directory.clone();
        let filename = format!("{capture_id}.diagnostics.json");
        let request = self.request.clone();
        let capture_id = capture_id.clone();
        let failure = sanitized_error(error);
        tokio::task::spawn_blocking(move || {
            let mut payload = json!({
                "schemaVersion": pageknot_model::PUBLIC_SCHEMA_VERSION,
                "captureId": capture_id,
                "status": status,
                "request": request,
                "events": events,
                "droppedEvents": dropped_events,
                "failure": failure,
            });
            clamp_json_strings(&mut payload);
            let mut bytes = serde_json::to_vec_pretty(&payload).map_err(|error| {
                diagnostic_error(format!("failed to encode capture diagnostics: {error}"))
            })?;
            bytes.push(b'\n');
            if bytes.len() > MAXIMUM_DIAGNOSTIC_BYTES {
                return Err(diagnostic_error(
                    "capture diagnostics exceed the bundle byte limit",
                ));
            }
            let final_path = directory.join(filename);
            let mut staging = tempfile::NamedTempFile::new_in(&directory).map_err(|error| {
                diagnostic_error(format!(
                    "failed to create diagnostic bundle staging file: {error}"
                ))
            })?;
            staging.write_all(&bytes).map_err(|error| {
                diagnostic_error(format!("failed to write capture diagnostics: {error}"))
            })?;
            staging.as_file().sync_all().map_err(|error| {
                diagnostic_error(format!("failed to sync capture diagnostics: {error}"))
            })?;
            staging.persist_noclobber(&final_path).map_err(|error| {
                diagnostic_error(format!(
                    "failed to commit capture diagnostics: {}",
                    error.error
                ))
            })?;
            PortablePath::from_path_buf(final_path.into_std_path_buf())
        })
        .await
        .map_err(|error| {
            diagnostic_error(format!("diagnostic bundle writer task failed: {error}"))
        })?
    }
}

pub(crate) fn sanitized_event(event: &CaptureEvent) -> Value {
    let mut value = match event {
        CaptureEvent::Warning {
            capture_id,
            warning,
        } => json!({
            "type": "warning",
            "captureId": capture_id,
            "warning": {
                "code": warning.code,
                "frameId": warning.frame_id,
                "resourceId": warning.resource_id,
            },
        }),
        CaptureEvent::CaptureFailed { capture_id, error } => json!({
            "type": "capture.failed",
            "captureId": capture_id,
            "error": sanitized_error(error),
        }),
        CaptureEvent::NavigationStarted { capture_id, url } => json!({
            "type": "navigation.started",
            "captureId": capture_id,
            "url": url,
        }),
        CaptureEvent::NavigationRedirected {
            capture_id,
            from,
            to,
            status,
        } => json!({
            "type": "navigation.redirected",
            "captureId": capture_id,
            "from": from,
            "to": to,
            "status": status,
        }),
        _ => serde_json::to_value(event).unwrap_or_else(|_| {
            json!({
                "type": "diagnostic.event.encode-failed",
            })
        }),
    };
    clamp_json_strings(&mut value);
    value
}

fn sanitized_request(request: &CaptureRequest) -> Value {
    let redaction = RedactionPolicy::default();
    let redacted_url = RedactedUrl::from_url(&request.url, &redaction);
    let browser = match &request.browser {
        BrowserSpec::Auto => json!({"kind": "auto"}),
        BrowserSpec::Executable(_) => json!({
            "kind": "executable",
            "path": "[configured]",
        }),
        BrowserSpec::Remote(endpoint) => json!({
            "kind": "remote",
            "endpoint": RedactedUrl::from_url(endpoint, &redaction),
        }),
    };
    let (artifact_target, conflict) = match &request.artifact {
        ArtifactSpec::Html(options) => {
            let target = match options.target {
                ArtifactTarget::File(_) => json!({"kind": "file"}),
                ArtifactTarget::Bytes { max_bytes } => {
                    json!({"kind": "bytes", "maxBytes": max_bytes})
                }
            };
            (target, options.conflict)
        }
    };
    let user_agent = match request.environment.user_agent {
        UserAgentPolicy::BrowserDefault => "browser-default",
        UserAgentPolicy::Override(_) => "override",
    };
    let mut value = json!({
        "url": redacted_url,
        "urlSha256": ContentDigest::sha256(request.url.as_str()),
        "artifact": {
            "format": "html",
            "target": artifact_target,
            "conflict": conflict,
        },
        "browser": browser,
        "headed": request.headed,
        "environment": {
            "viewport": request.environment.viewport,
            "locale": request.environment.locale,
            "timezone": request.environment.timezone,
            "colorScheme": request.environment.color_scheme,
            "reducedMotion": request.environment.reduced_motion,
            "userAgent": user_agent,
        },
        "readiness": request.readiness,
        "capture": {
            "missingResources": request.capture.missing_resources,
            "preservePasswordValues": request.capture.preserve_password_values,
            "scope": request.capture.scope,
            "optimizations": request.capture.optimizations,
            "allowedFileRootCount": request.capture.allowed_file_roots.len(),
        },
        "credentials": {
            "headerNames": request
                .credentials
                .headers
                .iter()
                .map(|header| header.name.clone())
                .collect::<Vec<_>>(),
            "cookieCount": request.credentials.cookies.len(),
        },
        "network": request.network,
        "limits": request.limits,
        "verification": request.verification,
        "screenshotsRequested": request.diagnostics.screenshots,
    });
    clamp_json_strings(&mut value);
    value
}

fn sanitized_error(error: &PageKnotError) -> Value {
    let mut chain = Vec::new();
    let mut current = Some(error);
    while let Some(error) = current.take() {
        chain.push(json!({
            "code": error.code,
            "stage": error.stage,
            "retryable": error.retryable,
        }));
        if chain.len() == 16 {
            break;
        }
        current = error.source.as_deref();
    }
    json!({
        "code": error.code,
        "stage": error.stage,
        "retryable": error.retryable,
        "chain": chain,
    })
}

fn prepare_directory(directory: &Utf8PathBuf) -> Result<()> {
    let created = match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(diagnostic_error(
                "diagnostic destination must be a directory",
            ));
        }
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(directory).map_err(|error| {
                diagnostic_error(format!("failed to create diagnostic directory: {error}"))
            })?;
            true
        }
        Err(error) => {
            return Err(diagnostic_error(format!(
                "failed to inspect diagnostic directory: {error}"
            )));
        }
    };
    if created {
        set_owner_directory_permissions(directory.as_std_path())?;
    }
    let probe = tempfile::NamedTempFile::new_in(directory).map_err(|error| {
        diagnostic_error(format!("diagnostic directory is not writable: {error}"))
    })?;
    drop(probe);
    Ok(())
}

#[cfg(unix)]
fn set_owner_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
        diagnostic_error(format!(
            "failed to protect diagnostic directory permissions: {error}"
        ))
    })
}

#[cfg(not(unix))]
fn set_owner_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn clamp_json_strings(value: &mut Value) {
    match value {
        Value::String(text) => truncate_utf8(text, MAXIMUM_DIAGNOSTIC_STRING_BYTES),
        Value::Array(values) => {
            for value in values {
                clamp_json_strings(value);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                clamp_json_strings(value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn truncate_utf8(value: &mut String, maximum_bytes: usize) {
    if value.len() <= maximum_bytes {
        return;
    }
    let mut boundary = maximum_bytes;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value.push('…');
}

fn diagnostic_error(message: impl Into<String>) -> PageKnotError {
    PageKnotError::new("pageknot.output.diagnostics", ErrorStage::Commit, message)
}

#[cfg(test)]
mod tests {
    use pageknot_model::{
        CaptureCredentials, CaptureWarning, DiagnosticsPolicy, RequestHeader, SecretString,
    };

    use super::*;

    #[tokio::test]
    async fn diagnostic_bundle_excludes_secret_values_and_failure_messages() -> Result<()> {
        let root = tempfile::tempdir().map_err(|error| {
            diagnostic_error(format!(
                "failed to create diagnostic test directory: {error}"
            ))
        })?;
        let directory = root.path().join("bundles");
        let mut request =
            CaptureRequest::builder("https://example.com/?token=url-secret&view=full")?.build()?;
        request.credentials = CaptureCredentials {
            headers: vec![RequestHeader {
                name: "Authorization".to_owned(),
                value: SecretString::new("header-secret"),
            }],
            cookies: Vec::new(),
        };
        request.diagnostics = DiagnosticsPolicy {
            directory: Some(PortablePath::from_path_buf(directory)?),
            screenshots: false,
        };
        let diagnostics = CaptureDiagnostics::prepare(&request)
            .await?
            .ok_or_else(|| diagnostic_error("diagnostics were not prepared"))?;
        let capture_id = CaptureId::new();
        let warning = CaptureEvent::Warning {
            capture_id: capture_id.clone(),
            warning: CaptureWarning {
                code: "pageknot.resource.fixture".to_owned(),
                message: "warning-secret".to_owned(),
                frame_id: None,
                resource_id: None,
            },
        };
        let error = PageKnotError::new(
            "pageknot.resource.fixture",
            ErrorStage::Resource,
            "error-secret",
        );

        let path = diagnostics
            .write(
                &capture_id,
                CaptureStatus::Failed,
                vec![sanitized_event(&warning)],
                0,
                &error,
            )
            .await?;
        let bundle = fs::read_to_string(path.as_utf8_path()).map_err(|error| {
            diagnostic_error(format!("failed to read diagnostic test bundle: {error}"))
        })?;

        for secret in [
            "url-secret",
            "header-secret",
            "warning-secret",
            "error-secret",
        ] {
            assert!(!bundle.contains(secret), "{bundle}");
        }
        assert!(bundle.contains("Authorization"));
        assert!(bundle.contains("pageknot.resource.fixture"));
        Ok(())
    }
}
