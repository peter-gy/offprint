use std::collections::BTreeMap;
use std::error::Error;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use offprint::{BrowserInfo, CaptureArtifact, Offprint};
use offprint_test_support::{FixtureResponse, FixtureServer};
use serde::Serialize;
use sysinfo::{Pid, ProcessesToUpdate, System, get_current_pid};

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

const CAPTURES: u32 = 32;
const MAXIMUM_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024;
const MAXIMUM_RUST_GROWTH_BYTES: u64 = 256 * 1024 * 1024;
const MAXIMUM_BROWSER_RSS_BYTES: u64 = 3 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct MemorySample {
    rust_rss_bytes: u64,
    browser_rss_bytes: u64,
    browser_processes: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureSample {
    sequence: u32,
    elapsed_milliseconds: u128,
    artifact_bytes: u64,
    memory: MemorySample,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepeatedCaptureReport {
    schema_version: u32,
    captures: u32,
    browser: BrowserInfo,
    profile_marker: String,
    observed_processes: Vec<OwnedProcessIdentity>,
    baseline: MemorySample,
    peak: MemorySample,
    final_sample: MemorySample,
    samples: Vec<CaptureSample>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct OwnedProcessIdentity {
    pid: u32,
    start_time_seconds: u64,
}

#[derive(Debug, Default)]
struct BrowserProcessTracker {
    profile_path: Option<PathBuf>,
    processes: BTreeMap<u32, u64>,
}

#[tokio::test]
#[ignore = "requires a locally installed compatible Chromium browser"]
async fn repeated_capture_memory_and_process_use_remain_bounded() -> TestResult {
    let server = FixtureServer::start().await?;
    server
        .register(
            "/image.svg",
            FixtureResponse {
                status: 200,
                content_type: "image/svg+xml".to_owned(),
                headers: BTreeMap::new(),
                body: br#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64">
                  <rect width="64" height="64" fill="rgb(30, 100, 180)"/>
                </svg>"#
                    .to_vec(),
            },
        )
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r##"<!doctype html><html><head><title>Repeated capture</title>
                <style>body{font:16px system-ui}img{width:64px;height:64px}</style>
                </head><body><h1>bounded lifecycle</h1><img src="/image.svg">
                <output id="state"></output><script>
                document.querySelector("#state").textContent = "rendered";
                </script></body></html>"##,
            ),
        )
        .await?;

    let root_pid = get_current_pid()?;
    let mut system = System::new();
    let mut process_tracker = BrowserProcessTracker::default();
    let baseline = memory_sample(&mut system, root_pid, &mut process_tracker);
    let offprint = Offprint::builder()
        .maximum_contexts(1)
        .browser_recycle_after_jobs(CAPTURES + 1)
        .build()?;
    let source = server.url("/")?;
    let mut samples = Vec::with_capacity(CAPTURES as usize);
    let mut peak = baseline;
    let mut browser = None;

    for sequence in 1..=CAPTURES {
        let started = Instant::now();
        let result = offprint
            .capture(source.as_str())?
            .bytes(MAXIMUM_ARTIFACT_BYTES)
            .await?;
        assert_eq!(result.verification.network_requests, 0);
        let CaptureArtifact::Bytes { bytes, content, .. } = result.artifact else {
            return Err("repeated capture returned a file artifact".into());
        };
        assert_eq!(bytes, u64::try_from(content.len())?);
        assert!(String::from_utf8_lossy(&content).contains("rendered"));
        if browser.is_none() {
            browser = Some(offprint_html::inspect_html(&content)?.browser);
        }

        let memory = memory_sample(&mut system, root_pid, &mut process_tracker);
        peak.rust_rss_bytes = peak.rust_rss_bytes.max(memory.rust_rss_bytes);
        peak.browser_rss_bytes = peak.browser_rss_bytes.max(memory.browser_rss_bytes);
        peak.browser_processes = peak.browser_processes.max(memory.browser_processes);
        samples.push(CaptureSample {
            sequence,
            elapsed_milliseconds: started.elapsed().as_millis(),
            artifact_bytes: bytes,
            memory,
        });
    }

    offprint.close().await?;
    let final_sample = wait_for_browser_exit(&mut system, root_pid, &mut process_tracker).await?;
    server.close().await;

    let warm_rust_rss = samples.first().map_or(baseline.rust_rss_bytes, |sample| {
        sample.memory.rust_rss_bytes
    });
    assert!(
        peak.rust_rss_bytes.saturating_sub(warm_rust_rss) <= MAXIMUM_RUST_GROWTH_BYTES,
        "Rust RSS grew from {warm_rust_rss} to {} bytes",
        peak.rust_rss_bytes
    );
    assert!(
        peak.browser_rss_bytes <= MAXIMUM_BROWSER_RSS_BYTES,
        "browser RSS reached {} bytes",
        peak.browser_rss_bytes
    );
    assert_eq!(
        final_sample.browser_processes, 0,
        "browser processes survived Offprint close"
    );
    let profile_path = process_tracker
        .profile_path
        .as_ref()
        .ok_or("repeated capture did not observe its Chromium profile marker")?;
    assert!(
        !profile_path.exists(),
        "owned Chromium profile survived Offprint close: {}",
        profile_path.display()
    );
    let profile_marker = profile_path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or("owned Chromium profile marker is not valid UTF-8")?
        .to_owned();
    let observed_processes = process_tracker
        .processes
        .iter()
        .map(|(pid, start_time_seconds)| OwnedProcessIdentity {
            pid: *pid,
            start_time_seconds: *start_time_seconds,
        })
        .collect();

    write_report(&RepeatedCaptureReport {
        schema_version: 2,
        captures: CAPTURES,
        browser: browser.ok_or("capture report omitted browser metadata")?,
        profile_marker,
        observed_processes,
        baseline,
        peak,
        final_sample,
        samples,
    })?;
    Ok(())
}

fn memory_sample(
    system: &mut System,
    root_pid: Pid,
    tracker: &mut BrowserProcessTracker,
) -> MemorySample {
    system.refresh_processes(ProcessesToUpdate::All, true);
    let rust_rss_bytes = system.process(root_pid).map_or(0, sysinfo::Process::memory);
    if tracker.profile_path.is_none() {
        tracker.profile_path = system.processes().iter().find_map(|(pid, process)| {
            (*pid != root_pid
                && is_descendant(system, *pid, root_pid)
                && is_browser_process(process))
            .then(|| browser_profile_path(*pid, process))
            .flatten()
        });
    }
    let mut browser_rss_bytes = 0_u64;
    let mut browser_processes = 0_u32;
    for (pid, process) in system.processes() {
        if *pid == root_pid || !is_browser_process(process) {
            continue;
        }
        let pid_value = pid.as_u32();
        let start_time = process.start_time();
        let previously_owned = tracker.processes.get(&pid_value) == Some(&start_time);
        let descendant = is_descendant(system, *pid, root_pid);
        let profile_owned = !previously_owned
            && !descendant
            && tracker
                .profile_path
                .as_ref()
                .is_some_and(|profile| process_uses_profile(*pid, process, profile));
        if !previously_owned && !profile_owned && !descendant {
            continue;
        }
        tracker.processes.insert(pid_value, start_time);
        browser_rss_bytes = browser_rss_bytes.saturating_add(process.memory());
        browser_processes = browser_processes.saturating_add(1);
    }
    MemorySample {
        rust_rss_bytes,
        browser_rss_bytes,
        browser_processes,
    }
}

fn is_browser_process(process: &sysinfo::Process) -> bool {
    let name = process.name().to_string_lossy().to_ascii_lowercase();
    name.contains("chrome") || name.contains("chromium")
}

fn browser_profile_path(pid: Pid, process: &sysinfo::Process) -> Option<PathBuf> {
    process
        .cmd()
        .iter()
        .find_map(|argument| profile_path_from_argument(argument))
        .or_else(|| {
            browser_command_line(pid).and_then(|command| {
                command
                    .split_whitespace()
                    .find_map(|argument| profile_path_from_argument(argument.as_ref()))
            })
        })
}

fn process_uses_profile(pid: Pid, process: &sysinfo::Process, profile: &std::path::Path) -> bool {
    process
        .cmd()
        .iter()
        .filter_map(|argument| profile_path_from_argument(argument))
        .any(|candidate| candidate == profile)
        || browser_command_line(pid).is_some_and(|command| {
            command
                .split_whitespace()
                .filter_map(|argument| profile_path_from_argument(argument.as_ref()))
                .any(|candidate| candidate == profile)
        })
}

fn profile_path_from_argument(argument: &std::ffi::OsStr) -> Option<PathBuf> {
    let argument = argument.to_string_lossy();
    let path = PathBuf::from(argument.strip_prefix("--user-data-dir=")?);
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|name| name.starts_with("offprint-browser-"))
        .then_some(path)
}

