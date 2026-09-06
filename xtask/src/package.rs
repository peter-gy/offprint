use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use clap::CommandFactory as _;
use clap_complete::{Shell, generate};
use flate2::Compression;
use flate2::GzBuilder;
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tar::{Builder as TarBuilder, Header as TarHeader};
use tempfile::TempDir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

const SUPPORTED_TARGETS: &[&str] = &[
    "x86_64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
];
pub(crate) const PUBLISHABLE_CRATES: &[&str] = &[
    "offprint",
    "offprint-artifact",
    "offprint-browser",
    "offprint-capture",
    "offprint-chromium",
    "offprint-cli",
    "offprint-document",
    "offprint-export",
    "offprint-html",
    "offprint-model",
    "offprint-protocol",
    "offprint-transform",
];
pub(crate) const CRATE_LICENSE: &str = "AGPL-3.0-or-later";
const MAXIMUM_CRATE_METADATA_BYTES: u64 = 1024 * 1024;
const MAXIMUM_CRATE_ENTRIES: usize = 100_000;
const MAXIMUM_CRATE_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;

struct VerifiedCrate {
    name: String,
    archive: PathBuf,
    root: PathBuf,
}

#[derive(Debug, Deserialize)]
struct Versions {
    product: String,
    artifact_format: u32,
    public_schema: u32,
    collector_protocol: String,
    chromium: ChromiumVersions,
}

