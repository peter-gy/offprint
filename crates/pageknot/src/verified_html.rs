use pageknot_model::{
    ArtifactManifest, ContentDigest, ErrorStage, PageKnotError, Result, VerificationPolicy,
    VerificationResult,
};

#[derive(Debug)]
pub(crate) struct OfflineHtmlArtifact<'a> {
    bytes: &'a [u8],
    manifest: ArtifactManifest,
    verification: &'a VerificationResult,
}

impl<'a> OfflineHtmlArtifact<'a> {
    pub(crate) fn new(
        bytes: &'a [u8],
        manifest: ArtifactManifest,
        verification: &'a VerificationResult,
    ) -> Result<Self> {
        let bytes_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if verification.schema_version != pageknot_model::PUBLIC_SCHEMA_VERSION
            || verification.level != VerificationPolicy::Offline
            || !verification.passed
            || verification.bytes != bytes_len
            || verification.artifact_sha256 != ContentDigest::sha256(bytes)
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
        Ok(Self {
            bytes,
            manifest,
            verification,
        })
    }

    pub(crate) fn into_parts(self) -> (&'a [u8], ArtifactManifest, &'a VerificationResult) {
        (self.bytes, self.manifest, self.verification)
    }
}
