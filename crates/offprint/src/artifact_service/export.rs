use std::borrow::Cow;
use std::collections::BTreeSet;

use offprint_artifact::{
    ArtifactDirectory, ArtifactTransaction, ArtifactTransactionLimits, PreparedArtifact,
};
use offprint_export::{FormatEvidence, VerifiedFormat};
use offprint_model::{
    ArtifactFormat, ArtifactManifest, ArtifactSource, BrowserSpec, CaptureArtifact, CaptureId,
    CaptureReceipt, ContentDigest, ErrorStage, ExportRequest, ExportResult, FormatSpec,
    FormatVerification, OffprintError, PortablePath, Result, VerificationMode,
};
use url::Url;

use super::markdown_bundle::read_markdown_bundle;
use super::{
    ArtifactService, BoundedFileReadError, offline_verification_result, read_bounded_file,
    read_input, stage_temporary_artifact,
};
use crate::runtime::{RuntimePagePurpose, RuntimePageRequest, operation_cancelled_error};
use crate::verified_html::OfflineHtmlArtifact;

pub(super) const MAXIMUM_EXPORT_BYTES: u64 = 256 * 1024 * 1024;
pub(super) const MAXIMUM_MARKDOWN_ASSETS: usize = 10_000;

impl ArtifactService {
    /// Derives verified artifact formats from one verified HTML capture.
    ///
    /// The source capture is reopened with networking denied once before any
    /// output is encoded. Every requested format then passes its own
    /// structural verifier before commit.
    pub async fn export(
        &self,
        input: ArtifactSource,
        request: ExportRequest,
    ) -> Result<ExportResult> {
        self.state.ensure_open()?;
        validate_export_request(&request)?;
        let bytes = read_input(input).await?;
        let pdf_options = request.formats.iter().find_map(|format| match format {
            FormatSpec::Pdf(options) => Some(*options),
            _ => None,
        });
        let (verification, proof, rendered_pdf) = match pdf_options {
            Some(options) => {
                let proof = offprint_html::verify_html(bytes.as_slice())?.into_proof();
                let rendered = self
                    .render_pdf(proof.bytes(), proof.manifest(), options)
                    .await?;
                let verification = offline_verification_result(
                    proof.verification().clone(),
                    rendered.observation,
                )?;
                (verification, proof, Some(rendered.bytes))
            }
            None => {
                let (verification, proof) = self
                    .verify_html_proof(bytes.as_slice(), VerificationMode::Offline)
                    .await?;
                (verification, proof, None)
            }
        };
        let source = OfflineHtmlArtifact::from_static_proof(proof, &verification)?;
        self.export_validated(source, request, rendered_pdf).await
    }

    /// Derives verified artifact formats from a completed offline capture.
    ///
    /// The capture's HTML bytes and verification record must describe the same
    /// artifact. Reusing that proof avoids a second offline browser reopen.
    pub async fn export_capture(
        &self,
        capture: &CaptureReceipt,
        request: ExportRequest,
    ) -> Result<ExportResult> {
        self.state.ensure_open()?;
        validate_export_request(&request)?;
        let bytes = match &capture.artifact {
            CaptureArtifact::File { path, .. } => {
                Cow::Owned(read_input(ArtifactSource::File(path.clone())).await?)
            }
            CaptureArtifact::Bytes { content, .. } => Cow::Borrowed(content.as_slice()),
        };
        let proof = offprint_html::verify_html(bytes.as_ref())?.into_proof();
        let source = OfflineHtmlArtifact::from_static_proof(proof, &capture.verification)?;
        self.export_validated(source, request, None).await
    }

    async fn export_validated(
        &self,
        source: OfflineHtmlArtifact<'_>,
        request: ExportRequest,
        mut rendered_pdf: Option<Vec<u8>>,
    ) -> Result<ExportResult> {
        let (html, manifest, verification) = source.into_parts();
        let source_artifact_sha256 = verification.artifact_sha256;
        let stem = offprint_artifact::portable_file_stem(&request.base_name);
        let mut prepared = Vec::with_capacity(request.formats.len());
        for format in request.formats {
            prepared.push(
                self.prepare_format(
                    html,
                    &manifest,
                    source_artifact_sha256,
                    &stem,
                    format,
                    &mut rendered_pdf,
                )
                .await?,
            );
        }
        prepare_export_directory(&request.output_directory).await?;
        let artifacts = ArtifactTransaction::stage(
            &request.output_directory,
            request.conflict,
            prepared,
            ArtifactTransactionLimits::new(
                MAXIMUM_MARKDOWN_ASSETS.saturating_add(1),
                MAXIMUM_EXPORT_BYTES,
            ),
        )?
        .commit()?;
        Ok(ExportResult {
            schema_version: offprint_model::PUBLIC_SCHEMA_VERSION,
            source_artifact_sha256: verification.artifact_sha256,
            capture_policy_sha256: manifest.capture_policy_sha256,
            resources: manifest.resources,
            artifacts,
        })
    }

