use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use pageknot_model::{BrowserEnvironment, ErrorStage, PageKnotError, PortablePath, Result};
use processkit::{
    Command, OutputBufferPolicy, ProcessGroup, ProcessGroupOptions, Stdin, StdioMode,
};
use tempfile::TempDir;
use tokio::time::{sleep, timeout};
use url::Url;

use crate::cdp::generated::cdp_browser::{
    CloseCommand, CloseParams, GetVersionCommand, GetVersionParams, SetDownloadBehaviorCommand,
    SetDownloadBehaviorParams,
};
use crate::cdp::generated::cdp_target::{
    CreateBrowserContextCommand, CreateBrowserContextParams, CreateTargetCommand,
    CreateTargetParams, DisposeBrowserContextCommand, DisposeBrowserContextParams,
};
use crate::{CdpClient, ChromiumPage};

const LAUNCH_TIMEOUT: Duration = Duration::from_secs(20);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const PORT_POLL_INTERVAL: Duration = Duration::from_millis(25);
const MAXIMUM_STDERR_BYTES: usize = 16 * 1024;
const MAXIMUM_STDERR_LINE_BYTES: usize = 4 * 1024;

type ProcessTask =
    tokio::task::JoinHandle<std::result::Result<processkit::Outcome, processkit::Error>>;

#[derive(Clone, Debug, Default)]
struct BrowserDiagnostics {
    state: Arc<StdMutex<BrowserDiagnosticsState>>,
}

#[derive(Debug, Default)]
struct BrowserDiagnosticsState {
    lines: VecDeque<String>,
    bytes: usize,
}

impl BrowserDiagnostics {
    fn push(&self, line: String) {
        let Some(mut state) = self.state.lock().ok() else {
            return;
        };
        let line = truncate_utf8(&line, MAXIMUM_STDERR_LINE_BYTES);
        let line_bytes = line.len().saturating_add(1);
        state.bytes = state.bytes.saturating_add(line_bytes);
        state.lines.push_back(line);
        while state.bytes > MAXIMUM_STDERR_BYTES {
            let Some(removed) = state.lines.pop_front() else {
                state.bytes = 0;
                break;
            };
            state.bytes = state.bytes.saturating_sub(removed.len().saturating_add(1));
        }
    }

