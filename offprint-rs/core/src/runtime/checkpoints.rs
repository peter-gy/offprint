use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, Weak};

use offprint_model::{ErrorStage, OffprintError, PortablePath, Result};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tokio_util::task::task_tracker::TaskTrackerToken;

use super::{closed_error, operation_cancelled_error};

#[derive(Debug, Default)]
pub(crate) struct CheckpointRegistry {
    admission: Mutex<Admission>,
    tasks: TaskTracker,
}

#[derive(Debug, Default)]
struct Admission {
    closed: bool,
    destinations: BTreeMap<PortablePath, Weak<AsyncMutex<()>>>,
}

#[derive(Debug)]
pub(crate) struct CheckpointLease {
    destination: PortablePath,
    _destination_guard: OwnedMutexGuard<()>,
    _runtime_guard: TaskTrackerToken,
}

impl CheckpointLease {
    pub(crate) const fn destination(&self) -> &PortablePath {
        &self.destination
    }

    pub(crate) async fn run_blocking<T: Send + 'static>(
        self: &Arc<Self>,
        operation: impl FnOnce(&PortablePath) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let lease = Arc::clone(self);
        tokio::task::spawn_blocking(move || operation(lease.destination()))
            .await
            .map_err(|error| {
                checkpoint_path_error(format!("resume checkpoint task failed: {error}"))
            })?
    }
}

impl CheckpointRegistry {
    pub(crate) async fn acquire(
        &self,
        destination: &PortablePath,
        cancellation: &CancellationToken,
    ) -> Result<Arc<CheckpointLease>> {
        let destination = resolve_destination(destination).await?;
        self.acquire_resolved(destination, cancellation).await
    }

    async fn acquire_resolved(
        &self,
        destination: PortablePath,
        cancellation: &CancellationToken,
    ) -> Result<Arc<CheckpointLease>> {
        let (gate, runtime_guard) = {
            let mut admission = self
                .admission
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if admission.closed {
                return Err(closed_error());
            }
            admission
                .destinations
                .retain(|_, gate| gate.strong_count() != 0);
            let gate = admission
                .destinations
                .get(&destination)
                .and_then(Weak::upgrade)
                .unwrap_or_else(|| {
                    let gate = Arc::new(AsyncMutex::new(()));
                    admission
                        .destinations
                        .insert(destination.clone(), Arc::downgrade(&gate));
                    gate
                });
            (gate, self.tasks.token())
        };
        let destination_guard = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(operation_cancelled_error()),
            guard = gate.lock_owned() => guard,
        };
        // Closing admission and registering the owner share one lock. A waiter
        // cannot become a new scheduler after shutdown seals the registry.
        if self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .closed
        {
            return Err(closed_error());
        }
        Ok(Arc::new(CheckpointLease {
            destination,
            _destination_guard: destination_guard,
            _runtime_guard: runtime_guard,
        }))
    }

    pub(crate) fn close_admission(&self) {
        let mut admission = self
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        admission.closed = true;
        self.tasks.close();
    }

    pub(crate) async fn wait(&self) {
        self.tasks.wait().await;
    }
}

