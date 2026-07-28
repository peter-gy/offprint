use std::fs;
use std::path::Path;

use pageknot_model::{ErrorStage, PageKnotError, Result};

use super::durable::{cleanup_transaction_root, create_directory, durable_rename, sync_directory};
use super::journal::{
    CommitMode, JournalEntry, JournalPhase, JournalStore, OutputKind, TransactionJournal,
    TransactionNamespace, load_latest_journal,
};
use super::plan::{output_matches_complete_set, path_matches_entry};

pub(super) fn recover_pending_transactions(
    namespace: &TransactionNamespace,
    maximum_assets: usize,
    maximum_bytes: u64,
) -> Result<()> {
    for root in namespace.transaction_roots()? {
        let loaded = load_latest_journal(&root, namespace.identity())
            .map_err(|error| recovery_error(&root, None, false, false, vec![error.to_string()]))?;
        let Some(mut journal) = loaded else {
            let empty = directory_is_empty(&root).map_err(|error| {
                recovery_error(&root, None, false, false, vec![error.to_string()])
            })?;
            if empty {
                cleanup_transaction_root(&root)?;
                continue;
            }
            return Err(recovery_error(
                &root,
                None,
                false,
                false,
                vec!["transaction directory has no valid durable journal".to_owned()],
            ));
        };
        match journal.phase {
            JournalPhase::Staging | JournalPhase::Prepared => {
                cleanup_transaction_root(&root).map_err(|error| {
                    recovery_error(
                        &root,
                        Some(journal.phase),
                        false,
                        true,
                        vec![error.to_string()],
                    )
                })?;
            }
            JournalPhase::Mutating => {
                let mut store = JournalStore::from_snapshot(&root, journal);
                store.persist(JournalPhase::RollingBack).map_err(|error| {
                    recovery_error(
                        &root,
                        Some(JournalPhase::Mutating),
                        false,
                        false,
                        vec![error.to_string()],
                    )
                })?;
                journal = store.snapshot().clone();
                rollback_transaction(
                    namespace.output_directory(),
                    &root,
                    &journal,
                    maximum_assets,
                    maximum_bytes,
                )?;
            }
            JournalPhase::RollingBack => {
                rollback_transaction(
                    namespace.output_directory(),
                    &root,
                    &journal,
                    maximum_assets,
                    maximum_bytes,
                )?;
            }
            JournalPhase::Committed => {
                finalize_committed(
                    namespace.output_directory(),
                    &root,
                    &journal,
                    maximum_assets,
                    maximum_bytes,
                )?;
            }
        }
    }
    Ok(())
}

pub(super) fn rollback_transaction(
    output_directory: &Path,
    root: &Path,
    journal: &TransactionJournal,
    maximum_assets: usize,
    maximum_bytes: u64,
) -> Result<()> {
    let failures = match journal.mode {
        CommitMode::Entries => rollback_entries(
            output_directory,
            root,
            &journal.entries,
            maximum_assets,
            maximum_bytes,
        ),
        CommitMode::DirectorySwap => rollback_directory_swap(
            output_directory,
            root,
            &journal.entries,
            maximum_assets,
            maximum_bytes,
        ),
    };
    if !failures.is_empty() {
        return Err(recovery_error(
            root,
            Some(JournalPhase::RollingBack),
            false,
            false,
            failures,
        ));
    }
    sync_directory(output_directory, ErrorStage::Commit).map_err(|error| {
        recovery_error(
            root,
            Some(JournalPhase::RollingBack),
            false,
            true,
            vec![error.to_string()],
        )
    })?;
    cleanup_transaction_root(root).map_err(|error| {
        recovery_error(
            root,
            Some(JournalPhase::RollingBack),
            false,
            true,
            vec![error.to_string()],
        )
    })
}

pub(super) fn finalize_committed(
    output_directory: &Path,
    root: &Path,
    journal: &TransactionJournal,
    maximum_assets: usize,
    maximum_bytes: u64,
) -> Result<()> {
    let complete = match journal.mode {
        CommitMode::Entries => journal.entries.iter().all(|entry| {
            path_matches_entry(
                &output_directory.join(&entry.destination_name),
                entry,
                maximum_assets,
                maximum_bytes,
            )
        }),
        CommitMode::DirectorySwap => output_matches_complete_set(
            output_directory,
            &journal.entries,
            maximum_assets,
            maximum_bytes,
        ),
    };
    if !complete {
        return Err(recovery_error(
            root,
            Some(JournalPhase::Committed),
            true,
            false,
            vec!["committed output set no longer matches its durable journal".to_owned()],
        ));
    }
    cleanup_transaction_root(root).map_err(|error| {
        recovery_error(
            root,
            Some(JournalPhase::Committed),
            true,
            true,
            vec![error.to_string()],
        )
    })
}

