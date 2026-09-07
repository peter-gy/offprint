use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use offprint_model::{
    BrowserInfo, BrowserSource, ErrorStage, ManagedBrowserState, OffprintError, Result,
};

use crate::discovery::probe;

mod archive;
mod cache;
mod catalog;
mod download;
mod installed;
mod leases;

pub use catalog::{
    DEFAULT_MANAGED_BROWSER_REVISION, MANAGED_BROWSER_CATALOG_VERSION, ManagedBrowserCatalogEntry,
    managed_browser_catalog,
};
pub use leases::ManagedBrowserLease;

use catalog::{entry_directory, entry_for_revision};

#[derive(Clone, Debug)]
pub struct ManagedBrowserManager {
    cache_dir: Utf8PathBuf,
}

impl ManagedBrowserManager {
    #[must_use]
    pub fn new(cache_dir: Utf8PathBuf) -> Self {
        Self { cache_dir }
    }

    #[must_use]
    pub fn cache_dir(&self) -> &Utf8Path {
        &self.cache_dir
    }

    pub async fn install(&self, revision: Option<&str>) -> Result<BrowserInfo> {
        let entry = entry_for_revision(revision.unwrap_or(DEFAULT_MANAGED_BROWSER_REVISION))?;
        cache::prepare(&self.cache_dir)?;
        let _lock = cache::acquire_lock(&self.cache_dir).await?;

        if let Some(browser) = installed::probe_entry(&self.cache_dir, entry).await? {
            return Ok(browser);
        }

        let final_dir = entry_directory(&self.cache_dir, entry);
        if final_dir.exists() {
            return Err(managed_error(
                "offprint.browser.install",
                format!(
                    "managed browser directory `{final_dir}` is present but does not match the trusted catalog"
                ),
            )
            .with_detail("revision", entry.revision)
            .with_detail(
                "recoveryCommand",
                format!("offprint browser remove {} --force", entry.revision),
            ));
        }

        let archive = download::download(&self.cache_dir, entry).await?;
        let staging = tempfile::Builder::new()
            .prefix(".offprint-browser-stage-")
            .tempdir_in(&self.cache_dir)
            .map_err(|error| {
                managed_error(
                    "offprint.browser.install",
                    format!("failed to create managed browser staging directory: {error}"),
                )
            })?;
        cache::set_owner_directory_permissions(staging.path())?;

        let archive_path = archive.to_path_buf();
        let staging_path = staging.path().to_owned();
        tokio::task::spawn_blocking(move || archive::extract(&archive_path, &staging_path))
            .await
            .map_err(|error| {
                managed_error(
                    "offprint.browser.install",
                    format!("managed browser extraction task failed: {error}"),
                )
            })??;

        let metadata_file = installed::write_metadata(staging.path(), entry)?;
        let staged_executable = cache::utf8_join(staging.path(), entry.executable)?;
        let mut browser = probe(&staged_executable, BrowserSource::Managed).await?;
        if browser.version != entry.version {
            return Err(managed_error(
                "offprint.browser.install",
                format!(
                    "managed browser probe returned version {}, expected {}",
                    browser.version, entry.version
                ),
            )
            .with_detail("revision", entry.revision));
        }

        cache::sync_staging(staging.path(), metadata_file)?;
        cache::commit_staging(staging, &final_dir)?;
        cache::sync_cache(&self.cache_dir)?;

        browser.executable_path = Some(final_dir.join(entry.executable).into());
        browser.revision = Some(entry.revision.to_owned());
        Ok(browser)
    }

    pub async fn installed(&self) -> Result<Vec<BrowserInfo>> {
        installed::discover(&self.cache_dir).await
    }

    pub async fn state(&self) -> Result<ManagedBrowserState> {
        let browsers = self.installed().await?;
        let installed_revisions = browsers
            .iter()
            .filter_map(|browser| browser.revision.clone())
            .collect::<Vec<_>>();
        Ok(ManagedBrowserState {
            cache_dir: self.cache_dir.clone().into(),
            selected_revision: installed_revisions.first().cloned(),
            installed_revisions,
            catalog_version: MANAGED_BROWSER_CATALOG_VERSION.to_owned(),
        })
    }

