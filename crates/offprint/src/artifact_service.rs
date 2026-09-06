use std::io;
use std::io::Write as _;
use std::path::Path;
use std::sync::Arc;

use offprint_artifact::FileArtifactWriter;
use offprint_document::{Document, NodeData};
use offprint_model::{
    ArtifactManifest, ArtifactSource, BrowserEnvironment, BrowserSpec, CaptureArtifact, CaptureId,
    CaptureReceipt, ConflictPolicy, ERROR_CODE_REGISTRY, ErrorStage, OffprintError, PortablePath,
    RedactedUrl, RedactionPolicy, Result, VerificationMode, VerificationReport,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use url::Url;

use crate::runtime::{
    RuntimePagePurpose, RuntimePageRequest, RuntimeState, operation_cancelled_error,
};

mod export;
mod markdown_bundle;

const MAXIMUM_INSPECTION_BYTES: u64 = 64 * 1024 * 1024;
const FILE_READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
/// Inspects and verifies Offprint HTML artifacts.
pub struct ArtifactService {
    state: Arc<RuntimeState>,
}

impl ArtifactService {
    pub(crate) const fn new(state: Arc<RuntimeState>) -> Self {
        Self { state }
    }

    /// Derives a portable file name from an in-memory capture title.
    ///
    /// The source host is used when the document has no title.
    pub fn suggested_capture_file_name(
        &self,
        result: &CaptureReceipt,
        extension: &str,
    ) -> Result<String> {
        validate_file_extension(extension)?;
        let CaptureArtifact::Bytes { content, .. } = &result.artifact else {
            return Err(OffprintError::new(
                "offprint.input.artifact",
                ErrorStage::Validation,
                "a suggested file name requires an in-memory capture",
            ));
        };
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
        let fallback = url::Url::parse(result.source.final_url.as_str())
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| "capture".to_owned());
        let stem = offprint_artifact::portable_file_stem(
            title
                .as_deref()
                .filter(|title| !title.trim().is_empty())
                .unwrap_or(&fallback),
        );
        Ok(format!("{stem}.{extension}"))
    }

    /// Commits an in-memory capture to `destination`.
    ///
    /// The returned capture contains the committed file artifact. Existing
    /// files follow `conflict`.
    pub fn commit_capture(
        &self,
        mut result: CaptureReceipt,
        destination: impl Into<PortablePath>,
        conflict: ConflictPolicy,
    ) -> Result<CaptureReceipt> {
        self.state.ensure_open()?;
        let CaptureArtifact::Bytes {
            content,
            bytes,
            sha256,
        } = &result.artifact
        else {
            return Err(OffprintError::new(
                "offprint.input.artifact",
                ErrorStage::Validation,
                "capture commit requires an in-memory artifact",
            ));
        };
        let content_bytes = u64::try_from(content.len()).unwrap_or(u64::MAX);
        let content_sha256 = offprint_model::ContentDigest::sha256(content);
        if *bytes != content_bytes
            || *sha256 != content_sha256
            || result.verification.bytes != content_bytes
            || result.verification.artifact_sha256 != content_sha256
        {
            return Err(OffprintError::new(
                "offprint.input.artifact",
                ErrorStage::Validation,
                "capture bytes and verification evidence do not match",
            ));
        }
        let destination = destination.into();
        let mut writer = FileArtifactWriter::create(destination.into_utf8_path_buf(), conflict)?;
        writer.write_all(content).map_err(|error| {
            OffprintError::new(
                "offprint.output.flush",
                ErrorStage::Encoding,
                format!("failed to write the capture staging artifact: {error}"),
            )
        })?;
        result.artifact = writer.finish()?.commit()?;
        Ok(result)
    }

    /// Parses and validates the embedded artifact manifest.
    ///
    /// Inputs are bounded to 64 MiB.
    pub async fn inspect(&self, input: ArtifactSource) -> Result<ArtifactManifest> {
        self.state.ensure_open()?;
        let bytes = read_input(input).await?;
        offprint_html::inspect_html(&bytes)
    }

    /// Checks the manifest, content digest, CSP, and embedded resource
    /// structure without launching a browser.
    pub async fn verify_static(&self, input: ArtifactSource) -> Result<VerificationReport> {
        self.state.ensure_open()?;
        let bytes = read_input(input).await?;
        offprint_html::verify_static(&bytes)
    }

    /// Verifies an artifact at the requested policy level.
    ///
    /// [`VerificationMode::Offline`] reopens the artifact in Chromium with
    /// network access denied and fails on attempted requests, page errors, or
    /// unstable browser state.
    pub async fn verify(
        &self,
        input: ArtifactSource,
        policy: VerificationMode,
    ) -> Result<VerificationReport> {
        self.state.ensure_open()?;
        let bytes = read_input(input).await?;
        self.verify_bytes(&bytes, policy).await
    }

    async fn verify_bytes(
        &self,
        bytes: &[u8],
        policy: VerificationMode,
    ) -> Result<VerificationReport> {
        self.verify_html(bytes, policy)
            .await
            .map(|(verification, _)| verification)
    }

    async fn verify_html(
        &self,
        bytes: &[u8],
        policy: VerificationMode,
    ) -> Result<(VerificationReport, ArtifactManifest)> {
        let (verification, proof) = self.verify_html_proof(bytes, policy).await?;
        let manifest = proof.manifest().clone();
        Ok((verification, manifest))
    }

    async fn verify_html_proof<'a>(
        &self,
        bytes: &'a [u8],
        policy: VerificationMode,
    ) -> Result<(
        VerificationReport,
        offprint_html::VerifiedHtmlProof<&'a [u8]>,
    )> {
        let proof = offprint_html::verify_html(bytes)?.into_proof();
        if policy == VerificationMode::Static {
            return Ok((proof.verification().clone(), proof));
        }
        let file =
            stage_temporary_artifact("offprint-verify-", ".html", bytes, "verification").await?;
        let url = Url::from_file_path(file.path()).map_err(|()| {
            OffprintError::new(
                "offprint.verification.path",
                ErrorStage::Verification,
                "artifact path cannot be represented as a file URL",
            )
        })?;
        let cancellation = self.state.operation_cancellation();
        let verifier = self
            .state
            .open_page(RuntimePageRequest {
                capture_id: CaptureId::new(),
                browser: BrowserSpec::Auto,
                environment: BrowserEnvironment::default(),
                headed: None,
                network: self.state.default_network_policy.clone(),
                maximum_frames: proof.manifest().frames.max(1),
                resource_observation: offprint_browser::ResourceObservationLimits::default(),
                purpose: RuntimePagePurpose::OfflineVerification,
                cancellation: cancellation.clone(),
            })
            .await?;
        let observation = tokio::select! {
            biased;
            () = cancellation.cancelled() => Err(operation_cancelled_error()),
            observation = verifier
                .page()?
                .verify_offline_url(&url, std::time::Duration::from_secs(120)) => observation,
        };
        let close = verifier.close().await;
        let observation = match (observation, close) {
            (Ok(observation), Ok(())) => observation,
            (Err(error), _) => return Err(error),
            (Ok(_), Err(error)) => return Err(error),
        };
        let verification = offline_verification_result(proof.verification().clone(), observation)?;
        Ok((verification, proof))
    }
}

