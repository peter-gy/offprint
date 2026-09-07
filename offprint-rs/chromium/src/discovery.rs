use std::collections::BTreeSet;
use std::io;
use std::process::Stdio;
use std::time::Duration;

use crate::ManagedBrowserManager;
use camino::Utf8PathBuf;
use offprint_model::{
    BrowserCandidate, BrowserCandidateState, BrowserInfo, BrowserProduct, BrowserSource,
    BrowserSourcePolicy, ErrorStage, OffprintError, Result,
};
#[cfg(target_os = "windows")]
use std::path::PathBuf;
use tokio::io::AsyncReadExt as _;
use tokio::process::Command;
use tokio::time::timeout;

const MINIMUM_CHROMIUM_MAJOR: u32 = 120;
#[cfg(not(windows))]
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
// Allow for PowerShell and .NET startup before reading Windows file metadata.
#[cfg(windows)]
const PROBE_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_PROBE_BYTES: u64 = 4096;

#[derive(Clone, Debug)]
pub struct ChromiumDiscovery {
    explicit_path: Option<Utf8PathBuf>,
    managed_cache: Option<Utf8PathBuf>,
    source_policy: BrowserSourcePolicy,
}

impl ChromiumDiscovery {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            explicit_path: None,
            managed_cache: None,
            source_policy: BrowserSourcePolicy::Auto,
        }
    }

    #[must_use]
    pub fn with_explicit_path(mut self, path: Utf8PathBuf) -> Self {
        self.explicit_path = Some(path);
        self
    }

    #[must_use]
    pub fn with_managed_cache(mut self, cache_dir: Utf8PathBuf) -> Self {
        self.managed_cache = Some(cache_dir);
        self
    }

    #[must_use]
    pub const fn with_source_policy(mut self, policy: BrowserSourcePolicy) -> Self {
        self.source_policy = policy;
        self
    }

    pub async fn discover(&self) -> DiscoveryResult {
        let mut seen = BTreeSet::new();
        let mut candidates = Vec::new();
        let mut failures = Vec::new();
        if let Some(path) = &self.explicit_path {
            push_probed_candidate(
                path,
                BrowserSource::Explicit,
                &mut seen,
                &mut candidates,
                &mut failures,
            )
            .await;
        }
        if self.source_policy != BrowserSourcePolicy::System
            && let Some(cache_dir) = &self.managed_cache
        {
            match ManagedBrowserManager::new(cache_dir.clone())
                .installed()
                .await
            {
                Ok(browsers) => {
                    for browser in browsers {
                        let is_new = browser
                            .executable_path
                            .as_ref()
                            .is_none_or(|path| seen.insert(path.as_utf8_path().to_owned()));
                        if is_new {
                            candidates.push(compatible_candidate(browser, candidates.len()));
                        }
                    }
                }
                Err(error) => failures.push(error),
            }
        }
        if self.source_policy != BrowserSourcePolicy::Managed {
            for path in system_candidate_paths() {
                push_probed_candidate(
                    &path,
                    BrowserSource::System,
                    &mut seen,
                    &mut candidates,
                    &mut failures,
                )
                .await;
            }
        }
        DiscoveryResult {
            selected: candidates
                .first()
                .map(|candidate| candidate.browser.clone()),
            candidates,
            failures,
        }
    }
}

impl Default for ChromiumDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct DiscoveryResult {
    pub selected: Option<BrowserInfo>,
    pub candidates: Vec<BrowserCandidate>,
    pub failures: Vec<OffprintError>,
}

async fn push_probed_candidate(
    path: &Utf8PathBuf,
    source: BrowserSource,
    seen: &mut BTreeSet<Utf8PathBuf>,
    candidates: &mut Vec<BrowserCandidate>,
    failures: &mut Vec<OffprintError>,
) {
    if !seen.insert(path.clone()) {
        return;
    }
    match probe(path, source).await {
        Ok(browser) => {
            let priority = candidates.len();
            candidates.push(compatible_candidate(browser, priority));
        }
        Err(error) => failures.push(error),
    }
}

fn compatible_candidate(browser: BrowserInfo, priority: usize) -> BrowserCandidate {
    BrowserCandidate {
        state: if priority == 0 {
            BrowserCandidateState::Selected
        } else {
            BrowserCandidateState::Compatible
        },
        reason_code: "offprint.browser.compatible".to_owned(),
        browser,
        priority: u32::try_from(priority).unwrap_or(u32::MAX),
        active_leases: 0,
    }
}

fn system_candidate_paths() -> Vec<Utf8PathBuf> {
    let mut seen = BTreeSet::new();
    let mut paths = Vec::new();
    for path in platform_candidates() {
        if seen.insert(path.clone()) {
            paths.push(path);
        }
    }
    for executable in path_executable_names() {
        if let Some(path) = find_in_path(executable)
            && seen.insert(path.clone())
        {
            paths.push(path);
        }
    }
    paths
}