    fn snapshot(&self) -> Option<String> {
        let state = self.state.lock().ok()?;
        if state.lines.is_empty() {
            None
        } else {
            Some(state.lines.iter().cloned().collect::<Vec<_>>().join("\n"))
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChromiumLaunchOptions {
    pub executable: PortablePath,
    pub headless: bool,
    pub extra_arguments: Vec<String>,
}

impl ChromiumLaunchOptions {
    #[must_use]
    pub fn new(executable: impl Into<PortablePath>) -> Self {
        Self {
            executable: executable.into(),
            headless: true,
            extra_arguments: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct ChromiumProcess {
    process: ProcessTask,
    process_group: ProcessGroup,
    profile: TempDir,
    client: CdpClient,
    endpoint: Url,
    keeper_context_id: String,
    diagnostics: BrowserDiagnostics,
}

impl ChromiumProcess {
    pub async fn launch(options: ChromiumLaunchOptions) -> Result<Self> {
        if !options.executable.is_file() {
            return Err(browser_error(
                "pageknot.browser.executable",
                format!("browser executable `{}` is unavailable", options.executable),
            ));
        }
        let profile = tempfile::Builder::new()
            .prefix("pageknot-browser-")
            .tempdir()
            .map_err(|error| {
                browser_error(
                    "pageknot.browser.profile",
                    format!("failed to create an ephemeral browser profile: {error}"),
                )
            })?;
        let diagnostics = BrowserDiagnostics::default();
        let stderr_diagnostics = diagnostics.clone();
        let profile_display = profile.path().to_string_lossy().into_owned();

        let mut command = Command::new(options.executable.as_utf8_path().as_std_path())
            .arg("--remote-debugging-address=127.0.0.1")
            .arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", profile.path().display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-background-networking")
            .arg("--disable-component-update")
            .arg("--disable-default-apps")
            .arg("--disable-extensions")
            .arg("--disable-quic")
            .arg("--disable-sync")
            .arg("--disable-features=MediaRouter,Translate")
            .arg("--disable-prompt-on-repost")
            .arg("--disable-renderer-backgrounding")
            .arg("--disable-background-timer-throttling")
            .arg("--disable-backgrounding-occluded-windows")
            .stdin(Stdin::empty())
            .stdout(StdioMode::Null)
            .stderr(StdioMode::Piped)
            .output_buffer(
                OutputBufferPolicy::unbounded().with_max_bytes(MAXIMUM_STDERR_LINE_BYTES),
            )
            .on_stderr_line(move |line| {
                stderr_diagnostics.push(redact_stderr_line(line, &profile_display));
            })
            .kill_on_parent_death()
            .create_no_window()
            .no_timeout();
        if options.headless {
            command = command.arg("--headless=new");
        }
        command = command
            .args(
                options
                    .extra_arguments
                    .into_iter()
                    .filter(|argument| !argument.starts_with("--force-webrtc-ip-handling-policy=")),
            )
            .arg("--force-webrtc-ip-handling-policy=disable_non_proxied_udp")
            .arg("about:blank");

        let process_group = ProcessGroup::with_options(
            ProcessGroupOptions::default().shutdown_timeout(SHUTDOWN_TIMEOUT),
        )
        .map_err(|error| process_error("pageknot.browser.containment", "create", error))?;
        let process = process_group.start(&command).await.map_err(|error| {
            process_error("pageknot.browser.launch", "launch Chromium", error).retryable(true)
        })?;
        let process = tokio::spawn(process.drain());
        let endpoint = match wait_for_endpoint(&process_group, profile.path()).await {
            Ok(endpoint) => endpoint,
            Err(error) => {
                let _ignored = process_group.kill_all();
                let _ignored = timeout(SHUTDOWN_TIMEOUT, process).await;
                return Err(with_browser_diagnostics(error, &diagnostics));
            }
        };
        let client = match CdpClient::connect(endpoint.clone()).await {
            Ok(client) => client,
            Err(error) => {
                let _ignored = process_group.kill_all();
                let _ignored = timeout(SHUTDOWN_TIMEOUT, process).await;
                return Err(with_browser_diagnostics(error, &diagnostics));
            }
        };
        client.mark_owned_browser();
        client
            .execute::<SetDownloadBehaviorCommand>(
                SetDownloadBehaviorParams::new("deny".to_owned()),
                None,
            )
            .await?;
        let mut keeper_context_params = CreateBrowserContextParams::new();
        keeper_context_params.dispose_on_detach = Some(false);
        let keeper_context = client
            .execute::<CreateBrowserContextCommand>(keeper_context_params, None)
            .await?;
        let keeper_context_id = keeper_context.browser_context_id;
        let mut keeper_params = CreateTargetParams::new("about:blank".to_owned());
        keeper_params.browser_context_id = Some(keeper_context_id.clone());
        keeper_params.background = Some(true);
        let keeper = client
            .execute::<CreateTargetCommand>(keeper_params, None)
            .await?;
        let _keeper_target_id = keeper.target_id;

        Ok(Self {
            process,
            process_group,
            profile,
            client,
            endpoint,
            keeper_context_id,
            diagnostics,
        })
    }

    #[must_use]
    pub const fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    #[must_use]
    pub fn profile_path(&self) -> &std::path::Path {
        self.profile.path()
    }

    #[must_use]
    pub fn client(&self) -> &CdpClient {
        &self.client
    }

    pub async fn new_page(&self, environment: &BrowserEnvironment) -> Result<ChromiumPage> {
        ChromiumPage::create(self.client.clone(), environment).await
    }

    pub async fn is_healthy(&self) -> bool {
        self.client
            .execute_with_timeout::<GetVersionCommand>(
                GetVersionParams::new(),
                None,
                Duration::from_secs(2),
            )
            .await
            .is_ok()
    }

    pub fn terminate_process_tree(&self) -> Result<()> {
        self.process_group.kill_all().map_err(|error| {
            process_error(
                "pageknot.browser.terminate",
                "terminate Chromium process tree",
                error,
            )
            .retryable(true)
        })
    }

    pub async fn close(self) -> Result<()> {
        let _ignored = self
            .client
            .execute_with_timeout::<DisposeBrowserContextCommand>(
                DisposeBrowserContextParams::new(self.keeper_context_id.clone()),
                None,
                Duration::from_secs(2),
            )
            .await;
        let _ignored = self
            .client
            .execute_with_timeout::<CloseCommand>(CloseParams::new(), None, Duration::from_secs(2))
            .await;
        let _ignored = self.client.close().await;

        let mut wait = Box::pin(self.process);
        match timeout(SHUTDOWN_TIMEOUT, wait.as_mut()).await {
            Ok(result) => {
                map_process_task(result, &self.diagnostics)?;
            }
            Err(_) => {
                self.process_group.kill_all().map_err(|error| {
                    process_error(
                        "pageknot.browser.shutdown",
                        "terminate Chromium process tree",
                        error,
                    )
                    .retryable(true)
                })?;
                let result = timeout(SHUTDOWN_TIMEOUT, wait.as_mut())
                    .await
                    .map_err(|_| {
                        with_browser_diagnostics(
                            browser_error(
                                "pageknot.browser.shutdown",
                                "Chromium did not exit after its process tree was terminated",
                            )
                            .retryable(true),
                            &self.diagnostics,
                        )
                    })?;
                map_process_task(result, &self.diagnostics)?;
            }
        }
        self.process_group.shutdown().await.map_err(|error| {
            process_error(
                "pageknot.browser.shutdown",
                "finish Chromium process tree shutdown",
                error,
            )
            .retryable(true)
        })
    }
}

fn map_process_task(
    result: std::result::Result<
        std::result::Result<processkit::Outcome, processkit::Error>,
        tokio::task::JoinError,
    >,
    diagnostics: &BrowserDiagnostics,
) -> Result<()> {
    let outcome = result.map_err(|error| {
        with_browser_diagnostics(
            browser_error(
                "pageknot.browser.shutdown",
                format!("Chromium process monitor failed: {error}"),
            )
            .retryable(true),
            diagnostics,
        )
    })?;
    outcome.map_err(|error| {
        with_browser_diagnostics(
            process_error("pageknot.browser.shutdown", "reap Chromium process", error)
                .retryable(true),
            diagnostics,
        )
    })?;
    Ok(())
}

fn with_browser_diagnostics(
    error: PageKnotError,
    diagnostics: &BrowserDiagnostics,
) -> PageKnotError {
    match diagnostics.snapshot() {
        Some(stderr) => error.with_detail("browserStderr", stderr),
        None => error,
    }
}

fn redact_stderr_line(line: &str, profile_path: &str) -> String {
    let line = line
        .chars()
        .filter(|character| !character.is_control() || *character == '\t')
        .collect::<String>()
        .replace(profile_path, "<browser-profile>");
    let mut redact_next = false;
    line.split_whitespace()
        .map(|token| {
            if redact_next {
                redact_next = false;
                return "<redacted>".to_owned();
            }
            let lower = token.to_ascii_lowercase();
            if lower.contains("://") {
                return "<url>".to_owned();
            }
            if matches!(
                lower.trim_matches(|character: char| !character.is_ascii_alphanumeric()),
                "authorization" | "cookie" | "password" | "secret" | "token"
            ) {
                redact_next = true;
                return token.to_owned();
            }
            for separator in ['=', ':'] {
                if let Some((name, _)) = token.split_once(separator)
                    && sensitive_diagnostic_name(name)
                {
                    return format!("{name}{separator}<redacted>");
                }
            }
            token.to_owned()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn sensitive_diagnostic_name(name: &str) -> bool {
    let name = name
        .trim_matches(|character: char| !character.is_ascii_alphanumeric())
        .to_ascii_lowercase();
    ["authorization", "cookie", "password", "secret", "token"]
        .iter()
        .any(|sensitive| name.contains(sensitive))
}

fn truncate_utf8(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_owned();
    }
    let mut boundary = maximum_bytes;
    while !value.is_char_boundary(boundary) {
        boundary = boundary.saturating_sub(1);
    }
    value[..boundary].to_owned()
}

async fn wait_for_endpoint(group: &ProcessGroup, profile: &std::path::Path) -> Result<Url> {
    let active_port = profile.join("DevToolsActivePort");
    let started = Instant::now();
    loop {
        if group.members().map_or(true, |members| members.is_empty()) {
            return Err(browser_error(
                "pageknot.browser.launch",
                "Chromium exited during startup",
            ));
        }
        if let Ok(contents) = tokio::fs::read_to_string(&active_port).await
            && let Some(endpoint) = parse_active_port(&contents)?
        {
            return Ok(endpoint);
        }
        if started.elapsed() >= LAUNCH_TIMEOUT {
            return Err(browser_error(
                "pageknot.browser.launch_timeout",
                "Chromium did not publish its DevTools endpoint",
            )
            .retryable(true));
        }
        sleep(PORT_POLL_INTERVAL).await;
    }
}

fn parse_active_port(contents: &str) -> Result<Option<Url>> {
    let mut lines = contents.lines();
    let Some(port) = lines.next() else {
        return Ok(None);
    };
    let Some(path) = lines.next() else {
        return Ok(None);
    };
    let port = port.parse::<u16>().map_err(|error| {
        browser_error(
            "pageknot.browser.devtools_port",
            format!("Chromium published an invalid DevTools port: {error}"),
        )
    })?;
    let path = path.trim_start_matches('/');
    Url::parse(&format!("ws://127.0.0.1:{port}/{path}"))
        .map(Some)
        .map_err(|error| {
            browser_error(
                "pageknot.browser.devtools_url",
                format!("Chromium published an invalid DevTools endpoint: {error}"),
            )
        })
}

fn browser_error(code: &'static str, message: impl Into<String>) -> PageKnotError {
    PageKnotError::new(code, ErrorStage::Browser, message)
}

fn process_error(
    code: &'static str,
    action: &'static str,
    error: processkit::Error,
) -> PageKnotError {
    browser_error(code, format!("failed to {action}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{BrowserDiagnostics, MAXIMUM_STDERR_BYTES, parse_active_port, redact_stderr_line};

    #[test]
    fn devtools_active_port_builds_a_loopback_endpoint() {
        let endpoint = parse_active_port("9222\n/devtools/browser/abc\n")
            .ok()
            .flatten();

        assert_eq!(
            endpoint.as_ref().map(url::Url::as_str),
            Some("ws://127.0.0.1:9222/devtools/browser/abc")
        );
    }

    #[test]
    fn incomplete_devtools_active_port_waits_for_more_data() {
        assert_eq!(parse_active_port("9222\n").ok(), Some(None));
    }

    #[test]
    fn chromium_stderr_diagnostics_are_redacted() {
        let profile = "/tmp/pageknot-browser-secret";
        let line = format!(
            "profile={profile} endpoint=https://example.test/path?token=value Authorization: bearer-secret"
        );

        let redacted = redact_stderr_line(&line, profile);

        assert!(!redacted.contains(profile));
        assert!(!redacted.contains("example.test"));
        assert!(!redacted.contains("bearer-secret"));
        assert!(redacted.contains("<browser-profile>"));
        assert!(redacted.contains("<url>"));
        assert!(redacted.contains("<redacted>"));
    }

    #[test]
    fn chromium_stderr_diagnostics_keep_a_bounded_tail() {
        let diagnostics = BrowserDiagnostics::default();
        for index in 0..100 {
            diagnostics.push(format!("{index:03}:{}", "x".repeat(1024)));
        }

        let snapshot = diagnostics.snapshot();

        assert!(snapshot.as_ref().is_some_and(|value| {
            value.len() <= MAXIMUM_STDERR_BYTES && value.contains("099:")
        }));
        assert!(snapshot.is_some_and(|value| !value.contains("000:")));
    }
}
