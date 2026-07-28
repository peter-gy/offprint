use std::collections::BTreeSet;
use std::io;
use std::path::Path;
use std::sync::Arc;

use pageknot_export::VariantEvidence;
use pageknot_model::{
    ArtifactExportRequest, ArtifactExportResult, ArtifactInput, ArtifactManifest, ArtifactVariant,
    ArtifactVariantKind, ArtifactVariantVerification, BrowserEnvironment, BrowserSpec, CaptureId,
    ContentDigest, ERROR_CODE_REGISTRY, ErrorStage, PageKnotError, PortablePath, RedactedUrl,
    RedactionPolicy, Result, VerificationPolicy, VerificationResult,
};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::export_transaction::{ExportTransaction, PreparedVariant, markdown_bundle_digest};
use crate::runtime::{RuntimePageRequest, RuntimeState};

mod markdown_bundle;

use self::markdown_bundle::read_markdown_bundle;

const MAXIMUM_INSPECTION_BYTES: u64 = 64 * 1024 * 1024;
const MAXIMUM_EXPORT_BYTES: u64 = 256 * 1024 * 1024;
const MAXIMUM_MARKDOWN_ASSETS: usize = 10_000;
const FILE_READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
/// Inspects and verifies PageKnot HTML artifacts.
pub struct ArtifactService {
    state: Arc<RuntimeState>,
}

impl ArtifactService {
    pub(crate) const fn new(state: Arc<RuntimeState>) -> Self {
        Self { state }
    }

    /// Parses and validates the embedded artifact manifest.
    ///
    /// Inputs are bounded to 64 MiB.
    pub async fn inspect(&self, input: ArtifactInput) -> Result<ArtifactManifest> {
        self.state.ensure_open()?;
        let bytes = read_input(input).await?;
        pageknot_html::inspect_html(&bytes)
    }

    /// Checks the manifest, content digest, CSP, and embedded resource
    /// structure without launching a browser.
    pub async fn verify_static(&self, input: ArtifactInput) -> Result<VerificationResult> {
        self.state.ensure_open()?;
        let bytes = read_input(input).await?;
        pageknot_html::verify_static(&bytes)
    }

