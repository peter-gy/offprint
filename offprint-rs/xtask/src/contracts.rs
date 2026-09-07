use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

mod inventory;
mod python;
mod typescript;

const CONTRACTS: &[(&str, &str)] = &[
    ("export-request.schema.json", "ExportRequest"),
    ("export-result.schema.json", "ExportResult"),
    ("artifact-manifest.schema.json", "ArtifactManifest"),
    ("artifact-verification.schema.json", "ArtifactVerification"),
    ("format-verification.schema.json", "FormatVerification"),
    ("batch-request.schema.json", "BatchRequest"),
    ("batch-result.schema.json", "BatchResult"),
    ("browser-doctor-report.schema.json", "BrowserDoctorReport"),
    (
        "browser-operation-result.schema.json",
        "BrowserOperationResult",
    ),
    ("capture-event.schema.json", "CaptureEvent"),
    ("content-policy.schema.json", "ContentPolicy"),
    ("capture-request.schema.json", "CaptureRequest"),
    ("capture-receipt.schema.json", "CaptureReceipt"),
    ("crawl-request.schema.json", "CrawlRequest"),
    ("crawl-result.schema.json", "CrawlResult"),
    ("error.schema.json", "OffprintErrorRecord"),
    ("resume-manifest.schema.json", "ResumeManifest"),
    ("verification-report.schema.json", "VerificationReport"),
];

pub(super) const PUBLIC_ALIASES: &[(&str, &str, &str)] = &[
    ("FormatSpec", "ExportRequest", "FormatSpec"),
    ("ArtifactFormat", "FormatVerification", "ArtifactFormat"),
    ("BatchJob", "BatchRequest", "BatchJob"),
    ("BrowserInfo", "ArtifactManifest", "BrowserInfo"),
    ("CaptureLimits", "CaptureRequest", "CaptureLimits"),
    ("CaptureScope", "ContentPolicy", "CaptureScope"),
    ("ConflictPolicy", "ExportRequest", "ConflictPolicy"),
    ("CrawlPageOutcome", "CrawlResult", "CrawlPageOutcome"),
    ("ErrorStage", "OffprintErrorRecord", "ErrorStage"),
    ("ExportedArtifact", "ExportResult", "ExportedArtifact"),
    ("MarkdownOptions", "ExportRequest", "MarkdownOptions"),
    ("PdfOptions", "ExportRequest", "PdfOptions"),
    ("ResumeOptions", "BatchRequest", "ResumeOptions"),
    (
        "ScheduledCaptureOutcome",
        "BatchResult",
        "ScheduledCaptureOutcome",
    ),
    ("VerificationMode", "VerificationReport", "VerificationMode"),
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
            path: PathBuf::from("sdk/node/contracts.generated.d.ts"),
            content: typescript::generate(&schemas).into_bytes(),
        },
        GeneratedFile {
            path: PathBuf::from("sdk/python/src/offprint/contracts.py"),
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
