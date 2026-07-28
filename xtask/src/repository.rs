use std::fs;
use std::path::{Path, PathBuf};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
};

use serde::Deserialize;

use crate::package::{CRATE_LICENSE, PUBLISHABLE_CRATES};

const TEXT_EXTENSIONS: &[&str] = &[
    "cjs", "css", "html", "js", "json", "md", "py", "rs", "sh", "toml", "ts", "yml", "yaml",
];
const IGNORED_DIRECTORIES: &[&str] = &[
    ".git",
    ".mypy_cache",
    ".pytest_cache",
    ".venv",
    "node_modules",
    "target",
];

#[derive(Debug, Deserialize)]
struct VersionManifest {
    product: String,
    collector_protocol: String,
    rust: RustVersionManifest,
    chromium: ChromiumVersionManifest,
}

#[derive(Debug, Deserialize)]
struct RustVersionManifest {
    toolchain: String,
}

#[derive(Debug, Deserialize)]
struct ChromiumVersionManifest {
    managed_catalog: String,
    managed_revision: String,
    managed_version: String,
    managed_sha256: BTreeMap<String, String>,
}

pub fn check(root: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    collect_files(root, &mut files)?;
    files.sort();

    let mut violations = Vec::new();
    for path in &files {
        check_text_file(path, &mut violations)?;
        if path.extension().and_then(|value| value.to_str()) == Some("md") {
            check_markdown_links(path, &mut violations)?;
        }
        if is_workflow(root, path) {
            check_workflow_action_pins(path, &mut violations)?;
        }
    }
    check_scheduled_evidence_uploads(
        &root.join(".github/workflows/scheduled.yml"),
        &mut violations,
    )?;
    check_crate_package_metadata(root, &mut violations)?;
    check_version_alignment(root, &mut violations)?;
    check_binding_contract_wiring(root, &mut violations)?;

    if violations.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "repository checks failed:\n{}",
            violations
                .into_iter()
                .map(|violation| format!("- {violation}"))
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}

fn check_binding_contract_wiring(root: &Path, violations: &mut Vec<String>) -> Result<(), String> {
    let node_path = root.join("bindings/node/index.d.ts");
    let node = fs::read_to_string(&node_path)
        .map_err(|error| format!("failed to read {}: {error}", node_path.display()))?;
    for required in [
        "from \"./contracts.generated.js\"",
        "events(): AsyncIterableIterator<CaptureEvent>",
        "result(): Promise<CaptureResult>",
        "start(request: CaptureRequest): Promise<CaptureJob>",
        "batch(request: BatchRequest): Promise<BatchResult>",
        "crawl(request: CrawlRequest): Promise<CrawlResult>",
        "inspect(path: string): Promise<ArtifactManifest>",
        "Promise<VerificationResult>",
        "Promise<ArtifactExportResult>",
        "Promise<ArtifactVariantVerification>",
        "ensure(): Promise<BrowserInfo>",
    ] {
        if !node.contains(required) {
            violations.push(format!(
                "{} does not consume the generated binding contract `{required}`",
                node_path.display()
            ));
        }
    }

    let python_path = root.join("bindings/python/python/pageknot/__init__.pyi");
    let python = fs::read_to_string(&python_path)
        .map_err(|error| format!("failed to read {}: {error}", python_path.display()))?;
    for required in [
        "from ._contracts import (",
        "class CaptureEvents(AsyncIterator[_CaptureEvent])",
        "async def result(self) -> _CaptureResult",
        "request: _CaptureRequest",
        "request: _BatchRequest",
        "request: _CrawlRequest",
        ") -> _ArtifactManifest",
        ") -> _VerificationResult",
        ") -> _ArtifactExportResult",
        ") -> _ArtifactVariantVerification",
        "async def ensure(self) -> _BrowserInfo",
        ") -> _CaptureResult",
    ] {
        if !python.contains(required) {
            violations.push(format!(
                "{} does not consume the generated binding contract `{required}`",
                python_path.display()
            ));
        }
    }
    Ok(())
}

