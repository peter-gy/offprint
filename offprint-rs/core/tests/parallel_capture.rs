use std::{collections::BTreeMap, error::Error, time::Duration};

use offprint::{
    BatchJob, BatchRequest, CaptureArtifact, CaptureOutput, CaptureRequest, CaptureStatus,
    Milliseconds, MissingResourcePolicy, Offprint, PUBLIC_SCHEMA_VERSION, ReadinessMode,
    ScheduledCaptureOutcome,
};
use offprint_test_support::{FixtureResponse, FixtureServer};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn parallel_capture_isolates_resources_failures_and_cancellation() -> TestResult {
    let server = FixtureServer::start().await?;
    server.register("/asset.svg", FixtureResponse { status:200, content_type:"image/svg+xml".into(), headers:BTreeMap::new(), body:br#"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="blue"/></svg>"#.to_vec() }).await?;
    let mut requests = Vec::new();
    for index in 0..8 {
        let images = (0..256)
            .map(|image| format!("<img src='/asset.svg?image={image}' width=32 height=32>"))
            .collect::<String>();
        let missing = if index == 6 {
            "<img src='/missing.svg'>"
        } else {
            ""
        };
        server.register(format!("/page-{index}"), FixtureResponse::html(format!("<!doctype html><title>Batch {index}</title><main><h1>Owned capture {index}</h1>{images}{missing}</main>"))).await?;
        let mut request = CaptureRequest::builder(server.url(&format!("/page-{index}"))?.as_str())?
            .output(CaptureOutput::memory(8 * 1024 * 1024))
            .build()?;
        request.readiness.mode = ReadinessMode::Load;
        request.content.missing_resources = MissingResourcePolicy::Fail;
        requests.push(request);
    }
    let service = Offprint::builder().maximum_contexts(8).build()?;
    let batch = tokio::time::timeout(
        Duration::from_secs(60),
        service.captures().batch(BatchRequest {
            schema_version: PUBLIC_SCHEMA_VERSION,
            concurrency: 8,
            resume: None,
            jobs: requests
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, request)| BatchJob {
                    id: format!("page-{i}"),
                    request,
                })
                .collect(),
        }),
    )
    .await??;
    let failures = batch
        .outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            ScheduledCaptureOutcome::Failed { id, error, .. } => Some((id, error)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!((batch.succeeded, batch.failed), (7, 1), "{failures:?}");
    for (index, outcome) in batch.outcomes.iter().enumerate() {
        assert_eq!(outcome.id(), format!("page-{index}"));
        if index == 6 {
            assert!(matches!(
                outcome,
                ScheduledCaptureOutcome::Failed { error, .. }
                    if error.code.as_str() == "offprint.resource.status"
            ));
            continue;
        }
        let ScheduledCaptureOutcome::Succeeded { result, .. } = outcome else {
            return Err(format!("capture {index} failed: {outcome:?}").into());
        };
        assert_eq!(result.resources.failed, 0);
        assert_eq!(result.resources.embedded, 256);
        assert_eq!(result.verification.network_requests, 0);
        let CaptureArtifact::Bytes { content, .. } = &result.artifact else {
            return Err("missing memory artifact".into());
        };
        let html = std::str::from_utf8(content)?;
        assert!(html.contains(&format!("Owned capture {index}")));
        for other in 0..8 {
            if other != index {
                assert!(!html.contains(&format!("Owned capture {other}")));
            }
        }
    }
    let mut slow = requests[0].clone();
    slow.readiness.delay = Milliseconds::new(10000);
    let cancelled = service.captures().start(slow).await?;
    let survivor = service.captures().start(requests[1].clone()).await?;
    tokio::time::timeout(Duration::from_secs(30), async {
        while cancelled.status() != CaptureStatus::WaitingForReadiness {
            assert!(!cancelled.status().is_terminal());
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|error| {
        format!(
            "readiness wait failed: {error}; cancelled={:?}, survivor={:?}",
            cancelled.status(),
            survivor.status()
        )
    })?;
    cancelled.cancel();
    let cancellation = tokio::time::timeout(Duration::from_secs(10), cancelled.result()).await?;
    assert_eq!(
        cancellation.err().map(|error| error.code.to_string()),
        Some("offprint.runtime.cancelled".into())
    );
    let receipt = tokio::time::timeout(Duration::from_secs(30), survivor.result()).await??;
    assert_eq!(receipt.resources.failed, 0);
    let receipt = service
        .captures()
        .start(requests[2].clone())
        .await?
        .result()
        .await?;
    assert_eq!(receipt.resources.failed, 0);
    tokio::time::timeout(Duration::from_secs(10), service.close()).await??;
    server.close().await;
    Ok(())
}
