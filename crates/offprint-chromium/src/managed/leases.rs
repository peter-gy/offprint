use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use camino::Utf8Path;
use fs2::FileExt as _;
use offprint_model::Result;

use super::cache::set_owner_file_permissions;
use super::managed_error;

const LEASE_FILE_PREFIX: &str = ".offprint-browser-lease-";
static NEXT_LEASE_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct ManagedBrowserLease {
    file: Option<File>,
    path: PathBuf,
}

impl Drop for ManagedBrowserLease {
    fn drop(&mut self) {
        if let Some(file) = self.file.take() {
            let _ignored = fs2::FileExt::unlock(&file);
            drop(file);
        }
        let _ignored = fs::remove_file(&self.path);
    }
}

pub(super) fn create(cache_dir: &Utf8Path, revision: &str) -> Result<ManagedBrowserLease> {
    let identifier = NEXT_LEASE_ID.fetch_add(1, Ordering::Relaxed);
    let path = cache_dir.join(format!(
        "{LEASE_FILE_PREFIX}{revision}-{}-{identifier}",
        std::process::id()
    ));
    let file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)
        .map_err(|error| {
            managed_error(
                "offprint.browser.active",
                format!("failed to create managed browser lease marker: {error}"),
            )
        })?;
    set_owner_file_permissions(path.as_std_path())?;
    file.try_lock_exclusive().map_err(|error| {
        managed_error(
            "offprint.browser.active",
            format!("failed to lock managed browser lease marker: {error}"),
        )
        .retryable(true)
    })?;
    Ok(ManagedBrowserLease {
        file: Some(file),
        path: path.into_std_path_buf(),
    })
}

pub(super) fn count_active(cache_dir: &Utf8Path, revision: &str) -> Result<u32> {
    let prefix = format!("{LEASE_FILE_PREFIX}{revision}-");
    let entries = fs::read_dir(cache_dir).map_err(|error| {
        managed_error(
            "offprint.browser.active",
            format!("failed to inspect managed browser leases: {error}"),
        )
    })?;
    let mut active = 0_u32;
    for entry in entries {
        let entry = entry.map_err(|error| {
            managed_error(
                "offprint.browser.active",
                format!("failed to inspect a managed browser lease: {error}"),
            )
        })?;
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            managed_error(
                "offprint.browser.active",
                format!("failed to inspect a managed browser lease marker: {error}"),
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(managed_error(
                "offprint.browser.active",
                "managed browser lease marker must be a regular file",
            ));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| {
                managed_error(
                    "offprint.browser.active",
                    format!("failed to open a managed browser lease marker: {error}"),
                )
            })?;
        match file.try_lock_exclusive() {
            Ok(()) => {
                let _ignored = fs2::FileExt::unlock(&file);
                drop(file);
                fs::remove_file(&path).map_err(|error| {
                    managed_error(
                        "offprint.browser.active",
                        format!("failed to remove a stale managed browser lease: {error}"),
                    )
                })?;
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                active = active.saturating_add(1);
            }
            Err(error) => {
                return Err(managed_error(
                    "offprint.browser.active",
                    format!("failed to inspect a managed browser lease lock: {error}"),
                )
                .retryable(true));
            }
        }
    }
    Ok(active)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{count_active, create};
    use crate::managed::cache;

    #[test]
    fn lease_markers_report_live_owners_and_clean_up_on_drop()
    -> Result<(), Box<dyn std::error::Error>> {
        let cache_dir = TempDir::new()?;
        let cache_path = camino::Utf8Path::from_path(cache_dir.path())
            .ok_or("temporary cache path is not UTF-8")?;
        cache::prepare(cache_path)?;
        let lease = create(cache_path, "1654411")?;

        assert_eq!(count_active(cache_path, "1654411")?, 1);

        drop(lease);
        assert_eq!(count_active(cache_path, "1654411")?, 0);
        Ok(())
    }
}
