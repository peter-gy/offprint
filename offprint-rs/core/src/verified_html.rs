use offprint_model::{
    ArtifactManifest, ErrorStage, OffprintError, Result, VerificationMode, VerificationReport,
};

#[derive(Debug)]
pub(crate) struct OfflineHtmlArtifact<'a> {
    html: offprint_html::VerifiedHtmlProof<&'a [u8]>,
    verification: &'a VerificationReport,
}

impl<'a> OfflineHtmlArtifact<'a> {
    pub(crate) fn from_static_proof(
        html: offprint_html::VerifiedHtmlProof<&'a [u8]>,
        verification: &'a VerificationReport,
    ) -> Result<Self> {
        let bytes_len = u64::try_from(html.bytes().len()).unwrap_or(u64::MAX);
        if verification.schema_version != offprint_model::PUBLIC_SCHEMA_VERSION
            || verification.mode != VerificationMode::Offline
            || verification.bytes != bytes_len
            || verification.artifact_sha256 != html.sha256()
            || verification.network_requests != 0
        {
            return Err(OffprintError::new(
                "offprint.verification.record",
                ErrorStage::Verification,
                "representation source requires successful offline HTML verification",
            ));
        }
        Ok(Self { html, verification })
    }

    pub(crate) fn into_parts(self) -> (&'a [u8], ArtifactManifest, &'a VerificationReport) {
        let (bytes, manifest, _sha256, _static_verification) = self.html.into_parts();
        (bytes, manifest, self.verification)
    }
}
