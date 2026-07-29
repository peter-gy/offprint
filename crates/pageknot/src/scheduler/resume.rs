use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;

use pageknot_artifact::FileArtifactWriter;
use pageknot_model::{
    ArtifactResult, ArtifactSpec, ArtifactTarget, BatchRequest, ConflictPolicy, ContentDigest,
    CrawlFrontierItem, CrawlRequest, ErrorStage, PortablePath, Result, ResumeJobRecord,
    ResumeJobStatus, ResumeManifest, ResumeOptions, ScheduleKind, ScheduledCaptureOutcome,
};
use sha2::{Digest as _, Sha256};
use tokio::io::AsyncReadExt as _;

use super::{SCHEMA_VERSION, scheduler_error, sort_frontier};

const MAXIMUM_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;

pub(super) async fn load_or_create_manifest(
    options: Option<&ResumeOptions>,
    kind: ScheduleKind,
    plan_sha256: ContentDigest,
    pending: Vec<String>,
    frontier: Vec<CrawlFrontierItem>,
) -> Result<ResumeManifest> {
    let Some(options) = options else {
        return Ok(new_manifest(kind, plan_sha256, pending, frontier));
    };
    let metadata = tokio::fs::symlink_metadata(&options.manifest).await;
    let mut manifest = match metadata {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(scheduler_error(
                    "pageknot.scheduler.manifest_read",
                    ErrorStage::Validation,
                    "resume manifest must be a directly addressed regular file",
                ));
            }
            if metadata.len() > MAXIMUM_MANIFEST_BYTES {
                return Err(scheduler_error(
                    "pageknot.scheduler.manifest_read",
                    ErrorStage::Validation,
                    "resume manifest exceeds the supported byte limit",
                ));
            }
            let bytes = tokio::fs::read(&options.manifest).await.map_err(|error| {
                scheduler_error(
                    "pageknot.scheduler.manifest_read",
                    ErrorStage::Validation,
                    format!("failed to read the resume manifest: {error}"),
                )
            })?;
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_MANIFEST_BYTES {
                return Err(scheduler_error(
                    "pageknot.scheduler.manifest_read",
                    ErrorStage::Validation,
                    "resume manifest exceeds the supported byte limit",
                ));
            }
            serde_json::from_slice::<ResumeManifest>(&bytes).map_err(|error| {
                scheduler_error(
                    "pageknot.scheduler.manifest_read",
                    ErrorStage::Validation,
                    format!("resume manifest is invalid: {error}"),
                )
            })?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            new_manifest(kind, plan_sha256, pending.clone(), frontier.clone())
        }
        Err(error) => {
            return Err(scheduler_error(
                "pageknot.scheduler.manifest_read",
                ErrorStage::Validation,
                format!("failed to inspect the resume manifest: {error}"),
            ));
        }
    };
    if manifest.schema_version != SCHEMA_VERSION
        || manifest.kind != kind
        || manifest.plan_sha256 != plan_sha256
    {
        return Err(scheduler_error(
            "pageknot.scheduler.manifest_mismatch",
            ErrorStage::Validation,
            "resume manifest does not describe this scheduler plan",
        ));
    }
    if manifest.frontier.is_empty() && manifest.jobs.is_empty() && kind == ScheduleKind::Crawl {
        manifest.pending = pending;
        manifest.frontier = frontier;
    }
    Ok(manifest)
}

fn new_manifest(
    kind: ScheduleKind,
    plan_sha256: ContentDigest,
    pending: Vec<String>,
    frontier: Vec<CrawlFrontierItem>,
) -> ResumeManifest {
    ResumeManifest {
        schema_version: SCHEMA_VERSION,
        kind,
        plan_sha256,
        jobs: BTreeMap::new(),
        pending,
        frontier,
    }
}

