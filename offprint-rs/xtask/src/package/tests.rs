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
    fs::create_dir_all(temporary.path().join("offprint-rs")).map_err(|error| error.to_string())?;
    fs::write(
        temporary.path().join("offprint-rs/Cargo.toml"),
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
