use pageknot_model::{
    CaptureRequest, ContentDigest, ErrorStage, PageKnotError, ScheduledCaptureOutcome,
};
use tokio_util::sync::CancellationToken;

use crate::{CaptureJob, CaptureService};

#[derive(Debug)]
struct ScheduledJob {
    job: CaptureJob,
    terminal: bool,
}

impl ScheduledJob {
    fn new(job: CaptureJob) -> Self {
        Self {
            job,
            terminal: false,
        }
    }

    async fn wait(
        mut self,
        cancellation: &CancellationToken,
    ) -> pageknot_model::Result<pageknot_model::CaptureResult> {
        let result = tokio::select! {
            biased;
            () = cancellation.cancelled() => {
                self.job.cancel();
                self.job.wait().await
            }
            result = self.job.wait() => result,
        };
        self.terminal = true;
        result
    }
}

impl Drop for ScheduledJob {
    fn drop(&mut self) {
        if !self.terminal {
            self.job.cancel();
        }
    }
}

pub(super) async fn run(
    service: CaptureService,
    id: String,
    request: CaptureRequest,
    request_sha256: ContentDigest,
    cancellation: CancellationToken,
) -> ScheduledCaptureOutcome {
    let started = tokio::select! {
        biased;
        () = cancellation.cancelled() => {
            return failed(id, request_sha256, cancellation_error());
        }
        result = service.start(request) => result,
    };
    match started {
        Ok(job) => match ScheduledJob::new(job).wait(&cancellation).await {
            Ok(result) => ScheduledCaptureOutcome::Succeeded {
                id,
                request_sha256,
                result,
            },
            Err(error) => failed(id, request_sha256, error),
        },
        Err(error) => failed(id, request_sha256, error),
    }
}

pub(super) fn failed(
    id: String,
    request_sha256: ContentDigest,
    error: PageKnotError,
) -> ScheduledCaptureOutcome {
    ScheduledCaptureOutcome::Failed {
        id,
        request_sha256,
        error,
    }
}

fn cancellation_error() -> PageKnotError {
    PageKnotError::new(
        "pageknot.runtime.cancelled",
        ErrorStage::Shutdown,
        "scheduled capture was cancelled",
    )
}
