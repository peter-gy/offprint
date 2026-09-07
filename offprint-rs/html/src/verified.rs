use offprint_document::Document;
use offprint_model::{ArtifactManifest, ContentDigest, VerificationReport};

/// Static HTML proof whose storage can cross an asynchronous boundary.
#[derive(Debug)]
pub struct VerifiedHtmlProof<B> {
    bytes: B,
    manifest: ArtifactManifest,
    sha256: ContentDigest,
    verification: VerificationReport,
}

impl<B> VerifiedHtmlProof<B> {
    /// Returns the validated artifact manifest.
    #[must_use]
    pub const fn manifest(&self) -> &ArtifactManifest {
        &self.manifest
    }

    /// Returns the digest of the exact artifact bytes.
    #[must_use]
    pub const fn sha256(&self) -> ContentDigest {
        self.sha256
    }

    /// Returns the successful static verification record.
    #[must_use]
    pub const fn verification(&self) -> &VerificationReport {
        &self.verification
    }

    /// Decomposes the proof into its bound values.
    #[must_use]
    pub fn into_parts(self) -> (B, ArtifactManifest, ContentDigest, VerificationReport) {
        (self.bytes, self.manifest, self.sha256, self.verification)
    }
}

impl<B: AsRef<[u8]>> VerifiedHtmlProof<B> {
    /// Returns the exact bytes covered by the digest and verification record.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_ref()
    }
}

/// HTML bytes bound to the parsed document and its static proof.
#[derive(Debug)]
pub struct VerifiedHtml<B> {
    proof: VerifiedHtmlProof<B>,
    document: Document,
}

impl<B> VerifiedHtml<B> {
    pub(crate) const fn new(
        bytes: B,
        document: Document,
        manifest: ArtifactManifest,
        sha256: ContentDigest,
        verification: VerificationReport,
    ) -> Self {
        Self {
            proof: VerifiedHtmlProof {
                bytes,
                manifest,
                sha256,
                verification,
            },
            document,
        }
    }

    /// Returns the parsed artifact document used by static verification.
    #[must_use]
    pub const fn document(&self) -> &Document {
        &self.document
    }

    /// Returns the validated artifact manifest.
    #[must_use]
    pub const fn manifest(&self) -> &ArtifactManifest {
        self.proof.manifest()
    }

    /// Returns the digest of the exact artifact bytes.
    #[must_use]
    pub const fn sha256(&self) -> ContentDigest {
        self.proof.sha256()
    }

    /// Returns the successful static verification record.
    #[must_use]
    pub const fn verification(&self) -> &VerificationReport {
        self.proof.verification()
    }

    pub(crate) fn into_verification_and_manifest(self) -> (VerificationReport, ArtifactManifest) {
        let (_bytes, manifest, _sha256, verification) = self.proof.into_parts();
        (verification, manifest)
    }

    /// Drops the parsed document and retains the portable static proof.
    #[must_use]
    pub fn into_proof(self) -> VerifiedHtmlProof<B> {
        self.proof
    }

    /// Decomposes the verified HTML into its bound values.
    #[must_use]
    pub fn into_parts(
        self,
    ) -> (
        B,
        Document,
        ArtifactManifest,
        ContentDigest,
        VerificationReport,
    ) {
        let (bytes, manifest, sha256, verification) = self.proof.into_parts();
        (bytes, self.document, manifest, sha256, verification)
    }
}

impl<B: AsRef<[u8]>> VerifiedHtml<B> {
    /// Returns the exact bytes covered by the digest and verification record.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.proof.bytes()
    }
}
