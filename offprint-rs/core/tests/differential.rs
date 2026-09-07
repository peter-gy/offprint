use std::error::Error;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use image::ImageReader;
use offprint::{BrowserInfo, CaptureArtifact, Offprint, PortablePath, ResourceSummary};
use offprint_chromium::{ChromiumLaunchOptions, ChromiumProcess};
use offprint_model::BrowserEnvironment;
use offprint_test_support::{FixtureResponse, FixtureServer};
use serde::Serialize;
use serde_json::Value;
use sysinfo::{Pid, ProcessesToUpdate, System, get_current_pid};
use tempfile::TempDir;
use tokio::process::Command;
use tokio::sync::oneshot;
use url::Url;

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

const MAXIMUM_SCREENSHOT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DifferentialReport {
    schema_version: u32,
    offprint_duration_milliseconds: u128,
    singlefile_duration_milliseconds: u128,
    offprint_bytes: u64,
    singlefile_bytes: u64,
    offprint_peak_resident_bytes: u64,
    singlefile_peak_resident_bytes: u64,
    browser: BrowserInfo,
    offprint_resources: ResourceSummary,
    pixel_similarity: f64,
    offprint_invariants: Value,
    singlefile_invariants: Value,
    frame_comparison: FrameComparison,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FrameComparison {
    offprint_complete: bool,
    singlefile_complete: bool,
}

#[derive(Debug)]
struct OfflineProbe {
    invariants: Value,
    screenshot: Vec<u8>,
}

#[tokio::test]
#[ignore = "requires OFFPRINT_SINGLEFILE_EXECUTABLE and a compatible Chromium browser"]
async fn singlefile_differential_fixture_preserves_offline_invariants() -> TestResult {
    let singlefile = std::env::var_os("OFFPRINT_SINGLEFILE_EXECUTABLE")
        .map(PathBuf::from)
        .ok_or("OFFPRINT_SINGLEFILE_EXECUTABLE is unset")?;
    let metadata = std::fs::symlink_metadata(&singlefile)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("SingleFile CLI must be a directly addressed regular file".into());
    }

    let server = FixtureServer::start().await?;
    server
        .register(
            "/frame",
            FixtureResponse::html(
                "<!doctype html><html><body><p id=\"frame-state\">frame rendered</p></body></html>",
            ),
        )
        .await?;
    server
        .register(
            "/",
            FixtureResponse::html(
                r##"<!doctype html><html><head><title>Differential fixture</title>
                <style>
                  body{font:18px system-ui;color:rgb(20,30,50);background:white}
                  main{width:480px;padding:24px;border:2px solid rgb(40,100,170)}
                </style></head><body><main>
                <h1>observed state</h1><section id="shadow"></section>
                <label>Name <input id="name" value="before"></label>
                <canvas id="canvas" width="80" height="40"></canvas>
                <iframe title="captured frame" src="/frame"></iframe>
                <output id="state">pending</output>
                </main><script>
                  document.querySelector("#name").value = "after";
                  document.querySelector("#state").textContent = "rendered";
                  document.querySelector("#shadow")
                    .attachShadow({mode:"open"}).innerHTML = "<strong>shadow text</strong>";
                  const context = document.querySelector("#canvas").getContext("2d");
                  context.fillStyle = "rgb(20, 140, 90)";
                  context.fillRect(0, 0, 80, 40);
                </script></body></html>"##,
            ),
        )
        .await?;
    let source = server.url("/")?;
    let output = TempDir::new()?;
    let offprint_path = output.path().join("offprint.html");
    let singlefile_path = output.path().join("singlefile.html");

    let offprint = Offprint::builder().build()?;
    let browser = offprint.browsers().ensure().await?;
    let browser_path = browser
        .executable_path
        .as_ref()
        .ok_or("selected browser has no executable path")?;
    let process_id = get_current_pid()?;
    let offprint_memory = PeakMemoryMonitor::start(process_id);
    let offprint_started = Instant::now();
    let result = offprint
        .capture(source.as_str())?
        .save(PortablePath::from_path_buf(offprint_path.clone())?)
        .await;
    let offprint_duration = offprint_started.elapsed();
    let offprint_peak_resident_bytes = offprint_memory.stop().await?;
    let result = result?;
    assert_eq!(result.verification.network_requests, 0);
    assert!(result.resources.is_complete());
    let offprint_resources = result.resources;
    let CaptureArtifact::File {
        bytes: offprint_bytes,
        ..
    } = result.artifact
    else {
        return Err("Offprint differential capture returned byte output".into());
    };
    offprint.close().await?;

    let singlefile_started = Instant::now();
    let singlefile_memory = PeakMemoryMonitor::start(process_id);
    let mut singlefile_command = Command::new(&singlefile);
    singlefile_command
        .kill_on_drop(true)
        .arg(source.as_str())
        .arg(&singlefile_path)
        .arg(format!(
            "--browser-executable-path={}",
            browser_path.as_utf8_path()
        ))
        .args([
            "--browser-headless=true",
            "--browser-width=1440",
            "--browser-height=900",
            "--browser-wait-until=load",
            "--browser-wait-until-delay=100",
        ]);
    let output = tokio::time::timeout(Duration::from_secs(120), singlefile_command.output()).await;
    let singlefile_duration = singlefile_started.elapsed();
    let singlefile_peak_resident_bytes = singlefile_memory.stop().await?;
    let output =
        output.map_err(|_| "SingleFile CLI exceeded the 120 second differential budget")??;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "SingleFile CLI exited with {}: {}",
            output.status,
            truncate(&stderr, 4_096)
        )
        .into());
    }
    let singlefile_bytes = std::fs::metadata(&singlefile_path)?.len();

    let process = ChromiumProcess::launch(ChromiumLaunchOptions::new(browser_path.clone())).await?;
    let offprint_probe = probe_artifact(&process, &offprint_path).await?;
    let singlefile_probe = probe_artifact(&process, &singlefile_path).await?;
    process.close().await?;
    server.close().await;

    let offprint_common_invariants = common_invariants(&offprint_probe.invariants);
    let singlefile_common_invariants = common_invariants(&singlefile_probe.invariants);
    let offprint_frame_complete = frame_is_complete(&offprint_probe.invariants);
    let singlefile_frame_complete = frame_is_complete(&singlefile_probe.invariants);
    let similarity = pixel_similarity(&offprint_probe.screenshot, &singlefile_probe.screenshot)?;

    write_report(&DifferentialReport {
        schema_version: 1,
        offprint_duration_milliseconds: offprint_duration.as_millis(),
        singlefile_duration_milliseconds: singlefile_duration.as_millis(),
        offprint_bytes,
        singlefile_bytes,
        offprint_peak_resident_bytes,
        singlefile_peak_resident_bytes,
        browser,
        offprint_resources,
        pixel_similarity: similarity,
        offprint_invariants: offprint_probe.invariants,
        singlefile_invariants: singlefile_probe.invariants,
        frame_comparison: FrameComparison {
            offprint_complete: offprint_frame_complete,
            singlefile_complete: singlefile_frame_complete,
        },
    })?;
    assert_eq!(offprint_common_invariants, singlefile_common_invariants);
    assert!(offprint_frame_complete);
    assert!(
        similarity >= 0.85,
        "differential screenshot similarity was {similarity:.4}"
    );
    Ok(())
}

