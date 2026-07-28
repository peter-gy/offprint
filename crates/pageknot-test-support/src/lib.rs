//! Hermetic HTTP, HTTPS, artifact, and fixture-manifest support for PageKnot
//! contract tests.
//!
//! The fixture catalog maps each required browser behavior to an executable
//! test runner shared by local commands and CI shards.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]

mod manifest;
mod server;

pub use manifest::{
    FIXTURE_MANIFEST_SCHEMA_VERSION, FixtureCapability, FixtureDefinition, FixtureExpectation,
    FixtureGroup, FixtureManifest, FixtureRunner, fixture_definition, fixture_manifest,
};
pub use server::{
    FixtureCluster, FixtureDelivery, FixtureRequest, FixtureResponse, FixtureRoute, FixtureServer,
};
