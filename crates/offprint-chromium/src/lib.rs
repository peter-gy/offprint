//! Chromium discovery, managed installation, process ownership, and CDP
//! implementation of the Offprint browser contracts.
//!
//! The public [`offprint_browser`] traits remain the service boundary. This
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
pub use collector::{COLLECTOR_BUNDLE, probe_collector_handshake};
pub use discovery::{ChromiumDiscovery, DiscoveryResult};
pub use launch::{ChromiumLaunchOptions, ChromiumProcess};
pub use managed::{
    DEFAULT_MANAGED_BROWSER_REVISION, MANAGED_BROWSER_CATALOG_VERSION, ManagedBrowserCatalogEntry,
    ManagedBrowserLease, ManagedBrowserManager, managed_browser_catalog,
};
pub use offprint_browser::{
    AttachedFrame, LoadedResource, NavigationRedirect, NavigationResult, ObservationLimits,
    ObservedFrame, OfflineBrowserObservation, ResourceObservationLimits,
};
pub use page::ChromiumPage;
pub use remote::{probe_remote_browser, resolve_remote_endpoint};
pub use transport::{CdpClient, CdpEvent};