async fn probe_artifact(process: &ChromiumProcess, path: &Path) -> TestResult<OfflineProbe> {
    let page = process.new_page(&BrowserEnvironment::default()).await?;
    let url = Url::from_file_path(path).map_err(|()| "artifact path cannot become a file URL")?;
    let observation = page
        .verify_offline_url(
            &url,
            Duration::from_secs(30),
            offprint_browser::RenderingMedia::Screen,
        )
        .await?;
    assert!(
        observation.attempted_urls.is_empty(),
        "artifact attempted network access: {:?}",
        observation.attempted_urls
    );
    assert!(
        observation.page_errors.is_empty(),
        "artifact raised page errors: {:?}",
        observation.page_errors
    );
    assert!(observation.stable);
    let invariants = page
        .evaluate(
            r##"JSON.stringify({
              title: document.title,
              heading: document.querySelector("h1")?.textContent,
              state: document.querySelector("#state")?.textContent,
              input: document.querySelector("#name")?.value,
              shadow: document.querySelector("#shadow")?.shadowRoot
                ?.querySelector("strong")?.textContent ??
                document.querySelector("#shadow strong")?.textContent,
              frame: document.querySelector("iframe")?.contentDocument
                ?.querySelector("#frame-state")?.textContent,
              canvasWidth: document.querySelector("canvas")?.width ??
                document.querySelector("img[data-offprint-canvas]")?.width ??
                document.querySelector("img[data-offprint-visual-fallback]")?.width
            })"##,
        )
        .await?
        .as_str()
        .and_then(|json| serde_json::from_str(json).ok())
        .ok_or("artifact invariants are not valid JSON")?;
    let screenshot = page
        .capture_page_screenshot(MAXIMUM_SCREENSHOT_BYTES)
        .await?;
    page.close().await?;
    Ok(OfflineProbe {
        invariants,
        screenshot,
    })
}

