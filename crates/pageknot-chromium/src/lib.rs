//! Chromium discovery, managed installation, process ownership, and CDP
//! implementation of the PageKnot browser contracts.
//!
//! The public [`pageknot_browser`] traits remain the service boundary. This
//! crate owns Chromium-specific transport and protocol behavior.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

mod backend;
mod cdp;
mod collector;
mod discovery;
mod launch;
mod managed;
mod offline;
mod page;
mod pdf;
mod proxy;
mod remote;
mod resources;
mod targets;
mod transport;

pub use backend::{ChromiumBackend, ChromiumBackendOptions};
pub use collector::{
    COLLECTOR_BUNDLE, collect_frame_observation, collect_page_observation,
    probe_collector_handshake,
};
pub use discovery::{ChromiumDiscovery, DiscoveryResult};
pub use launch::{ChromiumLaunchOptions, ChromiumProcess};
pub use managed::{
    DEFAULT_MANAGED_BROWSER_REVISION, MANAGED_BROWSER_CATALOG_VERSION, ManagedBrowserCatalogEntry,
    ManagedBrowserLease, ManagedBrowserManager, managed_browser_catalog,
};
pub use page::ChromiumPage;
pub use pageknot_browser::{
    AttachedFrame, CollectedPageObservation, CollectorLimits, LoadedResource, NavigationRedirect,
    NavigationResult, OfflineBrowserObservation, ResourceObservationLimits,
};
pub use remote::{probe_remote_browser, resolve_remote_endpoint};
pub use transport::{CdpClient, CdpEvent};