fn check_crate_package_metadata(root: &Path, violations: &mut Vec<String>) -> Result<(), String> {
    let workspace_path = root.join("Cargo.toml");
    let workspace = fs::read_to_string(&workspace_path)
        .map_err(|error| format!("failed to read {}: {error}", workspace_path.display()))?;
    let workspace: toml::Value =
        toml::from_str(&workspace).map_err(|error| format!("invalid Cargo.toml: {error}"))?;
    let workspace_table = workspace
        .get("workspace")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "Cargo.toml has no workspace table".to_owned())?;
    let package = workspace_table
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| "Cargo.toml has no workspace package table".to_owned())?;
    if package.get("license").and_then(toml::Value::as_str) != Some(CRATE_LICENSE) {
        violations.push(format!(
            "{} workspace license must be {CRATE_LICENSE}",
            workspace_path.display()
        ));
    }
    if package.get("license-file").is_some() {
        violations.push(format!(
            "{} must use SPDX license metadata without license-file",
            workspace_path.display()
        ));
    }
    let root_license_path = root.join("LICENSE");
    let root_license = fs::read(&root_license_path)
        .map_err(|error| format!("failed to read {}: {error}", root_license_path.display()))?;

    for name in PUBLISHABLE_CRATES {
        let path = root.join("crates").join(name).join("Cargo.toml");
        let manifest = fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let manifest: toml::Value = toml::from_str(&manifest)
            .map_err(|error| format!("invalid {}: {error}", path.display()))?;
        let package = manifest
            .get("package")
            .and_then(toml::Value::as_table)
            .ok_or_else(|| format!("{} has no package table", path.display()))?;
        if !inherits_workspace_value(package, "license") {
            violations.push(format!(
                "{} must inherit the workspace license",
                path.display()
            ));
        }
        if package.get("license-file").is_some() {
            violations.push(format!(
                "{} must use SPDX license metadata without license-file",
                path.display()
            ));
        }
        if package.get("publish").and_then(toml::Value::as_bool) == Some(false) {
            violations.push(format!("{} must remain publishable", path.display()));
        }
        let license_path = root.join("crates").join(name).join("LICENSE");
        let metadata = fs::symlink_metadata(&license_path)
            .map_err(|error| format!("failed to inspect {}: {error}", license_path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            violations.push(format!(
                "{} must be a directly addressed regular file",
                license_path.display()
            ));
        } else {
            let license = fs::read(&license_path)
                .map_err(|error| format!("failed to read {}: {error}", license_path.display()))?;
            if license != root_license {
                violations.push(format!(
                    "{} differs from the repository root LICENSE",
                    license_path.display()
                ));
            }
        }
    }

    let test_support_path = root.join("crates/pageknot-test-support/Cargo.toml");
    let test_support = fs::read_to_string(&test_support_path)
        .map_err(|error| format!("failed to read {}: {error}", test_support_path.display()))?;
    let test_support: toml::Value = toml::from_str(&test_support)
        .map_err(|error| format!("invalid {}: {error}", test_support_path.display()))?;
    if test_support
        .get("package")
        .and_then(|package| package.get("publish"))
        .and_then(toml::Value::as_bool)
        != Some(false)
    {
        violations.push(format!(
            "{} must set publish = false",
            test_support_path.display()
        ));
    }
    let test_support_member = workspace_table
        .get("members")
        .and_then(toml::Value::as_array)
        .is_some_and(|members| {
            members
                .iter()
                .any(|member| member.as_str() == Some("crates/pageknot-test-support"))
        });
    if !test_support_member {
        violations.push(
            "Cargo.toml must keep crates/pageknot-test-support as a workspace member".to_owned(),
        );
    }
    let test_support_dependency = workspace_table
        .get("dependencies")
        .and_then(|dependencies| dependencies.get("pageknot-test-support"))
        .and_then(toml::Value::as_table)
        .and_then(|dependency| dependency.get("path"))
        .and_then(toml::Value::as_str);
    if test_support_dependency != Some("crates/pageknot-test-support") {
        violations.push(
            "Cargo.toml must keep pageknot-test-support as a workspace path dependency".to_owned(),
        );
    }
    Ok(())
}