#[derive(Debug, Deserialize)]
struct ChromiumVersions {
    cdp_revision: String,
    managed_revision: String,
    managed_version: String,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct BuildMetadata {
    schema_version: u32,
    product: String,
    version: String,
    target: String,
    source_revision: String,
    rustc: String,
    artifact_format: u32,
    public_schema: u32,
    collector_protocol: String,
    cdp_revision: String,
    managed_browser_revision: String,
    managed_browser_version: String,
}

pub fn verify_metadata(
    root: &Path,
    metadata_path: &Path,
    target: &str,
    source_revision: &str,
) -> Result<(), String> {
    let file_metadata = fs::symlink_metadata(metadata_path)
        .map_err(|error| format!("failed to inspect {}: {error}", metadata_path.display()))?;
    if file_metadata.file_type().is_symlink() || !file_metadata.is_file() {
        return Err(format!(
            "release metadata must be a directly addressed regular file: {}",
            metadata_path.display()
        ));
    }
    let contents = fs::read(metadata_path)
        .map_err(|error| format!("failed to read {}: {error}", metadata_path.display()))?;
    let actual: BuildMetadata = serde_json::from_slice(&contents)
        .map_err(|error| format!("invalid {}: {error}", metadata_path.display()))?;
    let versions = read_versions(root)?;
    let expected = BuildMetadata {
        schema_version: 1,
        product: "offprint".to_owned(),
        version: versions.product,
        target: target.to_owned(),
        source_revision: source_revision.to_owned(),
        rustc: actual.rustc.clone(),
        artifact_format: versions.artifact_format,
        public_schema: versions.public_schema,
        collector_protocol: versions.collector_protocol,
        cdp_revision: versions.chromium.cdp_revision,
        managed_browser_revision: versions.chromium.managed_revision,
        managed_browser_version: versions.chromium.managed_version,
    };
    if actual.rustc.trim().is_empty() {
        return Err("release metadata has no Rust compiler identity".to_owned());
    }
    if actual != expected {
        return Err(format!(
            "release metadata differs from the tagged source: expected {expected:?}, found {actual:?}"
        ));
    }
    Ok(())
}

pub fn verify_crate_packages(root: &Path, directory: &Path) -> Result<(), String> {
    let root_license = fs::read(root.join("LICENSE"))
        .map_err(|error| format!("failed to read root LICENSE: {error}"))?;
    let version = workspace_package_version(root)?;
    let mut archives = Vec::new();
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("failed to read {}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| {
            format!(
                "failed to read a package archive under {}: {error}",
                directory.display()
            )
        })?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("crate") {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(format!(
                "crate package must be a directly addressed regular file: {}",
                path.display()
            ));
        }
        archives.push(path);
    }
    archives.sort();

    let mut missing = PUBLISHABLE_CRATES.iter().copied().collect::<BTreeSet<_>>();
    let mut verified = Vec::with_capacity(archives.len());
    for archive in &archives {
        let package = verify_crate_archive(archive, &version, &root_license)?;
        if !missing.remove(package.name.as_str()) {
            return Err(format!(
                "{} is an unexpected or duplicate crate package",
                archive.display()
            ));
        }
        verified.push(package);
    }
    if !missing.is_empty() {
        return Err(format!(
            "crate packages are missing for {}",
            missing.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }
    verify_packaged_workspace(&verified)
}

fn verify_crate_archive(
    path: &Path,
    workspace_version: &str,
    root_license: &[u8],
) -> Result<VerifiedCrate, String> {
    let file =
        File::open(path).map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let mut manifest = None;
    let mut license = None;
    for entry in archive
        .entries()
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?
    {
        let mut entry =
            entry.map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let entry_path = entry
            .path()
            .map_err(|error| format!("invalid path in {}: {error}", path.display()))?
            .into_owned();
        let mut components = entry_path.components();
        let Some(std::path::Component::Normal(archive_root)) = components.next() else {
            continue;
        };
        let Some(std::path::Component::Normal(file_name)) = components.next() else {
            continue;
        };
        if components.next().is_some() {
            continue;
        }
        let destination = if file_name == "Cargo.toml" {
            &mut manifest
        } else if file_name == "LICENSE" {
            &mut license
        } else {
            continue;
        };
        if destination.is_some() {
            return Err(format!(
                "{} contains duplicate {} entries",
                path.display(),
                file_name.to_string_lossy()
            ));
        }
        if !entry.header().entry_type().is_file() {
            return Err(format!(
                "{} contains a non-file {} entry",
                path.display(),
                file_name.to_string_lossy()
            ));
        }
        let bytes = read_bounded_entry(
            &mut entry,
            MAXIMUM_CRATE_METADATA_BYTES,
            path,
            file_name.to_string_lossy().as_ref(),
        )?;
        *destination = Some((archive_root.to_owned(), bytes));
    }

    let (manifest_root, manifest) =
        manifest.ok_or_else(|| format!("{} has no root Cargo.toml", path.display()))?;
    let (license_root, license) =
        license.ok_or_else(|| format!("{} has no root LICENSE", path.display()))?;
    if manifest_root != license_root {
        return Err(format!(
            "{} stores Cargo.toml and LICENSE under different archive roots",
            path.display()
        ));
    }
    if license != root_license {
        return Err(format!(
            "{} LICENSE differs from the repository root",
            path.display()
        ));
    }

    let manifest: toml::Value = toml::from_slice(&manifest)
        .map_err(|error| format!("invalid Cargo.toml in {}: {error}", path.display()))?;
    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("{} Cargo.toml has no package table", path.display()))?;
    let name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("{} Cargo.toml has no package name", path.display()))?;
    let version = package
        .get("version")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("{} Cargo.toml has no package version", path.display()))?;
    if version != workspace_version {
        return Err(format!(
            "{} package version {version} differs from workspace version {workspace_version}",
            path.display()
        ));
    }
    if package.get("license").and_then(toml::Value::as_str) != Some(CRATE_LICENSE) {
        return Err(format!(
            "{} package license must be {CRATE_LICENSE}",
            path.display()
        ));
    }
    if package.get("license-file").is_some() {
        return Err(format!(
            "{} package must use SPDX license metadata without license-file",
            path.display()
        ));
    }
    let expected_root = format!("{name}-{version}");
    if manifest_root != std::ffi::OsStr::new(&expected_root) {
        return Err(format!(
            "{} archive root must be {expected_root}",
            path.display()
        ));
    }
    Ok(VerifiedCrate {
        name: name.to_owned(),
        archive: path.to_owned(),
        root: PathBuf::from(expected_root),
    })
}

