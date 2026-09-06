use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use offprint_model::{CaptureEvent, CaptureId, CaptureStatus, OffprintError};
use serde_json::Value;

use crate::diagnostics::{CaptureDiagnostics, sanitized_event};

const DIAGNOSTIC_EVENT_CAPACITY: usize = 4_096;

#[derive(Debug)]
pub(super) struct DiagnosticsRecorder {
    diagnostics: CaptureDiagnostics,
    events: Mutex<VecDeque<Value>>,
    dropped_events: AtomicU64,
}

impl DiagnosticsRecorder {
    pub(super) fn new(diagnostics: CaptureDiagnostics) -> Self {
        Self {
            diagnostics,
            events: Mutex::new(VecDeque::new()),
            dropped_events: AtomicU64::new(0),
        }
    }

    pub(super) fn record(&self, event: &CaptureEvent) {
        let mut events = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if events.len() == DIAGNOSTIC_EVENT_CAPACITY {
            events.pop_front();
            self.dropped_events.fetch_add(1, Ordering::AcqRel);
        }
        events.push_back(sanitized_event(event));
    }

    pub(super) async fn attach(
        &self,
        capture_id: &CaptureId,
        status: CaptureStatus,
        error: OffprintError,
    ) -> OffprintError {
        let events = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect();
        match self
            .diagnostics
            .write(
                capture_id,
                status,
                events,
                self.dropped_events.load(Ordering::Acquire),
                &error,
            )
            .await
        {
            Ok(path) => error.with_diagnostics_path(path),
            Err(diagnostic_error) => {
                error.with_detail("diagnosticsError", diagnostic_error.code.to_string())
            }
        }
    }
}
