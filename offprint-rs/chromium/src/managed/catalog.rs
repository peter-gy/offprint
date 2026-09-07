use camino::{Utf8Path, Utf8PathBuf};
use offprint_model::Result;

use super::managed_error;

pub const MANAGED_BROWSER_CATALOG_VERSION: &str = "2026-07-27";
pub const DEFAULT_MANAGED_BROWSER_REVISION: &str = "1654411";

const MANAGED_BROWSER_VERSION: &str = "151.0.7922.47";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagedBrowserCatalogEntry {
    pub revision: &'static str,
    pub version: &'static str,
    pub platform: &'static str,
    pub archive_url: &'static str,
    pub archive_sha256: &'static str,
    pub archive_bytes: u64,
    pub executable: &'static str,
}

pub(super) const MANAGED_BROWSER_CATALOG: &[ManagedBrowserCatalogEntry] = &[
    ManagedBrowserCatalogEntry {
        revision: DEFAULT_MANAGED_BROWSER_REVISION,
        version: MANAGED_BROWSER_VERSION,
        platform: "linux64",
        archive_url: "https://storage.googleapis.com/chrome-for-testing-public/151.0.7922.47/linux64/chrome-linux64.zip",
        archive_sha256: "14ac03a67e154e3f8bbc57e03ef03315fda8fedff8e045eee8b31500283a33f4",
        archive_bytes: 193_274_825,
        executable: "chrome-linux64/chrome",
    },
    ManagedBrowserCatalogEntry {
        revision: DEFAULT_MANAGED_BROWSER_REVISION,
        version: MANAGED_BROWSER_VERSION,
        platform: "mac-arm64",
        archive_url: "https://storage.googleapis.com/chrome-for-testing-public/151.0.7922.47/mac-arm64/chrome-mac-arm64.zip",
        archive_sha256: "9529990b6afd9867a862c7a5bff2a4a8eef84614d910acac22e4c5fa5c24daee",
        archive_bytes: 187_097_179,
        executable: "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    },
    ManagedBrowserCatalogEntry {
        revision: DEFAULT_MANAGED_BROWSER_REVISION,
        version: MANAGED_BROWSER_VERSION,
        platform: "mac-x64",
        archive_url: "https://storage.googleapis.com/chrome-for-testing-public/151.0.7922.47/mac-x64/chrome-mac-x64.zip",
        archive_sha256: "90f49258b8929867640ca59cf138191d25b4b34759e1509687e59a66be9ac99b",
        archive_bytes: 197_089_507,
        executable: "chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    },
    ManagedBrowserCatalogEntry {
        revision: DEFAULT_MANAGED_BROWSER_REVISION,
        version: MANAGED_BROWSER_VERSION,
        platform: "win32",
        archive_url: "https://storage.googleapis.com/chrome-for-testing-public/151.0.7922.47/win32/chrome-win32.zip",
        archive_sha256: "ce6a808b65afc47c4ed2b4fbbd17409832636fe864a359a26157cb4cb828f6b5",
        archive_bytes: 175_079_530,
        executable: "chrome-win32/chrome.exe",
    },
    ManagedBrowserCatalogEntry {
        revision: DEFAULT_MANAGED_BROWSER_REVISION,
        version: MANAGED_BROWSER_VERSION,
        platform: "win64",
        archive_url: "https://storage.googleapis.com/chrome-for-testing-public/151.0.7922.47/win64/chrome-win64.zip",
        archive_sha256: "fc77bb98b550b7da23b14edfa282b59a022e7fdb075ac7625d2a5152ceb22396",
        archive_bytes: 201_077_750,
        executable: "chrome-win64/chrome.exe",
    },
];

#[must_use]
pub const fn managed_browser_catalog() -> &'static [ManagedBrowserCatalogEntry] {
    MANAGED_BROWSER_CATALOG
}

fn current_platform() -> Option<&'static str> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    {
        return Some("linux64");
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        return Some("mac-arm64");
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        return Some("mac-x64");
    }
    #[cfg(all(target_os = "windows", target_arch = "x86"))]
    {
        return Some("win32");
    }
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        return Some("win64");
    }
    #[allow(unreachable_code)]
    None
}

pub(super) fn entries_for_current_platform() -> Result<Vec<&'static ManagedBrowserCatalogEntry>> {
    let platform = selected_platform()?;
    Ok(MANAGED_BROWSER_CATALOG
        .iter()
        .filter(|entry| entry.platform == platform)
        .collect())
}

pub(super) fn entry_for_revision(revision: &str) -> Result<&'static ManagedBrowserCatalogEntry> {
    validate_revision(revision)?;
    let platform = selected_platform()?;
    MANAGED_BROWSER_CATALOG
        .iter()
        .find(|entry| entry.revision == revision && entry.platform == platform)
        .ok_or_else(|| {
            managed_error(
                "offprint.browser.install",
                format!(
                    "managed browser revision `{revision}` is absent from trusted catalog {} for {platform}",
                    MANAGED_BROWSER_CATALOG_VERSION
                ),
            )
            .with_detail("revision", revision)
            .with_detail("catalogVersion", MANAGED_BROWSER_CATALOG_VERSION)
        })
}

pub(super) fn entry_directory(
    cache_dir: &Utf8Path,
    entry: &ManagedBrowserCatalogEntry,
) -> Utf8PathBuf {
    cache_dir.join(format!("chromium-{}", entry.revision))
}

fn selected_platform() -> Result<&'static str> {
    current_platform().ok_or_else(|| {
        managed_error(
            "offprint.browser.install",
            "the current operating system and architecture have no trusted managed browser build",
        )
    })
}

fn validate_revision(revision: &str) -> Result<()> {
    if revision.is_empty()
        || revision.len() > 32
        || !revision.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(managed_error(
            "offprint.browser.install",
            "managed browser revision must contain 1 to 32 ASCII digits",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MANAGED_BROWSER_CATALOG, validate_revision};

    #[test]
    fn catalog_has_one_record_for_each_supported_platform() {
        let platforms = MANAGED_BROWSER_CATALOG
            .iter()
            .map(|entry| entry.platform)
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(
            platforms,
            ["linux64", "mac-arm64", "mac-x64", "win32", "win64"]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn revision_rejects_path_syntax() {
        assert!(validate_revision("../1654411").is_err());
        assert!(validate_revision("1654411").is_ok());
    }
}