fn verify_packaged_workspace(packages: &[VerifiedCrate]) -> Result<(), String> {
    let staging = TempDir::new()
        .map_err(|error| format!("failed to create crate verification directory: {error}"))?;
    for package in packages {
        unpack_crate(package, staging.path())?;
    }
    let manifest = verification_workspace_manifest(packages)?;
    fs::write(staging.path().join("Cargo.toml"), manifest)
        .map_err(|error| format!("failed to write crate verification workspace: {error}"))?;

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let lock = Command::new(&cargo)
        .current_dir(staging.path())
        .arg("generate-lockfile")
        .output()
        .map_err(|error| format!("failed to resolve packaged crates: {error}"))?;
    if !lock.status.success() {
        return Err(format!(
            "packaged crate workspace failed to resolve with status {}: {}",
            lock.status,
            String::from_utf8_lossy(&lock.stderr).trim()
        ));
    }
    let mut command = Command::new(cargo);
    command.current_dir(staging.path()).args([
        "check",
        "--workspace",
        "--lib",
        "--bins",
        "--examples",
        "--locked",
    ]);
    let output = command
        .output()
        .map_err(|error| format!("failed to build packaged crates: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "packaged crate workspace failed to build with status {}: {}",
            output.status,
            stderr.trim()
        ))
    }
}

fn unpack_crate(package: &VerifiedCrate, destination: &Path) -> Result<(), String> {
    let file = File::open(&package.archive)
        .map_err(|error| format!("failed to open {}: {error}", package.archive.display()))?;
    let mut archive = tar::Archive::new(GzDecoder::new(file));
    let mut entries = 0_usize;
    let mut unpacked_bytes = 0_u64;
    for entry in archive
        .entries()
        .map_err(|error| format!("failed to read {}: {error}", package.archive.display()))?
    {
        let mut entry = entry
            .map_err(|error| format!("failed to read {}: {error}", package.archive.display()))?;
        entries = entries.saturating_add(1);
        if entries > MAXIMUM_CRATE_ENTRIES {
            return Err(format!(
                "{} exceeds {MAXIMUM_CRATE_ENTRIES} entries",
                package.archive.display()
            ));
        }
        let path = entry
            .path()
            .map_err(|error| format!("invalid path in {}: {error}", package.archive.display()))?
            .into_owned();
        let mut components = path.components();
        if components.next() != Some(std::path::Component::Normal(package.root.as_os_str()))
            || components.any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(format!(
                "{} contains an entry outside {}",
                package.archive.display(),
                package.root.display()
            ));
        }
        let entry_type = entry.header().entry_type();
        if !entry_type.is_file() && !entry_type.is_dir() {
            return Err(format!(
                "{} contains a link or special entry at {}",
                package.archive.display(),
                path.display()
            ));
        }
        if entry_type.is_file() {
            unpacked_bytes =
                unpacked_bytes.saturating_add(entry.header().size().map_err(|error| {
                    format!(
                        "invalid entry size in {} at {}: {error}",
                        package.archive.display(),
                        path.display()
                    )
                })?);
            if unpacked_bytes > MAXIMUM_CRATE_UNPACKED_BYTES {
                return Err(format!(
                    "{} exceeds {MAXIMUM_CRATE_UNPACKED_BYTES} unpacked bytes",
                    package.archive.display()
                ));
            }
        }
        if !entry.unpack_in(destination).map_err(|error| {
            format!(
                "failed to unpack {} from {}: {error}",
                path.display(),
                package.archive.display()
            )
        })? {
            return Err(format!(
                "{} contains an entry outside the verification directory",
                package.archive.display()
            ));
        }
    }
    Ok(())
}

fn verification_workspace_manifest(packages: &[VerifiedCrate]) -> Result<String, String> {
    let members = packages
        .iter()
        .map(|package| toml::Value::String(portable_name(&package.root)))
        .collect();
    let mut workspace = toml::Table::new();
    workspace.insert("resolver".to_owned(), toml::Value::String("3".to_owned()));
    workspace.insert("members".to_owned(), toml::Value::Array(members));

    let mut crates_io = toml::Table::new();
    for package in packages {
        let mut dependency = toml::Table::new();
        dependency.insert(
            "path".to_owned(),
            toml::Value::String(portable_name(&package.root)),
        );
        crates_io.insert(package.name.clone(), toml::Value::Table(dependency));
    }
    let mut patch = toml::Table::new();
    patch.insert("crates-io".to_owned(), toml::Value::Table(crates_io));
    let mut manifest = toml::Table::new();
    manifest.insert("workspace".to_owned(), toml::Value::Table(workspace));
    manifest.insert("patch".to_owned(), toml::Value::Table(patch));
    toml::to_string(&toml::Value::Table(manifest))
        .map_err(|error| format!("failed to encode crate verification workspace: {error}"))
}