fn inherits_workspace_value(package: &toml::value::Table, key: &str) -> bool {
    package
        .get(key)
        .and_then(toml::Value::as_table)
        .and_then(|value| value.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true)
}

fn is_workflow(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| {
        relative.starts_with(".github/workflows")
            && matches!(
                relative.extension().and_then(|value| value.to_str()),
                Some("yml" | "yaml")
            )
    })
}

fn check_workflow_action_pins(path: &Path, violations: &mut Vec<String>) -> Result<(), String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        let action = line
            .strip_prefix("uses: ")
            .or_else(|| line.strip_prefix("- uses: "));
        let Some(action) = action else {
            continue;
        };
        if action.starts_with("./") || action.starts_with("docker://") {
            continue;
        }
        let pinned = action.rsplit_once('@').is_some_and(|(_, revision)| {
            revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
        if !pinned {
            violations.push(format!(
                "{}:{} does not pin its action to a full commit",
                path.display(),
                index + 1
            ));
        }
    }
    Ok(())
}

fn check_scheduled_evidence_uploads(
    path: &Path,
    violations: &mut Vec<String>,
) -> Result<(), String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    for (name, report_path) in [
        (
            "Upload differential evidence",
            "target/benchmark-evidence/singlefile-differential.json",
        ),
        (
            "Upload performance evidence",
            "target/benchmark-evidence/performance.json",
        ),
    ] {
        let Some(step) = workflow_named_step(&text, name) else {
            violations.push(format!("{} has no `{name}` step", path.display()));
            continue;
        };
        if !step.iter().any(|line| line.trim() == "if: always()") {
            violations.push(format!(
                "{} step `{name}` must run with `if: always()`",
                path.display()
            ));
        }
        if !step
            .iter()
            .any(|line| line.trim().starts_with("uses: actions/upload-artifact@"))
        {
            violations.push(format!(
                "{} step `{name}` must use `actions/upload-artifact`",
                path.display()
            ));
        }
        if !step
            .iter()
            .any(|line| line.trim() == format!("path: {report_path}"))
        {
            violations.push(format!(
                "{} step `{name}` must upload `{report_path}`",
                path.display()
            ));
        }
        if !step
            .iter()
            .any(|line| line.trim() == "if-no-files-found: error")
        {
            violations.push(format!(
                "{} step `{name}` must fail when its report is missing",
                path.display()
            ));
        }
    }
    Ok(())
}

fn workflow_named_step<'a>(text: &'a str, name: &str) -> Option<Vec<&'a str>> {
    let marker = format!("- name: {name}");
    let mut lines = text.lines();
    let first_line = lines.find(|line| line.trim() == marker)?;
    let indentation = first_line
        .len()
        .saturating_sub(first_line.trim_start().len());
    let mut step = vec![first_line];
    for line in lines {
        let next_indentation = line.len().saturating_sub(line.trim_start().len());
        if !line.trim().is_empty() && next_indentation <= indentation {
            break;
        }
        step.push(line);
    }
    Some(step)
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("failed to read {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "failed to read an entry under {}: {error}",
                directory.display()
            )
        })?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let name = entry.file_name();
            if !IGNORED_DIRECTORIES
                .iter()
                .any(|ignored| name == std::ffi::OsStr::new(ignored))
            {
                collect_files(&path, files)?;
            }
            continue;
        }
        let extension = path.extension().and_then(|value| value.to_str());
        if extension.is_some_and(|extension| TEXT_EXTENSIONS.contains(&extension))
            || path.file_name().and_then(|value| value.to_str()) == Some("justfile")
        {
            files.push(path);
        }
    }
    Ok(())
}

