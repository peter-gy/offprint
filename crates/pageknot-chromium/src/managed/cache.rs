use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use camino::{Utf8Path, Utf8PathBuf};
use fs2::FileExt as _;
use pageknot_model::Result;
use tempfile::TempDir;

use super::managed_error;

const CACHE_LOCK_FILE: &str = ".pageknot-browser.lock";
const CACHE_LOCK_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub(super) struct CacheLock {
    file: File,
}

impl Drop for CacheLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

pub(super) fn prepare(cache_dir: &Utf8Path) -> Result<()> {
    match fs::symlink_metadata(cache_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(managed_error(
                "pageknot.browser.install",
                format!("managed browser cache `{cache_dir}` must be a directory"),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(cache_dir).map_err(|error| {
                managed_error(
                    "pageknot.browser.install",
                    format!("failed to create managed browser cache `{cache_dir}`: {error}"),
                )
            })?;
        }
        Err(error) => {
            return Err(managed_error(
                "pageknot.browser.install",
                format!("failed to inspect managed browser cache `{cache_dir}`: {error}"),
            ));
        }
    }
    set_owner_directory_permissions(cache_dir.as_std_path())
}

pub(super) fn optional_metadata(cache_dir: &Utf8Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(cache_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(managed_error(
                "pageknot.browser.install",
                format!("managed browser cache `{cache_dir}` must be a directory"),
            ))
        }
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(managed_error(
            "pageknot.browser.install",
            format!("failed to inspect managed browser cache `{cache_dir}`: {error}"),
        )),
    }
}

pub(super) async fn acquire_lock(cache_dir: &Utf8Path) -> Result<CacheLock> {
    let path = cache_dir.join(CACHE_LOCK_FILE);
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| {
            managed_error(
                "pageknot.browser.install",
                format!("failed to open managed browser cache lock `{path}`: {error}"),
            )
        })?;
    set_owner_file_permissions(path.as_std_path())?;
    let started = Instant::now();
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(CacheLock { file }),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if started.elapsed() >= CACHE_LOCK_TIMEOUT {
                    return Err(managed_error(
                        "pageknot.browser.install",
                        "timed out waiting for the managed browser cache lock",
                    )
                    .retryable(true));
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) => {
                return Err(managed_error(
                    "pageknot.browser.install",
                    format!("failed to lock the managed browser cache: {error}"),
                )
                .retryable(true));
            }
        }
    }
}

pub(super) fn commit_staging(staging: TempDir, destination: &Utf8Path) -> Result<()> {
    let staging_path = staging.keep();
    fs::rename(&staging_path, destination).map_err(|error| {
        let _cleanup = fs::remove_dir_all(&staging_path);
        managed_error(
            "pageknot.browser.install",
            format!("failed to commit managed browser directory atomically: {error}"),
        )
    })
}

pub(super) fn sync_staging(staging: &Path, metadata_file: &str) -> Result<()> {
    let metadata = staging.join(metadata_file);
    File::open(&metadata)
        .and_then(|file| file.sync_all())
        .map_err(|error| {
            managed_error(
                "pageknot.browser.install",
                format!("failed to sync managed browser commit metadata: {error}"),
            )
        })?;
    sync_directory(staging)
}

pub(super) fn sync_cache(cache_dir: &Utf8Path) -> Result<()> {
    sync_directory(cache_dir.as_std_path())
}

pub(super) fn utf8_join(root: &Path, relative: &str) -> Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(root.join(relative)).map_err(|path| {
        managed_error(
            "pageknot.browser.install",
            format!(
                "managed browser path is not valid UTF-8: {}",
                path.display()
            ),
        )
    })
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| {
            managed_error(
                "pageknot.browser.install",
                format!(
                    "failed to sync managed browser directory `{}`: {error}",
                    path.display()
                ),
            )
        })
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn set_owner_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
        managed_error(
            "pageknot.browser.install",
            format!(
                "failed to restrict managed browser directory `{}`: {error}",
                path.display()
            ),
        )
    })
}

#[cfg(not(unix))]
pub(super) fn set_owner_directory_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn set_owner_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        managed_error(
            "pageknot.browser.install",
            format!(
                "failed to restrict managed browser file `{}`: {error}",
                path.display()
            ),
        )
    })
}

#[cfg(not(unix))]
pub(super) fn set_owner_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}