fn rollback_entries(
    output_directory: &Path,
    root: &Path,
    entries: &[JournalEntry],
    maximum_assets: usize,
    maximum_bytes: u64,
) -> Vec<String> {
    let mut failures = Vec::new();
    for (index, entry) in entries.iter().enumerate().rev() {
        let destination = output_directory.join(&entry.destination_name);
        let backup = root.join("backups").join(format!("entry-{index}"));
        let discard = root.join("discard").join(format!("entry-{index}"));
        let Some(backup_exists) = observed_exists(&backup, &mut failures) else {
            continue;
        };
        let Some(destination_exists) = observed_exists(&destination, &mut failures) else {
            continue;
        };
        if backup_exists {
            if destination_exists {
                if path_matches_entry(&destination, entry, maximum_assets, maximum_bytes) {
                    if let Err(error) = ensure_parent_and_move(&destination, &discard) {
                        failures.push(format!(
                            "failed to retain the committed output `{}` during rollback: {error}",
                            destination.display()
                        ));
                        continue;
                    }
                } else {
                    failures.push(format!(
                        "destination `{}` changed after PageKnot created its backup",
                        destination.display()
                    ));
                    continue;
                }
            }
            if let Err(error) = validate_backup_kind(&backup, entry.previous_kind) {
                failures.push(error.to_string());
                continue;
            }
            if let Err(error) = durable_rename(&backup, &destination) {
                failures.push(format!(
                    "failed to restore `{}`: {error}",
                    destination.display()
                ));
            }
            continue;
        }
        if entry.existed {
            if !destination_exists {
                failures.push(format!(
                    "original destination `{}` and its backup are both missing",
                    destination.display()
                ));
            } else if path_matches_entry(&destination, entry, maximum_assets, maximum_bytes) {
                failures.push(format!(
                    "replacement at `{}` remains but its original backup is missing",
                    destination.display()
                ));
            }
            continue;
        }
        if !destination_exists {
            continue;
        }
        if path_matches_entry(&destination, entry, maximum_assets, maximum_bytes) {
            if let Err(error) = ensure_parent_and_move(&destination, &discard) {
                failures.push(format!(
                    "failed to retract committed output `{}`: {error}",
                    destination.display()
                ));
            }
        } else {
            failures.push(format!(
                "new destination `{}` does not match the PageKnot journal",
                destination.display()
            ));
        }
    }
    failures
}

fn rollback_directory_swap(
    output_directory: &Path,
    root: &Path,
    entries: &[JournalEntry],
    maximum_assets: usize,
    maximum_bytes: u64,
) -> Vec<String> {
    let previous = root.join("previous-output");
    let discard = root.join("discard-output");
    let previous_exists = match direct_exists(&previous) {
        Ok(exists) => exists,
        Err(error) => return vec![error.to_string()],
    };
    let output_exists = match direct_exists(output_directory) {
        Ok(exists) => exists,
        Err(error) => return vec![error.to_string()],
    };
    if previous_exists {
        if output_exists {
            let complete = output_matches_complete_set(
                output_directory,
                entries,
                maximum_assets,
                maximum_bytes,
            );
            let empty = match directory_is_empty(output_directory) {
                Ok(empty) => empty,
                Err(error) => return vec![error.to_string()],
            };
            if complete || empty {
                let discard_exists = match direct_exists(&discard) {
                    Ok(exists) => exists,
                    Err(error) => return vec![error.to_string()],
                };
                if discard_exists {
                    return vec![
                        "both committed and retained directory-swap outputs are present".to_owned(),
                    ];
                }
                if let Err(error) = durable_rename(output_directory, &discard) {
                    return vec![format!(
                        "failed to retract the committed output directory: {error}"
                    )];
                }
            } else {
                return vec![
                    "output directory changed after PageKnot retained the previous directory"
                        .to_owned(),
                ];
            }
        }
        if let Err(error) = durable_rename(&previous, output_directory) {
            return vec![format!(
                "failed to restore the previous output directory: {error}"
            )];
        }
        return Vec::new();
    }
    if !output_exists {
        return vec!["output directory and its retained predecessor are both missing".to_owned()];
    }
    match directory_is_empty(output_directory) {
        Ok(true) => Vec::new(),
        Ok(false) => {
            vec!["directory-swap output is present without its retained predecessor".to_owned()]
        }
        Err(error) => vec![error.to_string()],
    }
}

