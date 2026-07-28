use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use pageknot_model::Result;

use super::cache::set_owner_directory_permissions;
use super::managed_error;

const MAXIMUM_EXPANDED_BYTES: u64 = 1024 * 1024 * 1024;
const MAXIMUM_ARCHIVE_ENTRIES: usize = 100_000;
const MAXIMUM_SYMLINK_BYTES: u64 = 16 * 1024;

#[derive(Debug)]
struct PendingSymlink {
    path: PathBuf,
    target: PathBuf,
}

pub(super) fn extract(archive_path: &Path, staging: &Path) -> Result<()> {
    let file = File::open(archive_path).map_err(|error| {
        managed_error(
            "pageknot.browser.install",
            format!("failed to open managed browser archive: {error}"),
        )
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| {
        managed_error(
            "pageknot.browser.install",
            format!("managed browser archive is not a valid ZIP file: {error}"),
        )
    })?;
    if archive.len() > MAXIMUM_ARCHIVE_ENTRIES {
        return Err(managed_error(
            "pageknot.browser.install",
            "managed browser archive exceeds the entry limit",
        ));
    }

    let mut paths = BTreeSet::new();
    let mut expanded_bytes = 0_u64;
    let mut symlinks = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            managed_error(
                "pageknot.browser.install",
                format!("failed to read managed browser archive entry: {error}"),
            )
        })?;
        let path = safe_archive_path(&entry)?;
        if path.as_os_str().is_empty() {
            continue;
        }
        if !paths.insert(path.clone()) {
            return Err(managed_error(
                "pageknot.browser.install",
                format!(
                    "managed browser archive contains duplicate path `{}`",
                    path.display()
                ),
            ));
        }
        reject_symlink_path_overlap(&path, entry.is_symlink(), &paths, &symlinks)?;
        expanded_bytes = expanded_bytes.checked_add(entry.size()).ok_or_else(|| {
            managed_error(
                "pageknot.browser.install",
                "managed browser expanded byte count overflowed",
            )
        })?;
        if expanded_bytes > MAXIMUM_EXPANDED_BYTES {
            return Err(managed_error(
                "pageknot.browser.install",
                "managed browser archive exceeds the expanded byte limit",
            ));
        }
        validate_entry_type(&entry)?;
        let output = staging.join(&path);
        if entry.is_dir() {
            fs::create_dir_all(&output).map_err(|error| {
                managed_error(
                    "pageknot.browser.install",
                    format!(
                        "failed to create managed browser directory `{}`: {error}",
                        output.display()
                    ),
                )
            })?;
            set_owner_directory_permissions(&output)?;
        } else if entry.is_symlink() {
            if entry.size() > MAXIMUM_SYMLINK_BYTES {
                return Err(managed_error(
                    "pageknot.browser.install",
                    "managed browser archive contains an oversized symbolic link",
                ));
            }
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|error| {
                managed_error(
                    "pageknot.browser.install",
                    format!("failed to read managed browser symbolic link: {error}"),
                )
            })?;
            let target = PathBuf::from(String::from_utf8(bytes).map_err(|error| {
                managed_error(
                    "pageknot.browser.install",
                    format!("managed browser symbolic link target is not UTF-8: {error}"),
                )
            })?);
            validate_symlink_target(&path, &target)?;
            symlinks.push(PendingSymlink { path, target });
        } else {
            extract_file(&mut entry, &output)?;
        }
    }
    create_symlinks(staging, &symlinks)
}

fn extract_file<R: Read>(entry: &mut zip::read::ZipFile<'_, R>, output: &Path) -> Result<()> {
    let parent = output.parent().ok_or_else(|| {
        managed_error(
            "pageknot.browser.install",
            "managed browser archive entry has no parent directory",
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        managed_error(
            "pageknot.browser.install",
            format!(
                "failed to create managed browser directory `{}`: {error}",
                parent.display()
            ),
        )
    })?;
    let mut output_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)
        .map_err(|error| {
            managed_error(
                "pageknot.browser.install",
                format!(
                    "failed to create managed browser file `{}`: {error}",
                    output.display()
                ),
            )
        })?;
    let copied = io::copy(entry, &mut output_file).map_err(|error| {
        managed_error(
            "pageknot.browser.install",
            format!(
                "failed to extract managed browser file `{}`: {error}",
                output.display()
            ),
        )
    })?;
    if copied != entry.size() {
        return Err(managed_error(
            "pageknot.browser.install",
            format!(
                "managed browser file `{}` ended at {copied} bytes, expected {}",
                output.display(),
                entry.size()
            ),
        ));
    }
    output_file.sync_all().map_err(|error| {
        managed_error(
            "pageknot.browser.install",
            format!(
                "failed to sync managed browser file `{}`: {error}",
                output.display()
            ),
        )
    })?;
    set_extracted_file_permissions(output, entry.unix_mode())
}