fn validate_file_extension(extension: &str) -> Result<()> {
    if extension.is_empty() || !extension.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(OffprintError::new(
            "offprint.input.output",
            ErrorStage::Validation,
            "capture file extension must contain ASCII letters or digits",
        ));
    }
    Ok(())
}

pub(super) fn offline_verification_result(
    static_result: VerificationReport,
    observation: offprint_browser::OfflineBrowserObservation,
) -> Result<VerificationReport> {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StagingOperation {
    Create,
    Write,
    Sync,
}

impl StagingOperation {
    const fn code(self) -> &'static str {
        match self {
            Self::Create => "offprint.output.staging",
            Self::Write => "offprint.output.flush",
            Self::Sync => "offprint.output.sync",
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Write => "write",
            Self::Sync => "sync",
        }
    }
}

async fn stage_temporary_artifact(
    prefix: &str,
    suffix: &str,
    bytes: &[u8],
    purpose: &'static str,
) -> Result<tempfile::NamedTempFile> {
    let file = map_staging_io(
        StagingOperation::Create,
        purpose,
        tempfile::Builder::new()
            .prefix(prefix)
            .suffix(suffix)
            .tempfile(),
    )?;
    let writer = map_staging_io(
        StagingOperation::Create,
        purpose,
        file.as_file().try_clone(),
    )?;
    let mut writer = tokio::fs::File::from_std(writer);
    map_staging_io(
        StagingOperation::Write,
        purpose,
        writer.write_all(bytes).await,
    )?;
    map_staging_io(StagingOperation::Sync, purpose, writer.sync_all().await)?;
    Ok(file)
}