fn ensure_parent_and_move(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    if !direct_exists(parent)? {
        create_directory(parent, ErrorStage::Commit)?;
    }
    if direct_exists(destination)? {
        return Err(PageKnotError::new(
            "pageknot.export.output",
            ErrorStage::Commit,
            "export rollback retention path is already occupied",
        ));
    }
    durable_rename(source, destination)
}

fn validate_backup_kind(path: &Path, expected: Option<OutputKind>) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        PageKnotError::new(
            "pageknot.export.output",
            ErrorStage::Commit,
            format!("failed to inspect an export rollback backup: {error}"),
        )
    })?;
    let observed = if metadata.file_type().is_symlink() {
        None
    } else if metadata.is_file() {
        Some(OutputKind::File)
    } else if metadata.is_dir() {
        Some(OutputKind::Directory)
    } else {
        None
    };
    if observed != expected {
        return Err(PageKnotError::new(
            "pageknot.export.output",
            ErrorStage::Commit,
            "export rollback backup kind does not match its durable journal",
        )
        .with_detail("path", path.display().to_string()));
    }
    Ok(())
}

fn directory_is_empty(path: &Path) -> Result<bool> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        recovery_io_error(
            path,
            "failed to inspect an export transaction directory",
            error,
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(PageKnotError::new(
            "pageknot.export.output",
            ErrorStage::Commit,
            "export recovery requires a directly addressed directory",
        )
        .with_detail("path", path.display().to_string()));
    }
    let mut entries = fs::read_dir(path).map_err(|error| {
        recovery_io_error(
            path,
            "failed to enumerate an export transaction directory",
            error,
        )
    })?;
    match entries.next() {
        None => Ok(true),
        Some(Ok(_)) => Ok(false),
        Some(Err(error)) => Err(recovery_io_error(
            path,
            "failed to enumerate an export transaction directory",
            error,
        )),
    }
}

fn direct_exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(recovery_io_error(
            path,
            "failed to inspect an export recovery path",
            error,
        )),
    }
}

fn observed_exists(path: &Path, failures: &mut Vec<String>) -> Option<bool> {
    match direct_exists(path) {
        Ok(exists) => Some(exists),
        Err(error) => {
            failures.push(error.to_string());
            None
        }
    }
}

fn recovery_io_error(path: &Path, message: &'static str, error: std::io::Error) -> PageKnotError {
    PageKnotError::new(
        "pageknot.export.output",
        ErrorStage::Commit,
        format!("{message}: {error}"),
    )
    .with_detail("path", path.display().to_string())
    .with_detail("ioKind", format!("{:?}", error.kind()))
}

fn recovery_error(
    root: &Path,
    phase: Option<JournalPhase>,
    commit_complete: bool,
    outputs_recovered: bool,
    failures: Vec<String>,
) -> PageKnotError {
    let recovery_paths = retained_paths(root);
    PageKnotError::new(
        "pageknot.export.output",
        ErrorStage::Commit,
        if outputs_recovered {
            "export outputs were recovered but transaction cleanup remains pending"
        } else {
            "an incomplete export transaction requires recovery"
        },
    )
    .retryable(outputs_recovered)
    .with_detail("transactionPath", root.display().to_string())
    .with_detail(
        "phase",
        phase.map_or_else(|| "unknown".to_owned(), |phase| format!("{phase:?}")),
    )
    .with_detail("commitComplete", commit_complete)
    .with_detail("rollbackComplete", outputs_recovered)
    .with_detail("partialCommit", !commit_complete && !outputs_recovered)
    .with_detail("recoveryPending", true)
    .with_detail("recoveryPaths", recovery_paths)
    .with_detail("recoveryFailures", failures)
}

fn retained_paths(root: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    for relative in ["backups", "discard", "previous-output", "discard-output"] {
        let path = root.join(relative);
        if direct_exists(&path).is_ok_and(|exists| exists) {
            paths.push(path.display().to_string());
        }
    }
    if paths.is_empty() {
        paths.push(root.display().to_string());
    }
    paths
}
