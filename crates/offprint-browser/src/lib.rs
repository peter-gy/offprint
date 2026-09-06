//! Browser-independent contracts used by the Offprint capture service.
//!
//! Implement [`BrowserBackend`] to connect another browser runtime while
//! preserving Offprint job, network, resource, and shutdown semantics.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![warn(missing_docs)]

mod backend;
mod network;
mod observation;

pub use backend::{
    AttachedFrame, BodyStream, BrowserAcquireRequest, BrowserBackend, BrowserContext,
    BrowserContextRequest, BrowserLease, LoadedResource, NavigationRedirect, NavigationResult,
    ObservationLimits, ObservedFrame, OfflineBrowserObservation, PageSession, ReadinessObservation,
    ResourceObservationLimits,
};
pub use network::{AddressClass, NetworkGuard};
pub use observation::{
    FrameObservation, FrameOwnerObservation, ObservationViewport, ObservationWarning,
    SelectionObservation, VisualFallback, VisualFallbackKind,
};