pub(crate) async fn probe(path: &Utf8PathBuf, source: BrowserSource) -> Result<BrowserInfo> {
    if !path.is_file() {
        return Err(browser_error(
            "offprint.browser.executable",
            format!("browser executable `{path}` is unavailable"),
        ));
    }
    let text = String::from_utf8(probe_output(path).await?).map_err(|error| {
        browser_error(
            "offprint.browser.version",
            format!("browser version output is not UTF-8: {error}"),
        )
    })?;
    let (product, version) = parse_version_output(text.trim()).ok_or_else(|| {
        browser_error(
            "offprint.browser.version",
            format!("browser version output is unrecognized for `{path}`"),
        )
    })?;
    let major = version
        .split('.')
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| {
            browser_error(
                "offprint.browser.version",
                format!("browser version `{version}` has no numeric major version"),
            )
        })?;
    if major < MINIMUM_CHROMIUM_MAJOR {
        return Err(browser_error(
            "offprint.browser.incompatible",
            format!(
                "browser major {major} is older than the minimum supported major {MINIMUM_CHROMIUM_MAJOR}"
            ),
        ));
    }
    Ok(BrowserInfo {
        product,
        version,
        source,
        executable_path: Some(path.clone().into()),
        endpoint: None,
        revision: None,
        protocol_version: "1.3".to_owned(),
    })
}

async fn probe_output(path: &Utf8PathBuf) -> Result<Vec<u8>> {
    let probe_error = |error| {
        browser_error(
            "offprint.browser.probe",
            format!("browser version probe failed for `{path}`: {error}"),
        )
    };
    let mut child = probe_command(path)
        .map_err(probe_error)?
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(probe_error)?;
    let result = timeout(PROBE_TIMEOUT, async {
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("version probe output is unavailable"))?;
        let mut bytes = Vec::new();
        stdout
            .take(MAX_PROBE_BYTES + 1)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() as u64 > MAX_PROBE_BYTES {
            return Err(io::Error::other(
                "version probe output exceeds its byte limit",
            ));
        }
        if !child.wait().await?.success() {
            return Err(io::Error::other("version probe exited unsuccessfully"));
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| {
        browser_error(
            "offprint.browser.probe_timeout",
            format!("browser version probe timed out for `{path}`"),
        )
    })
    .and_then(|result| result.map_err(probe_error));
    if result.is_err() {
        let _terminated = child.kill().await;
    }
    result
}

#[cfg(not(windows))]
fn probe_command(path: &Utf8PathBuf) -> io::Result<Command> {
    let mut command = Command::new(path);
    command.arg("--version");
    Ok(command)
}

#[cfg(windows)]
fn probe_command(path: &Utf8PathBuf) -> io::Result<Command> {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Windows Chrome is a GUI executable. Read its version resource with a
    // fixed script and pass the untrusted path as data, not PowerShell source.
    const VERSION_PROBE: &str = r#"
$ErrorActionPreference = 'Stop'
try {
    [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
    $path = [IO.Path]::GetFullPath($env:OFFPRINT_BROWSER_PROBE_PATH)
    $info = [Diagnostics.FileVersionInfo]::GetVersionInfo($path)
    [Console]::WriteLine($info.ProductName + ' ' + $info.ProductVersion)
} catch {
    exit 1
}
"#;
    let system_root = std::env::var_os("SystemRoot")
        .ok_or_else(|| io::Error::other("SystemRoot is unavailable"))?;
    let powershell = PathBuf::from(system_root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let mut command = Command::new(powershell);
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            VERSION_PROBE,
        ])
        .env("OFFPRINT_BROWSER_PROBE_PATH", path)
        .creation_flags(CREATE_NO_WINDOW);
    Ok(command)
}

fn parse_version_output(output: &str) -> Option<(BrowserProduct, String)> {
    let product = if output.starts_with("Google Chrome") {
        BrowserProduct::Chrome
    } else if output.starts_with("Chromium") {
        BrowserProduct::Chromium
    } else if output.starts_with("Microsoft Edge") {
        BrowserProduct::Edge
    } else {
        return None;
    };
    let version = output
        .split_ascii_whitespace()
        .find(|part| {
            part.bytes()
                .next()
                .is_some_and(|byte| byte.is_ascii_digit())
        })?
        .to_owned();
    Some((product, version))
}

fn find_in_path(executable: &str) -> Option<Utf8PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|directory| directory.join(executable))
        .find(|path| path.is_file())
        .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
}

#[cfg(target_os = "macos")]
fn platform_candidates() -> Vec<Utf8PathBuf> {
    [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Google Chrome Beta.app/Contents/MacOS/Google Chrome Beta",
        "/Applications/Google Chrome Dev.app/Contents/MacOS/Google Chrome Dev",
        "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/Applications/Microsoft Edge Beta.app/Contents/MacOS/Microsoft Edge Beta",
        "/Applications/Microsoft Edge Dev.app/Contents/MacOS/Microsoft Edge Dev",
        "/Applications/Microsoft Edge Canary.app/Contents/MacOS/Microsoft Edge Canary",
    ]
    .into_iter()
    .map(Utf8PathBuf::from)
    .collect()
}