    async fn prepare_format(
        &self,
        html: &[u8],
        manifest: &ArtifactManifest,
        source_artifact_sha256: ContentDigest,
        stem: &str,
        format: FormatSpec,
        rendered_pdf: &mut Option<Vec<u8>>,
    ) -> Result<PreparedArtifact> {
        match format {
            FormatSpec::Pdf(options) => {
                let rendered = match rendered_pdf.take() {
                    Some(encoded) => encoded,
                    None => self.render_pdf(html, manifest, options).await?.bytes,
                };
                let verified = offprint_export::prepare_pdf(
                    &rendered,
                    html,
                    manifest,
                    source_artifact_sha256,
                    MAXIMUM_EXPORT_BYTES,
                )?;
                prepare_file(format!("{stem}.pdf"), verified)
            }
            FormatSpec::Markdown(options) => {
                let verified = offprint_export::prepare_markdown(html, manifest, options)?;
                prepare_directory(
                    format!("{stem}-markdown"),
                    "index.md",
                    verified,
                    MAXIMUM_MARKDOWN_ASSETS,
                    MAXIMUM_EXPORT_BYTES,
                )
            }
            FormatSpec::Zip => {
                let verified = offprint_export::prepare_zip(html, manifest)?;
                prepare_file(format!("{stem}.zip"), verified)
            }
            FormatSpec::SelfExtractingHtml => {
                let verified = offprint_export::prepare_self_extracting(html)?;
                prepare_file(format!("{stem}.compressed.html"), verified)
            }
            FormatSpec::Mhtml => {
                let verified = offprint_export::prepare_mhtml(html, manifest)?;
                prepare_file(format!("{stem}.mhtml"), verified)
            }
        }
    }

    /// Runs the verifier owned by `format` against an exported path.
    pub async fn verify_format(
        &self,
        path: PortablePath,
        format: ArtifactFormat,
    ) -> Result<FormatVerification> {
        self.state.ensure_open()?;
        match format {
            ArtifactFormat::Html => Err(export_error(
                "offprint.export.verify",
                ErrorStage::Verification,
                "Offprint HTML uses the artifact verification service",
            )),
            ArtifactFormat::Markdown => {
                let bundle = read_markdown_bundle(&path).await?;
                let evidence = offprint_export::verify_markdown(&bundle)?;
                let directory = ArtifactDirectory::new(bundle.into_files())?;
                Ok(evidence.into_verification(directory.bytes(), directory.sha256()))
            }
            ArtifactFormat::Pdf
            | ArtifactFormat::Zip
            | ArtifactFormat::SelfExtractingHtml
            | ArtifactFormat::Mhtml => {
                let bytes = read_export_file(&path).await?;
                let evidence = verify_file_format(format, &bytes)?;
                Ok(evidence.into_verification(
                    u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                    ContentDigest::sha256(&bytes),
                ))
            }
        }
    }

    async fn render_pdf(
        &self,
        html: &[u8],
        manifest: &ArtifactManifest,
        options: offprint_model::PdfOptions,
    ) -> Result<RenderedPdf> {
        let file =
            stage_temporary_artifact("offprint-pdf-", ".html", html, "PDF source HTML").await?;
        let url = Url::from_file_path(file.path()).map_err(|()| {
            OffprintError::new(
                "offprint.verification.path",
                ErrorStage::Encoding,
                "PDF source path cannot be represented as a file URL",
            )
        })?;
        let source_url = Url::parse(manifest.source.final_url.as_str()).map_err(|error| {
            OffprintError::new(
                "offprint.input.artifact",
                ErrorStage::Validation,
                format!("captured source URL cannot be used for PDF links: {error}"),
            )
        })?;
        let cancellation = self.state.operation_cancellation();
        let page = self
            .state
            .open_page(RuntimePageRequest {
                capture_id: CaptureId::new(),
                browser: BrowserSpec::Auto,
                environment: manifest.environment.clone(),
                headed: None,
                network: self.state.default_network_policy.clone(),
                maximum_frames: manifest.frames.max(1),
                resource_observation: offprint_browser::ResourceObservationLimits::default(),
                purpose: RuntimePagePurpose::OfflineVerification,
                cancellation: cancellation.clone(),
            })
            .await?;
        let render = tokio::select! {
            biased;
            () = cancellation.cancelled() => Err(operation_cancelled_error()),
            render = async {
            let observation = page
                .page()?
                .verify_offline_url(&url, std::time::Duration::from_secs(120))
                .await?;
            validate_offline_observation(&observation)?;
            let bytes = page
                .page()?
                .print_to_pdf(
                    &source_url,
                    options.landscape,
                    options.prefer_css_page_size,
                    MAXIMUM_EXPORT_BYTES,
                )
                .await?;
            Ok(RenderedPdf { bytes, observation })
            } => render,
        };
        let close = page.close().await;
        match (render, close) {
            (Ok(rendered), Ok(())) => Ok(rendered),
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        }
    }
}

#[derive(Debug)]
struct RenderedPdf {
    bytes: Vec<u8>,
    observation: offprint_browser::OfflineBrowserObservation,
}