fn map_staging_io<T>(
    operation: StagingOperation,
    purpose: &'static str,
    result: io::Result<T>,
) -> Result<T> {
    result.map_err(|error| canonical_staging_error(operation, purpose, error))
}

fn canonical_staging_error(
    operation: StagingOperation,
    purpose: &'static str,
    error: io::Error,
) -> OffprintError {
    let code = operation.code();
    let Some(definition) = ERROR_CODE_REGISTRY
        .iter()
        .find(|definition| definition.code == code)
    else {
        return OffprintError::new(
            "offprint.internal.panic",
            ErrorStage::Internal,
            format!("canonical error registry is missing `{code}`"),
        )
        .with_detail("requestedCode", code);
    };
    OffprintError::new(
        definition.code,
        definition.stage,
        format!(
            "failed to {} the {purpose} staging artifact: {error}",
            operation.name()
        ),
    )
    .retryable(definition.retryable)
    .with_detail("ioKind", format!("{:?}", error.kind()))
    .with_detail("operation", operation.name())
}

async fn read_input(input: ArtifactSource) -> Result<Vec<u8>> {
    match input {
        ArtifactSource::Bytes(bytes) => {
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_INSPECTION_BYTES {
                return Err(input_too_large());
            }
            Ok(bytes)
        }
        ArtifactSource::File(path) => match read_bounded_file(
            path.as_utf8_path().as_std_path(),
            MAXIMUM_INSPECTION_BYTES,
            false,
        )
        .await
        {
            Ok(bytes) => Ok(bytes),
            Err(BoundedFileReadError::TooLarge) => Err(input_too_large()),
            Err(BoundedFileReadError::NotDirectFile) => Err(OffprintError::new(
                "offprint.artifact.read",
                ErrorStage::Verification,
                format!("artifact input is not a regular file: `{path}`"),
            )),
            Err(BoundedFileReadError::Io(error)) => Err(OffprintError::new(
                "offprint.artifact.read",
                ErrorStage::Verification,
                format!("failed to read artifact `{path}`: {error}"),
            )),
        },
    }
}

fn input_too_large() -> OffprintError {
    OffprintError::new(
        "offprint.artifact.size",
        ErrorStage::Verification,
        "artifact exceeds the inspection byte limit",
    )
}

#[derive(Debug)]
enum BoundedFileReadError {
    Io(io::Error),
    NotDirectFile,
    TooLarge,
}

async fn read_bounded_file(
    path: &Path,
    maximum_bytes: u64,
    directly_addressed: bool,
) -> std::result::Result<Vec<u8>, BoundedFileReadError> {
    if directly_addressed {
        validate_direct_file_path(path).await?;
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(BoundedFileReadError::Io)?;
    let metadata = file.metadata().await.map_err(BoundedFileReadError::Io)?;
    if !metadata.is_file() {
        return Err(BoundedFileReadError::NotDirectFile);
    }
    if directly_addressed {
        validate_direct_file_path(path).await?;
    }
    read_bounded_open_file(file, maximum_bytes).await
}

async fn validate_direct_file_path(path: &Path) -> std::result::Result<(), BoundedFileReadError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(BoundedFileReadError::Io)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(BoundedFileReadError::NotDirectFile);
    }
    Ok(())
}