#[cfg(unix)]
fn browser_command_line(pid: Pid) -> Option<String> {
    let pid = pid.as_u32().to_string();
    let output = std::process::Command::new("ps")
        .args(["-ww", "-p", pid.as_str(), "-o", "command="])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(not(unix))]
fn browser_command_line(_pid: Pid) -> Option<String> {
    None
}

fn is_descendant(system: &System, mut pid: Pid, root_pid: Pid) -> bool {
    for _ in 0..64 {
        let Some(parent) = system.process(pid).and_then(sysinfo::Process::parent) else {
            return false;
        };
        if parent == root_pid {
            return true;
        }
        if parent == pid {
            return false;
        }
        pid = parent;
    }
    false
}

async fn wait_for_browser_exit(
    system: &mut System,
    root_pid: Pid,
    tracker: &mut BrowserProcessTracker,
) -> TestResult<MemorySample> {
    for _ in 0..100 {
        let sample = memory_sample(system, root_pid, tracker);
        if sample.browser_processes == 0 {
            return Ok(sample);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    Ok(memory_sample(system, root_pid, tracker))
}

fn write_report(report: &RepeatedCaptureReport) -> TestResult {
    let manifest_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_directory
        .ancestors()
        .nth(2)
        .map(std::path::Path::to_owned)
        .ok_or("workspace root could not be resolved")?;
    let directory = root.join("target").join("benchmark-evidence");
    std::fs::create_dir_all(&directory)?;
    let mut bytes = serde_json::to_vec_pretty(report)?;
    bytes.push(b'\n');
    std::fs::write(directory.join("repeated-capture.json"), bytes)?;
    Ok(())
}