fn common_invariants(invariants: &Value) -> Value {
    let mut common = invariants.clone();
    if let Some(object) = common.as_object_mut() {
        object.remove("frame");
    }
    common
}

fn frame_is_complete(invariants: &Value) -> bool {
    invariants
        .get("frame")
        .and_then(Value::as_str)
        .is_some_and(|value| value == "frame rendered")
}

fn pixel_similarity(left: &[u8], right: &[u8]) -> TestResult<f64> {
    let left = ImageReader::new(std::io::Cursor::new(left))
        .with_guessed_format()?
        .decode()?
        .to_rgb8();
    let right = ImageReader::new(std::io::Cursor::new(right))
        .with_guessed_format()?
        .decode()?
        .to_rgb8();
    if left.dimensions() != right.dimensions() {
        return Ok(0.0);
    }
    let total_difference = left
        .as_raw()
        .iter()
        .zip(right.as_raw())
        .map(|(left, right)| u64::from(left.abs_diff(*right)))
        .sum::<u64>();
    let maximum_difference =
        255_u64.saturating_mul(u64::try_from(left.as_raw().len()).unwrap_or(u64::MAX));
    if maximum_difference == 0 {
        Ok(1.0)
    } else {
        Ok(1.0 - total_difference as f64 / maximum_difference as f64)
    }
}

struct PeakMemoryMonitor {
    stop: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<u64>,
}

impl PeakMemoryMonitor {
    fn start(root: Pid) -> Self {
        let (stop, mut stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            let mut system = System::new();
            let mut peak = 0_u64;
            loop {
                system.refresh_processes(ProcessesToUpdate::All, true);
                let mut resident_bytes = system.process(root).map_or(0, sysinfo::Process::memory);
                for (pid, process) in system.processes() {
                    if *pid != root && is_descendant(&system, *pid, root) {
                        resident_bytes = resident_bytes.saturating_add(process.memory());
                    }
                }
                peak = peak.max(resident_bytes);
                tokio::select! {
                    result = &mut stopped => {
                        let _ = result;
                        return peak;
                    }
                    () = tokio::time::sleep(Duration::from_millis(20)) => {}
                }
            }
        });
        Self { stop, task }
    }

    async fn stop(self) -> TestResult<u64> {
        let _ = self.stop.send(());
        Ok(self.task.await?)
    }
}

fn is_descendant(system: &System, mut pid: Pid, root: Pid) -> bool {
    for _ in 0..64 {
        let Some(parent) = system.process(pid).and_then(sysinfo::Process::parent) else {
            return false;
        };
        if parent == root {
            return true;
        }
        if parent == pid {
            return false;
        }
        pid = parent;
    }
    false
}

fn truncate(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        value.to_owned()
    } else {
        let mut end = maximum_bytes;
        while !value.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        value[..end].to_owned()
    }
}

fn write_report(report: &DifferentialReport) -> TestResult {
    let manifest_directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_directory
        .ancestors()
        .nth(2)
        .map(Path::to_owned)
        .ok_or("workspace root could not be resolved")?;
    let directory = root.join("offprint-rs/target").join("benchmark-evidence");
    std::fs::create_dir_all(&directory)?;
    let mut bytes = serde_json::to_vec_pretty(report)?;
    bytes.push(b'\n');
    let destination = directory.join("singlefile-differential.json");
    let mut staging = tempfile::NamedTempFile::new_in(&directory)?;
    staging.write_all(&bytes)?;
    staging.as_file_mut().flush()?;
    staging.as_file().sync_all()?;
    staging.persist(&destination)?;
    #[cfg(unix)]
    std::fs::File::open(directory)?.sync_all()?;
    Ok(())
}