fn safe_archive_path<R: Read>(entry: &zip::read::ZipFile<'_, R>) -> Result<PathBuf> {
    let path = entry.enclosed_name().ok_or_else(|| {
        managed_error(
            "pageknot.browser.install",
            format!(
                "managed browser archive path `{}` escapes the extraction root",
                entry.name()
            ),
        )
    })?;
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(managed_error(
            "pageknot.browser.install",
            format!("managed browser archive path `{}` is unsafe", entry.name()),
        ));
    }
    Ok(path)
}

fn validate_entry_type<R: Read>(entry: &zip::read::ZipFile<'_, R>) -> Result<()> {
    let Some(mode) = entry.unix_mode() else {
        return Ok(());
    };
    const FILE_TYPE_MASK: u32 = 0o170_000;
    const REGULAR_FILE: u32 = 0o100_000;
    const DIRECTORY: u32 = 0o040_000;
    const SYMLINK: u32 = 0o120_000;
    let kind = mode & FILE_TYPE_MASK;
    if kind == 0 || kind == REGULAR_FILE || kind == DIRECTORY || kind == SYMLINK {
        Ok(())
    } else {
        Err(managed_error(
            "pageknot.browser.install",
            format!(
                "managed browser archive entry `{}` has unsupported special-file mode",
                entry.name()
            ),
        ))
    }
}

fn validate_symlink_target(link_path: &Path, target: &Path) -> Result<()> {
    if target.is_absolute() {
        return Err(managed_error(
            "pageknot.browser.install",
            "managed browser archive contains an absolute symbolic link",
        ));
    }
    let parent = link_path.parent().unwrap_or_else(|| Path::new(""));
    let mut depth = 0_usize;
    for component in parent.components().chain(target.components()) {
        match component {
            Component::Normal(_) => depth = depth.saturating_add(1),
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(managed_error(
                    "pageknot.browser.install",
                    "managed browser symbolic link escapes the extraction root",
                ));
            }
        }
    }
    Ok(())
}