fn check_text_file(path: &Path, violations: &mut Vec<String>) -> Result<(), String> {
    let bytes =
        fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|error| format!("{} is not UTF-8: {error}", path.display()))?;
    if text.contains('\r') {
        violations.push(format!("{} contains a carriage return", path.display()));
    }
    if text.contains('\u{2014}') {
        violations.push(format!("{} contains an em dash", path.display()));
    }
    if !text.is_empty() && !text.ends_with('\n') {
        violations.push(format!("{} has no final newline", path.display()));
    }
    for (index, line) in text.lines().enumerate() {
        if line.ends_with(' ') || line.ends_with('\t') {
            violations.push(format!(
                "{}:{} has trailing whitespace",
                path.display(),
                index + 1
            ));
        }
    }
    Ok(())
}

fn check_markdown_links(path: &Path, violations: &mut Vec<String>) -> Result<(), String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let mut rest = text.as_str();
    while let Some(marker) = rest.find("](") {
        rest = &rest[marker + 2..];
        let Some(end) = rest.find(')') else {
            break;
        };
        let raw_target = rest[..end].trim();
        rest = &rest[end + 1..];
        let target = raw_target
            .split_once('#')
            .map_or(raw_target, |(target, _)| target)
            .trim_start_matches('<')
            .trim_end_matches('>');
        if target.is_empty()
            || target.starts_with('#')
            || target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with("mailto:")
        {
            continue;
        }
        let Some(parent) = path.parent() else {
            continue;
        };
        if !parent.join(target).exists() {
            violations.push(format!(
                "{} references missing local target `{raw_target}`",
                path.display()
            ));
        }
    }
    Ok(())
}

