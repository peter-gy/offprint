use std::time::Duration;

use camino::Utf8Path;
use offprint_model::Result;
use sha2::{Digest as _, Sha256};
use tempfile::TempPath;
use tokio::io::AsyncWriteExt as _;

use super::cache::set_owner_file_permissions;
use super::catalog::ManagedBrowserCatalogEntry;
use super::managed_error;

const DOWNLOAD_HOST: &str = "storage.googleapis.com";
const MAXIMUM_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);

pub(super) async fn download(
    cache_dir: &Utf8Path,
    entry: &ManagedBrowserCatalogEntry,
) -> Result<TempPath> {
    if entry.archive_bytes > MAXIMUM_ARCHIVE_BYTES {
        return Err(managed_error(
            "offprint.browser.install",
            "trusted managed browser archive exceeds the installer byte limit",
        ));
    }
    let url = url::Url::parse(entry.archive_url).map_err(|error| {
        managed_error(
            "offprint.browser.install",
            format!("trusted managed browser URL is invalid: {error}"),
        )
    })?;
    if url.scheme() != "https" || url.host_str() != Some(DOWNLOAD_HOST) {
        return Err(managed_error(
            "offprint.browser.install",
            "trusted managed browser URL violates the distribution policy",
        ));
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(30))
        .timeout(DOWNLOAD_TIMEOUT)
        .user_agent(format!("offprint/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| {
            managed_error(
                "offprint.browser.install",
                format!("failed to create managed browser download client: {error}"),
            )
        })?;
    let mut response = client.get(url).send().await.map_err(|error| {
        managed_error(
            "offprint.browser.install",
            format!("managed browser download failed: {error}"),
        )
        .retryable(error.is_connect() || error.is_timeout())
    })?;
    if !response.status().is_success() {
        return Err(managed_error(
            "offprint.browser.install",
            format!(
                "managed browser distribution returned HTTP {}",
                response.status().as_u16()
            ),
        )
        .retryable(response.status().is_server_error()));
    }
    if response.content_length() != Some(entry.archive_bytes) {
        return Err(managed_error(
            "offprint.browser.install",
            format!(
                "managed browser archive length is {:?}, expected {}",
                response.content_length(),
                entry.archive_bytes
            ),
        ));
    }

    let temporary = tempfile::Builder::new()
        .prefix(".offprint-browser-download-")
        .suffix(".zip")
        .tempfile_in(cache_dir)
        .map_err(|error| {
            managed_error(
                "offprint.browser.install",
                format!("failed to create managed browser download file: {error}"),
            )
        })?;
    let (file, path) = temporary.into_parts();
    set_owner_file_permissions(path.as_ref())?;
    let mut file = tokio::fs::File::from_std(file);
    let mut hasher = Sha256::new();
    let mut received = 0_u64;
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        managed_error(
            "offprint.browser.install",
            format!("failed to read managed browser archive: {error}"),
        )
        .retryable(true)
    })? {
        received = received
            .checked_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX))
            .ok_or_else(|| {
                managed_error(
                    "offprint.browser.install",
                    "managed browser archive byte count overflowed",
                )
            })?;
        if received > entry.archive_bytes || received > MAXIMUM_ARCHIVE_BYTES {
            return Err(managed_error(
                "offprint.browser.install",
                "managed browser archive exceeded its declared byte limit",
            ));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|error| {
            managed_error(
                "offprint.browser.install",
                format!("failed to write managed browser archive: {error}"),
            )
        })?;
    }
    if received != entry.archive_bytes {
        return Err(managed_error(
            "offprint.browser.install",
            format!(
                "managed browser archive ended at {received} bytes, expected {}",
                entry.archive_bytes
            ),
        ));
    }
    let digest = hex::encode(hasher.finalize());
    if digest != entry.archive_sha256 {
        return Err(managed_error(
            "offprint.browser.install",
            "managed browser archive digest does not match the trusted catalog",
        )
        .with_detail("expectedSha256", entry.archive_sha256)
        .with_detail("actualSha256", digest));
    }
    file.flush().await.map_err(|error| {
        managed_error(
            "offprint.browser.install",
            format!("failed to flush managed browser archive: {error}"),
        )
    })?;
    file.sync_all().await.map_err(|error| {
        managed_error(
            "offprint.browser.install",
            format!("failed to sync managed browser archive: {error}"),
        )
    })?;
    Ok(path)
}
