use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

const MAXIMUM_AUTHORED_LINES: usize = 990;
const TEXT_EXTENSIONS: &[&str] = &[
    "cjs", "css", "html", "js", "json", "jsx", "md", "py", "rs", "sh", "toml", "ts", "tsx", "yml",
    "yaml",
];
const PRODUCTION_SOURCE_EXTENSIONS: &[&str] = &[
    "cjs", "css", "html", "js", "jsx", "py", "rs", "sh", "ts", "tsx",
];

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: String,
    dependencies: Vec<CargoDependency>,
}

#[derive(Debug, Deserialize)]
struct CargoDependency {
    name: String,
    kind: Option<String>,
}

pub(super) fn repository_text_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
        ])
        .output()
        .map_err(|error| format!("failed to list repository files: {error}"))?;
    if !output.status.success() {
        return Err(format!("git ls-files exited with status {}", output.status));
    }
    let paths = std::str::from_utf8(&output.stdout)
        .map_err(|error| format!("git ls-files returned a non-UTF-8 path: {error}"))?;
    let mut files = Vec::new();
    for raw in paths.split_terminator('\0') {
        let relative = Path::new(raw);
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
        {
            return Err(format!("git reported an unsafe repository path `{raw}`"));
        }
        let path = root.join(relative);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => continue,
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!("failed to inspect {}: {error}", path.display()));
            }
        }
        if is_text_path(relative) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

pub(super) fn check(
    root: &Path,
    files: &[PathBuf],
    violations: &mut Vec<String>,
) -> Result<(), String> {
    check_authored_file_sizes(root, files, violations)?;
    check_dependency_direction(root, violations)
}

fn check_authored_file_sizes(
    root: &Path,
    files: &[PathBuf],
    violations: &mut Vec<String>,
) -> Result<(), String> {
    for path in files {
        let relative = path.strip_prefix(root).map_err(|error| {
            format!(
                "tracked path {} is outside {}: {error}",
                path.display(),
                root.display()
            )
        })?;
        if !is_handwritten_production_source(relative)
            || is_generated(relative)
            || is_test(relative)
            || relative.starts_with("fixtures")
            || relative.components().any(|component| {
                matches!(component.as_os_str().to_str(), Some("vendor" | "vendored"))
            })
        {
            continue;
        }
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        let text = fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let lines = text.lines().count();
        if lines > MAXIMUM_AUTHORED_LINES {
            violations.push(format!(
                "{} has {lines} authored lines, exceeding the {MAXIMUM_AUTHORED_LINES}-line limit",
                path.display()
            ));
        }
    }
    Ok(())
}

fn check_dependency_direction(root: &Path, violations: &mut Vec<String>) -> Result<(), String> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(root)
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .output()
        .map_err(|error| format!("failed to inspect workspace dependencies: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata exited with status {}",
            output.status
        ));
    }
    let metadata: CargoMetadata = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("invalid cargo metadata output: {error}"))?;
    for package in metadata.packages {
        for dependency in package
            .dependencies
            .iter()
            .filter(|dependency| dependency.kind.as_deref() != Some("dev"))
        {
            if dependency_points_outward(&package.name, &dependency.name) {
                violations.push(format!(
                    "{} has an outward production dependency on {}",
                    package.name, dependency.name
                ));
            }
        }
    }
    Ok(())
}

fn dependency_points_outward(package: &str, dependency: &str) -> bool {
    if !is_workspace_package(package) || !is_workspace_package(dependency) {
        return false;
    }
    !allowed_workspace_dependencies(package).contains(&dependency)
}

fn is_workspace_package(package: &str) -> bool {
    package == "xtask" || package.starts_with("offprint")
}

fn allowed_workspace_dependencies(package: &str) -> &'static [&'static str] {
    match package {
        "offprint-model" => &[],
        "offprint-artifact"
        | "offprint-capture"
        | "offprint-document"
        | "offprint-protocol"
        | "offprint-test-support" => &["offprint-model"],
        "offprint-browser" => &["offprint-model"],
        "offprint-html" => &["offprint-document", "offprint-model"],
        "offprint-chromium" => &["offprint-browser", "offprint-model", "offprint-protocol"],
        "offprint-export" | "offprint-transform" => {
            &["offprint-document", "offprint-html", "offprint-model"]
        }
        "offprint" => &[
            "offprint-artifact",
            "offprint-browser",
            "offprint-capture",
            "offprint-chromium",
            "offprint-document",
            "offprint-export",
            "offprint-html",
            "offprint-model",
            "offprint-protocol",
            "offprint-transform",
        ],
        "offprint-cli" | "offprint-node" | "offprint-python" => &["offprint"],
        "offprint-bench" => &[
            "offprint",
            "offprint-capture",
            "offprint-document",
            "offprint-model",
            "offprint-test-support",
        ],
        "xtask" => &[
            "offprint",
            "offprint-browser",
            "offprint-chromium",
            "offprint-cli",
            "offprint-model",
            "offprint-protocol",
            "offprint-test-support",
        ],
        _ => &[],
    }
}