fn check_version_alignment(root: &Path, violations: &mut Vec<String>) -> Result<(), String> {
    let versions = fs::read_to_string(root.join("versions.toml"))
        .map_err(|error| format!("failed to read versions.toml: {error}"))?;
    let versions: VersionManifest =
        toml::from_str(&versions).map_err(|error| format!("invalid versions.toml: {error}"))?;
    if versions.collector_protocol != pageknot_protocol::COLLECTOR_PROTOCOL_VERSION_STRING {
        violations.push(format!(
            "collector protocol {} differs from host protocol {}",
            versions.collector_protocol,
            pageknot_protocol::COLLECTOR_PROTOCOL_VERSION_STRING,
        ));
    }
    check_collector_protocol_source(root, &versions.collector_protocol, violations)?;
    check_managed_browser_versions(&versions, violations);

    let rust_toolchain = fs::read_to_string(root.join("rust-toolchain.toml"))
        .map_err(|error| format!("failed to read rust-toolchain.toml: {error}"))?;
    let rust_toolchain: toml::Value = toml::from_str(&rust_toolchain)
        .map_err(|error| format!("invalid rust-toolchain.toml: {error}"))?;
    let toolchain_channel = rust_toolchain
        .get("toolchain")
        .and_then(|toolchain| toolchain.get("channel"))
        .and_then(toml::Value::as_str);
    if toolchain_channel != Some(versions.rust.toolchain.as_str()) {
        violations.push(format!(
            "Rust toolchain channel {:?} differs from version manifest toolchain {}",
            toolchain_channel, versions.rust.toolchain
        ));
    }

    let cargo = fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|error| format!("failed to read Cargo.toml: {error}"))?;
    let cargo: toml::Value =
        toml::from_str(&cargo).map_err(|error| format!("invalid Cargo.toml: {error}"))?;
    let cargo_version = cargo
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str);
    if cargo_version != Some(versions.product.as_str()) {
        violations.push(format!(
            "Cargo workspace version {:?} differs from product version {}",
            cargo_version, versions.product
        ));
    }

    let node = fs::read_to_string(root.join("bindings/node/package.json"))
        .map_err(|error| format!("failed to read Node.js package metadata: {error}"))?;
    let node: serde_json::Value = serde_json::from_str(&node)
        .map_err(|error| format!("invalid Node.js package metadata: {error}"))?;
    let node_version = node.get("version").and_then(serde_json::Value::as_str);
    if node_version != Some(versions.product.as_str()) {
        violations.push(format!(
            "Node.js version {:?} differs from product version {}",
            node_version, versions.product
        ));
    }
    let optional_dependencies = node
        .get("optionalDependencies")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "Node.js package metadata has no optionalDependencies object".to_owned())?;
    let expected_platform_packages = optional_dependencies
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    for (name, version) in optional_dependencies {
        if version.as_str() != Some(versions.product.as_str()) {
            violations.push(format!(
                "Node.js optional dependency {name} version {:?} differs from product version {}",
                version.as_str(),
                versions.product
            ));
        }
    }

    let platform_root = root.join("bindings/node/npm");
    let entries = fs::read_dir(&platform_root).map_err(|error| {
        format!(
            "failed to read Node.js platform packages under {}: {error}",
            platform_root.display()
        )
    })?;
    let mut platform_manifests = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!(
                "failed to read a Node.js platform package under {}: {error}",
                platform_root.display()
            )
        })?;
        let manifest = entry.path().join("package.json");
        if manifest.is_file() {
            platform_manifests.push(manifest);
        }
    }
    platform_manifests.sort();
    let mut actual_platform_packages = BTreeSet::new();
    for manifest in &platform_manifests {
        let package = fs::read_to_string(manifest)
            .map_err(|error| format!("failed to read {}: {error}", manifest.display()))?;
        let package: serde_json::Value = serde_json::from_str(&package)
            .map_err(|error| format!("invalid {}: {error}", manifest.display()))?;
        let name = package
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("{} has no package name", manifest.display()))?;
        let version = package.get("version").and_then(serde_json::Value::as_str);
        actual_platform_packages.insert(name.to_owned());
        if version != Some(versions.product.as_str()) {
            violations.push(format!(
                "Node.js platform package {name} version {:?} differs from product version {}",
                version, versions.product
            ));
        }
    }
    if actual_platform_packages != expected_platform_packages {
        violations.push(format!(
            "Node.js platform packages {:?} differ from root optional dependencies {:?}",
            actual_platform_packages, expected_platform_packages
        ));
    }

    let python = fs::read_to_string(root.join("bindings/python/pyproject.toml"))
        .map_err(|error| format!("failed to read Python package metadata: {error}"))?;
    let python: toml::Value =
        toml::from_str(&python).map_err(|error| format!("invalid pyproject.toml: {error}"))?;
    let python_version = python
        .get("project")
        .and_then(|project| project.get("version"))
        .and_then(toml::Value::as_str);
    if python_version != Some(versions.product.as_str()) {
        violations.push(format!(
            "Python version {:?} differs from product version {}",
            python_version, versions.product
        ));
    }
    if let Ok(tag) = env::var("GITHUB_REF_NAME")
        && let Some(tag_version) = release_tag_version(&tag)
        && tag_version != versions.product
    {
        violations.push(format!(
            "release tag {tag} differs from product version {}",
            versions.product
        ));
    }
    Ok(())
}

fn check_collector_protocol_source(
    root: &Path,
    protocol_version: &str,
    violations: &mut Vec<String>,
) -> Result<(), String> {
    let (major, minor) = protocol_version
        .split_once('.')
        .ok_or_else(|| format!("collector protocol `{protocol_version}` is not major.minor"))?;
    if major.parse::<u32>().is_err() || minor.parse::<u32>().is_err() {
        return Err(format!(
            "collector protocol `{protocol_version}` is not numeric"
        ));
    }
    let expected =
        format!("export const protocol = {{ major: {major}, minor: {minor} }} as const;");
    let collector = fs::read_to_string(root.join("collector/src/constants.ts"))
        .map_err(|error| format!("failed to read collector protocol source: {error}"))?;
    if !collector.lines().any(|line| line.trim() == expected) {
        violations.push(format!(
            "collector/src/constants.ts does not declare protocol {protocol_version}"
        ));
    }
    Ok(())
}

