use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use futures_util::future::join_all;
use futures_util::{StreamExt as _, stream};
use pageknot_document::{Document, NodeData};
use pageknot_model::{
    ArtifactResult, ArtifactSpec, ArtifactTarget, BatchRequest, BatchResult, BrowserSpec,
    CaptureRequest, ConflictPolicy, ContentDigest, CrawlFrontierItem, CrawlPageOutcome,
    CrawlRequest, CrawlResult, ErrorStage, HtmlArtifactSpec, PageKnotError, PortablePath, Result,
    ResumeJobStatus, ResumeManifest, ScheduleKind, ScheduledCaptureOutcome,
};
use serde::Serialize;
use url::Url;

use crate::runtime::RuntimeState;

mod job;
mod resume;

use resume::{
    load_or_create_manifest, persist_manifest_if_configured, record_from_outcome,
    refresh_crawl_resume_state, resume_artifact_is_current, validate_batch_manifest,
    validate_crawl_manifest,
};

const SCHEMA_VERSION: u32 = pageknot_model::PUBLIC_SCHEMA_VERSION;
const MAXIMUM_BATCH_JOBS: usize = 100_000;
const MAXIMUM_CONCURRENCY: u16 = 256;

#[derive(Clone, Debug)]
pub(crate) struct SchedulerService {
    state: Arc<RuntimeState>,
}

impl SchedulerService {
    pub(crate) const fn new(state: Arc<RuntimeState>) -> Self {
        Self { state }
    }

