use std::sync::Arc;
use std::time::{Duration, Instant};

use pageknot_artifact::{
    FileArtifactWriter, MemoryArtifact, MemoryArtifactWriter, StagedFileArtifact,
};
use pageknot_model::{
    ArtifactFormat, ArtifactKind, ArtifactManifest, ArtifactResult, ArtifactSpec, ArtifactTarget,
    BrowserInfo, CaptureEvent, CaptureId, CaptureRequest, CaptureResult, CaptureStatus,
    CaptureTerminalStatus, ContentDigest, ErrorStage, ManifestGenerator, ManifestSource,
    ManifestVerification, Milliseconds, PageKnotError, Result, StructuralRepair,
    VerificationPolicy,
};

use crate::capture_service::JobState;
use crate::runtime::RuntimeState;

use super::frames::CapturedIntermediate;
use super::{check_cancelled, offline, transition};

const ARTIFACT_VERSION: u32 = pageknot_model::ARTIFACT_FORMAT_VERSION;
const SCHEMA_VERSION: u32 = pageknot_model::PUBLIC_SCHEMA_VERSION;

pub(super) struct FinalizeContext<'a> {
    pub(super) runtime: &'a Arc<RuntimeState>,
    pub(super) job: &'a Arc<JobState>,
    pub(super) capture_id: &'a CaptureId,
    pub(super) request: &'a CaptureRequest,
    pub(super) browser_info: BrowserInfo,
    pub(super) total_started: Instant,
    pub(super) validation: Duration,
    pub(super) browser: Duration,
}

pub(super) async fn finalize(
    context: FinalizeContext<'_>,
    captured: CapturedIntermediate,
    mut writer: PreparedWriter,
) -> Result<CaptureResult> {
    let FinalizeContext {
        runtime,
        job,
        capture_id,
        request,
        browser_info,
        total_started,
        validation,
        browser,
    } = context;
    transition(job, CaptureStatus::Transforming).await?;
    job.emit(CaptureEvent::TransformStarted {
        capture_id: capture_id.clone(),
    });
    let transform_started = Instant::now();
    let transformed = pageknot_transform::transform_document(&captured.html)?;
    let transform = transform_started.elapsed();

    transition(job, CaptureStatus::Encoding).await?;
    let encoding_started = Instant::now();
    let policy_sha256 = policy_digest(request)?;
    let manifest = ArtifactManifest {
        schema_version: SCHEMA_VERSION,
        format: ArtifactFormat {
            kind: ArtifactKind::Html,
            version: ARTIFACT_VERSION,
        },
        generator: ManifestGenerator {
            name: "PageKnot".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        source: ManifestSource::from(captured.source.clone()),
        captured_at: runtime.now(),
        browser: browser_info,
        environment: request.environment.clone(),
        view_state: captured.view_state,
        policy_sha256,
        frames: captured.frames,
        resources: captured.resources,
        resource_records: captured.resource_records,
        warning_codes: captured
            .warnings
            .iter()
            .map(|warning| warning.code.clone())
            .collect(),
        structural_repair: StructuralRepair {
            applied: captured.structural_repair,
            script_sha256: captured
                .structural_repair
                .then(pageknot_html::structural_repair_script_digest),
        },
        verification: ManifestVerification {
            level: request.verification,
            passed: true,
            network_requests: 0,
        },
    };
    pageknot_transform::encode_artifact_to(&transformed, manifest, &mut writer)?;
    let encoded_bytes = writer.bytes();
    job.emit(CaptureEvent::ArtifactEncoding {
        capture_id: capture_id.clone(),
        bytes: encoded_bytes,
    });
    let mut staged = writer.finish()?;
    let (verification, encoding, verification_elapsed) = {
        let readback = staged.readback(request.limits.artifact_bytes)?;
        let encoding = encoding_started.elapsed();

        transition(job, CaptureStatus::Verifying).await?;
        job.emit(CaptureEvent::VerificationStarted {
            capture_id: capture_id.clone(),
        });
        let verification_started = Instant::now();
        let static_result = pageknot_html::verify_static(readback.as_bytes())?;
        let verification = match request.verification {
            VerificationPolicy::Static => static_result,
            VerificationPolicy::Offline => {
                offline::verify(
                    runtime,
                    job.cancellation(),
                    capture_id,
                    request,
                    readback.as_bytes(),
                    static_result,
                )
                .await?
            }
        };
        (verification, encoding, verification_started.elapsed())
    };
    check_cancelled(job.cancellation())?;

    transition(job, CaptureStatus::Committing).await?;
    job.claim_commit()?;
    let commit_started = Instant::now();
    let artifact = staged.commit()?;
    let commit = commit_started.elapsed();
    let mut timings = captured.timings;
    timings.total = Milliseconds::from(total_started.elapsed());
    timings.validation = Milliseconds::from(validation);
    timings.browser = Milliseconds::from(browser);
    timings.transform = Milliseconds::from(transform);
    timings.encoding = Milliseconds::from(encoding);
    timings.verification = Milliseconds::from(verification_elapsed);
    timings.commit = Milliseconds::from(commit);
    Ok(CaptureResult {
        schema_version: SCHEMA_VERSION,
        capture_id: capture_id.clone(),
        status: CaptureTerminalStatus::Succeeded,
        source: captured.source,
        artifact,
        verification,
        resources: captured.resources,
        warnings: captured.warnings,
        timings,
    })
}

#[derive(Debug)]
enum PreparedWriterTarget {
    File(FileArtifactWriter),
    Memory(MemoryArtifactWriter),
}

#[derive(Debug)]
pub(super) struct PreparedWriter {
    target: PreparedWriterTarget,
    maximum_bytes: u64,
    bytes: u64,
}

impl PreparedWriter {
    pub(super) fn new(spec: &ArtifactSpec, maximum_bytes: u64) -> Result<Self> {
        let ArtifactSpec::Html(spec) = spec;
        let (target, maximum_bytes) = match &spec.target {
            ArtifactTarget::File(path) => (
                PreparedWriterTarget::File(FileArtifactWriter::create(
                    path.as_utf8_path().to_owned(),
                    spec.conflict,
                )?),
                maximum_bytes,
            ),
            ArtifactTarget::Bytes { max_bytes } => (
                PreparedWriterTarget::Memory(MemoryArtifactWriter::new(*max_bytes)),
                maximum_bytes.min(*max_bytes),
            ),
        };
        Ok(Self {
            target,
            maximum_bytes,
            bytes: 0,
        })
    }

    const fn bytes(&self) -> u64 {
        self.bytes
    }

    fn finish(self) -> Result<PendingArtifact> {
        match self.target {
            PreparedWriterTarget::File(writer) => writer.finish().map(PendingArtifact::File),
            PreparedWriterTarget::Memory(writer) => writer.finish().map(PendingArtifact::Memory),
        }
    }
}

impl std::io::Write for PreparedWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let requested = u64::try_from(buffer.len()).unwrap_or(u64::MAX);
        let next = self.bytes.saturating_add(requested);
        if next > self.maximum_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::FileTooLarge,
                "artifact exceeds the configured byte limit",
            ));
        }
        let written = match &mut self.target {
            PreparedWriterTarget::File(writer) => writer.write(buffer)?,
            PreparedWriterTarget::Memory(writer) => writer.write(buffer)?,
        };
        self.bytes = self
            .bytes
            .saturating_add(u64::try_from(written).unwrap_or(u64::MAX));
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match &mut self.target {
            PreparedWriterTarget::File(writer) => writer.flush(),
            PreparedWriterTarget::Memory(writer) => writer.flush(),
        }
    }
}

