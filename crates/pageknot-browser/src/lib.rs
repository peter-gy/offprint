//! Browser-independent contracts used by the PageKnot capture service.
//!
//! Implement [`BrowserBackend`] to connect another browser runtime while
//! preserving PageKnot job, network, resource, and shutdown semantics.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![warn(missing_docs)]

mod backend;
mod network;

pub use backend::{
    AttachedFrame, BodyStream, BrowserAcquireRequest, BrowserBackend, BrowserContext,
    BrowserContextRequest, BrowserLease, CollectedPageObservation, CollectorLimits, LoadedResource,
    NavigationRedirect, NavigationResult, OfflineBrowserObservation, PageSession,
    ReadinessObservation, ResourceObservationLimits,
};
pub use network::{AddressClass, NetworkGuard};
