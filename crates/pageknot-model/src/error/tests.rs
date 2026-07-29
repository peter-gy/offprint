use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::{ERROR_CODE_REGISTRY, ErrorCode, ErrorStage, PageKnotError};

const NON_ERROR_IDENTIFIERS: &[&str] = &[
    "pageknot.",
    "pageknot.browser.",
    "pageknot.browser.compatible",
    "pageknot.browser.remote",
    "pageknot.browser.shadowed_by_remote",
    "pageknot.canvas.capture_failed",
    "pageknot.canvas.capture_unavailable",
    "pageknot.capture.fixture",
    "pageknot.export.test",
    "pageknot.frame.cross_origin",
    "pageknot.html",
    "pageknot.internal.fixture",
    "pageknot.internal.test",
    "pageknot._native",
    "pageknot.bash",
    "pageknot.elv",
    "pageknot.exe",
    "pageknot.fish",
    "pageknot.resource.css_preservation",
    "pageknot.resource.fixture",
    "pageknot.test.mismatch",
];

#[test]
fn error_code_rejects_non_namespaced_values() {
    let result = ErrorCode::new("invalid");

    assert_eq!(
        result.as_ref().map_err(|error| error.code.as_str()),
        Err("pageknot.input.error_code")
    );
}

#[test]
fn error_serialization_keeps_recovery_fields_structured() {
    let error = PageKnotError::new(
        "pageknot.browser.unavailable",
        ErrorStage::Browser,
        "no compatible browser was found",
    )
    .with_detail("recoveryCommand", "pageknot browser install");
    let json = serde_json::to_value(error);

    assert_eq!(
        json.as_ref()
            .ok()
            .and_then(|value| value["details"]["recoveryCommand"].as_str()),
        Some("pageknot browser install")
    );
}

#[test]
fn registered_error_uses_catalog_metadata() {
    let error = PageKnotError::new(
        "pageknot.artifact.read",
        ErrorStage::Internal,
        "artifact is unavailable",
    );

    assert_eq!(error.stage, ErrorStage::Verification);
    assert!(error.retryable);
}

#[test]
fn private_error_preserves_caller_metadata() {
    let error = PageKnotError::new(
        "pageknot.internal.fixture",
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
        "pageknot.browser.target_crashed",
        "pageknot.export.pdf_metadata",
        "pageknot.input.credentials",
        "pageknot.output.diagnostics",
        "pageknot.runtime.interrupted",
        "pageknot.verification.embedded_resource",
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
            for identifier in pageknot_identifiers(&contents) {
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

fn pageknot_identifiers(source: &str) -> impl Iterator<Item = String> + '_ {
    source
        .match_indices("\"pageknot.")
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
