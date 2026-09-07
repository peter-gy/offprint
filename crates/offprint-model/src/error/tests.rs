use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::{ERROR_CODE_REGISTRY, ErrorCode, ErrorStage, OffprintError};

const NON_ERROR_IDENTIFIERS: &[&str] = &[
    "offprint.",
    "offprint.browser.",
    "offprint.browser.compatible",
    "offprint.browser.remote",
    "offprint.browser.shadowed_by_remote",
    "offprint.canvas.capture_failed",
    "offprint.canvas.capture_unavailable",
    "offprint.capture.fixture",
    "offprint.cssom.unreadable",
    "offprint.export.test",
    "offprint.frame.cross_origin",
    "offprint.html",
    "offprint.internal.fixture",
    "offprint.internal.test",
    "offprint._native",
    "offprint.bash",
    "offprint.elv",
    "offprint.exe",
    "offprint.fish",
    "offprint.resource.css_preservation",
    "offprint.resource.fixture",
    "offprint.test.mismatch",
];

#[test]
fn error_code_rejects_non_namespaced_values() {
    let result = ErrorCode::new("invalid");

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("offprint.input.error_code")
    );
}

#[test]
fn error_serialization_keeps_recovery_fields_structured() {
    let error = OffprintError::new(
        "offprint.browser.unavailable",
        ErrorStage::Browser,
        "no compatible browser was found",
    )
    .with_detail("recoveryCommand", "offprint browser install");
    let json = serde_json::to_value(error);

    assert_eq!(
        json.as_ref()
            .ok()
            .and_then(|value| value["details"]["recoveryCommand"].as_str()),
        Some("offprint browser install")
    );
}

#[test]
fn registered_error_uses_catalog_metadata() {
    let error = OffprintError::new(
        "offprint.artifact.read",
        ErrorStage::Internal,
        "artifact is unavailable",
    );

    assert_eq!(error.stage, ErrorStage::Verification);
    assert!(error.retryable);
}

#[test]
fn private_error_preserves_caller_metadata() {
    let error = OffprintError::new(
        "offprint.internal.fixture",
        ErrorStage::Internal,
        "fixture failed",
    )
    .retryable(true);

    assert_eq!(error.stage, ErrorStage::Internal);
    assert!(error.retryable);
}

#[test]
fn public_error_code_registry_is_sorted_unique_and_well_formed() {
    let mut codes = BTreeSet::new();

    for definition in ERROR_CODE_REGISTRY {
        assert!(ErrorCode::new(definition.code).is_ok());
        assert!(codes.insert(definition.code), "{}", definition.code);
        assert!(!definition.description.trim().is_empty());
    }
    assert!(
        ERROR_CODE_REGISTRY
            .windows(2)
            .all(|pair| pair[0].code < pair[1].code)
    );
}

#[test]
fn public_error_code_registry_covers_cross_boundary_failures() {
    for code in [
        "offprint.browser.target_crashed",
        "offprint.export.pdf_metadata",
        "offprint.input.credentials",
        "offprint.output.diagnostics",
        "offprint.runtime.interrupted",
        "offprint.verification.embedded_resource",
    ] {
        assert!(
            ERROR_CODE_REGISTRY
                .iter()
                .any(|definition| definition.code == code),
            "{code}"
        );
    }
}

#[test]
fn production_error_identifiers_are_registered() -> Result<(), String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = manifest
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "model crate must belong to a workspace".to_owned())?;
    let mut source_roots = vec![manifest.join("src")];
    if workspace.join("crates").is_dir() {
        source_roots.extend(
            ["bindings", "crates", "xtask"]
                .map(|directory| workspace.join(directory))
                .into_iter()
                .filter(|directory| directory.is_dir()),
        );
    }
    let registered = ERROR_CODE_REGISTRY
        .iter()
        .map(|definition| definition.code)
        .collect::<BTreeSet<_>>();
    let mut missing = BTreeSet::new();

    for root in source_roots {
        for source in rust_sources(&root) {
            let Ok(contents) = fs::read_to_string(&source) else {
                continue;
            };
            for identifier in offprint_identifiers(&contents) {
                if !registered.contains(identifier.as_str())
                    && !NON_ERROR_IDENTIFIERS.contains(&identifier.as_str())
                {
                    missing.insert(identifier);
                }
            }
        }
    }

    assert!(missing.is_empty(), "unregistered identifiers: {missing:?}");
    Ok(())
}

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut sources = Vec::new();
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() && !excluded_directory(&path) {
                pending.push(path);
            } else if file_type.is_file()
                && path.extension().is_some_and(|extension| extension == "rs")
            {
                sources.push(path);
            }
        }
    }
    sources
}

fn excluded_directory(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        name == ".git" || name == "dist" || name == "node_modules" || name == "target"
    })
}

fn offprint_identifiers(source: &str) -> impl Iterator<Item = String> + '_ {
    source
        .match_indices("\"offprint.")
        .filter_map(|(start, _)| {
            let value = &source[start + 1..];
            let end = value.find('"')?;
            let value = &value[..end];
            value
                .bytes()
                .all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'.'
                        || byte == b'_'
                })
                .then(|| value.to_owned())
        })
}
