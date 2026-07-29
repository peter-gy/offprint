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
    if dependency == "pageknot-test-support" && dependency_rank(package).is_some() {
        return true;
    }
    if is_frontend(package) && dependency.starts_with("pageknot-") && dependency != "pageknot" {
        return true;
    }
    let (Some(package_rank), Some(dependency_rank)) =
        (dependency_rank(package), dependency_rank(dependency))
    else {
        return false;
    };
    dependency_rank >= package_rank
}

fn is_frontend(package: &str) -> bool {
    matches!(
        package,
        "pageknot-cli" | "pageknot-node" | "pageknot-python"
    )
}

fn dependency_rank(package: &str) -> Option<u8> {
    match package {
        "pageknot-model" => Some(0),
        "pageknot-artifact" | "pageknot-capture" | "pageknot-document" | "pageknot-protocol" => {
            Some(1)
        }
        "pageknot-browser" | "pageknot-html" => Some(2),
        "pageknot-chromium" | "pageknot-export" | "pageknot-transform" => Some(3),
        "pageknot" => Some(4),
        "pageknot-cli" | "pageknot-node" | "pageknot-python" => Some(5),
        _ => None,
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
    path == Path::new("crates/pageknot-chromium/src/cdp_generated.rs")
        || path.starts_with("crates/pageknot-chromium/src/cdp/generated")
        || path.starts_with("collector/dist")
        || path.starts_with("crates/pageknot-chromium/generated")
        || path == Path::new("bindings/node/contracts.generated.d.ts")
        || path == Path::new("bindings/python/python/pageknot/_contracts.pyi")
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
            "pageknot-chromium",
            "pageknot-browser"
        ));
        assert!(!dependency_points_outward("pageknot", "pageknot-html"));
        assert!(!dependency_points_outward("xtask", "pageknot"));
    }

    #[test]
    fn dependency_direction_rejects_sideways_and_outward_edges() {
        assert!(dependency_points_outward(
            "pageknot-browser",
            "pageknot-html"
        ));
        assert!(dependency_points_outward("pageknot-document", "pageknot"));
        assert!(dependency_points_outward(
            "pageknot-chromium",
            "pageknot-test-support"
        ));
        assert!(dependency_points_outward(
            "pageknot-cli",
            "pageknot-artifact"
        ));
        assert!(!dependency_points_outward("pageknot-cli", "pageknot"));
    }

    #[test]
    fn size_exemptions_are_explicit() {
        assert!(is_generated(Path::new(
            "crates/pageknot-chromium/src/cdp_generated.rs"
        )));
        assert!(is_generated(Path::new(
            "bindings/python/python/pageknot/_contracts.pyi"
        )));
        assert!(is_test(Path::new(
            "crates/pageknot/tests/fixture_matrix.rs"
        )));
        assert!(is_test(Path::new(
            "crates/pageknot-cli/src/runner/tests.rs"
        )));
        assert!(!is_generated(Path::new("crates/pageknot/src/lib.rs")));
        assert!(!is_test(Path::new("crates/pageknot/src/lib.rs")));
        assert!(is_handwritten_production_source(Path::new(
            "crates/pageknot/src/lib.rs"
        )));
        assert!(!is_handwritten_production_source(Path::new("SPEC.md")));
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
