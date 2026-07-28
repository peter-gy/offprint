use std::collections::BTreeSet;
use std::process::Stdio;
use std::time::Duration;

use crate::ManagedBrowserManager;
use camino::Utf8PathBuf;
use pageknot_model::{
    BrowserCandidate, BrowserCandidateState, BrowserChannel, BrowserInfo, BrowserProduct,
    BrowserSource, ErrorStage, PageKnotError, Result,
};
#[cfg(target_os = "windows")]
use std::path::PathBuf;
use tokio::process::Command;
use tokio::time::timeout;

const MINIMUM_CHROMIUM_MAJOR: u32 = 120;
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct ChromiumDiscovery {
    explicit_path: Option<Utf8PathBuf>,
    managed_cache: Option<Utf8PathBuf>,
    channel: BrowserChannel,
}

impl ChromiumDiscovery {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            explicit_path: None,
            managed_cache: None,
            channel: BrowserChannel::Auto,
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
    pub const fn with_channel(mut self, channel: BrowserChannel) -> Self {
        self.channel = channel;
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
        if self.channel != BrowserChannel::System
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
        if self.channel != BrowserChannel::Managed {
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
    pub failures: Vec<PageKnotError>,
}

async fn push_probed_candidate(
    path: &Utf8PathBuf,
    source: BrowserSource,
    seen: &mut BTreeSet<Utf8PathBuf>,
    candidates: &mut Vec<BrowserCandidate>,
    failures: &mut Vec<PageKnotError>,
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
        reason_code: "pageknot.browser.compatible".to_owned(),
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
            "pageknot.browser.executable",
            format!("browser executable `{path}` is unavailable"),
        ));
    }
    let mut command = Command::new(path);
    command
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = timeout(PROBE_TIMEOUT, command.output())
        .await
        .map_err(|_| {
            browser_error(
                "pageknot.browser.probe_timeout",
                format!("browser version probe timed out for `{path}`"),
            )
        })?
        .map_err(|error| {
            browser_error(
                "pageknot.browser.probe",
                format!("failed to execute browser `{path}`: {error}"),
            )
        })?;
    if !output.status.success() {
        return Err(browser_error(
            "pageknot.browser.probe",
            format!("browser version probe failed for `{path}`"),
        ));
    }
    let text = String::from_utf8(output.stdout).map_err(|error| {
        browser_error(
            "pageknot.browser.version",
            format!("browser version output is not UTF-8: {error}"),
        )
    })?;
    let (product, version) = parse_version_output(text.trim()).ok_or_else(|| {
        browser_error(
            "pageknot.browser.version",
            format!("browser version output is unrecognized for `{path}`"),
        )
    })?;
    let major = version
        .split('.')
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or_else(|| {
            browser_error(
                "pageknot.browser.version",
                format!("browser version `{version}` has no numeric major version"),
            )
        })?;
    if major < MINIMUM_CHROMIUM_MAJOR {
        return Err(browser_error(
            "pageknot.browser.incompatible",
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

fn browser_error(code: &'static str, message: impl Into<String>) -> PageKnotError {
    PageKnotError::new(code, ErrorStage::Browser, message)
}

#[cfg(test)]
mod tests {
    use pageknot_model::BrowserProduct;

    use super::parse_version_output;

    #[test]
    fn chrome_version_output_maps_to_a_typed_product() {
        assert_eq!(
            parse_version_output("Google Chrome 150.0.7871.187"),
            Some((BrowserProduct::Chrome, "150.0.7871.187".to_owned()))
        );
    }

    #[test]
    fn unrelated_executable_output_is_rejected() {
        assert_eq!(parse_version_output("PageKnot 1.0.0"), None);
    }
}