fn read_bounded_entry(
    reader: &mut impl Read,
    maximum_bytes: u64,
    archive: &Path,
    entry: &str,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(maximum_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read {entry} from {}: {error}", archive.display()))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > maximum_bytes {
        return Err(format!(
            "{entry} in {} exceeds {maximum_bytes} bytes",
            archive.display()
        ));
    }
    Ok(bytes)
}

fn workspace_package_version(root: &Path) -> Result<String, String> {
    let path = root.join("Cargo.toml");
    let manifest = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let manifest: toml::Value =
        toml::from_str(&manifest).map_err(|error| format!("invalid Cargo.toml: {error}"))?;
    manifest
        .get("workspace")
        .and_then(|workspace| workspace.get("package"))
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "workspace package version is missing".to_owned())
}

pub fn create(root: &Path, target: &str, binary: &Path, output: &Path) -> Result<(), String> {
    if !SUPPORTED_TARGETS.contains(&target) {
        return Err(format!("unsupported release target `{target}`"));
    }
    let binary_metadata = fs::symlink_metadata(binary)
        .map_err(|error| format!("failed to inspect {}: {error}", binary.display()))?;
    if binary_metadata.file_type().is_symlink() || !binary_metadata.is_file() {
        return Err(format!(
            "release binary must be a directly addressed regular file: {}",
            binary.display()
        ));
    }
    fs::create_dir_all(output)
        .map_err(|error| format!("failed to create {}: {error}", output.display()))?;

    let versions = read_versions(root)?;
    let archive_root = format!("offprint-{}-{target}", versions.product);
    let staging = TempDir::new()
        .map_err(|error| format!("failed to create release staging directory: {error}"))?;
    let staged_root = staging.path().join(&archive_root);
    fs::create_dir(&staged_root)
        .map_err(|error| format!("failed to create release root: {error}"))?;

    let binary_name = if target.contains("windows") {
        "offprint.exe"
    } else {
        "offprint"
    };
    copy_regular(binary, &staged_root.join(binary_name))?;
    copy_regular(&root.join("README.md"), &staged_root.join("README.md"))?;
    copy_regular(&root.join("LICENSE"), &staged_root.join("LICENSE"))?;
    let metadata = build_metadata(root, target, &versions)?;
    let mut metadata_bytes =
        serde_json::to_vec_pretty(&metadata).map_err(|error| error.to_string())?;
    metadata_bytes.push(b'\n');
    fs::write(staged_root.join("build-metadata.json"), metadata_bytes)
        .map_err(|error| format!("failed to write release metadata: {error}"))?;
    write_completions(&staged_root)?;

    let mut checksums = String::new();
    for name in release_file_names(&staged_root)? {
        let digest = digest_file(&staged_root.join(&name))?;
        checksums.push_str(&format!("{digest}  {}\n", portable_name(&name)));
    }
    fs::write(staged_root.join("SHA256SUMS"), checksums)
        .map_err(|error| format!("failed to write release checksums: {error}"))?;

    let epoch = source_date_epoch()?;
    let archive = if target.contains("windows") {
        let path = output.join(format!("{archive_root}.zip"));
        create_zip(&path, &staged_root, &archive_root)?;
        path
    } else {
        let path = output.join(format!("{archive_root}.tar.gz"));
        create_tar_gz(&path, &staged_root, &archive_root, epoch)?;
        path
    };
    let archive_digest = digest_file(&archive)?;
    let digest_path = PathBuf::from(format!("{}.sha256", archive.display()));
    let archive_name = archive
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "release archive name is not UTF-8".to_owned())?;
    fs::write(&digest_path, format!("{archive_digest}  {archive_name}\n"))
        .map_err(|error| format!("failed to write {}: {error}", digest_path.display()))
}