#[cfg(target_os = "linux")]
fn platform_candidates() -> Vec<Utf8PathBuf> {
    [
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/google-chrome-beta",
        "/usr/bin/google-chrome-unstable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/microsoft-edge",
        "/usr/bin/microsoft-edge-stable",
        "/usr/bin/microsoft-edge-beta",
        "/usr/bin/microsoft-edge-dev",
        "/snap/bin/chromium",
    ]
    .into_iter()
    .map(Utf8PathBuf::from)
    .collect()
}

#[cfg(target_os = "windows")]
fn platform_candidates() -> Vec<Utf8PathBuf> {
    let mut paths = Vec::new();
    for variable in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
        let Some(root) = std::env::var_os(variable) else {
            continue;
        };
        for relative in [
            "Google\\Chrome\\Application\\chrome.exe",
            "Google\\Chrome Beta\\Application\\chrome.exe",
            "Google\\Chrome Dev\\Application\\chrome.exe",
            "Google\\Chrome SxS\\Application\\chrome.exe",
            "Chromium\\Application\\chrome.exe",
            "Microsoft\\Edge\\Application\\msedge.exe",
            "Microsoft\\Edge Beta\\Application\\msedge.exe",
            "Microsoft\\Edge Dev\\Application\\msedge.exe",
            "Microsoft\\Edge SxS\\Application\\msedge.exe",
        ] {
            let path = PathBuf::from(&root).join(relative);
            if let Ok(path) = Utf8PathBuf::from_path_buf(path) {
                paths.push(path);
            }
        }
    }
    paths
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn platform_candidates() -> Vec<Utf8PathBuf> {
    Vec::new()
}

#[cfg(target_os = "windows")]
fn path_executable_names() -> &'static [&'static str] {
    &["chrome.exe", "chromium.exe", "msedge.exe"]
}

#[cfg(not(target_os = "windows"))]
fn path_executable_names() -> &'static [&'static str] {
    &[
        "google-chrome",
        "google-chrome-stable",
        "google-chrome-beta",
        "google-chrome-unstable",
        "chromium",
        "chromium-browser",
        "microsoft-edge",
        "microsoft-edge-stable",
        "microsoft-edge-beta",
        "microsoft-edge-dev",
    ]
}

fn browser_error(code: &'static str, message: impl Into<String>) -> OffprintError {
    OffprintError::new(code, ErrorStage::Browser, message)
}

#[cfg(test)]
mod tests {
    use offprint_model::BrowserProduct;

    use super::parse_version_output;

    #[test]
    fn browser_version_output_maps_to_a_typed_product() {
        for (output, product) in [
            ("Google Chrome 150.0.7871.187", BrowserProduct::Chrome),
            (
                "Google Chrome for Testing 150.0.7871.187",
                BrowserProduct::Chrome,
            ),
            ("Chromium 150.0.7871.187", BrowserProduct::Chromium),
            ("Microsoft Edge 150.0.7871.187", BrowserProduct::Edge),
        ] {
            assert_eq!(
                parse_version_output(output),
                Some((product, "150.0.7871.187".to_owned()))
            );
        }
    }

    #[test]
    fn unrelated_executable_output_is_rejected() {
        assert_eq!(parse_version_output("Offprint 1.0.0"), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn discovery_rejects_excessive_version_output() -> Result<(), Box<dyn std::error::Error>>
    {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir()?;
        let path = directory.path().join("chromium");
        std::fs::write(&path, "#!/bin/sh\nprintf '%5000s' x\n")?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        let path = camino::Utf8PathBuf::from_path_buf(path)
            .map_err(|_| std::io::Error::other("fixture path is not UTF-8"))?;
        let result = super::ChromiumDiscovery::new()
            .with_explicit_path(path)
            .with_source_policy(offprint_model::BrowserSourcePolicy::Managed)
            .discover()
            .await;

        assert!(result.selected.is_none());
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].code.as_str(), "offprint.browser.probe");
        Ok(())
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn discovery_rejects_missing_version_metadata_at_a_literal_path()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("Chrome's [test] $browser.exe");
        std::fs::write(&path, b"not an executable")?;
        let path = camino::Utf8PathBuf::from_path_buf(path)
            .map_err(|_| std::io::Error::other("fixture path is not UTF-8"))?;
        let result = super::ChromiumDiscovery::new()
            .with_explicit_path(path)
            .with_source_policy(offprint_model::BrowserSourcePolicy::Managed)
            .discover()
            .await;

        assert!(result.selected.is_none());
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].code.as_str(), "offprint.browser.version");
        Ok(())
    }
}
