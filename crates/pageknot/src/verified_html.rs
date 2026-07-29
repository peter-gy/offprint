use pageknot_model::{
    ArtifactManifest, ErrorStage, PageKnotError, Result, VerificationPolicy, VerificationResult,
};

#[derive(Debug)]
pub(crate) struct OfflineHtmlArtifact<'a> {
    html: pageknot_html::VerifiedHtmlProof<&'a [u8]>,
    verification: &'a VerificationResult,
}

impl<'a> OfflineHtmlArtifact<'a> {
    pub(crate) fn from_static_proof(
        html: pageknot_html::VerifiedHtmlProof<&'a [u8]>,
        verification: &'a VerificationResult,
    ) -> Result<Self> {
        let bytes_len = u64::try_from(html.bytes().len()).unwrap_or(u64::MAX);
        if verification.schema_version != pageknot_model::PUBLIC_SCHEMA_VERSION
            || verification.level != VerificationPolicy::Offline
            || !verification.passed
            || verification.bytes != bytes_len
            || verification.artifact_sha256 != html.sha256()
            || verification.network_requests != 0
            || !verification.attempted_urls.is_empty()
            || !verification.page_errors.is_empty()
            || !verification.frame_failures.is_empty()
            || !verification.stable
        {
            return Err(PageKnotError::new(
                "pageknot.verification.record",
                ErrorStage::Verification,
                "representation source requires successful offline HTML verification",
            ));
        }
        Ok(Self { html, verification })
    }

    pub(crate) fn into_parts(self) -> (&'a [u8], ArtifactManifest, &'a VerificationResult) {
        let (bytes, manifest, _sha256, _static_verification) = self.html.into_parts();
        (bytes, manifest, self.verification)
    }
}
