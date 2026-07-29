#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::future::Future;
use std::hint::black_box;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use base64::Engine as _;
use chrono::{DateTime, Utc};
use clap::{Parser, ValueEnum};
use data_url::DataUrl;
use pageknot::PageKnot;
use pageknot_capture::ContentStore;
use pageknot_document::{
    Document, RenderingRole, ResourceGraph, ResourceLocationKind, ResourceReference,
    discover_css_resources, discover_document_resources, serialize_document, serialize_document_to,
};
use pageknot_model::{
    ArtifactManifest, ArtifactSpec, FrameId, NodeId, OmissionReason, ResourceOutcome,
};
use pageknot_test_support::{FixtureResponse, FixtureServer};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use url::Url;

type BenchResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

const SCHEMA_VERSION: u32 = 1;
const DEFAULT_ITERATIONS: usize = 12;
const DEFAULT_BROWSER_ITERATIONS: usize = 3;
const WARMUP_ITERATIONS: usize = 2;
const CALIBRATION_NAME: &str = "sha256-4-mib";
const MIB: usize = 1024 * 1024;

#[derive(Debug, Parser)]
#[command(
    name = "pageknot-bench",
    about = "Measure PageKnot contracts and write a machine-readable report"
)]
struct Arguments {
    /// Select the benchmark corpus.
    #[arg(long, value_enum, default_value_t = Suite::Micro)]
    suite: Suite,

    /// Write the JSON report to this path.
    #[arg(long, default_value = "target/benchmark-evidence/performance.json")]
    output: PathBuf,

    /// Measured iterations for each micro benchmark.
    #[arg(long, default_value_t = DEFAULT_ITERATIONS)]
    iterations: usize,

    /// Measured iterations for each browser benchmark.
    #[arg(long, default_value_t = DEFAULT_BROWSER_ITERATIONS)]
    browser_iterations: usize,

    /// Use this Chrome or Chromium executable.
    #[arg(long)]
    browser_path: Option<PathBuf>,

    /// Compare normalized medians against this prior report.
    #[arg(long)]
    baseline: Option<PathBuf>,

    /// Fail when a normalized median exceeds the baseline by this percentage.
    #[arg(long, default_value_t = 75.0)]
    maximum_regression_percent: f64,