pub(super) async fn persist_manifest_if_configured(
    options: Option<&ResumeOptions>,
    manifest: &ResumeManifest,
) -> Result<()> {
    let Some(options) = options else {
        return Ok(());
    };
    let mut bytes = serde_json::to_vec_pretty(manifest).map_err(|error| {
        scheduler_error(
            "pageknot.scheduler.manifest_write",
            ErrorStage::Commit,
            format!("resume manifest could not be serialized: {error}"),
        )
    })?;
    bytes.push(b'\n');
    let mut writer = FileArtifactWriter::create(
        options.manifest.as_utf8_path().to_owned(),
        ConflictPolicy::Replace,
    )
    .map_err(|error| {
        scheduler_error(
            "pageknot.scheduler.manifest_write",
            ErrorStage::Commit,
            "resume manifest staging failed",
        )
        .with_source(error)
    })?;
    writer.write_all(&bytes).map_err(|error| {
        scheduler_error(
            "pageknot.scheduler.manifest_write",
            ErrorStage::Commit,
            format!("failed to stage the resume manifest: {error}"),
        )
    })?;
    writer
        .finish()
        .and_then(pageknot_artifact::StagedFileArtifact::commit)
        .map(|_| ())
        .map_err(|error| {
            scheduler_error(
                "pageknot.scheduler.manifest_write",
                ErrorStage::Commit,
                "failed to commit the resume manifest",
            )
            .with_source(error)
        })
}

pub(super) fn validate_batch_manifest(
    manifest: &ResumeManifest,
    request: &BatchRequest,
    request_digests: &[ContentDigest],
) -> Result<()> {
    if !manifest.frontier.is_empty() {
        return Err(manifest_shape_error(
            "batch resume manifests cannot contain a crawl frontier",
        ));
    }
    let mut jobs = BTreeMap::new();
    for (job, digest) in request.jobs.iter().zip(request_digests) {
        let ArtifactSpec::Html(options) = &job.request.artifact;
        let ArtifactTarget::File(path) = &options.target else {
            return Err(scheduler_error(
                "pageknot.scheduler.resume_target",
                ErrorStage::Validation,
                "resumable batch jobs require file artifact targets",
            ));
        };
        jobs.insert(job.id.as_str(), (*digest, path));
    }
    for (id, record) in &manifest.jobs {
        let Some((expected_digest, expected_path)) = jobs.get(id.as_str()) else {
            return Err(
                manifest_shape_error("batch resume manifest contains an unknown job")
                    .with_detail("jobId", id.as_str()),
            );
        };
        if record.request_sha256 != *expected_digest {
            return Err(
                manifest_shape_error("batch resume job digest does not match its request")
                    .with_detail("jobId", id.as_str()),
            );
        }
        if record.url.is_some() || record.depth.is_some() || record.ordinal.is_some() {
            return Err(
                manifest_shape_error("batch resume job contains crawl coordinates")
                    .with_detail("jobId", id.as_str()),
            );
        }
        validate_terminal_record(record)
            .map_err(|error| error.with_detail("jobId", id.as_str()))?;
        if record.status == ResumeJobStatus::Succeeded
            && record.artifact_path.as_ref() != Some(*expected_path)
        {
            return Err(manifest_shape_error(
                "batch resume artifact path does not match its request",
            )
            .with_detail("jobId", id.as_str()));
        }
    }

    let expected_pending = request
        .jobs
        .iter()
        .filter(|job| !manifest.jobs.contains_key(&job.id))
        .map(|job| job.id.clone())
        .collect::<Vec<_>>();
    if manifest.pending != expected_pending {
        return Err(manifest_shape_error(
            "batch resume pending jobs do not match terminal records",
        ));
    }
    Ok(())
}

