use std::future::Future;
use std::io::Write;

use futures_util::StreamExt as _;
use pageknot::{
    CaptureJob, CaptureResult, ErrorStage, PageKnot, PageKnotError, Result, VerificationResult,
};

use super::diagnostics::CaptureProgress;
use crate::command::OutputOptions;
use crate::output::{DiagnosticTone, write_diagnostic};

pub(super) async fn drive_verification<F, S>(
    verification: F,
    timeout: Option<std::time::Duration>,
    signal: S,
) -> Result<VerificationResult>
where
    F: Future<Output = Result<VerificationResult>>,
    S: Future<Output = std::io::Result<()>>,
{
    let operation = async {
        if let Some(timeout) = timeout {
            tokio::time::timeout(timeout, verification)
                .await
                .map_err(|_| {
                    PageKnotError::new(
                        "pageknot.runtime.timeout",
                        ErrorStage::Shutdown,
                        "artifact verification exceeded its deadline",
                    )
                    .retryable(true)
                })?
        } else {
            verification.await
        }
    };
    tokio::pin!(operation);
    tokio::pin!(signal);
    tokio::select! {
        biased;
        result = &mut operation => result,
        result = &mut signal => {
            result.map_err(signal_error)?;
            Err(interrupted_error("artifact verification"))
        }
    }
}

pub(super) async fn drive_scheduler<T, F, S>(operation: F, signal: S, name: &str) -> Result<T>
where
    F: Future<Output = Result<T>>,
    S: Future<Output = std::io::Result<()>>,
{
    tokio::pin!(operation);
    tokio::pin!(signal);
    tokio::select! {
        biased;
        result = &mut operation => result,
        result = &mut signal => {
            result.map_err(signal_error)?;
            Err(interrupted_error(name))
        }
    }
}

pub(super) async fn drive_capture<F>(
    job: &CaptureJob,
    options: &OutputOptions,
    diagnostics: &mut dyn Write,
    color: bool,
    interrupt: F,
) -> Result<CaptureResult>
where
    F: Future<Output = std::io::Result<()>>,
{
    let mut events = job.events();
    let mut result = Box::pin(job.wait());
    let mut interrupt = Box::pin(interrupt);
    let mut interrupted = false;
    let mut events_open = true;
    let mut progress = CaptureProgress::default();
    loop {
        tokio::select! {
            biased;
            result = &mut result => {
                if interrupted {
                    let diagnostics_path = result
                        .as_ref()
                        .err()
                        .and_then(|error| error.diagnostics_path.clone());
                    let source = result.err();
                    let mut error = PageKnotError::new(
                        "pageknot.runtime.interrupted",
                        ErrorStage::Shutdown,
                        "capture was interrupted",
                    );
                    if let Some(source) = source {
                        error = error.with_source(source);
                    }
                    if let Some(path) = diagnostics_path {
                        error = error.with_diagnostics_path(path);
                    }
                    return Err(error);
                }
                return result;
            }
            event = events.next(), if events_open => {
                match event {
                    Some(event) if !options.quiet => {
                        progress.render(&event, diagnostics, color)?;
                    }
                    Some(_) => {}
                    None => events_open = false,
                }
            }
            signal = &mut interrupt, if !interrupted => {
                signal.map_err(signal_error)?;
                interrupted = true;
                job.cancel();
                if !options.quiet {
                    write_diagnostic(
                        diagnostics,
                        color,
                        DiagnosticTone::Warning,
                        "interrupt",
                        format_args!("cancelling capture and rolling back output"),
                    )?;
                }
            }
        }
    }
}

fn interrupted_error(operation: &str) -> PageKnotError {
    PageKnotError::new(
        "pageknot.runtime.interrupted",
        ErrorStage::Shutdown,
        format!("{operation} was interrupted"),
    )
}

fn signal_error(error: std::io::Error) -> PageKnotError {
    PageKnotError::new(
        "pageknot.runtime.signal",
        ErrorStage::Shutdown,
        format!("failed to listen for interruption: {error}"),
    )
}

pub(super) async fn finish_runtime<T>(pageknot: &PageKnot, operation: Result<T>) -> Result<T> {
    let close = pageknot.close().await;
    match (operation, close) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) | (Ok(_), Err(error)) => Err(error),
    }
}
