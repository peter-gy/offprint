use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::Path;

use camino::Utf8Path;
use offprint_model::{BrowserInfo, BrowserSource, Result};
use serde::{Deserialize, Serialize};

use super::cache::{optional_metadata, set_owner_file_permissions};
use super::catalog::{
    MANAGED_BROWSER_CATALOG_VERSION, ManagedBrowserCatalogEntry, entries_for_current_platform,
    entry_directory,
};
use super::managed_error;
use crate::discovery::probe;

pub(super) const METADATA_FILE: &str = "offprint-browser.json";

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallationMetadata {
    format_version: u32,
    catalog_version: String,
    revision: String,
    version: String,
    platform: String,
    archive_sha256: String,
    archive_bytes: u64,
    executable: String,
}

pub(super) async fn discover(cache_dir: &Utf8Path) -> Result<Vec<BrowserInfo>> {
    let Some(_metadata) = optional_metadata(cache_dir)? else {
        return Ok(Vec::new());
    };
    let mut browsers = Vec::new();
    for entry in entries_for_current_platform()? {
        if let Some(browser) = probe_entry(cache_dir, entry).await? {
            browsers.push(browser);
        }
    }
    browsers.sort_by(|left, right| right.revision.cmp(&left.revision));
    Ok(browsers)
}

pub(super) async fn probe_entry(
    cache_dir: &Utf8Path,
    entry: &ManagedBrowserCatalogEntry,
) -> Result<Option<BrowserInfo>> {
    let final_dir = entry_directory(cache_dir, entry);
    if !final_dir.exists() {
        return Ok(None);
    }
    validate_installation(&final_dir, entry)?;
    let executable = final_dir.join(entry.executable);
    let mut browser = probe(&executable, BrowserSource::Managed).await?;
    if browser.version != entry.version {
        return Err(managed_error(
            "offprint.browser.install",
            format!(
                "managed browser revision `{}` reports version {}, expected {}",
                entry.revision, browser.version, entry.version
            ),
        )
        .with_detail("revision", entry.revision));
    }
    browser.revision = Some(entry.revision.to_owned());
    Ok(Some(browser))
}

pub(super) fn write_metadata(staging: &Path, entry: &ManagedBrowserCatalogEntry) -> Result<()> {
    let metadata = metadata_for(entry);
    let path = staging.join(METADATA_FILE);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|error| {
            managed_error(
                "offprint.browser.install",
                format!("failed to create managed browser metadata: {error}"),
            )
        })?;
    serde_json::to_writer_pretty(&mut file, &metadata).map_err(|error| {
        managed_error(
            "offprint.browser.install",
            format!("failed to serialize managed browser metadata: {error}"),
        )
    })?;
    file.write_all(b"\n").map_err(|error| {
        managed_error(
            "offprint.browser.install",
            format!("failed to finish managed browser metadata: {error}"),
        )
    })?;
    file.sync_all().map_err(|error| {
        managed_error(
            "offprint.browser.install",
            format!("failed to sync managed browser metadata: {error}"),
        )
    })?;
    set_owner_file_permissions(&path)
}

pub(super) fn validate_installation(
    directory: &Utf8Path,
    entry: &ManagedBrowserCatalogEntry,
) -> Result<()> {
    let directory_metadata = fs::symlink_metadata(directory).map_err(|error| {
        managed_error(
            "offprint.browser.install",
            format!("failed to inspect managed browser directory `{directory}`: {error}"),
        )
    })?;
    if directory_metadata.file_type().is_symlink() || !directory_metadata.is_dir() {
        return Err(managed_error(
            "offprint.browser.install",
            format!("managed browser path `{directory}` is not a trusted directory"),
        ));
    }
    let metadata_path = directory.join(METADATA_FILE);
    let metadata_file = File::open(&metadata_path).map_err(|error| {
        managed_error(
            "offprint.browser.install",
            format!("failed to read managed browser metadata `{metadata_path}`: {error}"),
        )
    })?;
    let metadata: InstallationMetadata =
        serde_json::from_reader(metadata_file).map_err(|error| {
            managed_error(
                "offprint.browser.install",
                format!("managed browser metadata is invalid: {error}"),
            )
        })?;
    if metadata != metadata_for(entry) {
        return Err(managed_error(
            "offprint.browser.install",
            format!(
                "managed browser metadata for revision `{}` does not match the trusted catalog",
                entry.revision
            ),
        ));
    }
    let root = fs::canonicalize(directory).map_err(|error| {
        managed_error(
            "offprint.browser.install",
            format!("failed to resolve managed browser directory: {error}"),
        )
    })?;
    let executable_path = directory.join(entry.executable);
    let executable = fs::canonicalize(&executable_path).map_err(|error| {
        managed_error(
            "offprint.browser.install",
            format!("managed browser executable `{executable_path}` is unavailable: {error}"),
        )
    })?;
    if !executable.starts_with(&root)
        || !fs::metadata(&executable).is_ok_and(|metadata| metadata.is_file())
    {
        return Err(managed_error(
            "offprint.browser.install",
            "managed browser executable is outside the trusted installation directory",
        ));
    }
    Ok(())
}

fn metadata_for(entry: &ManagedBrowserCatalogEntry) -> InstallationMetadata {
    InstallationMetadata {
        format_version: 1,
        catalog_version: MANAGED_BROWSER_CATALOG_VERSION.to_owned(),
        revision: entry.revision.to_owned(),
        version: entry.version.to_owned(),
        platform: entry.platform.to_owned(),
        archive_sha256: entry.archive_sha256.to_owned(),
        archive_bytes: entry.archive_bytes,
        executable: entry.executable.to_owned(),
    }
}
