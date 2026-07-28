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

pub(super) fn durable_rename(source: &Path, destination: &Path) -> Result<()> {
    let source_parent = parent(source);
    let destination_parent = parent(destination);
    sync_directory(source_parent, ErrorStage::Commit)?;
    if destination_parent != source_parent {
        sync_directory(destination_parent, ErrorStage::Commit)?;
    }
    fs::rename(source, destination).map_err(|error| {
        io_error(
            ErrorStage::Commit,
            "failed to durably rename an export transaction path",
            error,
        )
    })?;
    sync_directory(source_parent, ErrorStage::Commit)?;
    if destination_parent != source_parent {
        sync_directory(destination_parent, ErrorStage::Commit)?;
    }
    Ok(())
}

pub(super) fn durable_hard_link(source: &Path, destination: &Path) -> Result<()> {
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
    sync_directory(destination_parent, ErrorStage::Commit)
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
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error(stage, "failed to synchronize an export directory", error))
}

#[cfg(not(unix))]
pub(super) fn sync_directory(_path: &Path, _stage: ErrorStage) -> Result<()> {
    Ok(())
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
