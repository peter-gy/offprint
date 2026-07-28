use std::fmt;

use chrono::{DateTime, Utc};
use pageknot_model::CaptureId;

/// Supplies capture timestamps.
///
/// Implement this trait to inject a deterministic clock.
pub trait Clock: fmt::Debug + Send + Sync {
    /// Returns the current UTC timestamp.
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Clone, Copy, Debug, Default)]
/// The system UTC clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Supplies stable capture identifiers.
///
/// Implement this trait to make capture records deterministic in tests or
/// replay systems.
pub trait CaptureIdGenerator: fmt::Debug + Send + Sync {
    /// Returns the identifier for the next capture.
    fn next_capture_id(&self) -> CaptureId;
}

#[derive(Clone, Copy, Debug, Default)]
/// Generates capture identifiers from monotonic ULIDs.
pub struct UlidCaptureIdGenerator;

impl CaptureIdGenerator for UlidCaptureIdGenerator {
    fn next_capture_id(&self) -> CaptureId {
        CaptureId::new()
    }
}
