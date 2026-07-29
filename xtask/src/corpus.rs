use std::collections::BTreeSet;
use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::time::Instant;

use chrono::{DateTime, Utc};
use pageknot::{BrowserInfo, PageKnot, ResourceSummary};
use pageknot_model::{CaptureTimings, PageKnotError};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

const CORPUS_SCHEMA_VERSION: u32 = 1;
const REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorpusManifest {
    schema_version: u32,
    name: String,
    source: String,
    source_revision: String,
    entries: Vec<CorpusEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorpusEntry {
    id: String,
    url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CorpusReport {
    schema_version: u32,
    generated_at: DateTime<Utc>,
    corpus_name: String,
    corpus_source: String,
    corpus_source_revision: String,
    corpus_sha256: String,
    browser: BrowserInfo,
    attempted: usize,
    succeeded: usize,
    failed: usize,
    entries: Vec<CorpusResult>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CorpusResult {
    id: String,
    url: String,
    elapsed_milliseconds: u64,
    #[serde(flatten)]
    outcome: CorpusOutcome,
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "status",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
enum CorpusOutcome {
    Succeeded {
        artifact_bytes: u64,
        artifact_sha256: String,
        network_requests: u32,
        resources: ResourceSummary,
        warning_codes: Vec<String>,
        timings: CaptureTimings,
    },
    Failed {
        error: PageKnotError,
    },
}

pub async fn run(
    manifest_path: &Path,
    output_path: &Path,
    limit: Option<usize>,
    browser_path: Option<&str>,
) -> Result<(), String> {
    let manifest_bytes = fs::read(manifest_path)
        .map_err(|error| format!("failed to read corpus manifest: {error}"))?;
    let manifest: CorpusManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("failed to decode corpus manifest: {error}"))?;
    validate_manifest(&manifest)?;
    if limit == Some(0) {
        return Err("corpus limit must be greater than zero".to_owned());
    }

    let mut builder = PageKnot::builder();
    if let Some(path) = browser_path {
        builder = builder.browser_path(path);
    }
    let pageknot = builder.build().map_err(|error| error.to_string())?;
    let capture_result: Result<_, String> = async {
        let browser = pageknot
            .browsers()
            .ensure()
            .await
            .map_err(|error| error.to_string())?;
        let count = limit
            .unwrap_or(manifest.entries.len())
            .min(manifest.entries.len());
        let mut results = Vec::with_capacity(count);
        for entry in manifest.entries.iter().take(count) {
            results.push(capture_entry(&pageknot, entry).await);
        }
        Ok((browser, results))
    }
    .await;
    let close_result = pageknot
        .close()
        .await
        .map_err(|error| format!("failed to close the corpus browser: {error}"));
    let (browser, results) = match (capture_result, close_result) {
        (Err(error), _) => return Err(error),
        (Ok(_), Err(error)) => return Err(error),
        (Ok(result), Ok(())) => result,
    };

    let succeeded = results
        .iter()
        .filter(|entry| matches!(entry.outcome, CorpusOutcome::Succeeded { .. }))
        .count();
    let report = CorpusReport {
        schema_version: REPORT_SCHEMA_VERSION,
        generated_at: Utc::now(),
        corpus_name: manifest.name,
        corpus_source: manifest.source,
        corpus_source_revision: manifest.source_revision,
        corpus_sha256: hex::encode(Sha256::digest(&manifest_bytes)),
        browser,
        attempted: results.len(),
        succeeded,
        failed: results.len().saturating_sub(succeeded),
        entries: results,
    };
    write_report(output_path, &report)?;
    writeln!(std::io::stdout().lock(), "{}", output_path.display())
        .map_err(|error| format!("failed to write the corpus report path: {error}"))?;
    Ok(())
}

async fn capture_entry(pageknot: &PageKnot, entry: &CorpusEntry) -> CorpusResult {
    let started = Instant::now();
    let outcome = match pageknot.capture(&entry.url) {
        Ok(capture) => match capture.run().await {
            Ok(result) => {
                let artifact_bytes = result.artifact.bytes();
                let artifact_sha256 = result.artifact.sha256().to_hex();
                let warning_codes = result
                    .warnings
                    .iter()
                    .map(|warning| warning.code.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                CorpusOutcome::Succeeded {
                    artifact_bytes,
                    artifact_sha256,
                    network_requests: result.verification.network_requests,
                    resources: result.resources,
                    warning_codes,
                    timings: result.timings,
                }
            }
            Err(error) => CorpusOutcome::Failed { error },
        },
        Err(error) => CorpusOutcome::Failed { error },
    };
    CorpusResult {
        id: entry.id.clone(),
        url: entry.url.clone(),
        elapsed_milliseconds: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        outcome,
    }
}

fn validate_manifest(manifest: &CorpusManifest) -> Result<(), String> {
    if manifest.schema_version != CORPUS_SCHEMA_VERSION {
        return Err(format!(
            "unsupported corpus schema version {}",
            manifest.schema_version
        ));
    }
    if manifest.name.trim().is_empty()
        || manifest.source.trim().is_empty()
        || manifest.source_revision.trim().is_empty()
        || manifest.entries.is_empty()
    {
        return Err("corpus metadata and entries must be present".to_owned());
    }
    let mut identifiers = BTreeSet::new();
    for entry in &manifest.entries {
        if entry.id.trim().is_empty() || !identifiers.insert(entry.id.as_str()) {
            return Err(format!(
                "corpus entry ID `{}` is empty or duplicated",
                entry.id
            ));
        }
        pageknot_model::CaptureRequest::builder(&entry.url)
            .and_then(|builder| builder.build())
            .map_err(|error| format!("corpus entry `{}` is invalid: {error}", entry.id))?;
    }
    Ok(())
}

fn write_report(path: &Path, report: &CorpusReport) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create corpus report directory: {error}"))?;
    }
    let mut encoded = serde_json::to_vec_pretty(report)
        .map_err(|error| format!("failed to encode corpus report: {error}"))?;
    encoded.push(b'\n');
    let temporary = path.with_extension(format!(
        "{}.tmp-{}",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("json"),
        std::process::id()
    ));
    fs::write(&temporary, encoded)
        .map_err(|error| format!("failed to write corpus report: {error}"))?;
    fs::rename(&temporary, path).map_err(|error| format!("failed to commit corpus report: {error}"))
}