async fn read_bounded_open_file(
    mut file: tokio::fs::File,
    maximum_bytes: u64,
) -> std::result::Result<Vec<u8>, BoundedFileReadError> {
    let mut bytes = Vec::new();
    let mut total = 0_u64;
    // Binding runtimes poll this nested future on worker stacks smaller than
    // the native CLI stack, so keep the read chunk on the heap.
    let mut buffer = vec![0_u8; FILE_READ_BUFFER_BYTES];
    loop {
        if total == maximum_bytes {
            let mut trailing = [0_u8; 1];
            if file
                .read(&mut trailing)
                .await
                .map_err(BoundedFileReadError::Io)?
                != 0
            {
                return Err(BoundedFileReadError::TooLarge);
            }
            return Ok(bytes);
        }
        let remaining = maximum_bytes.saturating_sub(total);
        let maximum =
            usize::try_from(remaining.min(u64::try_from(buffer.len()).unwrap_or(u64::MAX)))
                .unwrap_or(buffer.len());
        let read = file
            .read(&mut buffer[..maximum])
            .await
            .map_err(BoundedFileReadError::Io)?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&buffer[..read]);
        total = total.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io::{self, Write as _};

    use offprint_model::{
        CaptureArtifact, CaptureReceipt, ConflictPolicy, ContentDigest, ERROR_CODE_REGISTRY,
        ErrorStage,
    };

    use super::{
        BoundedFileReadError, StagingOperation, map_staging_io, read_bounded_file,
        read_bounded_open_file, stage_temporary_artifact,
    };
    use crate::Offprint;

    type TestResult = std::result::Result<(), Box<dyn Error + Send + Sync>>;

    #[test]
    fn staging_io_failures_follow_the_canonical_registry() -> TestResult {
        for (operation, code, kind) in [
            (
                StagingOperation::Create,
                "offprint.output.staging",
                io::ErrorKind::PermissionDenied,
            ),
            (
                StagingOperation::Write,
                "offprint.output.flush",
                io::ErrorKind::WriteZero,
            ),
            (
                StagingOperation::Sync,
                "offprint.output.sync",
                io::ErrorKind::StorageFull,
            ),
        ] {
            let error =
                map_staging_io::<()>(operation, "test artifact", Err(io::Error::from(kind)))
                    .err()
                    .ok_or("injected staging failure was accepted")?;
            let definition = ERROR_CODE_REGISTRY
                .iter()
                .find(|definition| definition.code == code)
                .ok_or("staging error code is absent from the registry")?;

            assert_eq!(error.code.as_str(), definition.code);
            assert_eq!(error.stage, definition.stage);
            assert_eq!(error.stage, ErrorStage::Encoding);
            assert_eq!(error.retryable, definition.retryable);
            assert!(error.retryable);
            let io_kind = format!("{kind:?}");
            assert_eq!(
                error.details.get("ioKind").and_then(|value| value.as_str()),
                Some(io_kind.as_str())
            );
            assert_eq!(
                error
                    .details
                    .get("operation")
                    .and_then(|value| value.as_str()),
                Some(operation.name())
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn capture_file_name_and_commit_stay_in_the_artifact_service() -> TestResult {
        let mut result: CaptureReceipt = serde_json::from_str(include_str!(
            "../../../schemas/examples/capture-receipt.json"
        ))?;
        let content = b"<!doctype html><title>Portable Capture</title><main>ready</main>".to_vec();
        result.artifact = CaptureArtifact::Bytes {
            bytes: u64::try_from(content.len())?,
            sha256: ContentDigest::sha256(&content),
            content: content.clone(),
        };
        result.verification.bytes = u64::try_from(content.len())?;
        result.verification.artifact_sha256 = ContentDigest::sha256(&content);
        let directory = tempfile::tempdir()?;
        let offprint = Offprint::builder().build()?;
        let artifacts = offprint.artifacts();

        let file_name = artifacts.suggested_capture_file_name(&result, "html")?;
        let destination = directory.path().join(&file_name);
        let committed = artifacts.commit_capture(
            result,
            offprint_model::PortablePath::from_path_buf(destination.clone())?,
            ConflictPolicy::Replace,
        )?;

        assert_eq!(file_name, "portable-capture.html");
        assert_eq!(std::fs::read(&destination)?, content);
        assert!(matches!(committed.artifact, CaptureArtifact::File { .. }));
        offprint.close().await?;
        Ok(())
    }

    #[tokio::test]
    async fn temporary_artifact_is_complete_before_use() -> TestResult {
        let staged =
            stage_temporary_artifact("offprint-test-", ".html", b"complete", "test").await?;

        assert_eq!(std::fs::read(staged.path())?, b"complete");
        Ok(())
    }

    #[tokio::test]
    async fn bounded_open_file_detects_growth_after_open() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("artifact.html");
        std::fs::write(&path, b"safe")?;
        let file = tokio::fs::File::open(&path).await?;
        let mut writer = std::fs::OpenOptions::new().append(true).open(path)?;
        writer.write_all(b"-oversized")?;

        let result = read_bounded_open_file(file, 4).await;

        assert!(matches!(result, Err(BoundedFileReadError::TooLarge)));
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn directly_addressed_reader_rejects_a_symlink() -> TestResult {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir()?;
        let target = directory.path().join("target.bin");
        let link = directory.path().join("link.bin");
        std::fs::write(&target, b"content")?;
        symlink(target, &link)?;

        let result = read_bounded_file(&link, 64, true).await;

        assert!(matches!(result, Err(BoundedFileReadError::NotDirectFile)));
        Ok(())
    }
}
