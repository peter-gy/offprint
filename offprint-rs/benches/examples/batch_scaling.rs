use std::{collections::BTreeMap, error::Error, io::Write as _, time::Instant};

use offprint::Offprint;
use offprint_model::{
    BatchJob, BatchRequest, CaptureArtifact, CaptureOutput, CaptureRequest, PUBLIC_SCHEMA_VERSION,
    ReadinessMode, ScheduledCaptureOutcome,
};
use offprint_test_support::{FixtureResponse, FixtureServer};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let pages = workload_size("BATCH_PAGES", 16, 256)?;
    let images = workload_size("BATCH_IMAGES", 64, 4096)?;
    let server = FixtureServer::start().await?;
    server.register("/asset.svg", FixtureResponse {
        status: 200, content_type: "image/svg+xml".into(), headers: BTreeMap::new(),
        body: br#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="blue"/></svg>"#.to_vec(),
    }).await?;
    for index in 0..pages {
        let images = (0..images)
            .map(|image| format!("<img src='/asset.svg?image={image}' width=32 height=32>"))
            .collect::<String>();
        server.register(format!("/page-{index}"), FixtureResponse::html(format!("<!doctype html><title>Batch {index}</title><main><h1>Capture {index}</h1>{images}</main>"))).await?;
    }
    let service = Offprint::builder().maximum_contexts(8).build()?;
    let result = measure_batches(&service, &server, pages, images).await;
    let closed = service.close().await;
    server.close().await;
    result?;
    closed?;
    Ok(())
}

async fn measure_batches(
    service: &Offprint,
    server: &FixtureServer,
    pages: usize,
    images: usize,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    service.browsers().ensure().await?;
    let widths = match std::env::var("BATCH_WIDTH") {
        Ok(value) => vec![value.parse::<u16>()?],
        Err(_) => vec![1, 4, 8],
    };
    let iterations = std::env::var("BATCH_ITERATIONS")
        .unwrap_or_else(|_| "3".into())
        .parse::<u32>()?;
    if iterations == 0 {
        return Err("iteration count must be positive".into());
    }
    let mut failed = 0;
    for concurrency in widths {
        for iteration in 0..iterations {
            let jobs = (0..pages)
                .map(|index| {
                    let mut request =
                        CaptureRequest::builder(server.url(&format!("/page-{index}"))?.as_str())?
                            .output(CaptureOutput::memory(8 * 1024 * 1024))
                            .build()?;
                    request.readiness.mode = ReadinessMode::Load;
                    request.content.missing_resources = offprint_model::MissingResourcePolicy::Fail;
                    Ok(BatchJob {
                        id: format!("page-{index}"),
                        request,
                    })
                })
                .collect::<Result<Vec<_>, Box<dyn Error + Send + Sync>>>()?;
            let started = Instant::now();
            let batch = service
                .captures()
                .batch(BatchRequest {
                    schema_version: PUBLIC_SCHEMA_VERSION,
                    jobs,
                    concurrency,
                    resume: None,
                })
                .await?;
            let seconds = started.elapsed().as_secs_f64();
            let mut failures = Vec::new();
            let mut timings = Vec::new();
            for (index, outcome) in batch.outcomes.iter().enumerate() {
                if outcome.id() != format!("page-{index}") {
                    return Err("batch result order changed".into());
                }
                match outcome {
                    ScheduledCaptureOutcome::Succeeded { result, .. } => {
                        let CaptureArtifact::Bytes { content, .. } = &result.artifact else {
                            return Err("memory artifact missing".into());
                        };
                        let text_matches = String::from_utf8_lossy(content)
                            .contains(&format!(">Capture {index}</h1>"));
                        if !text_matches
                            || result.verification.network_requests != 0
                            || result.resources.failed != 0
                            || result.resources.embedded as usize != images
                        {
                            failures.push(json!({"index":index,"resources":result.resources,"warnings":result.warnings,"textMatches":text_matches,"networkRequests":result.verification.network_requests}));
                        }
                        timings.push(result.timings);
                    }
                    ScheduledCaptureOutcome::Failed { error, .. } => {
                        failures.push(json!({"index":index,"error":error}))
                    }
                    _ => return Err("unexpected resumed job".into()),
                }
            }
            failed += failures.len();
            writeln!(
                std::io::stdout().lock(),
                "{}",
                json!({"concurrency":concurrency,"pages":pages,"imagesPerPage":images,"iteration":iteration,"seconds":seconds,"succeeded":batch.succeeded,"failures":failures,"timings":timings})
            )?;
        }
    }
    if failed > 0 {
        return Err(format!("{failed} batch captures failed validation").into());
    }
    Ok(())
}

fn workload_size(
    name: &str,
    default: usize,
    maximum: usize,
) -> Result<usize, Box<dyn Error + Send + Sync>> {
    let value = match std::env::var(name) {
        Ok(value) => value.parse::<usize>()?,
        Err(std::env::VarError::NotPresent) => default,
        Err(error) => return Err(error.into()),
    };
    if value == 0 || value > maximum {
        return Err(format!("{name} must be between 1 and {maximum}").into());
    }
    Ok(value)
}