    /// Verifies an artifact at the requested policy level.
    ///
    /// [`VerificationPolicy::Offline`] reopens the artifact in Chromium with
    /// network access denied and fails on attempted requests, page errors, or
    /// unstable browser state.
    pub async fn verify(
        &self,
        input: ArtifactInput,
        policy: VerificationPolicy,
    ) -> Result<VerificationResult> {
        self.state.ensure_open()?;
        let bytes = read_input(input).await?;
        let static_result = pageknot_html::verify_static(&bytes)?;
        if policy == VerificationPolicy::Static {
            return Ok(static_result);
        }
        let maximum_frames = pageknot_html::inspect_html(&bytes)?.frames.max(1);
        let file =
            stage_temporary_artifact("pageknot-verify-", ".html", &bytes, "verification").await?;
        let url = Url::from_file_path(file.path()).map_err(|()| {
            PageKnotError::new(
                "pageknot.verification.path",
                ErrorStage::Verification,
                "artifact path cannot be represented as a file URL",
            )
        })?;
        let verifier = self
            .state
            .open_page(RuntimePageRequest {
                capture_id: CaptureId::new(),
                browser: BrowserSpec::Auto,
                environment: BrowserEnvironment::default(),
                headed: None,
                network: self.state.default_network_policy.clone(),
                maximum_frames,
                resource_observation: pageknot_browser::ResourceObservationLimits::default(),
                deny_network: true,
                cancellation: CancellationToken::new(),
            })
            .await?;
        let observation = verifier
            .page()?
            .verify_offline_url(&url, std::time::Duration::from_secs(120))
            .await;
        let close = verifier.close().await;
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
            return Err(PageKnotError::new(
                "pageknot.verification.network",
                ErrorStage::Verification,
                "artifact attempted a network request during offline verification",
            )
            .with_detail("attemptedRequests", attempted_urls.len())
            .with_detail("attemptedUrls", attempted_url_values));
        }
        if !observation.page_errors.is_empty() {
            return Err(PageKnotError::new(
                "pageknot.verification.page_error",
                ErrorStage::Verification,
                "artifact raised a page error during offline verification",
            )
            .with_detail("errors", observation.page_errors));
        }
        if !observation.stable {
            return Err(PageKnotError::new(
                "pageknot.verification.unstable",
                ErrorStage::Verification,
                "artifact did not reach a stable offline browser state",
            ));
        }
        Ok(VerificationResult {
            level: VerificationPolicy::Offline,
            attempted_urls,
            stable: true,
            ..static_result
        })
    }

    /// Derives verified artifact variants from one verified HTML capture.
    ///
    /// The source capture is reopened with networking denied once before any
    /// output is encoded. Every requested variant then passes its own
    /// structural verifier before commit.
    pub async fn export(
        &self,
        input: ArtifactInput,
        request: ArtifactExportRequest,
    ) -> Result<ArtifactExportResult> {
        self.state.ensure_open()?;
        validate_export_request(&request)?;
        prepare_export_directory(&request.output_directory).await?;
        let bytes = read_input(input).await?;
        let source_verification = self
            .verify(
                ArtifactInput::Bytes(bytes.clone()),
                VerificationPolicy::Offline,
            )
            .await?;
        let manifest = pageknot_html::inspect_html(&bytes)?;
        let stem = pageknot_artifact::portable_file_stem(&request.base_name);
        let mut prepared = Vec::with_capacity(request.variants.len());
        for variant in request.variants {
            let output = match variant {
                ArtifactVariant::Pdf(options) => {
                    let encoded = self.render_pdf(&bytes, &manifest, options).await?;
                    let evidence = pageknot_export::verify_pdf(&encoded)?;
                    PreparedVariant::file(
                        format!("{stem}.pdf"),
                        ArtifactVariantKind::Pdf,
                        encoded,
                        evidence,
                        MAXIMUM_EXPORT_BYTES,
                    )?
                }
                ArtifactVariant::Markdown(options) => {
                    let encoded = pageknot_export::encode_markdown(&bytes, &manifest, options)?;
                    PreparedVariant::markdown(
                        format!("{stem}-markdown"),
                        encoded,
                        MAXIMUM_MARKDOWN_ASSETS,
                        MAXIMUM_EXPORT_BYTES,
                    )?
                }
                ArtifactVariant::Zip => {
                    let encoded = pageknot_export::encode_zip(&bytes, &manifest)?;
                    let evidence = pageknot_export::verify_zip(&encoded)?;
                    PreparedVariant::file(
                        format!("{stem}.zip"),
                        ArtifactVariantKind::Zip,
                        encoded,
                        evidence,
                        MAXIMUM_EXPORT_BYTES,
                    )?
                }
                ArtifactVariant::SelfExtracting => {
                    let encoded = pageknot_export::encode_self_extracting(&bytes)?;
                    let evidence = pageknot_export::verify_self_extracting(&encoded)?;
                    PreparedVariant::file(
                        format!("{stem}.compressed.html"),
                        ArtifactVariantKind::SelfExtracting,
                        encoded,
                        evidence,
                        MAXIMUM_EXPORT_BYTES,
                    )?
                }
                ArtifactVariant::Mhtml => {
                    let encoded = pageknot_export::encode_mhtml(&bytes, &manifest)?;
                    let evidence = pageknot_export::verify_mhtml(&encoded)?;
                    PreparedVariant::file(
                        format!("{stem}.mhtml"),
                        ArtifactVariantKind::Mhtml,
                        encoded,
                        evidence,
                        MAXIMUM_EXPORT_BYTES,
                    )?
                }
            };
            prepared.push(output);
        }
        let variants = ExportTransaction::stage(
            &request.output_directory,
            request.conflict,
            prepared,
            MAXIMUM_MARKDOWN_ASSETS,
            MAXIMUM_EXPORT_BYTES,
        )?
        .commit()?;
        Ok(ArtifactExportResult {
            schema_version: pageknot_model::PUBLIC_SCHEMA_VERSION,
            source_artifact_sha256: source_verification.artifact_sha256,
            policy_sha256: manifest.policy_sha256,
            resources: manifest.resources,
            variants,
        })
    }

    /// Runs the verifier owned by `kind` against an exported path.
    pub async fn verify_variant(
        &self,
        path: PortablePath,
        kind: ArtifactVariantKind,
    ) -> Result<ArtifactVariantVerification> {
        self.state.ensure_open()?;
        match kind {
            ArtifactVariantKind::Markdown => {
                let bundle = read_markdown_bundle(&path).await?;
                let evidence = pageknot_export::verify_markdown(&bundle)?;
                let (bytes, sha256) = markdown_bundle_digest(&bundle);
                Ok(variant_verification(kind, bytes, sha256, evidence))
            }
            ArtifactVariantKind::Pdf
            | ArtifactVariantKind::Zip
            | ArtifactVariantKind::SelfExtracting
            | ArtifactVariantKind::Mhtml => {
                let bytes = read_export_file(&path).await?;
                let evidence = verify_file_variant(kind, &bytes)?;
                Ok(variant_verification(
                    kind,
                    u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                    ContentDigest::sha256(&bytes),
                    evidence,
                ))
            }
        }
    }

    async fn render_pdf(
        &self,
        html: &[u8],
        manifest: &ArtifactManifest,
        options: pageknot_model::PdfOptions,
    ) -> Result<Vec<u8>> {
        let file =
            stage_temporary_artifact("pageknot-pdf-", ".html", html, "PDF source HTML").await?;
        let url = Url::from_file_path(file.path()).map_err(|()| {
            PageKnotError::new(
                "pageknot.verification.path",
                ErrorStage::Encoding,
                "PDF source path cannot be represented as a file URL",
            )
        })?;
        let page = self
            .state
            .open_page(RuntimePageRequest {
                capture_id: CaptureId::new(),
                browser: BrowserSpec::Auto,
                environment: manifest.environment.clone(),
                headed: None,
                network: self.state.default_network_policy.clone(),
                maximum_frames: manifest.frames.max(1),
                resource_observation: pageknot_browser::ResourceObservationLimits::default(),
                deny_network: true,
                cancellation: CancellationToken::new(),
            })
            .await?;
        let render = async {
            let observation = page
                .page()?
                .verify_offline_url(&url, std::time::Duration::from_secs(120))
                .await?;
            validate_offline_observation(observation)?;
            page.page()?
                .print_to_pdf(
                    options.landscape,
                    options.prefer_css_page_size,
                    MAXIMUM_EXPORT_BYTES,
                )
                .await
        }
        .await;
        let close = page.close().await;
        match (render, close) {
            (Ok(bytes), Ok(())) => Ok(bytes),
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        }
    }
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
            Self::Create => "pageknot.output.staging",
            Self::Write => "pageknot.output.flush",
            Self::Sync => "pageknot.output.sync",
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
) -> PageKnotError {
    let code = operation.code();
    let Some(definition) = ERROR_CODE_REGISTRY
        .iter()
        .find(|definition| definition.code == code)
    else {
        return PageKnotError::new(
            "pageknot.internal.panic",
            ErrorStage::Internal,
            format!("canonical error registry is missing `{code}`"),
        )
        .with_detail("requestedCode", code);
    };
    PageKnotError::new(
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

async fn read_input(input: ArtifactInput) -> Result<Vec<u8>> {
    match input {
        ArtifactInput::Bytes(bytes) => {
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_INSPECTION_BYTES {
                return Err(input_too_large());
            }
            Ok(bytes)
        }
        ArtifactInput::File(path) => match read_bounded_file(
            path.as_utf8_path().as_std_path(),
            MAXIMUM_INSPECTION_BYTES,
            false,
        )
        .await
        {
            Ok(bytes) => Ok(bytes),
            Err(BoundedFileReadError::TooLarge) => Err(input_too_large()),
            Err(BoundedFileReadError::NotDirectFile) => Err(PageKnotError::new(
                "pageknot.artifact.read",
                ErrorStage::Verification,
                format!("artifact input is not a regular file: `{path}`"),
            )),
            Err(BoundedFileReadError::Io(error)) => Err(PageKnotError::new(
                "pageknot.artifact.read",
                ErrorStage::Verification,
                format!("failed to read artifact `{path}`: {error}"),
            )),
        },
    }
}

fn input_too_large() -> PageKnotError {
    PageKnotError::new(
        "pageknot.artifact.size",
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

fn validate_export_request(request: &ArtifactExportRequest) -> Result<()> {
    if request.variants.is_empty() {
        return Err(export_error(
            "pageknot.export.variant",
            ErrorStage::Validation,
            "artifact export requires at least one variant",
        ));
    }
    let mut kinds = BTreeSet::new();
    for variant in &request.variants {
        if !kinds.insert(variant.kind()) {
            return Err(export_error(
                "pageknot.export.variant",
                ErrorStage::Validation,
                format!(
                    "artifact variant `{:?}` is requested more than once",
                    variant.kind()
                ),
            ));
        }
    }
    if request.base_name.trim().is_empty() || request.base_name.len() > 512 {
        return Err(export_error(
            "pageknot.export.name",
            ErrorStage::Validation,
            "artifact export base name must contain between 1 and 512 bytes",
        ));
    }
    Ok(())
}

async fn prepare_export_directory(path: &PortablePath) -> Result<()> {
    tokio::fs::create_dir_all(path).await.map_err(|error| {
        export_error(
            "pageknot.export.output",
            ErrorStage::Validation,
            format!("failed to create the artifact export directory: {error}"),
        )
    })?;
    let metadata = tokio::fs::symlink_metadata(path).await.map_err(|error| {
        export_error(
            "pageknot.export.output",
            ErrorStage::Validation,
            format!("failed to inspect the artifact export directory: {error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(export_error(
            "pageknot.export.output",
            ErrorStage::Validation,
            "artifact export output must be a directly addressed directory",
        ));
    }
    Ok(())
}

async fn read_export_file(path: &PortablePath) -> Result<Vec<u8>> {
    match read_bounded_file(
        path.as_utf8_path().as_std_path(),
        MAXIMUM_EXPORT_BYTES,
        true,
    )
    .await
    {
        Ok(bytes) => Ok(bytes),
        Err(BoundedFileReadError::TooLarge | BoundedFileReadError::NotDirectFile) => {
            Err(export_error(
                "pageknot.export.verify",
                ErrorStage::Verification,
                "artifact variant must be a bounded directly addressed file",
            ))
        }
        Err(BoundedFileReadError::Io(error)) => Err(export_error(
            "pageknot.export.verify",
            ErrorStage::Verification,
            format!("failed to read the artifact variant: {error}"),
        )),
    }
}

fn verify_file_variant(kind: ArtifactVariantKind, bytes: &[u8]) -> Result<VariantEvidence> {
    match kind {
        ArtifactVariantKind::Pdf => pageknot_export::verify_pdf(bytes),
        ArtifactVariantKind::Zip => pageknot_export::verify_zip(bytes),
        ArtifactVariantKind::SelfExtracting => pageknot_export::verify_self_extracting(bytes),
        ArtifactVariantKind::Mhtml => pageknot_export::verify_mhtml(bytes),
        ArtifactVariantKind::Markdown => Err(export_error(
            "pageknot.export.verify",
            ErrorStage::Verification,
            "Markdown verification requires a directory bundle",
        )),
    }
}

fn variant_verification(
    kind: ArtifactVariantKind,
    bytes: u64,
    sha256: ContentDigest,
    evidence: VariantEvidence,
) -> ArtifactVariantVerification {
    ArtifactVariantVerification {
        kind,
        passed: evidence.structure_valid && evidence.content_valid,
        bytes,
        sha256,
        structure_valid: evidence.structure_valid,
        content_valid: evidence.content_valid,
    }
}

fn validate_offline_observation(
    observation: pageknot_browser::OfflineBrowserObservation,
) -> Result<()> {
    if !observation.attempted_urls.is_empty() {
        return Err(export_error(
            "pageknot.verification.network",
            ErrorStage::Verification,
            "artifact attempted a network request during variant rendering",
        )
        .with_detail("attemptedRequests", observation.attempted_urls.len()));
    }
    if !observation.page_errors.is_empty() {
        return Err(export_error(
            "pageknot.verification.page_error",
            ErrorStage::Verification,
            "artifact raised a page error during variant rendering",
        )
        .with_detail("errors", observation.page_errors));
    }
    if !observation.stable {
        return Err(export_error(
            "pageknot.verification.unstable",
            ErrorStage::Verification,
            "artifact did not reach a stable state before variant rendering",
        ));
    }
    Ok(())
}

fn export_error(
    code: &'static str,
    stage: ErrorStage,
    message: impl Into<String>,
) -> PageKnotError {
    PageKnotError::new(code, stage, message)
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io::{self, Write as _};

    use pageknot_model::{ERROR_CODE_REGISTRY, ErrorStage};

    use super::{
        BoundedFileReadError, StagingOperation, map_staging_io, read_bounded_file,
        read_bounded_open_file, stage_temporary_artifact,
    };

    type TestResult = std::result::Result<(), Box<dyn Error + Send + Sync>>;

    #[test]
    fn staging_io_failures_follow_the_canonical_registry() -> TestResult {
        for (operation, code, kind) in [
            (
                StagingOperation::Create,
                "pageknot.output.staging",
                io::ErrorKind::PermissionDenied,
            ),
            (
                StagingOperation::Write,
                "pageknot.output.flush",
                io::ErrorKind::WriteZero,
            ),
            (
                StagingOperation::Sync,
                "pageknot.output.sync",
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
    async fn temporary_artifact_is_complete_before_use() -> TestResult {
        let staged =
            stage_temporary_artifact("pageknot-test-", ".html", b"complete", "test").await?;

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
