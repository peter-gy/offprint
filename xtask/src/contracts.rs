use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

mod inventory;
mod python;
mod typescript;

const CONTRACTS: &[(&str, &str)] = &[
    (
        "artifact-export-request.schema.json",
        "ArtifactExportRequest",
    ),
    ("artifact-export-result.schema.json", "ArtifactExportResult"),
    ("artifact-manifest.schema.json", "ArtifactManifest"),
    (
        "artifact-variant-verification.schema.json",
        "ArtifactVariantVerification",
    ),
    ("batch-request.schema.json", "BatchRequest"),
    ("batch-result.schema.json", "BatchResult"),
    ("browser-doctor-report.schema.json", "BrowserDoctorReport"),
    (
        "browser-operation-result.schema.json",
        "BrowserOperationResult",
    ),
    ("capture-event.schema.json", "CaptureEvent"),
    ("capture-policy.schema.json", "CapturePolicy"),
    ("capture-request.schema.json", "CaptureRequest"),
    ("capture-result.schema.json", "CaptureResult"),
    ("crawl-request.schema.json", "CrawlRequest"),
    ("crawl-result.schema.json", "CrawlResult"),
    ("error.schema.json", "PageKnotErrorRecord"),
    ("resume-manifest.schema.json", "ResumeManifest"),
    ("verification-result.schema.json", "VerificationResult"),
];

pub(super) const PUBLIC_ALIASES: &[(&str, &str, &str)] = &[
    (
        "ArtifactVariant",
        "ArtifactExportRequest",
        "ArtifactVariant",
    ),
    (
        "ArtifactVariantKind",
        "ArtifactVariantVerification",
        "ArtifactVariantKind",
    ),
    ("BatchJob", "BatchRequest", "BatchJob"),
    ("BrowserInfo", "ArtifactManifest", "BrowserInfo"),
    ("CaptureLimits", "CaptureRequest", "CaptureLimits"),
    ("CaptureScope", "CapturePolicy", "CaptureScope"),
    ("ConflictPolicy", "ArtifactExportRequest", "ConflictPolicy"),
    ("CrawlPageOutcome", "CrawlResult", "CrawlPageOutcome"),
    ("ErrorStage", "PageKnotErrorRecord", "ErrorStage"),
    (
        "ExportedArtifact",
        "ArtifactExportResult",
        "ExportedArtifact",
    ),
    (
        "MarkdownOptions",
        "ArtifactExportRequest",
        "MarkdownOptions",
    ),
    ("PdfOptions", "ArtifactExportRequest", "PdfOptions"),
    ("ResumeOptions", "BatchRequest", "ResumeOptions"),
    (
        "ScheduledCaptureOutcome",
        "BatchResult",
        "ScheduledCaptureOutcome",
    ),
    (
        "VerificationPolicy",
        "VerificationResult",
        "VerificationPolicy",
    ),
    ("Viewport", "CaptureRequest", "Viewport"),
];

pub struct GeneratedFile {
    pub path: PathBuf,
    pub content: Vec<u8>,
}

pub fn contract_inventory(documents: &BTreeMap<&'static str, Vec<u8>>) -> Result<Vec<u8>, String> {
    inventory::contract_inventory(documents)
}

pub fn cli_json_contracts() -> Result<Vec<u8>, String> {
    inventory::cli_json_contracts()
}

pub fn binding_files(
    documents: &BTreeMap<&'static str, Vec<u8>>,
) -> Result<Vec<GeneratedFile>, String> {
    let schemas = parsed_contracts(documents)?;
    Ok(vec![
        GeneratedFile {
            path: PathBuf::from("bindings/node/contracts.generated.d.ts"),
            content: typescript::generate(&schemas).into_bytes(),
        },
        GeneratedFile {
            path: PathBuf::from("bindings/python/python/pageknot/_contracts.pyi"),
            content: python::generate(&schemas)?.into_bytes(),
        },
    ])
}

fn parsed_contracts(
    documents: &BTreeMap<&'static str, Vec<u8>>,
) -> Result<Vec<(&'static str, Value)>, String> {
    CONTRACTS
        .iter()
        .map(|&(schema_name, public_name)| {
            let schema = serde_json::from_slice(schema_bytes(documents, schema_name)?)
                .map_err(|error| format!("invalid generated schema {schema_name}: {error}"))?;
            Ok((public_name, schema))
        })
        .collect()
}

fn schema_bytes<'a>(
    documents: &'a BTreeMap<&'static str, Vec<u8>>,
    name: &str,
) -> Result<&'a [u8], String> {
    documents
        .get(name)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("missing generated schema {name}"))
}

pub(super) fn is_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters.next().is_some_and(|first| {
        (first.is_ascii_alphabetic() || first == '_')
            && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
    })
}