pub(super) fn validate_crawl_manifest(
    manifest: &ResumeManifest,
    request: &CrawlRequest,
) -> Result<()> {
    let total = manifest.jobs.len().saturating_add(manifest.frontier.len());
    if total > usize::try_from(request.maximum_pages).unwrap_or(usize::MAX) {
        return Err(manifest_shape_error(
            "crawl resume manifest exceeds the configured page limit",
        ));
    }

    let mut urls = BTreeSet::new();
    let mut ordinals = BTreeSet::new();
    let mut root = request.seed.url.clone();
    root.set_fragment(None);
    for (id, record) in &manifest.jobs {
        validate_terminal_record(record)
            .map_err(|error| error.with_detail("jobId", id.as_str()))?;
        let (Some(url), Some(depth), Some(ordinal)) =
            (record.url.as_ref(), record.depth, record.ordinal)
        else {
            return Err(
                manifest_shape_error("crawl resume job is missing URL, depth, or ordinal")
                    .with_detail("jobId", id.as_str()),
            );
        };
        if id != url.as_str() {
            return Err(
                manifest_shape_error("crawl resume job identifier does not match its URL")
                    .with_detail("jobId", id.as_str()),
            );
        }
        validate_crawl_coordinates(
            url,
            depth,
            ordinal,
            request,
            &root,
            &mut urls,
            &mut ordinals,
        )?;
        let item = CrawlFrontierItem {
            url: url.clone(),
            depth,
            ordinal,
        };
        let capture_request = super::crawl_capture_request(request, &item);
        let expected_digest = super::digest_serializable(&capture_request)?;
        if record.request_sha256 != expected_digest {
            return Err(
                manifest_shape_error("crawl resume job digest does not match its request")
                    .with_detail("jobId", id.as_str()),
            );
        }
        let expected_path = super::crawl_output_path(request, &item);
        if record.status == ResumeJobStatus::Succeeded
            && record.artifact_path.as_ref() != Some(&expected_path)
        {
            return Err(manifest_shape_error(
                "crawl resume artifact path does not match its request",
            )
            .with_detail("jobId", id.as_str()));
        }
    }
    for item in &manifest.frontier {
        validate_crawl_coordinates(
            &item.url,
            item.depth,
            item.ordinal,
            request,
            &root,
            &mut urls,
            &mut ordinals,
        )?;
    }
    if !ordinals.contains(&0) {
        return Err(manifest_shape_error(
            "crawl resume manifest is missing its seed page",
        ));
    }

    let expected_pending = manifest
        .frontier
        .iter()
        .map(|item| item.url.as_str().to_owned())
        .collect::<Vec<_>>();
    if manifest.pending != expected_pending {
        return Err(manifest_shape_error(
            "crawl resume pending jobs do not match the frontier",
        ));
    }
    Ok(())
}

fn validate_crawl_coordinates(
    url: &url::Url,
    depth: u16,
    ordinal: u32,
    request: &CrawlRequest,
    root: &url::Url,
    urls: &mut BTreeSet<String>,
    ordinals: &mut BTreeSet<u32>,
) -> Result<()> {
    if !matches!(url.scheme(), "http" | "https") || url.fragment().is_some() {
        return Err(manifest_shape_error(
            "crawl resume URLs must be normalized HTTP or HTTPS URLs",
        ));
    }
    if request.same_origin && url.origin() != request.seed.url.origin() {
        return Err(
            manifest_shape_error("crawl resume URL is outside the configured origin")
                .with_detail("url", url.as_str()),
        );
    }
    if (depth == 0 || ordinal == 0 || url == root) && (url != root || depth != 0 || ordinal != 0) {
        return Err(manifest_shape_error(
            "crawl resume seed must retain URL, depth, and ordinal zero",
        )
        .with_detail("url", url.as_str()));
    }
    if depth > request.maximum_depth {
        return Err(
            manifest_shape_error("crawl resume depth exceeds the configured limit")
                .with_detail("url", url.as_str()),
        );
    }
    if ordinal >= request.maximum_pages {
        return Err(
            manifest_shape_error("crawl resume ordinal exceeds the configured page limit")
                .with_detail("url", url.as_str()),
        );
    }
    if !urls.insert(url.as_str().to_owned()) {
        return Err(
            manifest_shape_error("crawl resume manifest contains a duplicate URL")
                .with_detail("url", url.as_str()),
        );
    }
    if !ordinals.insert(ordinal) {
        return Err(
            manifest_shape_error("crawl resume manifest contains a duplicate ordinal")
                .with_detail("ordinal", ordinal),
        );
    }
    Ok(())
}

fn validate_terminal_record(record: &ResumeJobRecord) -> Result<()> {
    let valid = match record.status {
        ResumeJobStatus::Succeeded => {
            record.capture_id.is_some()
                && record.artifact_sha256.is_some()
                && record.artifact_path.is_some()
                && record.error.is_none()
        }
        ResumeJobStatus::Failed => {
            record.capture_id.is_none()
                && record.artifact_sha256.is_none()
                && record.artifact_path.is_none()
                && record.error.is_some()
        }
    };
    if valid {
        Ok(())
    } else {
        Err(manifest_shape_error(
            "resume job terminal fields do not match its status",
        ))
    }
}