fn is_text_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| TEXT_EXTENSIONS.contains(&extension))
        || path.file_name().and_then(|value| value.to_str()) == Some("justfile")
}

fn is_handwritten_production_source(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| PRODUCTION_SOURCE_EXTENSIONS.contains(&extension))
}

fn is_generated(path: &Path) -> bool {
    path == Path::new("crates/offprint-chromium/src/cdp_generated.rs")
        || path.starts_with("crates/offprint-chromium/src/cdp/generated")
        || path.starts_with("collector/dist")
        || path.starts_with("crates/offprint-chromium/generated")
        || path == Path::new("bindings/node/contracts.generated.d.ts")
        || path == Path::new("bindings/python/python/offprint/contracts.py")
        || path.starts_with("schemas")
        || path.starts_with("fixtures/manifest")
}

fn is_test(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == "tests")
        || path.file_name().and_then(|value| value.to_str()) == Some("tests.rs")
        || path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| {
                name.ends_with("_test.rs")
                    || name.ends_with("_tests.rs")
                    || name.ends_with(".test.ts")
                    || (name.starts_with("test_") && name.ends_with(".py"))
            })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::TempDir;

    use super::{
        MAXIMUM_AUTHORED_LINES, check_authored_file_sizes, dependency_points_outward, is_generated,
        is_handwritten_production_source, is_test,
    };

    #[test]
    fn dependency_direction_allows_inward_edges() {
        assert!(!dependency_points_outward(
            "offprint-chromium",
            "offprint-browser"
        ));
        assert!(!dependency_points_outward("offprint", "offprint-html"));
        assert!(!dependency_points_outward(
            "offprint-browser",
            "offprint-model"
        ));
        assert!(!dependency_points_outward("xtask", "offprint"));
    }

    #[test]
    fn dependency_direction_rejects_sideways_and_outward_edges() {
        assert!(dependency_points_outward(
            "offprint-browser",
            "offprint-protocol"
        ));
        assert!(dependency_points_outward(
            "offprint-html",
            "offprint-browser"
        ));
        assert!(dependency_points_outward(
            "offprint-export",
            "offprint-browser"
        ));
        assert!(dependency_points_outward("offprint-document", "offprint"));
        assert!(dependency_points_outward(
            "offprint-chromium",
            "offprint-test-support"
        ));
        assert!(dependency_points_outward(
            "offprint-cli",
            "offprint-artifact"
        ));
        assert!(!dependency_points_outward("offprint-cli", "offprint"));
    }

    #[test]
    fn size_exemptions_are_explicit() {
        assert!(is_generated(Path::new(
            "crates/offprint-chromium/src/cdp_generated.rs"
        )));
        assert!(is_generated(Path::new(
            "bindings/python/python/offprint/contracts.py"
        )));
        assert!(is_test(Path::new(
            "crates/offprint/tests/fixture_matrix.rs"
        )));
        assert!(is_test(Path::new(
            "crates/offprint-cli/src/runner/tests.rs"
        )));
        assert!(!is_generated(Path::new("crates/offprint/src/lib.rs")));
        assert!(!is_test(Path::new("crates/offprint/src/lib.rs")));
        assert!(is_handwritten_production_source(Path::new(
            "crates/offprint/src/lib.rs"
        )));
        assert!(!is_handwritten_production_source(Path::new(
            "development_docs/product-contract.md"
        )));
        assert!(!is_handwritten_production_source(Path::new("Cargo.toml")));
    }

    #[test]
    fn authored_production_source_has_a_hard_line_limit() -> Result<(), String> {
        let temporary = TempDir::new().map_err(|error| error.to_string())?;
        let source = temporary.path().join("lib.rs");
        fs::write(&source, "line\n".repeat(MAXIMUM_AUTHORED_LINES + 1))
            .map_err(|error| error.to_string())?;
        let mut violations = Vec::new();

        check_authored_file_sizes(temporary.path(), &[source], &mut violations)?;

        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("exceeding the 990-line limit"));
        Ok(())
    }
}