    /// Runs independent capture requests with bounded concurrency.
    ///
    /// Each capture produces an independent outcome. When resume state is
    /// configured, each terminal outcome is checkpointed through an atomic
    /// file transaction.
    pub async fn batch(&self, mut request: BatchRequest) -> Result<BatchResult> {
        self.state.ensure_open()?;
        for job in &mut request.jobs {
            if matches!(job.request.browser, BrowserSpec::Auto) {
                job.request.browser = self.state.default_browser().clone();
            }
        }
        validate_concurrency(request.concurrency)?;
        validate_batch_jobs(&request)?;
        let plan_sha256 = digest_serializable(&request.jobs)?;
        let request_digests = request
            .jobs
            .iter()
            .map(|job| digest_serializable(&job.request))
            .collect::<Result<Vec<_>>>()?;
        let mut manifest = load_or_create_manifest(
            request.resume.as_ref(),
            ScheduleKind::Batch,
            plan_sha256,
            request.jobs.iter().map(|job| job.id.clone()).collect(),
            Vec::new(),
        )
        .await?;
        if request.resume.is_some() {
            validate_batch_manifest(&manifest, &request, &request_digests)?;
        }
        let mut outcomes = vec![None; request.jobs.len()];
        let mut pending = Vec::new();

        for (index, job) in request.jobs.iter().enumerate() {
            let digest = request_digests[index];
            let resumable = manifest
                .jobs
                .get(&job.id)
                .filter(|record| record.request_sha256 == digest);
            let resumable = match resumable {
                Some(record)
                    if record.status == ResumeJobStatus::Succeeded
                        && resume_artifact_is_current(record).await =>
                {
                    Some(record.clone())
                }
                Some(record)
                    if record.status == ResumeJobStatus::Failed
                        && !request
                            .resume
                            .as_ref()
                            .is_some_and(|resume| resume.retry_failed) =>
                {
                    Some(record.clone())
                }
                _ => None,
            };
            if let Some(record) = resumable {
                outcomes[index] = Some(ScheduledCaptureOutcome::Resumed {
                    id: job.id.clone(),
                    record,
                });
            } else {
                manifest.jobs.remove(&job.id);
                pending.push(index);
            }
        }
        refresh_batch_pending(&mut manifest, &request, &outcomes);
        persist_manifest_if_configured(request.resume.as_ref(), &manifest).await?;

        let captures = crate::CaptureService::new(Arc::clone(&self.state));
        let cancellation = self.state.operation_cancellation();
        let mut running = stream::iter(pending.into_iter().map(|index| {
            let service = captures.clone();
            let id = request.jobs[index].id.clone();
            let capture_request = request.jobs[index].request.clone();
            let request_sha256 = request_digests[index];
            let cancellation = cancellation.clone();
            async move {
                let outcome =
                    job::run(service, id, capture_request, request_sha256, cancellation).await;
                (index, outcome)
            }
        }))
        .buffer_unordered(usize::from(request.concurrency));

        while let Some((index, outcome)) = running.next().await {
            let id = outcome.id().to_owned();
            manifest.jobs.insert(id, record_from_outcome(&outcome));
            outcomes[index] = Some(outcome);
            refresh_batch_pending(&mut manifest, &request, &outcomes);
            if let Err(error) =
                persist_manifest_if_configured(request.resume.as_ref(), &manifest).await
            {
                cancellation.cancel();
                while running.next().await.is_some() {}
                return Err(error);
            }
        }

        let outcomes = outcomes
            .into_iter()
            .enumerate()
            .map(|(index, outcome)| {
                outcome.ok_or_else(|| {
                    scheduler_error(
                        "pageknot.scheduler.state",
                        ErrorStage::Internal,
                        "batch scheduler ended without a terminal job outcome",
                    )
                    .with_detail("jobIndex", index)
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(batch_result(plan_sha256, outcomes))
    }

    /// Captures a deterministic breadth-first URL frontier.
    ///
    /// Each page receives a stable file name derived from its breadth-first
    /// ordinal and URL digest. Frontier order determines link discovery and
    /// subsequent work independently of completion timing.
    pub async fn crawl(&self, mut request: CrawlRequest) -> Result<CrawlResult> {
        self.state.ensure_open()?;
        if matches!(request.seed.browser, BrowserSpec::Auto) {
            request.seed.browser = self.state.default_browser().clone();
        }
        validate_crawl_request(&request)?;
        tokio::fs::create_dir_all(&request.output_directory)
            .await
            .map_err(|error| {
                scheduler_error(
                    "pageknot.scheduler.output",
                    ErrorStage::Validation,
                    format!("failed to create the crawl output directory: {error}"),
                )
            })?;
        validate_direct_directory(&request.output_directory).await?;
        let plan_sha256 = crawl_plan_digest(&request)?;
        let root = normalized_page_url(request.seed.url.clone())?;
        let initial_frontier = vec![CrawlFrontierItem {
            url: root,
            depth: 0,
            ordinal: 0,
        }];
        let initial_pending = initial_frontier
            .iter()
            .map(|item| item.url.as_str().to_owned())
            .collect();
        let mut manifest = load_or_create_manifest(
            request.resume.as_ref(),
            ScheduleKind::Crawl,
            plan_sha256,
            initial_pending,
            initial_frontier,
        )
        .await?;
        validate_crawl_manifest(&manifest, &request)?;
        refresh_crawl_resume_state(&request, &mut manifest).await?;
        persist_crawl_pending(&mut manifest);
        persist_manifest_if_configured(request.resume.as_ref(), &manifest).await?;

        let mut fresh_outcomes = BTreeMap::new();
        let mut seen = crawl_seen_urls(&manifest)?;
        let captures = crate::CaptureService::new(Arc::clone(&self.state));
        let cancellation = self.state.operation_cancellation();

        while !manifest.frontier.is_empty() && manifest.jobs.len() < request.maximum_pages as usize
        {
            sort_frontier(&mut manifest.frontier);
            let remaining = request
                .maximum_pages
                .saturating_sub(u32::try_from(manifest.jobs.len()).unwrap_or(u32::MAX));
            let width = usize::from(request.concurrency)
                .min(usize::try_from(remaining).unwrap_or(usize::MAX))
                .min(manifest.frontier.len());
            let level_depth = manifest.frontier[0].depth;
            let level_width = manifest
                .frontier
                .iter()
                .take_while(|item| item.depth == level_depth)
                .count();
            let width = width.min(level_width);
            let active = manifest.frontier[..width].to_vec();
            let futures = active.iter().cloned().map(|item| {
                let service = captures.clone();
                let capture_request = crawl_capture_request(&request, &item);
                let cancellation = cancellation.clone();
                async move {
                    let request_sha256 = digest_serializable(&capture_request);
                    let outcome = match request_sha256 {
                        Ok(request_sha256) => {
                            job::run(
                                service,
                                item.url.as_str().to_owned(),
                                capture_request,
                                request_sha256,
                                cancellation,
                            )
                            .await
                        }
                        Err(error) => job::failed(
                            item.url.as_str().to_owned(),
                            ContentDigest::sha256(item.url.as_str()),
                            error,
                        ),
                    };
                    (item, outcome)
                }
            });

            for (item, outcome) in join_all(futures).await {
                manifest.frontier.retain(|pending| {
                    pending.url != item.url
                        || pending.depth != item.depth
                        || pending.ordinal != item.ordinal
                });
                let mut record = record_from_outcome(&outcome);
                record.url = Some(item.url.clone());
                record.depth = Some(item.depth);
                record.ordinal = Some(item.ordinal);
                manifest.jobs.insert(item.url.as_str().to_owned(), record);

                if let ScheduledCaptureOutcome::Succeeded { result, .. } = &outcome
                    && item.depth < request.maximum_depth
                {
                    let links =
                        crawl_links(result, &item.url, request.same_origin, &request.seed.url)
                            .await?;
                    for link in links {
                        if seen.len() >= request.maximum_pages as usize {
                            break;
                        }
                        if seen.insert(link.as_str().to_owned()) {
                            manifest.frontier.push(CrawlFrontierItem {
                                url: link,
                                depth: item.depth.saturating_add(1),
                                ordinal: u32::try_from(seen.len().saturating_sub(1))
                                    .unwrap_or(u32::MAX),
                            });
                        }
                    }
                }
                fresh_outcomes.insert(item.url.as_str().to_owned(), outcome);
                sort_frontier(&mut manifest.frontier);
                persist_crawl_pending(&mut manifest);
                persist_manifest_if_configured(request.resume.as_ref(), &manifest).await?;
            }
        }

        let mut outcomes = manifest
            .jobs
            .iter()
            .map(|(id, record)| {
                let url = record.url.clone().ok_or_else(|| {
                    scheduler_error(
                        "pageknot.scheduler.manifest_shape",
                        ErrorStage::Internal,
                        "crawl resume job is missing its URL",
                    )
                    .with_detail("jobId", id.as_str())
                })?;
                let depth = record.depth.ok_or_else(|| {
                    scheduler_error(
                        "pageknot.scheduler.manifest_shape",
                        ErrorStage::Internal,
                        "crawl resume job is missing its depth",
                    )
                    .with_detail("jobId", id.as_str())
                })?;
                let ordinal = record.ordinal.ok_or_else(|| {
                    scheduler_error(
                        "pageknot.scheduler.manifest_shape",
                        ErrorStage::Internal,
                        "crawl resume job is missing its ordinal",
                    )
                    .with_detail("jobId", id.as_str())
                })?;
                let capture =
                    fresh_outcomes
                        .remove(id)
                        .unwrap_or_else(|| ScheduledCaptureOutcome::Resumed {
                            id: id.clone(),
                            record: record.clone(),
                        });
                Ok(CrawlPageOutcome {
                    url,
                    depth,
                    ordinal,
                    capture,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        outcomes.sort_by_key(|outcome| outcome.ordinal);
        Ok(crawl_result(plan_sha256, outcomes))
    }
}

fn validate_batch_jobs(request: &BatchRequest) -> Result<()> {
    if request.jobs.is_empty() || request.jobs.len() > MAXIMUM_BATCH_JOBS {
        return Err(scheduler_error(
            "pageknot.scheduler.job_count",
            ErrorStage::Validation,
            format!("batch job count must be between 1 and {MAXIMUM_BATCH_JOBS}"),
        ));
    }
    let mut ids = BTreeSet::new();
    for job in &request.jobs {
        if job.id.trim().is_empty() || job.id.len() > 256 {
            return Err(scheduler_error(
                "pageknot.scheduler.job_id",
                ErrorStage::Validation,
                "batch job identifiers must contain between 1 and 256 bytes",
            ));
        }
        if !ids.insert(job.id.as_str()) {
            return Err(scheduler_error(
                "pageknot.scheduler.job_id",
                ErrorStage::Validation,
                format!("batch job identifier `{}` is duplicated", job.id),
            ));
        }
        job.request.validate()?;
        if request.resume.is_some() && !job.request.credentials.is_empty() {
            return Err(scheduler_error(
                "pageknot.scheduler.resume_credentials",
                ErrorStage::Validation,
                "resumable batch jobs require empty capture credentials",
            ));
        }
        if request.resume.is_some()
            && !matches!(
                job.request.artifact,
                ArtifactSpec::Html(HtmlArtifactSpec {
                    target: ArtifactTarget::File(_),
                    ..
                })
            )
        {
            return Err(scheduler_error(
                "pageknot.scheduler.resume_target",
                ErrorStage::Validation,
                "resumable batch jobs require file artifact targets",
            ));
        }
    }
    Ok(())
}

fn validate_crawl_request(request: &CrawlRequest) -> Result<()> {
    validate_concurrency(request.concurrency)?;
    if matches!(request.seed.browser, BrowserSpec::Remote(_)) {
        return Err(scheduler_error(
            "pageknot.input.browser_selection",
            ErrorStage::Validation,
            "crawl requires a PageKnot-owned Chrome or Chromium process",
        ));
    }
    request.seed.validate()?;
    if request.resume.is_some() && !request.seed.credentials.is_empty() {
        return Err(scheduler_error(
            "pageknot.scheduler.resume_credentials",
            ErrorStage::Validation,
            "resumable crawls require empty seed credentials",
        ));
    }
    if request.maximum_pages == 0 {
        return Err(scheduler_error(
            "pageknot.scheduler.job_count",
            ErrorStage::Validation,
            "crawl maximum pages must be greater than zero",
        ));
    }
    if request.maximum_depth > 10_000 {
        return Err(scheduler_error(
            "pageknot.scheduler.depth",
            ErrorStage::Validation,
            "crawl maximum depth exceeds the supported limit",
        ));
    }
    Ok(())
}

fn validate_concurrency(concurrency: u16) -> Result<()> {
    if concurrency == 0 || concurrency > MAXIMUM_CONCURRENCY {
        return Err(scheduler_error(
            "pageknot.scheduler.concurrency",
            ErrorStage::Validation,
            format!("scheduler concurrency must be between 1 and {MAXIMUM_CONCURRENCY}"),
        ));
    }
    Ok(())
}

fn digest_serializable(value: &impl Serialize) -> Result<ContentDigest> {
    serde_json::to_vec(value)
        .map(ContentDigest::sha256)
        .map_err(|error| {
            scheduler_error(
                "pageknot.scheduler.plan",
                ErrorStage::Validation,
                format!("scheduler plan could not be serialized: {error}"),
            )
        })
}

fn crawl_plan_digest(request: &CrawlRequest) -> Result<ContentDigest> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct CrawlPlan<'a> {
        seed: &'a CaptureRequest,
        output_directory: &'a PortablePath,
        maximum_pages: u32,
        maximum_depth: u16,
        same_origin: bool,
    }

    digest_serializable(&CrawlPlan {
        seed: &request.seed,
        output_directory: &request.output_directory,
        maximum_pages: request.maximum_pages,
        maximum_depth: request.maximum_depth,
        same_origin: request.same_origin,
    })
}

fn refresh_batch_pending(
    manifest: &mut ResumeManifest,
    request: &BatchRequest,
    outcomes: &[Option<ScheduledCaptureOutcome>],
) {
    manifest.pending = request
        .jobs
        .iter()
        .zip(outcomes)
        .filter(|(_, outcome)| outcome.is_none())
        .map(|(job, _)| job.id.clone())
        .collect();
}

fn persist_crawl_pending(manifest: &mut ResumeManifest) {
    manifest.pending = manifest
        .frontier
        .iter()
        .map(|item| item.url.as_str().to_owned())
        .collect();
}

fn crawl_seen_urls(manifest: &ResumeManifest) -> Result<BTreeSet<String>> {
    let mut seen = manifest
        .frontier
        .iter()
        .map(|item| item.url.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    for (id, record) in &manifest.jobs {
        let url = record.url.as_ref().ok_or_else(|| {
            scheduler_error(
                "pageknot.scheduler.manifest_shape",
                ErrorStage::Internal,
                "crawl resume job is missing its URL",
            )
            .with_detail("jobId", id.as_str())
        })?;
        seen.insert(url.as_str().to_owned());
    }
    Ok(seen)
}

fn sort_frontier(frontier: &mut [CrawlFrontierItem]) {
    frontier.sort_by(|left, right| {
        (left.depth, left.ordinal, left.url.as_str()).cmp(&(
            right.depth,
            right.ordinal,
            right.url.as_str(),
        ))
    });
}

fn crawl_capture_request(request: &CrawlRequest, item: &CrawlFrontierItem) -> CaptureRequest {
    let mut capture = request.seed.clone();
    capture.url = item.url.clone();
    capture.artifact = ArtifactSpec::Html(HtmlArtifactSpec {
        target: ArtifactTarget::File(crawl_output_path(request, item)),
        conflict: ConflictPolicy::Replace,
    });
    capture
}

fn crawl_output_path(request: &CrawlRequest, item: &CrawlFrontierItem) -> PortablePath {
    let hint = item
        .url
        .path_segments()
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
        .unwrap_or_else(|| item.url.host_str().unwrap_or("page"));
    let stem = pageknot_artifact::portable_file_stem(hint);
    let digest = ContentDigest::sha256(item.url.as_str()).to_hex();
    request
        .output_directory
        .join(format!("{:05}-{stem}-{}.html", item.ordinal, &digest[..12]))
        .into()
}

async fn crawl_links(
    result: &pageknot_model::CaptureResult,
    requested_url: &Url,
    same_origin: bool,
    seed_url: &Url,
) -> Result<Vec<Url>> {
    let bytes = match &result.artifact {
        ArtifactResult::File { path, .. } => tokio::fs::read(path).await.map_err(|error| {
            scheduler_error(
                "pageknot.scheduler.artifact_read",
                ErrorStage::Internal,
                format!("failed to read a captured crawl artifact: {error}"),
            )
        })?,
        ArtifactResult::Bytes { content, .. } => content.clone(),
    };
    let manifest = pageknot_html::inspect_html(&bytes)?;
    let base =
        Url::parse(manifest.source.final_url.as_str()).unwrap_or_else(|_| requested_url.clone());
    let document = Document::parse(&bytes);
    let mut links = BTreeSet::new();
    for id in document.walk() {
        let Some(NodeData::Element { name, attrs, .. }) = document.node(id).map(|node| &node.data)
        else {
            continue;
        };
        if !matches!(name.local.as_ref(), "a" | "area") {
            continue;
        }
        let nofollow = attrs
            .iter()
            .find(|attribute| attribute.name.local.as_ref() == "rel")
            .is_some_and(|attribute| {
                attribute
                    .value
                    .split_ascii_whitespace()
                    .any(|value| value.eq_ignore_ascii_case("nofollow"))
            });
        if nofollow {
            continue;
        }
        let Some(href) = attrs
            .iter()
            .find(|attribute| attribute.name.local.as_ref() == "href")
            .map(|attribute| attribute.value.as_ref())
        else {
            continue;
        };
        let Ok(link) = base.join(href) else {
            continue;
        };
        let Ok(link) = normalized_page_url(link) else {
            continue;
        };
        if same_origin && link.origin() != seed_url.origin() {
            continue;
        }
        links.insert(link);
    }
    Ok(links.into_iter().collect())
}

fn normalized_page_url(mut url: Url) -> Result<Url> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(scheduler_error(
            "pageknot.scheduler.url",
            ErrorStage::Validation,
            "crawl URLs must use HTTP or HTTPS",
        ));
    }
    url.set_fragment(None);
    Ok(url)
}

async fn validate_direct_directory(path: &PortablePath) -> Result<()> {
    let metadata = tokio::fs::symlink_metadata(path).await.map_err(|error| {
        scheduler_error(
            "pageknot.scheduler.output",
            ErrorStage::Validation,
            format!("failed to inspect the crawl output directory: {error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(scheduler_error(
            "pageknot.scheduler.output",
            ErrorStage::Validation,
            "crawl output must be a directly addressed directory",
        ));
    }
    Ok(())
}

fn batch_result(plan_sha256: ContentDigest, outcomes: Vec<ScheduledCaptureOutcome>) -> BatchResult {
    let succeeded = outcomes
        .iter()
        .filter(|outcome| outcome.succeeded())
        .count();
    let resumed = outcomes.iter().filter(|outcome| outcome.resumed()).count();
    BatchResult {
        schema_version: SCHEMA_VERSION,
        plan_sha256,
        failed: u32::try_from(outcomes.len().saturating_sub(succeeded)).unwrap_or(u32::MAX),
        succeeded: u32::try_from(succeeded).unwrap_or(u32::MAX),
        resumed: u32::try_from(resumed).unwrap_or(u32::MAX),
        outcomes,
    }
}

fn crawl_result(plan_sha256: ContentDigest, outcomes: Vec<CrawlPageOutcome>) -> CrawlResult {
    let succeeded = outcomes
        .iter()
        .filter(|outcome| outcome.capture.succeeded())
        .count();
    let resumed = outcomes
        .iter()
        .filter(|outcome| outcome.capture.resumed())
        .count();
    CrawlResult {
        schema_version: SCHEMA_VERSION,
        plan_sha256,
        failed: u32::try_from(outcomes.len().saturating_sub(succeeded)).unwrap_or(u32::MAX),
        succeeded: u32::try_from(succeeded).unwrap_or(u32::MAX),
        resumed: u32::try_from(resumed).unwrap_or(u32::MAX),
        outcomes,
    }
}

fn scheduler_error(
    code: &'static str,
    stage: ErrorStage,
    message: impl Into<String>,
) -> PageKnotError {
    PageKnotError::new(code, stage, message)
}

#[cfg(test)]
mod tests {
    use pageknot_model::{
        BatchJob, BatchRequest, BrowserSpec, CaptureRequest, CrawlRequest, NetworkPolicy,
        PortablePath, RequestHeader, ResumeOptions, SecretString, VerificationPolicy,
    };
    use url::Url;

    use super::{validate_batch_jobs, validate_crawl_request};

    #[test]
    fn crawl_rejects_remote_browsers_at_the_service_boundary()
    -> Result<(), Box<dyn std::error::Error>> {
        let endpoint = Url::parse("http://127.0.0.1:9222")?;
        let seed = CaptureRequest::builder("https://example.com")
            .map(|builder| {
                builder
                    .browser(BrowserSpec::Remote(endpoint))
                    .network(NetworkPolicy::Unrestricted)
                    .verification(VerificationPolicy::Static)
            })
            .and_then(|builder| builder.build())?;
        let request = CrawlRequest {
            seed,
            output_directory: PortablePath::new("crawl-output"),
            maximum_pages: 1,
            maximum_depth: 1,
            concurrency: 1,
            same_origin: true,
            resume: None,
        };

        assert_eq!(
            validate_crawl_request(&request)
                .as_ref()
                .map_err(|error| error.code.as_str()),
            Err("pageknot.input.browser_selection")
        );
        Ok(())
    }

    #[test]
    fn resumable_batch_rejects_credentials_before_digesting_the_plan()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut capture = CaptureRequest::builder("https://example.com")?
            .output("capture.html")
            .build()?;
        capture.credentials.headers.push(RequestHeader {
            name: "authorization".to_owned(),
            value: SecretString::new("Bearer first-secret"),
        });
        let request = BatchRequest {
            jobs: vec![BatchJob {
                id: "credentialed".to_owned(),
                request: capture,
            }],
            concurrency: 1,
            resume: Some(ResumeOptions {
                manifest: PortablePath::new("resume.json"),
                retry_failed: false,
            }),
        };

        assert_eq!(
            validate_batch_jobs(&request)
                .as_ref()
                .map_err(|error| error.code.as_str()),
            Err("pageknot.scheduler.resume_credentials")
        );
        Ok(())
    }

    #[test]
    fn resumable_crawl_rejects_credentials_before_digesting_the_plan()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut seed = CaptureRequest::builder("https://example.com")?.build()?;
        seed.credentials.headers.push(RequestHeader {
            name: "authorization".to_owned(),
            value: SecretString::new("Bearer first-secret"),
        });
        let request = CrawlRequest {
            seed,
            output_directory: PortablePath::new("crawl-output"),
            maximum_pages: 1,
            maximum_depth: 1,
            concurrency: 1,
            same_origin: true,
            resume: Some(ResumeOptions {
                manifest: PortablePath::new("resume.json"),
                retry_failed: false,
            }),
        };

        assert_eq!(
            validate_crawl_request(&request)
                .as_ref()
                .map_err(|error| error.code.as_str()),
            Err("pageknot.scheduler.resume_credentials")
        );
        Ok(())
    }
}
