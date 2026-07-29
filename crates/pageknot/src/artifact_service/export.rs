use std::borrow::Cow;
use std::collections::BTreeSet;

use pageknot_artifact::{
    ArtifactDirectory, ArtifactTransaction, ArtifactTransactionLimits, PreparedArtifact,
};
use pageknot_export::{VariantEvidence, VerifiedVariant};
use pageknot_model::{
    ArtifactExportRequest, ArtifactExportResult, ArtifactInput, ArtifactManifest, ArtifactResult,
    ArtifactVariant, ArtifactVariantKind, ArtifactVariantVerification, BrowserSpec, CaptureId,
    CaptureResult, ContentDigest, ErrorStage, PageKnotError, PortablePath, Result,
    VerificationPolicy,
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
        let bytes = read_input(input).await?;
        let pdf_options = request.variants.iter().find_map(|variant| match variant {
            ArtifactVariant::Pdf(options) => Some(*options),
            _ => None,
        });
        let (verification, proof, rendered_pdf) = match pdf_options {
            Some(options) => {
                let proof = pageknot_html::verify_html(bytes.as_slice())?.into_proof();
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
                    .verify_html_proof(bytes.as_slice(), VerificationPolicy::Offline)
                    .await?;
                (verification, proof, None)
            }
        };
        let source = OfflineHtmlArtifact::from_static_proof(proof, &verification)?;
        self.export_validated(source, request, rendered_pdf).await
    }

    /// Derives verified artifact variants from a completed offline capture.
    ///
    /// The capture's HTML bytes and verification record must describe the same
    /// artifact. Reusing that proof avoids a second offline browser reopen.
    pub async fn export_capture(
        &self,
        capture: &CaptureResult,
        request: ArtifactExportRequest,
    ) -> Result<ArtifactExportResult> {
        self.state.ensure_open()?;
        validate_export_request(&request)?;
        let bytes = match &capture.artifact {
            ArtifactResult::File { path, .. } => {
                Cow::Owned(read_input(ArtifactInput::File(path.clone())).await?)
            }
            ArtifactResult::Bytes { content, .. } => Cow::Borrowed(content.as_slice()),
        };
        let proof = pageknot_html::verify_html(bytes.as_ref())?.into_proof();
        let source = OfflineHtmlArtifact::from_static_proof(proof, &capture.verification)?;
        self.export_validated(source, request, None).await
    }

    async fn export_validated(
        &self,
        source: OfflineHtmlArtifact<'_>,
        request: ArtifactExportRequest,
        mut rendered_pdf: Option<Vec<u8>>,
    ) -> Result<ArtifactExportResult> {
        let (html, manifest, verification) = source.into_parts();
        let source_artifact_sha256 = verification.artifact_sha256;
        let stem = pageknot_artifact::portable_file_stem(&request.base_name);
        let mut prepared = Vec::with_capacity(request.variants.len());
        for variant in request.variants {
            prepared.push(
                self.prepare_variant(
                    html,
                    &manifest,
                    source_artifact_sha256,
                    &stem,
                    variant,
                    &mut rendered_pdf,
                )
                .await?,
            );
        }
        prepare_export_directory(&request.output_directory).await?;
        let variants = ArtifactTransaction::stage(
            &request.output_directory,
            request.conflict,
            prepared,
            ArtifactTransactionLimits::new(
                MAXIMUM_MARKDOWN_ASSETS.saturating_add(1),
                MAXIMUM_EXPORT_BYTES,
            ),
        )?
        .commit()?;
        Ok(ArtifactExportResult {
            schema_version: pageknot_model::PUBLIC_SCHEMA_VERSION,
            source_artifact_sha256: verification.artifact_sha256,
            policy_sha256: manifest.policy_sha256,
            resources: manifest.resources,
            variants,
        })
    }

    async fn prepare_variant(
        &self,
        html: &[u8],
        manifest: &ArtifactManifest,
        source_artifact_sha256: ContentDigest,
        stem: &str,
        variant: ArtifactVariant,
        rendered_pdf: &mut Option<Vec<u8>>,
    ) -> Result<PreparedArtifact> {
        match variant {
            ArtifactVariant::Pdf(options) => {
                let rendered = match rendered_pdf.take() {
                    Some(encoded) => encoded,
                    None => self.render_pdf(html, manifest, options).await?.bytes,
                };
                let verified = pageknot_export::prepare_pdf(
                    &rendered,
                    html,
                    manifest,
                    source_artifact_sha256,
                    MAXIMUM_EXPORT_BYTES,
                )?;
                prepare_file(format!("{stem}.pdf"), verified)
            }
            ArtifactVariant::Markdown(options) => {
                let verified = pageknot_export::prepare_markdown(html, manifest, options)?;
                prepare_directory(
                    format!("{stem}-markdown"),
                    "index.md",
                    verified,
                    MAXIMUM_MARKDOWN_ASSETS,
                    MAXIMUM_EXPORT_BYTES,
                )
            }
            ArtifactVariant::Zip => {
                let verified = pageknot_export::prepare_zip(html, manifest)?;
                prepare_file(format!("{stem}.zip"), verified)
            }
            ArtifactVariant::SelfExtracting => {
                let verified = pageknot_export::prepare_self_extracting(html)?;
                prepare_file(format!("{stem}.compressed.html"), verified)
            }
            ArtifactVariant::Mhtml => {
                let verified = pageknot_export::prepare_mhtml(html, manifest)?;
                prepare_file(format!("{stem}.mhtml"), verified)
            }
        }
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
                let directory = ArtifactDirectory::new(bundle.into_files())?;
                Ok(evidence.into_verification(directory.bytes(), directory.sha256()))
            }
            ArtifactVariantKind::Pdf
            | ArtifactVariantKind::Zip
            | ArtifactVariantKind::SelfExtracting
            | ArtifactVariantKind::Mhtml => {
                let bytes = read_export_file(&path).await?;
                let evidence = verify_file_variant(kind, &bytes)?;
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
        options: pageknot_model::PdfOptions,
    ) -> Result<RenderedPdf> {
        let file =
            stage_temporary_artifact("pageknot-pdf-", ".html", html, "PDF source HTML").await?;
        let url = Url::from_file_path(file.path()).map_err(|()| {
            PageKnotError::new(
                "pageknot.verification.path",
                ErrorStage::Encoding,
                "PDF source path cannot be represented as a file URL",
            )
        })?;
        let source_url = Url::parse(manifest.source.final_url.as_str()).map_err(|error| {
            PageKnotError::new(
                "pageknot.input.artifact",
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
                resource_observation: pageknot_browser::ResourceObservationLimits::default(),
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
    observation: pageknot_browser::OfflineBrowserObservation,
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
        ArtifactVariantKind::Pdf => pageknot_export::verify_pageknot_pdf(bytes),
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

fn prepare_file(name: String, verified: VerifiedVariant<Vec<u8>>) -> Result<PreparedArtifact> {
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
    verified: VerifiedVariant<pageknot_export::MarkdownBundle>,
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
    observation: &pageknot_browser::OfflineBrowserObservation,
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
        .with_detail("errors", observation.page_errors.clone()));
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

pub(super) fn export_error(
    code: &'static str,
    stage: ErrorStage,
    message: impl Into<String>,
) -> PageKnotError {
    PageKnotError::new(code, stage, message)
}
