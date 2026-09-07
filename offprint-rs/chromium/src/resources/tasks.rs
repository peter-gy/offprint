use std::collections::BTreeMap;
use std::future::Future;

use futures_util::future::{BoxFuture, FutureExt as _, Shared};
use offprint_model::Result;
use tokio::sync::oneshot;
use tokio::task::{JoinError, JoinSet};

use super::identity::RequestKey;

type Completion = Shared<BoxFuture<'static, ()>>;

#[derive(Debug)]
struct TaskOutcome {
    key: RequestKey,
    generation: u64,
    result: Result<()>,
}

struct TaskTail {
    generation: u64,
    completion: Completion,
}

pub(super) struct OrderedBodyTasks {
    maximum: usize,
    next_generation: u64,
    tails: BTreeMap<RequestKey, TaskTail>,
    tasks: JoinSet<TaskOutcome>,
}

impl OrderedBodyTasks {
    pub(super) fn new(maximum: usize) -> Self {
        Self {
            maximum,
            next_generation: 0,
            tails: BTreeMap::new(),
            tasks: JoinSet::new(),
        }
    }

    pub(super) fn has_capacity(&self) -> bool {
        self.tasks.len() < self.maximum
    }

    pub(super) fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub(super) fn spawn<F>(&mut self, key: RequestKey, work: F) -> bool
    where
        F: Future<Output = Result<()>> + Send + 'static,
    {
        if !self.has_capacity() {
            return false;
        }
        self.next_generation = self.next_generation.saturating_add(1);
        let generation = self.next_generation;
        let previous = self.tails.get(&key).map(|tail| tail.completion.clone());
        let (completed, completion) = oneshot::channel();
        let tail = async move {
            let _ignored = completion.await;
        }
        .boxed()
        .shared();
        self.tails.insert(
            key.clone(),
            TaskTail {
                generation,
                completion: tail,
            },
        );
        self.tasks.spawn(async move {
            if let Some(previous) = previous {
                previous.await;
            }
            let result = work.await;
            let _ignored = completed.send(());
            TaskOutcome {
                key,
                generation,
                result,
            }
        });
        true
    }

    pub(super) async fn join_next(&mut self) -> Option<std::result::Result<Result<()>, JoinError>> {
        let joined = self.tasks.join_next().await?;
        Some(self.finish(joined))
    }

    pub(super) fn try_join_next(&mut self) -> Option<std::result::Result<Result<()>, JoinError>> {
        let joined = self.tasks.try_join_next()?;
        Some(self.finish(joined))
    }

    pub(super) async fn abort_and_drain(&mut self) {
        self.tasks.abort_all();
        while self.tasks.join_next().await.is_some() {}
        self.tails.clear();
    }

    fn finish(
        &mut self,
        joined: std::result::Result<TaskOutcome, JoinError>,
    ) -> std::result::Result<Result<()>, JoinError> {
        let outcome = joined?;
        if self
            .tails
            .get(&outcome.key)
            .is_some_and(|tail| tail.generation == outcome.generation)
        {
            self.tails.remove(&outcome.key);
        }
        Ok(outcome.result)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::{Mutex, oneshot};
    use tokio::time::timeout;

    use super::*;

    type TestResult = std::result::Result<(), Box<dyn Error + Send + Sync>>;

    #[tokio::test]
    async fn tasks_for_one_request_keep_event_order() -> TestResult {
        let mut tasks = OrderedBodyTasks::new(2);
        let order = Arc::new(Mutex::new(Vec::new()));
        let (release, wait_for_release) = oneshot::channel();
        let first_order = Arc::clone(&order);
        assert!(
            tasks.spawn(("session".into(), "request".into()), async move {
                first_order.lock().await.push(1);
                let _ignored = wait_for_release.await;
                Ok(())
            })
        );
        let second_order = Arc::clone(&order);
        assert!(
            tasks.spawn(("session".into(), "request".into()), async move {
                second_order.lock().await.push(2);
                Ok(())
            })
        );

        timeout(Duration::from_secs(1), async {
            while order.lock().await.as_slice() != [1] {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        tokio::task::yield_now().await;
        assert_eq!(order.lock().await.as_slice(), [1]);

        let _ignored = release.send(());
        while !tasks.is_empty() {
            assert!(tasks.join_next().await.is_some_and(|result| result.is_ok()));
        }
        assert_eq!(order.lock().await.as_slice(), [1, 2]);
        Ok(())
    }

    #[tokio::test]
    async fn tasks_for_distinct_requests_progress_independently() -> TestResult {
        let mut tasks = OrderedBodyTasks::new(2);
        let (release, wait_for_release) = oneshot::channel();
        assert!(tasks.spawn(("session".into(), "first".into()), async move {
            let _ignored = wait_for_release.await;
            Ok(())
        }));
        let (started, second_started) = oneshot::channel();
        assert!(
            tasks.spawn(("session".into(), "second".into()), async move {
                let _ignored = started.send(());
                Ok(())
            })
        );

        timeout(Duration::from_secs(1), second_started).await??;
        let _ignored = release.send(());
        while !tasks.is_empty() {
            let _ignored = tasks.join_next().await;
        }
        Ok(())
    }

    #[tokio::test]
    async fn task_count_is_bounded() {
        let mut tasks = OrderedBodyTasks::new(1);
        let (_release, wait_for_release) = oneshot::channel::<()>();
        assert!(tasks.spawn(("session".into(), "first".into()), async move {
            let _ignored = wait_for_release.await;
            Ok(())
        }));

        assert!(!tasks.spawn(("session".into(), "second".into()), async { Ok(()) }));
        tasks.abort_and_drain().await;
    }
}
