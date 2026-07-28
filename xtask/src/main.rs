#![forbid(unsafe_code)]

mod cdp;
mod contracts;
mod corpus;
mod package;
mod repository;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

use clap::{Parser, Subcommand};
use schemars::{JsonSchema, Schema, schema_for};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "PageKnot repository tasks")]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Codegen {
        #[arg(long)]
        check: bool,
    },
    CodegenCdp {
        #[arg(long)]
        check: bool,
    },
    TestFixture {
        fixture_id: String,
    },
    E2e {
        group: String,
    },
    ExploratoryCorpus {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        browser_path: Option<String>,
    },
    CheckRepository,
    Package {
        #[arg(long)]
        target: String,
        #[arg(long)]
        binary: PathBuf,
        #[arg(long, default_value = "dist")]
        output: PathBuf,
    },
    VerifyPackageMetadata {
        #[arg(long)]
        metadata: PathBuf,
        #[arg(long)]
        target: String,
        #[arg(long)]
        source_revision: String,
    },
    VerifyCratePackages {
        #[arg(long)]
        directory: PathBuf,
    },
}

#[tokio::main]
async fn main() {
    let result = run(Arguments::parse()).await;
    if let Err(error) = result {
        let _ = writeln!(std::io::stderr().lock(), "{error}");
        std::process::exit(1);
    }
}

async fn run(arguments: Arguments) -> Result<(), String> {
    match arguments.command {
        Command::Codegen { check } => generate_contracts(check),
        Command::CodegenCdp { check } => {
            let root = workspace_root();
            tokio::task::spawn_blocking(move || cdp::generate(&root, check))
                .await
                .map_err(|error| format!("CDP generation task failed: {error}"))?
        }
        Command::TestFixture { fixture_id } => test_fixture(&fixture_id),
        Command::E2e { group } => test_fixture_group(&group),
        Command::ExploratoryCorpus {
            manifest,
            output,
            limit,
            browser_path,
        } => corpus::run(&manifest, &output, limit, browser_path.as_deref()).await,
        Command::CheckRepository => repository::check(&workspace_root()),
        Command::Package {
            target,
            binary,
            output,
        } => package::create(&workspace_root(), &target, &binary, &output),
        Command::VerifyPackageMetadata {
            metadata,
            target,
            source_revision,
        } => package::verify_metadata(&workspace_root(), &metadata, &target, &source_revision),
        Command::VerifyCratePackages { directory } => {
            package::verify_crate_packages(&workspace_root(), &directory)
        }
    }
}

fn test_fixture(id: &str) -> Result<(), String> {
    let fixture = pageknot_test_support::fixture_definition(id)
        .ok_or_else(|| format!("unknown fixture ID `{id}`"))?;
    run_fixture(&fixture.runner)
}

fn test_fixture_group(group: &str) -> Result<(), String> {
    let manifest = pageknot_test_support::fixture_manifest();
    let mut runners = manifest
        .fixtures
        .iter()
        .filter(|fixture| {
            group == "all"
                || serde_json::to_value(fixture.group)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .as_deref()
                    == Some(group)
        })
        .map(|fixture| fixture.runner.clone())
        .collect::<Vec<_>>();
    if runners.is_empty() {
        return Err(format!("unknown or empty fixture group `{group}`"));
    }
    runners.sort();
    runners.dedup();
    for runner in &runners {
        run_fixture(runner)?;
    }
    Ok(())
}

fn run_fixture(runner: &pageknot_test_support::FixtureRunner) -> Result<(), String> {
    let mut command = ProcessCommand::new("cargo");
    command
        .current_dir(workspace_root())
        .args(["test", "--locked", "-p", &runner.package]);
    if let Some(target) = &runner.test_target {
        command.args(["--test", target]);
    } else {
        command.arg("--lib");
    }
    command.arg(&runner.filter).arg("--");
    if runner.ignored {
        command.arg("--ignored");
    }
    command.arg("--exact");
    if runner.serial {
        command.arg("--test-threads=1");
    }
    let status = command
        .status()
        .map_err(|error| format!("failed to start fixture test: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "fixture test `{}` exited with {status}",
            runner.filter
        ))
    }
}

