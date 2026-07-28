use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use pageknot_browser::NetworkGuard;
use pageknot_model::{
    CaptureEvent, CaptureId, CaptureRequest, CaptureResult, CaptureStatus, ErrorStage,
    PageKnotError, Result,
};
use tokio_util::sync::CancellationToken;

use crate::capture_service::JobState;
use crate::runtime::{RuntimePageRequest, RuntimeState};

mod artifact;
mod frames;
mod offline;
mod render;
mod resource_materialization;

use resource_materialization::validate_network_url;

pub(crate) async fn run_capture_job(
    runtime: Arc<RuntimeState>,
    job: Arc<JobState>,
    capture_id: CaptureId,
    request: CaptureRequest,
) {
    let duration = Duration::from(request.limits.duration);
    let cancellation = job.cancellation().clone();
    let (result, timed_out) = {
        let pipeline = run_pipeline(&runtime, &job, &capture_id, request);
        tokio::pin!(pipeline);
        let deadline = tokio::time::sleep(duration);
        tokio::pin!(deadline);
        tokio::select! {
            biased;
            result = &mut pipeline => (result, false),
            () = cancellation.cancelled() => (pipeline.await, false),
            () = &mut deadline => {
                let timed_out = job.request_cancel();
                (pipeline.await, timed_out)
            }
        }
    };

    match result {
        Ok(result) => {
            let event = CaptureEvent::CaptureSucceeded {
                capture_id: capture_id.clone(),
                verification: result.verification.clone(),
            };
            if let Err(error) = job.transition(CaptureStatus::Succeeded) {
                finish_failed_job(&job, capture_id, error).await;
                return;
            }
            job.emit(event);
            job.finish(Ok(result));
        }
        Err(error) if job.cancellation_won() => {
            if let Err(transition_error) = job
                .transition(CaptureStatus::Cancelling)
                .and_then(|()| job.transition(CaptureStatus::Cancelled))
            {
                finish_failed_job(&job, capture_id, transition_error.with_source(error)).await;
                return;
            }
            job.emit(CaptureEvent::CaptureCancelled {
                capture_id: capture_id.clone(),
            });
            let error = if timed_out {
                PageKnotError::new(
                    "pageknot.runtime.timeout",
                    ErrorStage::Shutdown,
                    "capture exceeded its total deadline",
                )
                .retryable(true)
                .with_source(error)
            } else {
                PageKnotError::new(
                    "pageknot.runtime.cancelled",
                    ErrorStage::Shutdown,
                    "capture was cancelled",
                )
                .with_source(error)
            };
            let error = job.attach_diagnostics(&capture_id, error).await;
            job.finish(Err(error));
        }
        Err(error) => {
            finish_failed_job(&job, capture_id, error).await;
        }
    }
}

async fn run_pipeline(
    runtime: &Arc<RuntimeState>,
    job: &Arc<JobState>,
    capture_id: &CaptureId,
    request: CaptureRequest,
) -> Result<CaptureResult> {
    let total_started = Instant::now();
    transition(job, CaptureStatus::Validating).await?;
    job.emit(CaptureEvent::CaptureStarted {
        capture_id: capture_id.clone(),
    });
    let validation_started = Instant::now();
    request.validate()?;
    let writer = artifact::PreparedWriter::new(&request.artifact, request.limits.artifact_bytes)?;
    let guard = NetworkGuard::new(request.network.clone(), &request.url)?;
    cancellable(
        job.cancellation(),
        validate_network_url(&guard, &request.url),
    )
    .await?;
    let validation = validation_started.elapsed();
    check_cancelled(job.cancellation())?;

    transition(job, CaptureStatus::WaitingForBrowser).await?;
    let browser_started = Instant::now();
    let runtime_page = cancellable(
        job.cancellation(),
        runtime.open_page(RuntimePageRequest {
            capture_id: capture_id.clone(),
            browser: request.browser.clone(),
            environment: request.environment.clone(),
            headed: request.headed,
            network: request.network.clone(),
            deny_network: false,
            maximum_frames: request.limits.frames,
            resource_observation: pageknot_browser::ResourceObservationLimits {
                maximum_resource_bytes: request.limits.resource_bytes,
                maximum_total_resource_bytes: request.limits.total_resource_bytes,
            },
            cancellation: job.cancellation().clone(),
        }),
    )
    .await
    .map_err(|error| error.with_detail("capturePhase", "capture"))?;
    let browser = browser_started.elapsed();
    job.emit(CaptureEvent::BrowserReady {
        capture_id: capture_id.clone(),
        browser: runtime_page.browser.clone(),
    });
    let browser_info = runtime_page.browser.clone();

    let page_result = cancellable(
        job.cancellation(),
        frames::capture(runtime_page.page()?, job, capture_id, &request, &guard),
    )
    .await;
    let page_close = runtime_page.close().await;
    let captured = match (page_result, page_close) {
        (Ok(captured), Ok(())) => captured,
        (Err(error), _) => return Err(error),
        (Ok(_), Err(error)) => return Err(error),
    };

    artifact::finalize(
        artifact::FinalizeContext {
            runtime,
            job,
            capture_id,
            request: &request,
            browser_info,
            total_started,
            validation,
            browser,
        },
        captured,
        writer,
    )
    .await
}

async fn transition(job: &JobState, status: CaptureStatus) -> Result<()> {
    check_cancelled(job.cancellation())?;
    job.transition(status)?;
    tokio::task::yield_now().await;
    check_cancelled(job.cancellation())
}

async fn finish_failed_job(job: &JobState, capture_id: CaptureId, error: PageKnotError) {
    let error = match job.transition(CaptureStatus::Failed) {
        Ok(()) => error,
        Err(transition_error) => transition_error.with_source(error),
    };
    let error = job.attach_diagnostics(&capture_id, error).await;
    job.emit(CaptureEvent::CaptureFailed {
        capture_id,
        error: error.clone(),
    });
    job.finish(Err(error));
}

async fn cancellable<T>(
    cancellation: &CancellationToken,
    future: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(cancelled_error()),
        result = future => result,
    }
}

fn check_cancelled(cancellation: &CancellationToken) -> Result<()> {
    if cancellation.is_cancelled() {
        Err(cancelled_error())
    } else {
        Ok(())
    }
}

fn cancelled_error() -> PageKnotError {
    PageKnotError::new(
        "pageknot.runtime.cancelled",
        ErrorStage::Shutdown,
        "capture cancellation requested",
    )
}
