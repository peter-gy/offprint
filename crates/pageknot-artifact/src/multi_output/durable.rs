use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::Path;

use pageknot_model::{ErrorStage, PageKnotError, Result};

pub(super) fn create_directory(path: &Path, stage: ErrorStage) -> Result<()> {
    fs::create_dir(path).map_err(|error| {
        io_error(
            stage,
            "failed to create an export transaction directory",
            error,
        )
    })?;
    sync_parent(path, stage)?;
    sync_directory(path, stage)
}

pub(super) fn write_new_file(path: &Path, bytes: &[u8], stage: ErrorStage) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|error| io_error(stage, "failed to create an export transaction file", error))?;
    file.write_all(bytes)
        .map_err(|error| io_error(stage, "failed to write an export transaction file", error))?;
    file.sync_all().map_err(|error| {
        io_error(
            stage,
            "failed to synchronize an export transaction file",
            error,
        )
    })?;
    sync_parent(path, stage)
}

#[derive(Debug)]
pub(super) enum DurabilityOutcome {
    Synced,
    AppliedWithSyncWarning,
}

pub(super) fn durable_rename(source: &Path, destination: &Path) -> Result<DurabilityOutcome> {
    durable_rename_with(source, destination, sync_directory)
}

fn durable_rename_with(
    source: &Path,
    destination: &Path,
    mut sync: impl FnMut(&Path, ErrorStage) -> Result<()>,
) -> Result<DurabilityOutcome> {
    let source_parent = parent(source);
    let destination_parent = parent(destination);
    sync(source_parent, ErrorStage::Commit)?;
    if destination_parent != source_parent {
        sync(destination_parent, ErrorStage::Commit)?;
    }
    fs::rename(source, destination).map_err(|error| {
        io_error(
            ErrorStage::Commit,
            "failed to durably rename an export transaction path",
            error,
        )
    })?;
    let mut warning = sync(source_parent, ErrorStage::Commit).err();
    if destination_parent != source_parent {
        warning = warning.or_else(|| sync(destination_parent, ErrorStage::Commit).err());
    }
    Ok(match warning {
        Some(_error) => DurabilityOutcome::AppliedWithSyncWarning,
        None => DurabilityOutcome::Synced,
    })
}

pub(super) fn durable_hard_link(source: &Path, destination: &Path) -> Result<DurabilityOutcome> {
    let source_parent = parent(source);
    let destination_parent = parent(destination);
    sync_directory(source_parent, ErrorStage::Commit)?;
    if destination_parent != source_parent {
        sync_directory(destination_parent, ErrorStage::Commit)?;
    }
    fs::hard_link(source, destination).map_err(|error| {
        io_error(
            ErrorStage::Commit,
            "failed to commit a verified file artifact variant",
            error,
        )
    })?;
    Ok(
        match sync_directory(destination_parent, ErrorStage::Commit) {
            Ok(()) => DurabilityOutcome::Synced,
            Err(_error) => DurabilityOutcome::AppliedWithSyncWarning,
        },
    )
}

pub(super) fn cleanup_transaction_root(root: &Path) -> Result<()> {
    let mut journals = Vec::new();
    let mut other = Vec::new();
    for entry in fs::read_dir(root).map_err(|error| {
        io_error(
            ErrorStage::Commit,
            "failed to enumerate an export transaction directory",
            error,
        )
    })? {
        let entry = entry.map_err(|error| {
            io_error(
                ErrorStage::Commit,
                "failed to enumerate an export transaction directory",
                error,
            )
        })?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("journal-") && name.ends_with(".json"))
        {
            journals.push(entry.path());
        } else {
            other.push(entry.path());
        }
    }
    journals.sort();
    other.sort();
    for path in other {
        remove_direct_path(&path)?;
    }
    sync_directory(root, ErrorStage::Commit)?;
    for path in journals {
        remove_direct_path(&path)?;
        sync_directory(root, ErrorStage::Commit)?;
    }
    let namespace = parent(root);
    fs::remove_dir(root).map_err(|error| {
        io_error(
            ErrorStage::Commit,
            "failed to remove a completed export transaction directory",
            error,
        )
    })?;
    sync_directory(namespace, ErrorStage::Commit)
}

pub(super) fn remove_direct_path(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        io_error(
            ErrorStage::Commit,
            "failed to inspect an export transaction path for removal",
            error,
        )
    })?;
    let operation = if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path)
    } else if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        return Err(PageKnotError::new(
            "pageknot.export.output",
            ErrorStage::Commit,
            "export transaction cleanup found an unsupported filesystem entry",
        ));
    };
    operation.map_err(|error| {
        io_error(
            ErrorStage::Commit,
            "failed to remove an export transaction path",
            error,
        )
    })
}

pub(super) fn sync_parent(path: &Path, stage: ErrorStage) -> Result<()> {
    sync_directory(parent(path), stage)
}

#[cfg(unix)]
pub(super) fn sync_directory(path: &Path, stage: ErrorStage) -> Result<()> {
    match File::open(path).and_then(|directory| directory.sync_all()) {
        Ok(()) => Ok(()),
        Err(error) if directory_sync_is_unavailable(&error) => Ok(()),
        Err(error) => Err(io_error(
            stage,
            "failed to synchronize an export directory",
            error,
        )),
    }
}

#[cfg(not(unix))]
pub(super) fn sync_directory(_path: &Path, _stage: ErrorStage) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn directory_sync_is_unavailable(error: &std::io::Error) -> bool {
    // macOS protected folders can permit atomic rename while rejecting the
    // read handle needed for directory fsync with EPERM.
    error.raw_os_error() == Some(1)
}

#[cfg(not(target_os = "macos"))]
fn directory_sync_is_unavailable(_error: &std::io::Error) -> bool {
    false
}

fn parent(path: &Path) -> &Path {
    path.parent().unwrap_or_else(|| Path::new("."))
}

fn io_error(stage: ErrorStage, message: &'static str, error: std::io::Error) -> PageKnotError {
    PageKnotError::new(
        "pageknot.export.output",
        stage,
        format!("{message}: {error}"),
    )
    .with_detail("ioKind", format!("{:?}", error.kind()))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn macos_protected_folder_sync_is_treated_as_unavailable() {
        assert!(super::directory_sync_is_unavailable(
            &std::io::Error::from_raw_os_error(1)
        ));
        assert!(!super::directory_sync_is_unavailable(
            &std::io::Error::from_raw_os_error(13)
        ));
    }
}

#[cfg(test)]
mod rename_tests {
    use std::fs;

    use pageknot_model::{ErrorStage, PageKnotError};

    use super::{DurabilityOutcome, durable_rename_with};

    #[test]
    fn post_rename_sync_failure_reports_an_applied_outcome() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("source");
        let destination = directory.path().join("destination");
        fs::write(&source, b"committed")?;
        let mut syncs = 0_usize;

        let outcome = durable_rename_with(&source, &destination, |_, _| {
            syncs = syncs.saturating_add(1);
            if syncs == 2 {
                Err(PageKnotError::new(
                    "pageknot.export.output",
                    ErrorStage::Commit,
                    "injected post-rename parent sync failure",
                ))
            } else {
                Ok(())
            }
        })
        .map_err(|error| std::io::Error::other(error.to_string()))?;

        assert!(matches!(outcome, DurabilityOutcome::AppliedWithSyncWarning));
        assert!(!source.exists());
        assert_eq!(fs::read(destination)?, b"committed");
        Ok(())
    }
}