fn generate_contracts(check: bool) -> Result<(), String> {
    let root = workspace_root();
    let schemas = root.join("schemas");
    let documents = schema_documents()?;
    let mut changed = Vec::new();
    for (name, content) in &documents {
        let path = schemas.join(name);
        update_file(&path, content, check, &mut changed)?;
    }
    for generated in contracts::binding_files(&documents)? {
        update_file(
            &root.join(generated.path),
            &generated.content,
            check,
            &mut changed,
        )?;
    }
    let license = fs::read(root.join("LICENSE"))
        .map_err(|error| format!("failed to read repository LICENSE: {error}"))?;
    update_file(
        &root.join("bindings/python/LICENSE"),
        &license,
        check,
        &mut changed,
    )?;
    for (name, content) in example_documents()? {
        let path = schemas.join("examples").join(name);
        update_file(&path, &content, check, &mut changed)?;
    }
    let fixture_manifest = pageknot_test_support::fixture_manifest();
    update_file(
        &root.join("fixtures/manifest/fixtures.json"),
        &pretty_json(&fixture_manifest)?,
        check,
        &mut changed,
    )?;
    update_file(
        &root.join("fixtures/manifest/fixtures.schema.json"),
        &pretty_json(&schema_for!(pageknot_test_support::FixtureManifest))?,
        check,
        &mut changed,
    )?;
    if check && !changed.is_empty() {
        return Err(format!(
            "generated contracts are stale: {}",
            changed
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(())
}

fn example_documents() -> Result<BTreeMap<&'static str, Vec<u8>>, String> {
    let mut documents = BTreeMap::new();
    let request = pageknot_model::CaptureRequest::builder("https://example.com/")
        .map_err(|error| error.to_string())?
        .output("example.html")
        .build()
        .map_err(|error| error.to_string())?;
    insert_example(
        &mut documents,
        "capture-request.json",
        serde_json::to_value(request).map_err(|error| error.to_string())?,
        canonical::<pageknot_model::CaptureRequest>,
    )?;

    let capture_id = "cap_01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let digest = "0000000000000000000000000000000000000000000000000000000000000000";
    let browser = json!({
        "product": "chrome",
        "version": "151.0.7922.47",
        "source": "managed",
        "executablePath": "/opt/pageknot/chrome",
        "revision": "1654411",
        "protocolVersion": "1.3"
    });
    let environment = json!({
        "viewport": {"width": 1440, "height": 900, "scale": 1},
        "locale": "en-US",
        "timezone": "UTC",
        "colorScheme": "light",
        "reducedMotion": "reduce",
        "userAgent": {"kind": "browser-default"}
    });
    let resources = json!({
        "discovered": 0,
        "embedded": 0,
        "external": 0,
        "omitted": 0,
        "failed": 0,
        "embeddedBytes": 0
    });
    let verification = json!({
        "schemaVersion": 1,
        "level": "offline",
        "passed": true,
        "artifactSha256": digest,
        "bytes": 1234,
        "networkRequests": 0,
        "attemptedUrls": [],
        "pageErrors": [],
        "frameFailures": [],
        "stable": true
    });
    let source = json!({
        "requestedUrl": "https://example.com/",
        "requestedUrlSha256": digest,
        "finalUrl": "https://example.com/",
        "finalUrlSha256": digest
    });

    insert_example(
        &mut documents,
        "capture-event.json",
        json!({
            "type": "resource.progress",
            "captureId": capture_id,
            "completed": 2,
            "discovered": 3,
            "bytes": 1024
        }),
        canonical::<pageknot_model::CaptureEvent>,
    )?;
    insert_example(
        &mut documents,
        "verification-result.json",
        verification.clone(),
        canonical::<pageknot_model::VerificationResult>,
    )?;
    insert_example(
        &mut documents,
        "capture-result.json",
        json!({
            "schemaVersion": 1,
            "captureId": capture_id,
            "status": "succeeded",
            "source": source,
            "artifact": {
                "kind": "file",
                "path": "example.html",
                "bytes": 1234,
                "sha256": digest
            },
            "verification": verification,
            "resources": resources,
            "warnings": [],
            "timings": {
                "total": 1000,
                "validation": 1,
                "browser": 100,
                "navigation": 200,
                "settle": 200,
                "collection": 200,
                "resources": 100,
                "transform": 50,
                "encoding": 50,
                "verification": 99,
                "commit": 0
            }
        }),
        canonical::<pageknot_model::CaptureResult>,
    )?;
    insert_example(
        &mut documents,
        "error.json",
        json!({
            "code": "pageknot.browser.unavailable",
            "message": "no compatible browser is available",
            "stage": "browser",
            "retryable": false,
            "details": {"recoveryCommand": "pageknot browser install"}
        }),
        canonical::<pageknot_model::PageKnotError>,
    )?;
    insert_example(
        &mut documents,
        "artifact-manifest.json",
        json!({
            "schemaVersion": 1,
            "format": {"kind": "html", "version": 1},
            "generator": {"name": "PageKnot", "version": "0.1.0"},
            "source": source,
            "capturedAt": "2026-07-27T12:00:00Z",
            "browser": browser,
            "environment": environment,
            "viewState": {"scrollX": "0", "scrollY": "0"},
            "policySha256": digest,
            "frames": 1,
            "resources": resources,
            "resourceRecords": [],
            "warningCodes": [],
            "structuralRepair": {"applied": false},
            "verification": {
                "level": "offline",
                "passed": true,
                "networkRequests": 0
            }
        }),
        canonical::<pageknot_model::ArtifactManifest>,
    )?;
    insert_example(
        &mut documents,
        "browser-doctor-report.json",
        json!({
            "schemaVersion": 1,
            "ready": true,
            "selected": browser,
            "candidates": [],
            "managedCache": {
                "cacheDir": "/var/cache/pageknot",
                "installedRevisions": ["1654411"],
                "selectedRevision": "1654411",
                "catalogVersion": "2026-07-27"
            },
            "collector": {
                "compatible": true,
                "hostVersion": pageknot_protocol::COLLECTOR_PROTOCOL_VERSION_STRING,
                "peerVersion": pageknot_protocol::COLLECTOR_PROTOCOL_VERSION_STRING,
                "capabilities": pageknot_protocol::CollectorCapability::ALL
                    .map(pageknot_protocol::CollectorCapability::as_str),
                "missingCapabilities": []
            },
            "output": {
                "directory": ".",
                "writable": true,
                "atomicCreate": true,
                "atomicReplace": true
            },
            "configuration": [],
            "network": {
                "profile": "standard",
                "permitsLoopbackInitialOrigin": true,
                "permitsPrivateAddresses": false,
                "revalidatesRedirects": true
            },
            "recovery": []
        }),
        canonical::<pageknot_model::BrowserDoctorReport>,
    )?;
    insert_example(
        &mut documents,
        "browser-operation-result.json",
        json!({
            "schemaVersion": 1,
            "action": "install",
            "browser": browser,
            "revision": "1654411",
            "cacheDir": "/var/cache/pageknot",
            "candidates": []
        }),
        canonical::<pageknot_model::BrowserOperationResult>,
    )?;

    let files = documents.keys().copied().collect::<Vec<_>>();
    documents.insert(
        "index.json",
        pretty_json(&json!({
            "schemaVersion": pageknot_model::PUBLIC_SCHEMA_VERSION,
            "files": files
        }))?,
    );
    Ok(documents)
}

fn canonical<T>(value: Value) -> Result<Value, String>
where
    T: DeserializeOwned + Serialize,
{
    let record: T = serde_json::from_value(value).map_err(|error| error.to_string())?;
    serde_json::to_value(record).map_err(|error| error.to_string())
}

fn insert_example(
    documents: &mut BTreeMap<&'static str, Vec<u8>>,
    name: &'static str,
    value: Value,
    canonicalize: impl FnOnce(Value) -> Result<Value, String>,
) -> Result<(), String> {
    documents.insert(name, pretty_json(&canonicalize(value)?)?);
    Ok(())
}

fn schema_documents() -> Result<BTreeMap<&'static str, Vec<u8>>, String> {
    let mut documents = BTreeMap::new();
    insert_schema::<pageknot_model::CaptureRequest>(&mut documents, "capture-request.schema.json")?;
    insert_schema::<pageknot_model::CapturePolicy>(&mut documents, "capture-policy.schema.json")?;
    insert_schema::<pageknot_model::CaptureEvent>(&mut documents, "capture-event.schema.json")?;
    insert_schema::<pageknot_model::CaptureResult>(&mut documents, "capture-result.schema.json")?;
    insert_schema::<pageknot_model::BatchRequest>(&mut documents, "batch-request.schema.json")?;
    insert_schema::<pageknot_model::BatchResult>(&mut documents, "batch-result.schema.json")?;
    insert_schema::<pageknot_model::CrawlRequest>(&mut documents, "crawl-request.schema.json")?;
    insert_schema::<pageknot_model::CrawlResult>(&mut documents, "crawl-result.schema.json")?;
    insert_schema::<pageknot_model::ResumeManifest>(&mut documents, "resume-manifest.schema.json")?;
    insert_schema::<pageknot_model::PageKnotError>(&mut documents, "error.schema.json")?;
    insert_schema::<pageknot_model::ArtifactManifest>(
        &mut documents,
        "artifact-manifest.schema.json",
    )?;
    insert_schema::<pageknot_model::VerificationResult>(
        &mut documents,
        "verification-result.schema.json",
    )?;
    insert_schema::<pageknot_model::ArtifactExportRequest>(
        &mut documents,
        "artifact-export-request.schema.json",
    )?;
    insert_schema::<pageknot_model::ArtifactExportResult>(
        &mut documents,
        "artifact-export-result.schema.json",
    )?;
    insert_schema::<pageknot_model::ArtifactVariantVerification>(
        &mut documents,
        "artifact-variant-verification.schema.json",
    )?;
    insert_schema::<pageknot_model::BrowserDoctorReport>(
        &mut documents,
        "browser-doctor-report.schema.json",
    )?;
    insert_schema::<pageknot_model::BrowserOperationResult>(
        &mut documents,
        "browser-operation-result.schema.json",
    )?;
    insert_schema::<pageknot_protocol::CollectorMessage>(
        &mut documents,
        "collector-message.schema.json",
    )?;
    insert_schema::<pageknot_protocol::PageObservation>(
        &mut documents,
        "page-observation.schema.json",
    )?;
    documents.insert("error-codes.json", error_code_registry()?);
    documents.insert(
        "binding-contracts.json",
        contracts::contract_inventory(&documents)?,
    );
    documents.insert("cli-json-contracts.json", contracts::cli_json_contracts()?);
    documents.insert("index.json", schema_index(&documents)?);
    Ok(documents)
}

fn insert_schema<T: JsonSchema>(
    documents: &mut BTreeMap<&'static str, Vec<u8>>,
    name: &'static str,
) -> Result<(), String> {
    documents.insert(name, pretty_json(&schema_for!(T))?);
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorCodeRecord {
    code: &'static str,
    stage: pageknot_model::ErrorStage,
    retryable: bool,
    description: &'static str,
}

fn error_code_registry() -> Result<Vec<u8>, String> {
    let records = pageknot_model::ERROR_CODE_REGISTRY
        .iter()
        .map(|definition| ErrorCodeRecord {
            code: definition.code,
            stage: definition.stage,
            retryable: definition.retryable,
            description: definition.description,
        })
        .collect::<Vec<_>>();
    pretty_json(&records)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SchemaIndex<'a> {
    schema_version: u32,
    artifact_format_version: u32,
    collector_protocol: pageknot_protocol::ProtocolVersion,
    files: Vec<&'a str>,
}

fn schema_index(documents: &BTreeMap<&'static str, Vec<u8>>) -> Result<Vec<u8>, String> {
    let index = SchemaIndex {
        schema_version: pageknot_model::PUBLIC_SCHEMA_VERSION,
        artifact_format_version: pageknot_model::ARTIFACT_FORMAT_VERSION,
        collector_protocol: pageknot_protocol::COLLECTOR_PROTOCOL_VERSION,
        files: documents.keys().copied().collect(),
    };
    pretty_json(&index)
}

fn pretty_json(value: &impl Serialize) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn update_file(
    path: &Path,
    expected: &[u8],
    check: bool,
    changed: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let current = fs::read(path).ok();
    if current.as_deref() == Some(expected) {
        return Ok(());
    }
    changed.push(path.to_owned());
    if check {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create schema directory {}: {error}",
                parent.display()
            )
        })?;
    }
    fs::write(path, expected)
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_owned)
}

#[allow(dead_code)]
fn _schema_type_is_serializable(_: Schema) {}