pub(super) fn record_from_outcome(outcome: &ScheduledCaptureOutcome) -> ResumeJobRecord {
    match outcome {
        ScheduledCaptureOutcome::Succeeded {
            request_sha256,
            result,
            ..
        } => {
            let (artifact_path, artifact_sha256) = match &result.artifact {
                ArtifactResult::File { path, sha256, .. } => (Some(path.clone()), Some(*sha256)),
                ArtifactResult::Bytes { sha256, .. } => (None, Some(*sha256)),
            };
            ResumeJobRecord {
                request_sha256: *request_sha256,
                status: ResumeJobStatus::Succeeded,
                capture_id: Some(result.capture_id.clone()),
                artifact_sha256,
                artifact_path,
                error: None,
                url: None,
                depth: None,
                ordinal: None,
            }
        }
        ScheduledCaptureOutcome::Failed {
            request_sha256,
            error,
            ..
        } => ResumeJobRecord {
            request_sha256: *request_sha256,
            status: ResumeJobStatus::Failed,
            capture_id: None,
            artifact_sha256: None,
            artifact_path: None,
            error: Some(error.clone()),
            url: None,
            depth: None,
            ordinal: None,
        },
        ScheduledCaptureOutcome::Resumed { record, .. } => record.clone(),
    }
}

pub(super) async fn resume_artifact_is_current(record: &ResumeJobRecord) -> bool {
    let (Some(path), Some(expected)) = (&record.artifact_path, record.artifact_sha256) else {
        return false;
    };
    file_sha256(path)
        .await
        .is_ok_and(|actual| actual == expected)
}