fn validate_export_request(request: &ExportRequest) -> Result<()> {
    if request.schema_version != offprint_model::PUBLIC_SCHEMA_VERSION {
        return Err(export_error(
            "offprint.input.schema_version",
            ErrorStage::Validation,
            "export request schema version is incompatible",
        )
        .with_detail("expected", offprint_model::PUBLIC_SCHEMA_VERSION)
        .with_detail("actual", request.schema_version));
    }
    if request.formats.is_empty() {
        return Err(export_error(
            "offprint.export.format",
            ErrorStage::Validation,
            "artifact export requires at least one format",
        ));
    }
    let mut kinds = BTreeSet::new();
    for format in &request.formats {
        if !kinds.insert(format.format()) {
            return Err(export_error(
                "offprint.export.format",
                ErrorStage::Validation,
                format!(
                    "artifact format `{:?}` is requested more than once",
                    format.format()
                ),
            ));
        }
    }
    if request.base_name.trim().is_empty() || request.base_name.len() > 512 {
        return Err(export_error(
            "offprint.export.name",
            ErrorStage::Validation,
            "artifact export base name must contain between 1 and 512 bytes",
        ));
    }
    Ok(())
}

async fn prepare_export_directory(path: &PortablePath) -> Result<()> {
    tokio::fs::create_dir_all(path).await.map_err(|error| {
        export_error(
            "offprint.export.output",
            ErrorStage::Validation,
            format!("failed to create the artifact export directory: {error}"),
        )
    })?;
    let metadata = tokio::fs::symlink_metadata(path).await.map_err(|error| {
        export_error(
            "offprint.export.output",
            ErrorStage::Validation,
            format!("failed to inspect the artifact export directory: {error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(export_error(
            "offprint.export.output",
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
                "offprint.export.verify",
                ErrorStage::Verification,
                "artifact format must be a bounded directly addressed file",
            ))
        }
        Err(BoundedFileReadError::Io(error)) => Err(export_error(
            "offprint.export.verify",
            ErrorStage::Verification,
            format!("failed to read the artifact format: {error}"),
        )),
    }
}

fn verify_file_format(format: ArtifactFormat, bytes: &[u8]) -> Result<FormatEvidence> {
    match format {
        ArtifactFormat::Html => Err(export_error(
            "offprint.export.verify",
            ErrorStage::Verification,
            "Offprint HTML uses the artifact verification service",
        )),
        ArtifactFormat::Pdf => offprint_export::verify_offprint_pdf(bytes),
        ArtifactFormat::Zip => offprint_export::verify_zip(bytes),
        ArtifactFormat::SelfExtractingHtml => offprint_export::verify_self_extracting(bytes),
        ArtifactFormat::Mhtml => offprint_export::verify_mhtml(bytes),
        ArtifactFormat::Markdown => Err(export_error(
            "offprint.export.verify",
            ErrorStage::Verification,
            "Markdown verification requires a directory bundle",
        )),
    }
}

fn prepare_file(name: String, verified: VerifiedFormat<Vec<u8>>) -> Result<PreparedArtifact> {
    let (bytes, evidence) = verified.into_parts();
    let verification = evidence.into_verification(
        u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        ContentDigest::sha256(&bytes),
    );
    PreparedArtifact::file(name, bytes, verification)
}

fn prepare_directory(
    name: String,
    entrypoint: &str,
    verified: VerifiedFormat<offprint_export::MarkdownBundle>,
    maximum_assets: usize,
    maximum_bytes: u64,
) -> Result<PreparedArtifact> {
    let (bundle, evidence) = verified.into_parts();
    bundle.validate_limits(maximum_assets, maximum_bytes, ErrorStage::Encoding)?;
    let directory = ArtifactDirectory::new(bundle.into_files())?;
    let verification = evidence.into_verification(directory.bytes(), directory.sha256());
    PreparedArtifact::directory(name, entrypoint, directory, verification)
}

fn validate_offline_observation(
    observation: &offprint_browser::OfflineBrowserObservation,
) -> Result<()> {
    if !observation.attempted_urls.is_empty() {
        return Err(export_error(
            "offprint.verification.network",
            ErrorStage::Verification,
            "artifact attempted a network request during format rendering",
        )
        .with_detail("attemptedRequests", observation.attempted_urls.len()));
    }
    if !observation.page_errors.is_empty() {
        return Err(export_error(
            "offprint.verification.page_error",
            ErrorStage::Verification,
            "artifact raised a page error during format rendering",
        )
        .with_detail("errors", observation.page_errors.clone()));
    }
    if !observation.stable {
        return Err(export_error(
            "offprint.verification.unstable",
            ErrorStage::Verification,
            "artifact did not reach a stable state before format rendering",
        ));
    }
    Ok(())
}

pub(super) fn export_error(
    code: &'static str,
    stage: ErrorStage,
    message: impl Into<String>,
) -> OffprintError {
    OffprintError::new(code, stage, message)
}