    /// Permit comparisons recorded on another OS or architecture.
    #[arg(long)]
    allow_environment_mismatch: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Suite {
    Micro,
    Browser,
    All,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    schema_version: u32,
    generated_at: DateTime<Utc>,
    git_revision: Option<String>,
    environment: Environment,
    configuration: Configuration,
    calibration_median_nanoseconds: u64,
    cases: Vec<CaseResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    comparison: Option<Comparison>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Environment {
    os: String,
    architecture: String,
    rustc: String,
    logical_cpus: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    browser_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    browser_revision: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Configuration {
    suite: String,
    warmup_iterations: usize,
    micro_iterations: usize,
    browser_iterations: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaseResult {
    name: String,
    category: String,
    input_bytes: u64,
    iterations: usize,
    minimum_nanoseconds: u64,
    median_nanoseconds: u64,
    p95_nanoseconds: u64,
    maximum_nanoseconds: u64,
    throughput_bytes_per_second: u64,
    normalized_median: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Comparison {
    baseline_path: String,
    environment_matches: bool,
    maximum_regression_percent: f64,
    passed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    missing_baseline_cases: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    missing_candidate_cases: Vec<String>,
    cases: Vec<CaseComparison>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaseComparison {
    name: String,
    baseline_normalized_median: f64,
    candidate_normalized_median: f64,
    change_percent: f64,
    passed: bool,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run(Arguments::parse()).await {
        let _ignored = writeln!(std::io::stderr().lock(), "{error}");
        std::process::exit(1);
    }
}

async fn run(arguments: Arguments) -> BenchResult {
    if arguments.iterations == 0 || arguments.browser_iterations == 0 {
        return Err("benchmark iteration counts must be greater than zero".into());
    }
    if !arguments.maximum_regression_percent.is_finite()
        || arguments.maximum_regression_percent < 0.0
    {
        return Err("maximum regression percent must be a finite non-negative value".into());
    }

    let calibration_bytes = deterministic_bytes(4 * MIB);
    let calibration = measure_sync(
        CALIBRATION_NAME,
        "calibration",
        calibration_bytes.len(),
        arguments.iterations,
        || {
            black_box(Sha256::digest(black_box(&calibration_bytes)));
            Ok(())
        },
    )?;
    let calibration_median = calibration.median_nanoseconds;

    let mut environment = Environment {
        os: std::env::consts::OS.to_owned(),
        architecture: std::env::consts::ARCH.to_owned(),
        rustc: env!("PAGEKNOT_BENCH_RUSTC_VERSION").to_owned(),
        logical_cpus: std::thread::available_parallelism().map_or(1, usize::from),
        browser_version: None,
        browser_revision: None,
    };
    let mut cases = vec![calibration];

    if matches!(arguments.suite, Suite::Micro | Suite::All) {
        cases.extend(micro_cases(arguments.iterations).await?);
    }
    if matches!(arguments.suite, Suite::Browser | Suite::All) {
        let (browser_cases, version, revision) = browser_cases(
            arguments.browser_iterations,
            arguments.browser_path.as_deref(),
        )
        .await?;
        environment.browser_version = Some(version);
        environment.browser_revision = revision;
        cases.extend(browser_cases);
    }
    normalize_cases(&mut cases, calibration_median);

    let mut report = Report {
        schema_version: SCHEMA_VERSION,
        generated_at: Utc::now(),
        git_revision: command_line("git", &["rev-parse", "HEAD"]),
        environment,
        configuration: Configuration {
            suite: format!("{:?}", arguments.suite).to_ascii_lowercase(),
            warmup_iterations: WARMUP_ITERATIONS,
            micro_iterations: arguments.iterations,
            browser_iterations: arguments.browser_iterations,
        },
        calibration_median_nanoseconds: calibration_median,
        cases,
        comparison: None,
    };

    let mut comparison_passed = true;
    if let Some(path) = &arguments.baseline {
        let baseline = read_report(path)?;
        let browser_matches = match (
            &report.environment.browser_version,
            &baseline.environment.browser_version,
        ) {
            (Some(candidate), Some(baseline)) => candidate == baseline,
            _ => true,
        };
        let environment_matches = report.environment.os == baseline.environment.os
            && report.environment.architecture == baseline.environment.architecture
            && report.environment.rustc == baseline.environment.rustc
            && browser_matches;
        if !environment_matches && !arguments.allow_environment_mismatch {
            return Err(format!(
                "baseline environment is {}-{} while this run is {}-{}",
                baseline.environment.os,
                baseline.environment.architecture,
                report.environment.os,
                report.environment.architecture
            )
            .into());
        }
        let comparison = compare(
            path,
            &baseline,
            &report,
            arguments.maximum_regression_percent,
            environment_matches,
        );
        comparison_passed = comparison.passed;
        report.comparison = Some(comparison);
    }

    write_report(&arguments.output, &report)?;
    writeln!(std::io::stdout().lock(), "{}", arguments.output.display())?;
    if comparison_passed {
        Ok(())
    } else {
        Err(format!(
            "performance comparison exceeded the {:.1}% regression limit",
            arguments.maximum_regression_percent
        )
        .into())
    }
}

async fn micro_cases(iterations: usize) -> BenchResult<Vec<CaseResult>> {
    let article = article_corpus(2_000);
    let article_bytes = article.len();
    let html = measure_sync(
        "html-parse-serialize",
        "document",
        article_bytes,
        iterations,
        || {
            let document = Document::parse(black_box(article.as_bytes()));
            black_box(serialize_document(&document)?);
            Ok(())
        },
    )?;

    let inline_resource = inline_resource_corpus(4 * MIB);
    let inline_resource_bytes = inline_resource.len();
    let inline_resource_document = Document::parse(inline_resource.as_bytes());
    let inline_resource_path =
        std::env::temp_dir().join(format!("pageknot-bench-inline-{}.html", std::process::id()));
    let inline_resource_case = measure_sync(
        "html-inline-resource-file-serialize",
        "document",
        inline_resource_bytes,
        iterations,
        || {
            let mut output = fs::File::create(&inline_resource_path)?;
            serialize_document_to(black_box(&inline_resource_document), &mut output)?;
            output.sync_all()?;
            Ok(())
        },
    )?;
    fs::remove_file(inline_resource_path)?;

    let css = css_corpus(1_000);
    let css_base = Url::parse("https://fixture.invalid/styles/main.css")?;
    let css_bytes = css.len();
    let css_case = measure_sync(
        "css-parse-url-rewrite",
        "document",
        css_bytes,
        iterations,
        || {
            let resources = discover_css_resources(black_box(&css), &css_base)?;
            let replacements = resources
                .resources()
                .iter()
                .map(|resource| (resource.id, "data:image/png;base64,cGFnZWtub3Q=".to_owned()))
                .collect::<BTreeMap<_, _>>();
            black_box(resources.rewrite(&replacements)?);
            Ok(())
        },
    )?;

    let srcset = srcset_corpus(250);
    let srcset_base = Url::parse("https://fixture.invalid/article/")?;
    let srcset_bytes = srcset.len();
    let srcset_case = measure_sync("srcset-parse", "document", srcset_bytes, iterations, || {
        let document = Document::parse(black_box(srcset.as_bytes()));
        black_box(discover_document_resources(&document, &srcset_base)?);
        Ok(())
    })?;

    let data_bytes = deterministic_bytes(MIB);
    let data_case = measure_sync(
        "data-url-encode-decode",
        "resource",
        data_bytes.len(),
        iterations,
        || {
            let encoded = base64::engine::general_purpose::STANDARD.encode(black_box(&data_bytes));
            let url = format!("data:application/octet-stream;base64,{encoded}");
            let parsed = DataUrl::process(black_box(&url))?;
            black_box(parsed.decode_to_vec()?);
            Ok(())
        },
    )?;

    let graph_case = measure_sync(
        "resource-graph-create-resolve",
        "resource",
        5_000 * 48,
        iterations,
        || {
            let base = Url::parse("https://fixture.invalid/")?;
            let mut graph = ResourceGraph::default();
            for index in 0_u32..5_000 {
                let resolved = base.join(&format!("asset/{index}.png"))?;
                let id = graph.discover(ResourceReference {
                    frame_id: FrameId::new(0),
                    node_id: NodeId::new(index),
                    location: ResourceLocationKind::HtmlAttribute,
                    original: resolved.as_str().to_owned(),
                    base_url: base.clone(),
                    resolved_url: resolved,
                    role: RenderingRole::Image,
                })?;
                graph.resolve(
                    id,
                    ResourceOutcome::Omitted {
                        reason: OmissionReason::NonRendering,
                    },
                )?;
            }
            graph.validate_complete()?;
            black_box(graph);
            Ok(())
        },
    )?;

    let store_bytes = deterministic_bytes(4 * MIB);
    let store_case = measure_content_store(&store_bytes, iterations).await?;

    let frames = frame_corpus(100);
    let frames_bytes = frames.len();
    let frame_case = measure_sync(
        "frame-embedding",
        "document",
        frames_bytes,
        iterations,
        || {
            let mut document = Document::parse(black_box(frames.as_bytes()));
            for index in 0..100 {
                document.embed_captured_frame(
                    index,
                    "<!doctype html><p>captured child</p>",
                    FrameId::new(index as u64 + 1),
                )?;
            }
            black_box(serialize_document(&document)?);
            Ok(())
        },
    )?;

    let manifest: ArtifactManifest = serde_json::from_slice(include_bytes!(
        "../../schemas/examples/artifact-manifest.json"
    ))?;
    let manifest_bytes = serde_json::to_vec(&manifest)?.len();
    let manifest_case = measure_sync(
        "manifest-serialization",
        "artifact",
        manifest_bytes,
        iterations,
        || {
            black_box(serde_json::to_vec(black_box(&manifest))?);
            Ok(())
        },
    )?;

    Ok(vec![
        html,
        inline_resource_case,
        css_case,
        srcset_case,
        data_case,
        graph_case,
        store_case,
        frame_case,
        manifest_case,
    ])
}

async fn browser_cases(
    iterations: usize,
    browser_path: Option<&Path>,
) -> BenchResult<(Vec<CaseResult>, String, Option<String>)> {
    let server = FixtureServer::start().await?;
    let result = browser_cases_with_server(&server, iterations, browser_path).await;
    server.close().await;
    result
}

async fn browser_cases_with_server(
    server: &FixtureServer,
    iterations: usize,
    browser_path: Option<&Path>,
) -> BenchResult<(Vec<CaseResult>, String, Option<String>)> {
    let image = deterministic_svg(64, 64);
    server
        .register(
            "/asset.svg",
            FixtureResponse {
                status: 200,
                content_type: "image/svg+xml".to_owned(),
                headers: BTreeMap::new(),
                body: image.into_bytes(),
            },
        )
        .await?;
    server
        .register(
            "/article",
            FixtureResponse::html(browser_article_corpus(200)),
        )
        .await?;
    server
        .register("/frames", FixtureResponse::html(browser_frame_corpus(24)))
        .await?;
    server
        .register("/images", FixtureResponse::html(browser_image_corpus(200)))
        .await?;

    let mut builder = PageKnot::builder();
    if let Some(path) = browser_path {
        let path = camino::Utf8PathBuf::from_path_buf(path.to_owned())
            .map_err(|path| format!("browser path is not UTF-8: {}", path.display()))?;
        builder = builder.browser_path(path);
    }
    let pageknot = builder.build()?;
    let result: BenchResult<_> = async {
        let browser = pageknot.browsers().ensure().await?;
        let article_url = server.url("/article")?;

        black_box(capture_once(&pageknot, article_url.as_str()).await?);
        let browser_version = browser.version;
        let browser_revision = browser.revision;

        let mut cases = Vec::new();
        for (name, path, input_bytes) in [
            (
                "end-to-end-static-article",
                "/article",
                browser_article_corpus(200).len(),
            ),
            (
                "end-to-end-frame-heavy",
                "/frames",
                browser_frame_corpus(24).len(),
            ),
            (
                "end-to-end-image-heavy",
                "/images",
                browser_image_corpus(200).len(),
            ),
        ] {
            let url = server.url(path)?;
            let case = measure_async(name, "browser", input_bytes, iterations, || {
                let pageknot = pageknot.clone();
                let url = url.clone();
                async move {
                    black_box(capture_once(&pageknot, url.as_str()).await?);
                    Ok(())
                }
            })
            .await?;
            cases.push(case);
        }

        let repeated = measure_async(
            "repeated-service-capture",
            "lifecycle",
            browser_article_corpus(200).len(),
            iterations.saturating_mul(2),
            || {
                let pageknot = pageknot.clone();
                let url = article_url.clone();
                async move {
                    black_box(capture_once(&pageknot, url.as_str()).await?);
                    Ok(())
                }
            },
        )
        .await?;
        cases.push(repeated);
        Ok((cases, browser_version, browser_revision))
    }
    .await;
    let close_result: BenchResult = pageknot.close().await.map_err(Into::into);
    match (result, close_result) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(result), Ok(())) => Ok(result),
    }
}

async fn capture_once(
    pageknot: &PageKnot,
    url: &str,
) -> BenchResult<pageknot_model::CaptureResult> {
    let mut request = pageknot_model::CaptureRequest::builder(url)?.build()?;
    request.artifact = ArtifactSpec::html_bytes(64 * MIB as u64);
    let result = pageknot.captures().start(request).await?.wait().await?;
    if !result.verification.passed || result.verification.network_requests != 0 {
        return Err("browser benchmark capture did not pass offline verification".into());
    }
    Ok(result)
}

fn measure_sync<F>(
    name: &str,
    category: &str,
    input_bytes: usize,
    iterations: usize,
    mut operation: F,
) -> BenchResult<CaseResult>
where
    F: FnMut() -> BenchResult,
{
    for _ in 0..WARMUP_ITERATIONS {
        operation()?;
    }
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        operation()?;
        samples.push(started.elapsed());
    }
    summarize(name, category, input_bytes, samples)
}

async fn measure_async<F, Fut>(
    name: &str,
    category: &str,
    input_bytes: usize,
    iterations: usize,
    mut operation: F,
) -> BenchResult<CaseResult>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = BenchResult>,
{
    for _ in 0..WARMUP_ITERATIONS {
        operation().await?;
    }
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        operation().await?;
        samples.push(started.elapsed());
    }
    summarize(name, category, input_bytes, samples)
}

async fn measure_content_store(bytes: &[u8], iterations: usize) -> BenchResult<CaseResult> {
    for _ in 0..WARMUP_ITERATIONS {
        let mut store = ContentStore::new()?;
        black_box(
            store
                .insert_bytes(bytes.to_vec(), 8 * MIB as u64, 8 * MIB as u64)
                .await?,
        );
    }

    let mut stores = Vec::with_capacity(iterations);
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let mut store = ContentStore::new()?;
        let payload = bytes.to_vec();
        let started = Instant::now();
        black_box(
            store
                .insert_bytes(payload, 8 * MIB as u64, 8 * MIB as u64)
                .await?,
        );
        samples.push(started.elapsed());
        stores.push(store);
    }
    let result = summarize("content-store-hash", "resource", bytes.len(), samples)?;
    black_box(stores);
    Ok(result)
}

fn summarize(
    name: &str,
    category: &str,
    input_bytes: usize,
    mut samples: Vec<Duration>,
) -> BenchResult<CaseResult> {
    if samples.is_empty() {
        return Err(format!("benchmark `{name}` produced no samples").into());
    }
    samples.sort_unstable();
    let iterations = samples.len();
    let minimum = duration_nanoseconds(samples[0]);
    let maximum = duration_nanoseconds(samples[iterations - 1]);
    let median = duration_nanoseconds(samples[iterations / 2]);
    let p95_index = ((iterations * 95).div_ceil(100)).saturating_sub(1);
    let p95 = duration_nanoseconds(samples[p95_index]);
    let throughput = u64::try_from(input_bytes)
        .unwrap_or(u64::MAX)
        .saturating_mul(1_000_000_000)
        .checked_div(median)
        .unwrap_or(u64::MAX);
    Ok(CaseResult {
        name: name.to_owned(),
        category: category.to_owned(),
        input_bytes: u64::try_from(input_bytes).unwrap_or(u64::MAX),
        iterations,
        minimum_nanoseconds: minimum,
        median_nanoseconds: median,
        p95_nanoseconds: p95,
        maximum_nanoseconds: maximum,
        throughput_bytes_per_second: throughput,
        normalized_median: 0.0,
    })
}

fn duration_nanoseconds(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn normalize_cases(cases: &mut [CaseResult], calibration_median: u64) {
    let denominator = calibration_median.max(1) as f64;
    for case in cases {
        case.normalized_median = case.median_nanoseconds as f64 / denominator;
    }
}

fn compare(
    baseline_path: &Path,
    baseline: &Report,
    candidate: &Report,
    maximum_regression_percent: f64,
    environment_matches: bool,
) -> Comparison {
    let baseline_cases = baseline
        .cases
        .iter()
        .map(|case| (case.name.as_str(), case))
        .collect::<BTreeMap<_, _>>();
    let candidate_names = candidate
        .cases
        .iter()
        .map(|case| case.name.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut cases = Vec::new();
    let mut missing_baseline_cases = Vec::new();
    let missing_candidate_cases = baseline
        .cases
        .iter()
        .filter(|case| !candidate_names.contains(case.name.as_str()))
        .map(|case| case.name.clone())
        .collect::<Vec<_>>();
    for candidate_case in &candidate.cases {
        let Some(baseline_case) = baseline_cases.get(candidate_case.name.as_str()) else {
            missing_baseline_cases.push(candidate_case.name.clone());
            continue;
        };
        let baseline_value = baseline_case.normalized_median.max(f64::EPSILON);
        let change_percent = ((candidate_case.normalized_median / baseline_value) - 1.0) * 100.0;
        cases.push(CaseComparison {
            name: candidate_case.name.clone(),
            baseline_normalized_median: baseline_case.normalized_median,
            candidate_normalized_median: candidate_case.normalized_median,
            change_percent,
            passed: change_percent <= maximum_regression_percent,
        });
    }
    let passed = !cases.is_empty()
        && missing_baseline_cases.is_empty()
        && missing_candidate_cases.is_empty()
        && cases.iter().all(|case| case.passed);
    Comparison {
        baseline_path: baseline_path.display().to_string(),
        environment_matches,
        maximum_regression_percent,
        passed,
        missing_baseline_cases,
        missing_candidate_cases,
        cases,
    }
}

fn read_report(path: &Path) -> BenchResult<Report> {
    let report: Report = serde_json::from_slice(&fs::read(path)?)?;
    if report.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported benchmark report schema version {}",
            report.schema_version
        )
        .into());
    }
    Ok(report)
}