fn reject_symlink_path_overlap(
    path: &Path,
    is_symlink: bool,
    extracted_paths: &BTreeSet<PathBuf>,
    symlinks: &[PendingSymlink],
) -> Result<()> {
    let below_pending_symlink = symlinks
        .iter()
        .any(|pending| path != pending.path && path.starts_with(&pending.path));
    let symlink_above_extracted_path = is_symlink
        && extracted_paths
            .iter()
            .any(|extracted| extracted != path && extracted.starts_with(path));
    if below_pending_symlink || symlink_above_extracted_path {
        return Err(managed_error(
            "pageknot.browser.install",
            "managed browser archive nests an entry beneath a symbolic link",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn create_symlinks(staging: &Path, symlinks: &[PendingSymlink]) -> Result<()> {
    use std::os::unix::fs::symlink;

    for pending in symlinks {
        let output = staging.join(&pending.path);
        let parent = output.parent().ok_or_else(|| {
            managed_error(
                "pageknot.browser.install",
                "managed browser symbolic link has no parent directory",
            )
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            managed_error(
                "pageknot.browser.install",
                format!(
                    "failed to create managed browser symbolic link parent `{}`: {error}",
                    parent.display()
                ),
            )
        })?;
        symlink(&pending.target, &output).map_err(|error| {
            managed_error(
                "pageknot.browser.install",
                format!(
                    "failed to create managed browser symbolic link `{}`: {error}",
                    output.display()
                ),
            )
        })?;
    }
    let root = fs::canonicalize(staging).map_err(|error| {
        managed_error(
            "pageknot.browser.install",
            format!("failed to resolve managed browser staging directory: {error}"),
        )
    })?;
    for pending in symlinks {
        let output = staging.join(&pending.path);
        let resolved = fs::canonicalize(&output).map_err(|error| {
            managed_error(
                "pageknot.browser.install",
                format!(
                    "managed browser symbolic link `{}` has an invalid target: {error}",
                    output.display()
                ),
            )
        })?;
        if !resolved.starts_with(&root) {
            return Err(managed_error(
                "pageknot.browser.install",
                "managed browser symbolic link resolves outside the extraction root",
            ));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn create_symlinks(_staging: &Path, symlinks: &[PendingSymlink]) -> Result<()> {
    if symlinks.is_empty() {
        Ok(())
    } else {
        Err(managed_error(
            "pageknot.browser.install",
            "managed browser archive contains symbolic links unsupported on this platform",
        ))
    }
}

#[cfg(unix)]
fn set_extracted_file_permissions(path: &Path, archive_mode: Option<u32>) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let executable = archive_mode.is_some_and(|mode| mode & 0o111 != 0);
    let mode = if executable { 0o700 } else { 0o600 };
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|error| {
        managed_error(
            "pageknot.browser.install",
            format!(
                "failed to set managed browser file permissions `{}`: {error}",
                path.display()
            ),
        )
    })
}

#[cfg(not(unix))]
fn set_extracted_file_permissions(_path: &Path, _archive_mode: Option<u32>) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs::{self, File};
    use std::io::Write as _;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;
    use zip::write::SimpleFileOptions;

    use super::{PendingSymlink, extract, reject_symlink_path_overlap, validate_symlink_target};

    #[test]
    fn symlink_target_must_stay_inside_staging() {
        assert!(validate_symlink_target(Path::new("a/b"), Path::new("../c")).is_ok());
        assert!(validate_symlink_target(Path::new("a"), Path::new("../../outside")).is_err());
    }

    #[test]
    fn archive_entries_cannot_cross_a_pending_symlink_boundary() {
        let extracted = BTreeSet::from([
            PathBuf::from("browser/link"),
            PathBuf::from("browser/link/file"),
        ]);
        let pending = vec![PendingSymlink {
            path: PathBuf::from("browser/link"),
            target: PathBuf::from("target"),
        }];

        assert!(
            reject_symlink_path_overlap(
                Path::new("browser/link/file"),
                false,
                &extracted,
                &pending,
            )
            .is_err()
        );
        assert!(reject_symlink_path_overlap(Path::new("browser"), true, &extracted, &[],).is_err());
    }

    #[test]
    fn extraction_rejects_duplicate_paths() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = TempDir::new()?;
        let archive_path = fixture.path().join("archive.zip");
        let file = File::create(&archive_path)?;
        let mut archive = zip::ZipWriter::new(file);
        archive.start_file("chrome//file", SimpleFileOptions::default())?;
        archive.write_all(b"first")?;
        archive.start_file("chrome/file", SimpleFileOptions::default())?;
        archive.write_all(b"second")?;
        archive.finish()?;
        let staging = fixture.path().join("staging");
        fs::create_dir(&staging)?;

        let Err(error) = extract(&archive_path, &staging) else {
            return Err("duplicate archive path was accepted".into());
        };

        assert_eq!(error.code.as_str(), "pageknot.browser.install");
        assert!(error.message.contains("duplicate path"));
        Ok(())
    }

    #[test]
    fn extraction_rejects_parent_traversal() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = TempDir::new()?;
        let archive_path = fixture.path().join("archive.zip");
        let file = File::create(&archive_path)?;
        let mut archive = zip::ZipWriter::new(file);
        archive.start_file("../outside", SimpleFileOptions::default())?;
        archive.write_all(b"escape")?;
        archive.finish()?;
        let staging = fixture.path().join("staging");
        fs::create_dir(&staging)?;

        let Err(error) = extract(&archive_path, &staging) else {
            return Err("archive traversal was accepted".into());
        };

        assert_eq!(error.code.as_str(), "pageknot.browser.install");
        assert!(!fixture.path().join("outside").exists());
        Ok(())
    }
}
