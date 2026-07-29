use std::borrow::Cow;
use std::collections::BTreeSet;

use pageknot_export::VariantEvidence;
use pageknot_model::{
    ArtifactExportRequest, ArtifactExportResult, ArtifactInput, ArtifactManifest, ArtifactResult,
    ArtifactVariant, ArtifactVariantKind, ArtifactVariantVerification, BrowserSpec, CaptureId,
    CaptureResult, ContentDigest, ErrorStage, PageKnotError, PortablePath, Result,
    VerificationPolicy,
};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::markdown_bundle::read_markdown_bundle;
use super::{
    ArtifactService, BoundedFileReadError, offline_verification_result, read_bounded_file,
    read_input, stage_temporary_artifact,
};
use crate::export_transaction::{ExportTransaction, PreparedVariant, markdown_bundle_digest};
use crate::runtime::RuntimePageRequest;
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
        let (verification, manifest, rendered_pdf) = match pdf_options {
            Some(options) => {
                let (static_result, manifest) = pageknot_html::verify_static_with_manifest(&bytes)?;
                let rendered = self.render_pdf(&bytes, &manifest, options).await?;
                let verification =
                    offline_verification_result(static_result, rendered.observation)?;
                (verification, manifest, Some(rendered.bytes))
            }
            None => {
                let (verification, manifest) = self
                    .verify_html(&bytes, VerificationPolicy::Offline)
                    .await?;
                (verification, manifest, None)
            }
        };
        let source = OfflineHtmlArtifact::new(&bytes, manifest, &verification)?;
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
        let (_, manifest) = pageknot_html::verify_static_with_manifest(&bytes)?;
        let source = OfflineHtmlArtifact::new(&bytes, manifest, &capture.verification)?;
        self.export_validated(source, request, None).await
    }

    async fn export_validated(
        &self,
        source: OfflineHtmlArtifact<'_>,
        request: ArtifactExportRequest,
        mut rendered_pdf: Option<Vec<u8>>,
    ) -> Result<ArtifactExportResult> {
        let (html, manifest, verification) = source.into_parts();
        let stem = pageknot_artifact::portable_file_stem(&request.base_name);
        let mut prepared = Vec::with_capacity(request.variants.len());
        for variant in request.variants {
            prepared.push(
                self.prepare_variant(html, &manifest, &stem, variant, &mut rendered_pdf)
                    .await?,
            );
        }
        prepare_export_directory(&request.output_directory).await?;
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
        stem: &str,
        variant: ArtifactVariant,
        rendered_pdf: &mut Option<Vec<u8>>,
    ) -> Result<PreparedVariant> {
        match variant {
            ArtifactVariant::Pdf(options) => {
                let encoded = match rendered_pdf.take() {
                    Some(encoded) => encoded,
                    None => self.render_pdf(html, manifest, options).await?.bytes,
                };
                let evidence = pageknot_export::verify_pdf(&encoded)?;
                PreparedVariant::file(
                    format!("{stem}.pdf"),
                    ArtifactVariantKind::Pdf,
                    encoded,
                    evidence,
                    MAXIMUM_EXPORT_BYTES,
                )
            }
            ArtifactVariant::Markdown(options) => {
                let encoded = pageknot_export::encode_markdown(html, manifest, options)?;
                PreparedVariant::markdown(
                    format!("{stem}-markdown"),
                    encoded,
                    MAXIMUM_MARKDOWN_ASSETS,
                    MAXIMUM_EXPORT_BYTES,
                )
            }
            ArtifactVariant::Zip => {
                let encoded = pageknot_export::encode_zip(html, manifest)?;
                let evidence = pageknot_export::verify_zip(&encoded)?;
                PreparedVariant::file(
                    format!("{stem}.zip"),
                    ArtifactVariantKind::Zip,
                    encoded,
                    evidence,
                    MAXIMUM_EXPORT_BYTES,
                )
            }
            ArtifactVariant::SelfExtracting => {
                let encoded = pageknot_export::encode_self_extracting(html)?;
                let evidence = pageknot_export::verify_self_extracting(&encoded)?;
                PreparedVariant::file(
                    format!("{stem}.compressed.html"),
                    ArtifactVariantKind::SelfExtracting,
                    encoded,
                    evidence,
                    MAXIMUM_EXPORT_BYTES,
                )
            }
            ArtifactVariant::Mhtml => {
                let encoded = pageknot_export::encode_mhtml(html, manifest)?;
                let evidence = pageknot_export::verify_mhtml(&encoded)?;
                PreparedVariant::file(
                    format!("{stem}.mhtml"),
                    ArtifactVariantKind::Mhtml,
                    encoded,
                    evidence,
                    MAXIMUM_EXPORT_BYTES,
                )
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
        }
        .await;
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