fn check_managed_browser_versions(versions: &VersionManifest, violations: &mut Vec<String>) {
    let chromium = &versions.chromium;
    if chromium.managed_catalog != pageknot_chromium::MANAGED_BROWSER_CATALOG_VERSION {
        violations.push(format!(
            "managed browser catalog {} differs from runtime catalog {}",
            chromium.managed_catalog,
            pageknot_chromium::MANAGED_BROWSER_CATALOG_VERSION
        ));
    }
    if chromium.managed_revision != pageknot_chromium::DEFAULT_MANAGED_BROWSER_REVISION {
        violations.push(format!(
            "managed browser revision {} differs from runtime revision {}",
            chromium.managed_revision,
            pageknot_chromium::DEFAULT_MANAGED_BROWSER_REVISION
        ));
    }
    let mut runtime_digests = BTreeMap::new();
    for entry in pageknot_chromium::managed_browser_catalog() {
        if entry.revision != chromium.managed_revision {
            violations.push(format!(
                "managed browser {} revision {} differs from version manifest revision {}",
                entry.platform, entry.revision, chromium.managed_revision
            ));
        }
        if entry.version != chromium.managed_version {
            violations.push(format!(
                "managed browser {} version {} differs from version manifest version {}",
                entry.platform, entry.version, chromium.managed_version
            ));
        }
        let platform = entry.platform.replace('-', "_");
        if runtime_digests
            .insert(platform.clone(), entry.archive_sha256.to_owned())
            .is_some()
        {
            violations.push(format!(
                "managed browser catalog repeats platform {platform}"
            ));
        }
    }
    if runtime_digests != chromium.managed_sha256 {
        violations.push(format!(
            "managed browser digests {:?} differ from runtime catalog {:?}",
            chromium.managed_sha256, runtime_digests
        ));
    }
    if chrono::NaiveDate::parse_from_str(&chromium.managed_catalog, "%Y-%m-%d").is_err() {
        violations.push(format!(
            "managed browser catalog `{}` is not a calendar date",
            chromium.managed_catalog
        ));
    }
    if chromium.managed_revision.is_empty()
        || !chromium
            .managed_revision
            .bytes()
            .all(|byte| byte.is_ascii_digit())
    {
        violations.push(format!(
            "managed browser revision `{}` is not numeric",
            chromium.managed_revision
        ));
    }
    if chromium.managed_version.is_empty()
        || !chromium
            .managed_version
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        violations.push(format!(
            "managed browser version `{}` is invalid",
            chromium.managed_version
        ));
    }
    for (platform, digest) in &chromium.managed_sha256 {
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            violations.push(format!(
                "managed browser digest for {platform} is not lowercase SHA-256"
            ));
        }
    }
}