async fn resolve_destination(destination: &PortablePath) -> Result<PortablePath> {
    let path = destination.as_std_path();
    let filename = path
        .file_name()
        .ok_or_else(|| checkpoint_path_error("checkpoint path must contain a file name"))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let metadata = tokio::fs::symlink_metadata(parent).await.map_err(|error| {
        checkpoint_path_error(format!("checkpoint directory is unavailable: {error}"))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(checkpoint_path_error(
            "checkpoint parent must be a directly addressed directory",
        ));
    }
    // Resolve the parent, not a possibly absent destination. Both the ownership
    // key and file operations use this path, including aliases through ancestors.
    let parent = tokio::fs::canonicalize(parent).await.map_err(|error| {
        checkpoint_path_error(format!("checkpoint directory cannot be resolved: {error}"))
    })?;
    PortablePath::from_path_buf(parent.join(filename))
}

fn checkpoint_path_error(message: impl Into<String>) -> OffprintError {
    OffprintError::new(
        "offprint.scheduler.manifest_write",
        ErrorStage::Commit,
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt as _;
    use tokio::sync::oneshot;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    #[tokio::test]
    async fn dropped_waiter_retains_writer_ownership_until_the_commit_ends() -> TestResult {
        let service = crate::Offprint::builder().build()?;
        let registry = &service.state.checkpoints;
        let directory = tempfile::tempdir()?;
        let path = PortablePath::from_path_buf(directory.path().join("resume.json"))?;
        let lease = registry.acquire(&path, &CancellationToken::new()).await?;
        let (entered, started) = oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let path = lease.destination().clone();
        let mut commit = Box::pin(lease.run_blocking(move |_| {
            assert!(entered.send(()).is_ok());
            released
                .recv_timeout(std::time::Duration::from_secs(5))
                .map_err(|error| checkpoint_path_error(error.to_string()))?;
            Ok(())
        }));
        assert!(commit.as_mut().now_or_never().is_none());
        tokio::time::timeout(std::time::Duration::from_secs(5), started).await??;
        // Dropping the caller's handle detaches the task, but not its lease.
        drop(commit);
        drop(lease);
        let cancellation = CancellationToken::new();
        let next = registry.acquire_resolved(path.clone(), &cancellation);
        tokio::pin!(next);
        assert!(next.as_mut().now_or_never().is_none());
        let mut closing = Box::pin(service.close());
        assert!(closing.as_mut().now_or_never().is_none());
        assert!(registry.wait().now_or_never().is_none());
        cancellation.cancel();
        assert!(next.await.is_err());
        release.send(())?;
        tokio::time::timeout(std::time::Duration::from_secs(5), closing).await??;
        assert!(
            registry
                .acquire(&path, &CancellationToken::new())
                .await
                .is_err()
        );
        Ok(())
    }

    #[tokio::test]
    async fn failed_writer_releases_destination_and_runtime_ownership() -> TestResult {
        let registry = CheckpointRegistry::default();
        let directory = tempfile::tempdir()?;
        let path = PortablePath::from_path_buf(directory.path().join("resume.json"))?;
        let lease = registry.acquire(&path, &CancellationToken::new()).await?;
        let failure = lease
            .run_blocking::<()>(|_| Err(checkpoint_path_error("injected commit failure")))
            .await;
        assert!(failure.is_err());
        drop(lease);
        let lease = registry.acquire(&path, &CancellationToken::new()).await?;
        drop(lease);
        registry.close_admission();
        assert!(registry.wait().now_or_never().is_some());
        Ok(())
    }

    #[tokio::test]
    async fn normalized_aliases_share_one_session_but_other_destinations_do_not() -> TestResult {
        let registry = CheckpointRegistry::default();
        let directory = tempfile::tempdir()?;
        std::fs::create_dir(directory.path().join("child"))?;
        let direct = PortablePath::from_path_buf(directory.path().join("resume.json"))?;
        let alias = PortablePath::from_path_buf(directory.path().join("child/../resume.json"))?;
        let direct = resolve_destination(&direct).await?;
        let alias = resolve_destination(&alias).await?;
        assert_eq!(direct, alias);
        let first = registry
            .acquire_resolved(direct.clone(), &CancellationToken::new())
            .await?;
        let cancellation = CancellationToken::new();
        let next = registry.acquire_resolved(alias, &cancellation);
        tokio::pin!(next);
        assert!(next.as_mut().now_or_never().is_none());
        let other = PortablePath::from_path_buf(directory.path().join("other.json"))?;
        let independent = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            registry.acquire(&other, &cancellation),
        )
        .await??;
        drop(first);
        let resumed = tokio::time::timeout(std::time::Duration::from_secs(5), next).await??;
        assert_eq!(resumed.destination(), &direct);
        drop((independent, resumed));
        registry.close_admission();
        registry.wait().await;
        Ok(())
    }
}
