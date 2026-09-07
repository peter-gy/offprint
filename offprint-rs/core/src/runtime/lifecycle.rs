use std::sync::atomic::{AtomicU8, Ordering};

use offprint_model::{ErrorStage, OffprintError, Result};
use tokio::sync::watch;

const OPEN: u8 = 0;
const CLOSING: u8 = 1;
const CLOSED: u8 = 2;

#[derive(Debug)]
pub(super) struct RuntimeLifecycle {
    phase: AtomicU8,
    terminal: watch::Sender<Option<Result<()>>>,
}

impl RuntimeLifecycle {
    pub(super) fn new() -> Self {
        let (terminal, _) = watch::channel(None);
        Self {
            phase: AtomicU8::new(OPEN),
            terminal,
        }
    }

    pub(super) fn ensure_open(&self) -> Result<()> {
        if self.phase.load(Ordering::Acquire) == OPEN {
            Ok(())
        } else {
            Err(closed_error())
        }
    }

    pub(super) fn begin_close(&self) -> (watch::Receiver<Option<Result<()>>>, bool) {
        let result = self.terminal.subscribe();
        let leader = self
            .phase
            .compare_exchange(OPEN, CLOSING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        (result, leader)
    }

    pub(super) fn complete(&self, result: Result<()>) {
        self.phase.store(CLOSED, Ordering::Release);
        self.terminal.send_replace(Some(result));
    }

    pub(super) fn mark_dropped(&self) {
        self.phase.store(CLOSED, Ordering::Release);
    }

    pub(super) async fn wait(mut result: watch::Receiver<Option<Result<()>>>) -> Result<()> {
        loop {
            if let Some(result) = result.borrow().clone() {
                return result;
            }
            result.changed().await.map_err(|_| {
                OffprintError::new(
                    "offprint.runtime.shutdown",
                    ErrorStage::Shutdown,
                    "runtime shutdown result became unavailable",
                )
            })?;
        }
    }
}

pub(super) fn closed_error() -> OffprintError {
    OffprintError::new(
        "offprint.runtime.closed",
        ErrorStage::Shutdown,
        "Offprint has been closed",
    )
}