fn write_report(path: &Path, report: &Report) -> BenchResult {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut encoded = serde_json::to_vec_pretty(report)?;
    encoded.push(b'\n');
    let temporary = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("json"),
        std::process::id()
    ));
    fs::write(&temporary, encoded)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn command_line(program: &str, arguments: &[&str]) -> Option<String> {
    let output = Command::new(program).args(arguments).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn deterministic_bytes(length: usize) -> Vec<u8> {
    (0..length)
        .map(|index| ((index.wrapping_mul(31).wrapping_add(17)) % 251) as u8)
        .collect()
}

fn article_corpus(paragraphs: usize) -> String {
    let mut html = String::from(
        "<!doctype html><html><head><title>Benchmark article</title></head><body><main>",
    );
    for index in 0..paragraphs {
        html.push_str(&format!(
            "<section id=\"s{index}\"><h2>Section {index}</h2><p>Rendered archival content {index} with <a href=\"#s0\">a stable link</a>.</p></section>"
        ));
    }
    html.push_str("</main></body></html>");
    html
}

fn inline_resource_corpus(bytes: usize) -> String {
    format!(
        "<!doctype html><html><head></head><body><img src=\"data:image/png;base64,{}\"></body></html>",
        "A".repeat(bytes)
    )
}

fn css_corpus(rules: usize) -> String {
    let mut css = String::new();
    for index in 0..rules {
        css.push_str(&format!(
            ".asset-{index}{{background-image:url('../images/{index}.png');color:rgb({},{},{});}}\n",
            index % 255,
            (index * 3) % 255,
            (index * 7) % 255
        ));
    }
    css
}

fn srcset_corpus(images: usize) -> String {
    let mut html = String::from("<!doctype html><html><head></head><body>");
    for index in 0..images {
        html.push_str(&format!(
            "<img src=\"fallback-{index}.png\" srcset=\"image-{index}-1.png 1x, image-{index}-2.png 2x, image-{index}-640.png 640w, image-{index}-1280.png 1280w\">"
        ));
    }
    html.push_str("</body></html>");
    html
}

fn frame_corpus(frames: usize) -> String {
    let mut html = String::from("<!doctype html><html><head></head><body>");
    for _ in 0..frames {
        html.push_str("<iframe src=\"child.html\"></iframe>");
    }
    html.push_str("</body></html>");
    html
}

fn browser_article_corpus(paragraphs: usize) -> String {
    let mut html = String::from(
        "<!doctype html><html><head><title>Static benchmark</title><style>main{max-width:72ch}img{width:64px;height:64px}</style></head><body><main><img src=\"/asset.svg\">",
    );
    for index in 0..paragraphs {
        html.push_str(&format!(
            "<p data-row=\"{index}\">A stable rendered paragraph for capture benchmark {index}.</p>"
        ));
    }
    html.push_str("</main></body></html>");
    html
}

fn browser_frame_corpus(frames: usize) -> String {
    let mut html =
        String::from("<!doctype html><html><head><title>Frame benchmark</title></head><body>");
    for index in 0..frames {
        let child = format!(
            "<!doctype html><html><head><style>body{{color:rgb({},30,60)}}</style></head><body><p>Frame {index}</p></body></html>",
            index * 7 % 255
        );
        html.push_str(&format!(
            "<iframe title=\"frame {index}\" srcdoc=\"{}\"></iframe>",
            child.replace('&', "&amp;").replace('"', "&quot;")
        ));
    }
    html.push_str("</body></html>");
    html
}

fn browser_image_corpus(images: usize) -> String {
    let mut html = String::from(
        "<!doctype html><html><head><title>Image benchmark</title><style>img{width:64px;height:64px}</style></head><body>",
    );
    for index in 0..images {
        html.push_str(&format!(
            "<img alt=\"asset {index}\" src=\"/asset.svg?variant={index}\">"
        ));
    }
    html.push_str("</body></html>");
    html
}

fn deterministic_svg(width: u32, height: u32) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\"><rect width=\"{width}\" height=\"{height}\" fill=\"rgb(24,96,160)\"/></svg>"
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use chrono::Utc;

    use super::{CaseResult, Configuration, Environment, Report, SCHEMA_VERSION, compare};

    #[test]
    fn comparison_requires_the_same_cases_in_both_reports() {
        let baseline = report(&["shared", "removed"]);
        let candidate = report(&["shared", "added"]);

        let comparison = compare(
            Path::new("baseline.json"),
            &baseline,
            &candidate,
            10.0,
            true,
        );

        assert!(!comparison.passed);
        assert_eq!(comparison.missing_baseline_cases, ["added"]);
        assert_eq!(comparison.missing_candidate_cases, ["removed"]);
        assert_eq!(comparison.cases.len(), 1);
        assert_eq!(comparison.cases[0].name, "shared");
    }

    fn report(names: &[&str]) -> Report {
        Report {
            schema_version: SCHEMA_VERSION,
            generated_at: Utc::now(),
            git_revision: None,
            environment: Environment {
                os: "test".to_owned(),
                architecture: "test".to_owned(),
                rustc: "test".to_owned(),
                logical_cpus: 1,
                browser_version: None,
                browser_revision: None,
            },
            configuration: Configuration {
                suite: "test".to_owned(),
                warmup_iterations: 0,
                micro_iterations: 1,
                browser_iterations: 1,
            },
            calibration_median_nanoseconds: 1,
            cases: names.iter().map(|name| case(name)).collect(),
            comparison: None,
        }
    }

    fn case(name: &str) -> CaseResult {
        CaseResult {
            name: name.to_owned(),
            category: "test".to_owned(),
            input_bytes: 1,
            iterations: 1,
            minimum_nanoseconds: 1,
            median_nanoseconds: 1,
            p95_nanoseconds: 1,
            maximum_nanoseconds: 1,
            throughput_bytes_per_second: 1,
            normalized_median: 1.0,
        }
    }
}