fn release_tag_version(tag: &str) -> Option<&str> {
    tag.strip_prefix('v').filter(|version| !version.is_empty())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{
        check_crate_package_metadata, check_scheduled_evidence_uploads, check_workflow_action_pins,
        release_tag_version,
    };
    use crate::package::PUBLISHABLE_CRATES;

    #[test]
    fn crate_packages_use_agpl_metadata_and_root_license_bytes() -> Result<(), String> {
        let temporary = TempDir::new().map_err(|error| error.to_string())?;
        fs::write(
            temporary.path().join("Cargo.toml"),
            r#"[workspace]
members = ["crates/pageknot-test-support"]

[workspace.package]
license = "AGPL-3.0-or-later"

[workspace.dependencies]
pageknot-test-support = { path = "crates/pageknot-test-support" }
"#,
        )
        .map_err(|error| error.to_string())?;
        fs::write(temporary.path().join("LICENSE"), b"canonical license\n")
            .map_err(|error| error.to_string())?;
        for name in PUBLISHABLE_CRATES {
            let directory = temporary.path().join("crates").join(name);
            fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
            fs::write(
                directory.join("Cargo.toml"),
                format!("[package]\nname = \"{name}\"\nlicense.workspace = true\n"),
            )
            .map_err(|error| error.to_string())?;
            fs::write(directory.join("LICENSE"), b"canonical license\n")
                .map_err(|error| error.to_string())?;
        }
        let test_support = temporary.path().join("crates/pageknot-test-support");
        fs::create_dir_all(&test_support).map_err(|error| error.to_string())?;
        fs::write(
            test_support.join("Cargo.toml"),
            "[package]\nname = \"pageknot-test-support\"\npublish = false\n",
        )
        .map_err(|error| error.to_string())?;

        let mut violations = Vec::new();
        check_crate_package_metadata(temporary.path(), &mut violations)?;
        assert!(violations.is_empty(), "{violations:?}");

        fs::write(
            temporary.path().join("crates/pageknot-model/LICENSE"),
            b"different license\n",
        )
        .map_err(|error| error.to_string())?;
        check_crate_package_metadata(temporary.path(), &mut violations)?;
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("differs from the repository root LICENSE"));

        fs::write(
            temporary.path().join("crates/pageknot-model/LICENSE"),
            b"canonical license\n",
        )
        .map_err(|error| error.to_string())?;
        fs::write(
            test_support.join("Cargo.toml"),
            "[package]\nname = \"pageknot-test-support\"\n",
        )
        .map_err(|error| error.to_string())?;
        violations.clear();
        check_crate_package_metadata(temporary.path(), &mut violations)?;
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("publish = false"));

        fs::write(
            test_support.join("Cargo.toml"),
            "[package]\nname = \"pageknot-test-support\"\npublish = false\n",
        )
        .map_err(|error| error.to_string())?;
        fs::write(
            temporary
                .path()
                .join("crates/pageknot-model/Cargo.toml"),
            "[package]\nname = \"pageknot-model\"\nlicense.workspace = true\nlicense-file.workspace = true\n",
        )
        .map_err(|error| error.to_string())?;
        violations.clear();
        check_crate_package_metadata(temporary.path(), &mut violations)?;
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("without license-file"));
        Ok(())
    }

    #[test]
    fn workflow_actions_require_full_commit_revisions() -> Result<(), String> {
        let temporary = TempDir::new().map_err(|error| error.to_string())?;
        let workflow = temporary.path().join("ci.yml");
        fs::write(
            &workflow,
            "steps:\n  - uses: actions/checkout@v4\n  - uses: ./local-action\n",
        )
        .map_err(|error| error.to_string())?;
        let mut violations = Vec::new();
        check_workflow_action_pins(&workflow, &mut violations)?;
        if violations.len() != 1 {
            return Err(format!(
                "expected one unpinned action violation, found {}",
                violations.len()
            ));
        }
        Ok(())
    }

    #[test]
    fn scheduled_evidence_uploads_survive_producer_failures() -> Result<(), String> {
        let temporary = TempDir::new().map_err(|error| error.to_string())?;
        let workflow = temporary.path().join("scheduled.yml");
        let valid = r#"steps:
  - name: Upload differential evidence
    if: always()
    uses: actions/upload-artifact@revision
    with:
      path: target/benchmark-evidence/singlefile-differential.json
      if-no-files-found: error
  - name: Upload performance evidence
    if: always()
    uses: actions/upload-artifact@revision
    with:
      path: target/benchmark-evidence/performance.json
      if-no-files-found: error
"#;
        fs::write(&workflow, valid).map_err(|error| error.to_string())?;
        let mut violations = Vec::new();
        check_scheduled_evidence_uploads(&workflow, &mut violations)?;
        assert!(violations.is_empty(), "{violations:?}");

        let invalid = valid.replacen("    if: always()\n", "", 1).replacen(
            "      if-no-files-found: error\n",
            "",
            1,
        );
        fs::write(&workflow, invalid).map_err(|error| error.to_string())?;
        check_scheduled_evidence_uploads(&workflow, &mut violations)?;
        assert_eq!(violations.len(), 2);
        assert!(violations[0].contains("if: always()"));
        assert!(violations[1].contains("report is missing"));
        Ok(())
    }

    #[test]
    fn release_tag_version_requires_the_v_prefix() {
        assert_eq!(release_tag_version("v1.2.3"), Some("1.2.3"));
        assert_eq!(release_tag_version("main"), None);
        assert_eq!(release_tag_version("v"), None);
    }
}
