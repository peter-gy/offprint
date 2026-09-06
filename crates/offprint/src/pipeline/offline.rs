use std::io::Write as _;
use std::sync::Arc;
use std::time::Duration;

use offprint_model::{
    CaptureId, CaptureRequest, ERROR_CODE_REGISTRY, ErrorStage, OffprintError, RedactedUrl,
    RedactionPolicy, Result, VerificationMode, VerificationReport,
};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::runtime::{RuntimePagePurpose, RuntimePageRequest, RuntimeState};

use super::cancellable;

pub(super) async fn verify(
    runtime: &Arc<RuntimeState>,
    cancellation: &CancellationToken,
    capture_id: &CaptureId,
    request: &CaptureRequest,
    bytes: &[u8],
    static_result: VerificationReport,
) -> Result<VerificationReport> {
    let temporary = stage_verification_html(bytes)?;
    let path = temporary.path().to_owned();
    let url = Url::from_file_path(&path).map_err(|()| {
        OffprintError::new(
            "offprint.verification.path",
            ErrorStage::Verification,
            "staging artifact path cannot be represented as a file URL",
        )
    })?;
    let verifier = cancellable(
        cancellation,
        runtime.open_page(RuntimePageRequest {
            capture_id: capture_id.clone(),
            browser: request.browser.clone(),
            environment: request.environment.clone(),
            headed: request.headed,
            network: request.network.clone(),
            purpose: RuntimePagePurpose::OfflineVerification,
            maximum_frames: request.limits.frames,
            resource_observation: offprint_browser::ResourceObservationLimits {
                maximum_resource_bytes: request.limits.resource_bytes,
                maximum_total_resource_bytes: request.limits.total_resource_bytes,
            },
            cancellation: cancellation.clone(),
        }),
    )
    .await
    .map_err(|error| error.with_detail("capturePhase", "offline-verification"))?;
    let observation = cancellable(
        cancellation,
        verifier
            .page()?
            .verify_offline_url(&url, Duration::from(request.limits.duration)),
    )
    .await;
    let close = verifier.close().await;
    drop(temporary);
    let observation = match (observation, close) {
        (Ok(observation), Ok(())) => observation,
        (Err(error), _) => return Err(error),
        (Ok(_), Err(error)) => return Err(error),
    };
    let attempted_urls = observation
        .attempted_urls
        .iter()
        .filter_map(|url| Url::parse(url).ok())
        .map(|url| RedactedUrl::from_url(&url, &RedactionPolicy::default()))
        .collect::<Vec<_>>();
    if !attempted_urls.is_empty() {
        let attempted_url_values = attempted_urls
            .iter()
            .map(|url| serde_json::Value::String(url.as_str().to_owned()))
            .collect::<Vec<_>>();
        return Err(OffprintError::new(
            "offprint.verification.network",
            ErrorStage::Verification,
            "artifact attempted a network request during offline verification",
        )
        .with_detail("attemptedRequests", attempted_urls.len())
        .with_detail("attemptedUrls", attempted_url_values));
    }
    if !observation.page_errors.is_empty() {
        return Err(OffprintError::new(
            "offprint.verification.page_error",
            ErrorStage::Verification,
            "artifact raised a page error during offline verification",
        )
        .with_detail("errors", observation.page_errors));
    }
    if !observation.stable {
        return Err(OffprintError::new(
            "offprint.verification.unstable",
            ErrorStage::Verification,
            "artifact did not reach a stable offline browser state",
        ));
    }
    Ok(VerificationReport {
        mode: VerificationMode::Offline,
        ..static_result
    })
}

fn stage_verification_html(bytes: &[u8]) -> Result<tempfile::NamedTempFile> {
    let mut file = tempfile::Builder::new()
        .prefix("offprint-verify-")
        .suffix(".html")
        .tempfile()
        .map_err(|error| {
            staging_error(format!(
                "failed to create offline verification staging: {error}"
            ))
        })?;
    file.write_all(bytes).map_err(|error| {
        staging_error(format!(
            "failed to write offline verification staging: {error}"
        ))
    })?;
    file.as_file_mut().flush().map_err(|error| {
        staging_error(format!(
            "failed to flush offline verification staging: {error}"
        ))
    })?;
    file.as_file().sync_all().map_err(|error| {
        staging_error(format!(
            "failed to synchronize offline verification staging: {error}"
        ))
    })?;
    Ok(file)
}

fn staging_error(message: impl Into<String>) -> OffprintError {
    registered_error("offprint.output.staging", message)
}

fn registered_error(code: &'static str, message: impl Into<String>) -> OffprintError {
    let message = message.into();
    match ERROR_CODE_REGISTRY
        .iter()
        .find(|definition| definition.code == code)
    {
        Some(definition) => OffprintError::new(definition.code, definition.stage, message)
            .retryable(definition.retryable),
        None => OffprintError::new(code, ErrorStage::Internal, message),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;
    use std::io::{Seek as _, SeekFrom};

    use offprint_model::ErrorStage;

    use super::{stage_verification_html, staging_error};

    #[test]
    fn staging_error_uses_the_canonical_registry_contract() {
        let error = staging_error("offline staging failed");

        assert_eq!(error.code.as_str(), "offprint.output.staging");
        assert_eq!(error.stage, ErrorStage::Encoding);
        assert!(error.retryable);
    }

    #[test]
    fn offline_browser_staging_keeps_an_html_suffix() -> Result<(), Box<dyn std::error::Error>> {
        let mut file = stage_verification_html(b"<!doctype html><title>capture</title>")?;
        let mut content = String::new();
        file.as_file_mut().seek(SeekFrom::Start(0))?;
        file.as_file_mut().read_to_string(&mut content)?;

        assert_eq!(
            file.path().extension().and_then(std::ffi::OsStr::to_str),
            Some("html")
        );
        assert_eq!(content, "<!doctype html><title>capture</title>");
        Ok(())
    }
}