async fn file_sha256(path: &PortablePath) -> Result<ContentDigest> {
    let metadata = tokio::fs::symlink_metadata(path).await.map_err(|error| {
        scheduler_error(
            "pageknot.scheduler.manifest_read",
            ErrorStage::Validation,
            format!("failed to inspect a resumed artifact: {error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(scheduler_error(
            "pageknot.scheduler.manifest_read",
            ErrorStage::Validation,
            "resumed artifact must be a directly addressed regular file",
        ));
    }
    let mut file = tokio::fs::File::open(path).await.map_err(|error| {
        scheduler_error(
            "pageknot.scheduler.manifest_read",
            ErrorStage::Validation,
            format!("failed to open a resumed artifact: {error}"),
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).await.map_err(|error| {
            scheduler_error(
                "pageknot.scheduler.manifest_read",
                ErrorStage::Validation,
                format!("failed to hash a resumed artifact: {error}"),
            )
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(ContentDigest::from_bytes(hasher.finalize().into()))
}

pub(super) async fn refresh_crawl_resume_state(
    request: &CrawlRequest,
    manifest: &mut ResumeManifest,
) -> Result<()> {
    let retry_failed = request
        .resume
        .as_ref()
        .is_some_and(|options| options.retry_failed);
    let mut requeue = Vec::new();
    let keys = manifest.jobs.keys().cloned().collect::<Vec<_>>();
    for key in keys {
        let Some(record) = manifest.jobs.get(&key) else {
            continue;
        };
        let retry = match record.status {
            ResumeJobStatus::Succeeded => !resume_artifact_is_current(record).await,
            ResumeJobStatus::Failed => retry_failed,
        };
        if retry {
            let (Some(url), Some(depth), Some(ordinal)) =
                (record.url.clone(), record.depth, record.ordinal)
            else {
                return Err(manifest_shape_error(
                    "crawl resume job is missing URL, depth, or ordinal",
                )
                .with_detail("jobId", key));
            };
            requeue.push(CrawlFrontierItem {
                url,
                depth,
                ordinal,
            });
            manifest.jobs.remove(&key);
        }
    }
    manifest.frontier.extend(requeue);
    sort_frontier(&mut manifest.frontier);
    manifest
        .frontier
        .dedup_by(|left, right| left.url == right.url);
    Ok(())
}

fn manifest_shape_error(message: impl Into<String>) -> pageknot_model::PageKnotError {
    scheduler_error(
        "pageknot.scheduler.manifest_shape",
        ErrorStage::Validation,
        message,
    )
}

#[cfg(test)]
mod tests {
    use pageknot_model::{
        BatchJob, CaptureId, CaptureRequest, CrawlRequest, PageKnotError, ResumeOptions,
    };

    use super::*;

    fn record(status: ResumeJobStatus) -> ResumeJobRecord {
        ResumeJobRecord {
            request_sha256: ContentDigest::sha256("request"),
            status,
            capture_id: None,
            artifact_sha256: None,
            artifact_path: None,
            error: None,
            url: None,
            depth: None,
            ordinal: None,
        }
    }

    #[test]
    fn succeeded_record_requires_capture_and_file_artifact_identity() {
        let mut record = record(ResumeJobStatus::Succeeded);
        record.capture_id = Some(CaptureId::new());
        record.artifact_sha256 = Some(ContentDigest::sha256("artifact"));

        assert_eq!(
            validate_terminal_record(&record)
                .as_ref()
                .map_err(|error| error.code.as_str()),
            Err("pageknot.scheduler.manifest_shape")
        );

        record.artifact_path = Some(PortablePath::new("capture.html"));
        assert!(validate_terminal_record(&record).is_ok());
    }

    #[test]
    fn failed_record_requires_error_and_no_artifact_identity() {
        let mut record = record(ResumeJobStatus::Failed);
        record.error = Some(PageKnotError::new(
            "pageknot.runtime.job",
            ErrorStage::Internal,
            "fixture failure",
        ));

        assert!(validate_terminal_record(&record).is_ok());

        record.artifact_sha256 = Some(ContentDigest::sha256("artifact"));
        assert_eq!(
            validate_terminal_record(&record)
                .as_ref()
                .map_err(|error| error.code.as_str()),
            Err("pageknot.scheduler.manifest_shape")
        );
    }

    #[test]
    fn batch_manifest_binds_succeeded_records_to_the_requested_artifact()
    -> Result<(), Box<dyn std::error::Error>> {
        let capture = CaptureRequest::builder("https://example.com")?
            .output("expected.html")
            .build()?;
        let digest = super::super::digest_serializable(&capture)?;
        let request = BatchRequest {
            jobs: vec![BatchJob {
                id: "page".to_owned(),
                request: capture,
            }],
            concurrency: 1,
            resume: Some(ResumeOptions {
                manifest: PortablePath::new("resume.json"),
                retry_failed: false,
            }),
        };
        let mut record = record(ResumeJobStatus::Succeeded);
        record.request_sha256 = digest;
        record.capture_id = Some(CaptureId::new());
        record.artifact_sha256 = Some(ContentDigest::sha256("artifact"));
        record.artifact_path = Some(PortablePath::new("different.html"));
        let manifest = ResumeManifest {
            schema_version: SCHEMA_VERSION,
            kind: ScheduleKind::Batch,
            plan_sha256: ContentDigest::sha256("plan"),
            jobs: BTreeMap::from([("page".to_owned(), record)]),
            pending: Vec::new(),
            frontier: Vec::new(),
        };

        assert_eq!(
            validate_batch_manifest(&manifest, &request, &[digest])
                .as_ref()
                .map_err(|error| error.code.as_str()),
            Err("pageknot.scheduler.manifest_shape")
        );
        Ok(())
    }

    #[test]
    fn crawl_manifest_preserves_the_seed_coordinates() -> Result<(), Box<dyn std::error::Error>> {
        let request = CrawlRequest {
            seed: CaptureRequest::builder("https://example.com")?.build()?,
            output_directory: PortablePath::new("crawl-output"),
            maximum_pages: 2,
            maximum_depth: 1,
            concurrency: 1,
            same_origin: true,
            resume: None,
        };
        let url = request.seed.url.clone();
        let item = CrawlFrontierItem {
            url: url.clone(),
            depth: 0,
            ordinal: 1,
        };
        let capture = super::super::crawl_capture_request(&request, &item);
        let mut record = record(ResumeJobStatus::Succeeded);
        record.request_sha256 = super::super::digest_serializable(&capture)?;
        record.capture_id = Some(CaptureId::new());
        record.artifact_sha256 = Some(ContentDigest::sha256("artifact"));
        record.artifact_path = Some(super::super::crawl_output_path(&request, &item));
        record.url = Some(url.clone());
        record.depth = Some(item.depth);
        record.ordinal = Some(item.ordinal);
        let manifest = ResumeManifest {
            schema_version: SCHEMA_VERSION,
            kind: ScheduleKind::Crawl,
            plan_sha256: ContentDigest::sha256("plan"),
            jobs: BTreeMap::from([(url.as_str().to_owned(), record)]),
            pending: Vec::new(),
            frontier: Vec::new(),
        };

        assert_eq!(
            validate_crawl_manifest(&manifest, &request)
                .as_ref()
                .map_err(|error| error.code.as_str()),
            Err("pageknot.scheduler.manifest_shape")
        );
        Ok(())
    }
}