    pub async fn lease(&self, revision: &str) -> Result<ManagedBrowserLease> {
        let entry = entry_for_revision(revision)?;
        cache::prepare(&self.cache_dir)?;
        let _lock = cache::acquire_lock(&self.cache_dir).await?;
        let directory = entry_directory(&self.cache_dir, entry);
        if !directory.exists() {
            return Err(managed_error(
                "offprint.browser.install",
                format!("managed browser revision `{revision}` is not installed"),
            )
            .with_detail("revision", revision));
        }
        installed::validate_installation(&directory, entry)?;
        let _active_leases = leases::count_active(&self.cache_dir, revision)?;
        leases::create(&self.cache_dir, revision)
    }

    pub async fn active_leases(&self, revision: &str) -> Result<u32> {
        entry_for_revision(revision)?;
        cache::prepare(&self.cache_dir)?;
        let _lock = cache::acquire_lock(&self.cache_dir).await?;
        leases::count_active(&self.cache_dir, revision)
    }

    pub async fn remove(&self, revision: &str) -> Result<()> {
        catalog::validate_revision(revision)?;
        cache::prepare(&self.cache_dir)?;
        let _lock = cache::acquire_lock(&self.cache_dir).await?;
        let active_leases = leases::count_active(&self.cache_dir, revision)?;
        if active_leases > 0 {
            return Err(managed_error(
                "offprint.browser.active",
                format!(
                    "managed browser revision `{revision}` has {active_leases} active cross-process leases"
                ),
            )
            .with_detail("revision", revision)
            .with_detail("activeLeases", active_leases));
        }
        let entry = entry_for_revision(revision)?;
        let final_dir = entry_directory(&self.cache_dir, entry);
        let Some(_browser) = installed::probe_entry(&self.cache_dir, entry).await? else {
            return Err(managed_error(
                "offprint.browser.install",
                format!("managed browser revision `{revision}` is not installed"),
            )
            .with_detail("revision", revision));
        };
        let trash = tempfile::Builder::new()
            .prefix(".offprint-browser-trash-")
            .tempdir_in(&self.cache_dir)
            .map_err(|error| {
                managed_error(
                    "offprint.browser.install",
                    format!("failed to reserve managed browser trash path: {error}"),
                )
            })?;
        let trash_path = trash.path().to_owned();
        drop(trash);
        fs::rename(&final_dir, &trash_path).map_err(|error| {
            managed_error(
                "offprint.browser.install",
                format!("failed to detach managed browser revision `{revision}`: {error}"),
            )
        })?;
        cache::sync_cache(&self.cache_dir)?;
        tokio::task::spawn_blocking(move || fs::remove_dir_all(&trash_path))
            .await
            .map_err(|error| {
                managed_error(
                    "offprint.browser.install",
                    format!("managed browser cleanup task failed: {error}"),
                )
            })?
            .map_err(|error| {
                managed_error(
                    "offprint.browser.install",
                    format!("failed to delete detached managed browser revision: {error}"),
                )
            })
    }
}

fn managed_error(code: &'static str, message: impl Into<String>) -> OffprintError {
    OffprintError::new(code, ErrorStage::Browser, message)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::{ManagedBrowserManager, cache, leases};

    #[tokio::test]
    async fn missing_cache_reports_no_installed_browsers() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = TempDir::new()?;
        let cache_path = camino::Utf8Path::from_path(root.path())
            .ok_or("temporary cache path is not UTF-8")?
            .join("missing");
        let manager = ManagedBrowserManager::new(cache_path);

        assert!(manager.installed().await?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn managed_removal_stops_at_a_cross_process_lease()
    -> Result<(), Box<dyn std::error::Error>> {
        let cache_dir = TempDir::new()?;
        let cache_path = camino::Utf8Path::from_path(cache_dir.path())
            .ok_or("temporary cache path is not UTF-8")?;
        cache::prepare(cache_path)?;
        // Active owners keep their lease even when a revision leaves the catalog.
        let lease = leases::create(cache_path, "1")?;
        let manager = ManagedBrowserManager::new(cache_path.to_owned());

        let error = manager.remove("1").await.err();

        assert_eq!(
            error.as_ref().map(|error| error.code.as_str()),
            Some("offprint.browser.active")
        );
        assert_eq!(
            error
                .as_ref()
                .and_then(|error| error.details.get("activeLeases")),
            Some(&serde_json::json!(1))
        );
        drop(lease);
        Ok(())
    }
}
