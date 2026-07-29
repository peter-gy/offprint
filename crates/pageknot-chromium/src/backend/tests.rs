use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use camino::Utf8PathBuf;
use pageknot_browser::{BrowserAcquireRequest, BrowserBackend as _};
use pageknot_model::{BrowserInstallationPolicy, BrowserSpec, CaptureId};
use tokio_util::sync::CancellationToken;

use super::{BrowserOwner, ChromiumBackend, ChromiumBackendOptions};
use crate::ChromiumDiscovery;

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn cancellation_while_waiting_for_backend_ownership_stops_acquisition() -> TestResult {
    let directory = tempfile::tempdir()?;
    let backend = Arc::new(test_backend(&directory)?);
    let owner = backend.owner.lock().await;
    let cancellation = CancellationToken::new();
    let task_backend = Arc::clone(&backend);
    let task_cancellation = cancellation.clone();
    let acquisition = tokio::spawn(async move {
        task_backend
            .acquire(acquire_request(), task_cancellation)
            .await
    });
    tokio::task::yield_now().await;

    cancellation.cancel();
    let result = tokio::time::timeout(Duration::from_secs(1), acquisition).await??;
    drop(owner);
    let error = match result {
        Err(error) => error,
        Ok(lease) => {
            lease.close().await?;
            return Err(std::io::Error::other("cancelled acquisition succeeded").into());
        }
    };

    assert_eq!(
        error.code.as_str(),
        "pageknot.browser.acquisition_cancelled"
    );
    backend.close().await?;
    Ok(())
}

#[tokio::test]
async fn closing_an_unstarted_backend_is_idempotent() -> TestResult {
    let directory = tempfile::tempdir()?;
    let backend = test_backend(&directory)?;

    backend.close().await?;
    backend.close().await?;

    assert!(backend.active_browser().await.is_none());
    Ok(())
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium"]
async fn local_browser_restarts_after_its_process_tree_crashes() -> TestResult {
    let directory = tempfile::tempdir()?;
    let backend = test_backend(&directory)?;
    let endpoints: TestResult<(String, String)> = async {
        let first_lease = backend
            .acquire(acquire_request(), CancellationToken::new())
            .await?;
        let first_endpoint = local_endpoint(&backend).await?;
        first_lease.close().await?;
        {
            let owner = backend.owner.lock().await;
            let Some(BrowserOwner::Local { process, .. }) = owner.as_ref() else {
                return Err(std::io::Error::other("expected an owned local browser").into());
            };
            process.terminate_process_tree()?;
        }
        let second_lease = backend
            .acquire(acquire_request(), CancellationToken::new())
            .await?;
        let second_endpoint = local_endpoint(&backend).await?;
        second_lease.close().await?;
        Ok((first_endpoint, second_endpoint))
    }
    .await;
    let cleanup = backend.close().await;
    let (first_endpoint, second_endpoint) = endpoints?;
    cleanup?;
    assert_ne!(first_endpoint, second_endpoint);
    Ok(())
}

fn test_backend(
    directory: &tempfile::TempDir,
) -> Result<ChromiumBackend, Box<dyn Error + Send + Sync>> {
    let cache_dir = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).map_err(|path| {
        std::io::Error::other(format!("non-UTF-8 test path: {}", path.display()))
    })?;
    Ok(ChromiumBackend::new(
        ChromiumBackendOptions::new(
            ChromiumDiscovery::new().with_managed_cache(cache_dir.clone()),
            cache_dir,
        )
        .with_browser_installation(BrowserInstallationPolicy::Explicit),
    ))
}

fn acquire_request() -> BrowserAcquireRequest {
    BrowserAcquireRequest {
        capture_id: CaptureId::new(),
        browser: BrowserSpec::Auto,
        headed: false,
    }
}

async fn local_endpoint(backend: &ChromiumBackend) -> TestResult<String> {
    let owner = backend.owner.lock().await;
    match owner.as_ref() {
        Some(BrowserOwner::Local { process, .. }) => Ok(process.endpoint().to_string()),
        Some(BrowserOwner::Remote { .. }) | None => {
            Err(std::io::Error::other("expected an owned local browser").into())
        }
    }
}