fn read_versions(root: &Path) -> Result<Versions, String> {
    let contents = fs::read_to_string(root.join("versions.toml"))
        .map_err(|error| format!("failed to read versions.toml: {error}"))?;
    let versions: Versions =
        toml::from_str(&contents).map_err(|error| format!("invalid versions.toml: {error}"))?;
    if versions.artifact_format != offprint_model::ARTIFACT_FORMAT_VERSION
        || versions.public_schema != offprint_model::PUBLIC_SCHEMA_VERSION
    {
        return Err(format!(
            "versions.toml public contract {}.{} differs from model {}.{}",
            versions.artifact_format,
            versions.public_schema,
            offprint_model::ARTIFACT_FORMAT_VERSION,
            offprint_model::PUBLIC_SCHEMA_VERSION,
        ));
    }
    if versions.collector_protocol != offprint_protocol::COLLECTOR_PROTOCOL_VERSION_STRING {
        return Err(format!(
            "versions.toml collector protocol {} differs from host protocol {}",
            versions.collector_protocol,
            offprint_protocol::COLLECTOR_PROTOCOL_VERSION_STRING,
        ));
    }
    Ok(versions)
}

fn build_metadata(root: &Path, target: &str, versions: &Versions) -> Result<BuildMetadata, String> {
    Ok(BuildMetadata {
        schema_version: 1,
        product: "offprint".to_owned(),
        version: versions.product.clone(),
        target: target.to_owned(),
        source_revision: command_output(root, "git", &["rev-parse", "HEAD"])
            .unwrap_or_else(|_| "working-tree".to_owned()),
        rustc: command_output(root, "rustc", &["--version", "--verbose"])?,
        artifact_format: versions.artifact_format,
        public_schema: versions.public_schema,
        collector_protocol: versions.collector_protocol.clone(),
        cdp_revision: versions.chromium.cdp_revision.clone(),
        managed_browser_revision: versions.chromium.managed_revision.clone(),
        managed_browser_version: versions.chromium.managed_version.clone(),
    })
}

fn command_output(root: &Path, program: &str, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .current_dir(root)
        .args(arguments)
        .output()
        .map_err(|error| format!("failed to start {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{program} exited with {}", output.status));
    }
    String::from_utf8(output.stdout)
        .map(|output| output.trim().to_owned())
        .map_err(|error| format!("{program} produced non-UTF-8 output: {error}"))
}