#[derive(Debug)]
enum PendingArtifact {
    File(StagedFileArtifact),
    Memory(MemoryArtifact),
}

#[derive(Debug)]
enum ArtifactReadback<'a> {
    File(Vec<u8>),
    Memory(&'a [u8]),
}

impl ArtifactReadback<'_> {
    fn as_bytes(&self) -> &[u8] {
        match self {
            Self::File(bytes) => bytes,
            Self::Memory(bytes) => bytes,
        }
    }
}

impl PendingArtifact {
    fn readback(&mut self, maximum_bytes: u64) -> Result<ArtifactReadback<'_>> {
        match self {
            Self::File(staged) => staged.read_all(maximum_bytes).map(ArtifactReadback::File),
            Self::Memory(memory) => Ok(ArtifactReadback::Memory(memory.content())),
        }
    }

    fn commit(self) -> Result<ArtifactResult> {
        match self {
            Self::File(staged) => staged.commit(),
            Self::Memory(memory) => Ok(memory.into_result()),
        }
    }
}

fn policy_digest(request: &CaptureRequest) -> Result<ContentDigest> {
    serde_json::to_vec(request)
        .map(ContentDigest::sha256)
        .map_err(|error| {
            PageKnotError::new(
                "pageknot.artifact.policy",
                ErrorStage::Encoding,
                format!("capture policy could not be serialized: {error}"),
            )
        })
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::{PendingArtifact, PreparedWriter, PreparedWriterTarget};

    #[test]
    fn prepared_writer_rejects_the_crossing_write_before_growth() {
        let mut writer = PreparedWriter {
            target: PreparedWriterTarget::Memory(pageknot_artifact::MemoryArtifactWriter::new(16)),
            maximum_bytes: 4,
            bytes: 0,
        };

        assert!(writer.write_all(b"page").is_ok());
        assert_eq!(
            writer
                .write_all(b"knot")
                .as_ref()
                .map_err(std::io::Error::kind),
            Err(std::io::ErrorKind::FileTooLarge)
        );
        assert_eq!(writer.bytes(), 4);
        assert!(matches!(
            writer.finish(),
            Ok(PendingArtifact::Memory(memory)) if memory.content() == b"page"
        ));
    }
}