fn copy_regular(source: &Path, destination: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("failed to inspect {}: {error}", source.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "release input must be a directly addressed regular file: {}",
            source.display()
        ));
    }
    fs::copy(source, destination).map(|_| ()).map_err(|error| {
        format!(
            "failed to copy {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })
}

fn create_tar_gz(
    destination: &Path,
    root: &Path,
    archive_root: &str,
    epoch: u64,
) -> Result<(), String> {
    let output = File::create(destination)
        .map_err(|error| format!("failed to create {}: {error}", destination.display()))?;
    let gzip = GzBuilder::new()
        .mtime(u32::try_from(epoch).unwrap_or(u32::MAX))
        .write(output, Compression::best());
    let mut archive = TarBuilder::new(gzip);
    for name in release_file_names(root)? {
        let source = root.join(&name);
        let archive_name = Path::new(archive_root).join(&name);
        append_tar_file(&mut archive, &source, &archive_name, epoch)?;
    }
    archive
        .finish()
        .map_err(|error| format!("failed to finish {}: {error}", destination.display()))?;
    let gzip = archive
        .into_inner()
        .map_err(|error| format!("failed to finish gzip stream: {error}"))?;
    gzip.finish()
        .map(|_| ())
        .map_err(|error| format!("failed to sync gzip stream: {error}"))
}

fn append_tar_file(
    archive: &mut TarBuilder<flate2::write::GzEncoder<File>>,
    source: &Path,
    archive_name: &Path,
    epoch: u64,
) -> Result<(), String> {
    let mut file = File::open(source)
        .map_err(|error| format!("failed to open {}: {error}", source.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("failed to inspect {}: {error}", source.display()))?;
    let mut header = TarHeader::new_gnu();
    header.set_size(metadata.len());
    header.set_mode(
        if source.file_name().and_then(|name| name.to_str()) == Some("offprint") {
            0o755
        } else {
            0o644
        },
    );
    header.set_mtime(epoch);
    header.set_uid(0);
    header.set_gid(0);
    header.set_cksum();
    archive
        .append_data(&mut header, archive_name, &mut file)
        .map_err(|error| format!("failed to archive {}: {error}", source.display()))
}

fn create_zip(destination: &Path, root: &Path, archive_root: &str) -> Result<(), String> {
    let output = File::create(destination)
        .map_err(|error| format!("failed to create {}: {error}", destination.display()))?;
    let mut archive = ZipWriter::new(output);
    for name in release_file_names(root)? {
        let source = root.join(&name);
        let archive_name = format!("{archive_root}/{}", portable_name(&name));
        let mode = if name == Path::new("offprint.exe") {
            0o755
        } else {
            0o644
        };
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(mode);
        archive
            .start_file(archive_name, options)
            .map_err(|error| format!("failed to start ZIP entry {}: {error}", name.display()))?;
        let mut file = File::open(&source)
            .map_err(|error| format!("failed to open {}: {error}", source.display()))?;
        std::io::copy(&mut file, &mut archive)
            .map_err(|error| format!("failed to write ZIP entry {}: {error}", name.display()))?;
    }
    archive
        .finish()
        .map(|_| ())
        .map_err(|error| format!("failed to finish {}: {error}", destination.display()))
}

fn release_file_names(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut names = Vec::new();
    collect_release_files(root, root, &mut names)?;
    names.sort();
    Ok(names)
}

fn collect_release_files(
    root: &Path,
    directory: &Path,
    names: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("failed to read {}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "release staging entry is a symlink: {}",
                entry.path().display()
            ));
        }
        if metadata.is_dir() {
            collect_release_files(root, &entry.path(), names)?;
        } else if metadata.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_owned();
            names.push(relative);
        } else {
            return Err(format!(
                "release staging entry is not a regular file: {}",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn portable_name(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn write_completions(root: &Path) -> Result<(), String> {
    let directory = root.join("completions");
    fs::create_dir(&directory)
        .map_err(|error| format!("failed to create completion directory: {error}"))?;
    let completions = [
        (Shell::Bash, "offprint.bash"),
        (Shell::Elvish, "offprint.elv"),
        (Shell::Fish, "offprint.fish"),
        (Shell::PowerShell, "_offprint.ps1"),
        (Shell::Zsh, "_offprint"),
    ];
    for (shell, name) in completions {
        let mut bytes = Vec::new();
        generate(
            shell,
            &mut offprint_cli::Cli::command(),
            "offprint",
            &mut bytes,
        );
        fs::write(directory.join(name), bytes)
            .map_err(|error| format!("failed to write {name}: {error}"))?;
    }
    Ok(())
}

fn digest_file(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|error| format!("failed to open {}: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

fn source_date_epoch() -> Result<u64, String> {
    match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(value) => value
            .parse::<u64>()
            .map_err(|error| format!("invalid SOURCE_DATE_EPOCH: {error}")),
        Err(std::env::VarError::NotPresent) => Ok(0),
        Err(error) => Err(format!("failed to read SOURCE_DATE_EPOCH: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::io::Cursor;
    use std::path::Path;

    use flate2::{Compression, GzBuilder};
    use tar::{Builder as TarBuilder, Header as TarHeader};
    use tempfile::TempDir;

    use super::{
        BuildMetadata, CRATE_LICENSE, PUBLISHABLE_CRATES, verify_crate_packages, verify_metadata,
    };

    #[test]
    fn crate_packages_include_the_root_license_and_agpl_metadata() -> Result<(), String> {
        let temporary = TempDir::new().map_err(|error| error.to_string())?;
        fs::write(
            temporary.path().join("Cargo.toml"),
            "[workspace.package]\nversion = \"0.1.0\"\n",
        )
        .map_err(|error| error.to_string())?;
        let license = b"canonical license\n";
        fs::write(temporary.path().join("LICENSE"), license).map_err(|error| error.to_string())?;
        let packages = temporary.path().join("packages");
        fs::create_dir(&packages).map_err(|error| error.to_string())?;
        for name in PUBLISHABLE_CRATES {
            write_crate(&packages, name, license, CRATE_LICENSE)?;
        }

        verify_crate_packages(temporary.path(), &packages)?;
        write_crate(
            &packages,
            PUBLISHABLE_CRATES[0],
            b"different license\n",
            CRATE_LICENSE,
        )?;
        let Err(mismatch) = verify_crate_packages(temporary.path(), &packages) else {
            return Err("a divergent packaged license passed verification".to_owned());
        };
        assert!(mismatch.contains("LICENSE differs"));
        write_crate(&packages, PUBLISHABLE_CRATES[0], license, "MIT")?;
        let Err(mismatch) = verify_crate_packages(temporary.path(), &packages) else {
            return Err("a package with different license metadata passed verification".to_owned());
        };
        assert!(mismatch.contains("package license must be AGPL-3.0-or-later"));
        write_crate_source(
            &packages,
            PUBLISHABLE_CRATES[0],
            license,
            CRATE_LICENSE,
            b"this is not Rust\n",
        )?;
        let Err(build_error) = verify_crate_packages(temporary.path(), &packages) else {
            return Err("a package with invalid Rust source passed verification".to_owned());
        };
        assert!(build_error.contains("failed to build"));
        Ok(())
    }

    #[test]
    fn release_metadata_matches_source_version_and_target() -> Result<(), String> {
        let temporary = TempDir::new().map_err(|error| error.to_string())?;
        fs::write(
            temporary.path().join("versions.toml"),
            r#"
product = "0.1.0"
artifact_format = 2
public_schema = 2
collector_protocol = "1.5"

[chromium]
cdp_revision = "cdp-revision"
managed_revision = "1654411"
managed_version = "151.0.7922.47"
"#,
        )
        .map_err(|error| error.to_string())?;
        let path = temporary.path().join("build-metadata.json");
        let metadata = BuildMetadata {
            schema_version: 1,
            product: "offprint".to_owned(),
            version: "0.1.0".to_owned(),
            target: "aarch64-apple-darwin".to_owned(),
            source_revision: "source-revision".to_owned(),
            rustc: "rustc 1.97.0".to_owned(),
            artifact_format: 2,
            public_schema: 2,
            collector_protocol: "1.5".to_owned(),
            cdp_revision: "cdp-revision".to_owned(),
            managed_browser_revision: "1654411".to_owned(),
            managed_browser_version: "151.0.7922.47".to_owned(),
        };
        fs::write(
            &path,
            serde_json::to_vec(&metadata).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;

        verify_metadata(
            temporary.path(),
            &path,
            "aarch64-apple-darwin",
            "source-revision",
        )?;
        let mismatch = verify_metadata(
            temporary.path(),
            &path,
            "x86_64-apple-darwin",
            "source-revision",
        );
        assert!(mismatch.is_err());
        Ok(())
    }

    fn write_crate(
        directory: &Path,
        name: &str,
        license: &[u8],
        license_id: &str,
    ) -> Result<(), String> {
        write_crate_source(
            directory,
            name,
            license,
            license_id,
            b"pub fn packaged() {}\n",
        )
    }

    fn write_crate_source(
        directory: &Path,
        name: &str,
        license: &[u8],
        license_id: &str,
        source: &[u8],
    ) -> Result<(), String> {
        let path = directory.join(format!("{name}-0.1.0.crate"));
        let file = File::create(&path)
            .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
        let gzip = GzBuilder::new().write(file, Compression::fast());
        let mut archive = TarBuilder::new(gzip);
        let root = format!("{name}-0.1.0");
        let manifest = format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\nlicense = \"{license_id}\"\n"
        );
        append_test_file(
            &mut archive,
            &format!("{root}/Cargo.toml"),
            manifest.as_bytes(),
        )?;
        append_test_file(&mut archive, &format!("{root}/LICENSE"), license)?;
        append_test_file(&mut archive, &format!("{root}/src/lib.rs"), source)?;
        archive
            .finish()
            .map_err(|error| format!("failed to finish {}: {error}", path.display()))?;
        let gzip = archive
            .into_inner()
            .map_err(|error| format!("failed to finish {}: {error}", path.display()))?;
        gzip.finish()
            .map(|_| ())
            .map_err(|error| format!("failed to finish {}: {error}", path.display()))
    }

    fn append_test_file(
        archive: &mut TarBuilder<flate2::write::GzEncoder<File>>,
        path: &str,
        bytes: &[u8],
    ) -> Result<(), String> {
        let mut header = TarHeader::new_gnu();
        header.set_size(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, path, &mut Cursor::new(bytes))
            .map_err(|error| format!("failed to append {path}: {error}"))
    }
}
