use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};

use fs2::FileExt as _;
use pageknot_model::{
    ArtifactVariantKind, ArtifactVariantVerification, ContentDigest, ErrorStage, PageKnotError,
    Result,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::durable::{create_directory, sync_directory, write_new_file};

const JOURNAL_SCHEMA_VERSION: u32 = 1;
const MAXIMUM_JOURNAL_BYTES: u64 = 64 * 1024;
const MAXIMUM_JOURNAL_FILES: usize = 16;
const MAXIMUM_JOURNAL_VARIANTS: usize = 8;
const TRANSACTION_PREFIX: &str = "txn-";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum OutputKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum CommitMode {
    Entries,
    DirectorySwap,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) enum JournalPhase {
    Staging,
    Prepared,
    Mutating,
    RollingBack,
    Committed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct JournalEntry {
    pub(super) destination_name: String,
    pub(super) kind: ArtifactVariantKind,
    pub(super) output_kind: OutputKind,
    pub(super) existed: bool,
    pub(super) previous_kind: Option<OutputKind>,
    pub(super) verification: ArtifactVariantVerification,
    pub(super) markdown_assets: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TransactionJournal {
    pub(super) schema_version: u32,
    pub(super) transaction_id: String,
    pub(super) output_identity: String,
    pub(super) sequence: u32,
    pub(super) phase: JournalPhase,
    pub(super) mode: CommitMode,
    pub(super) entries: Vec<JournalEntry>,
}

#[derive(Debug)]
pub(super) struct TransactionNamespace {
    output_directory: PathBuf,
    path: PathBuf,
    identity: String,
    _lock: File,
}

impl TransactionNamespace {
    pub(super) fn acquire(output_directory: &Path) -> Result<Self> {
        let canonical_output = fs::canonicalize(output_directory).map_err(|error| {
            journal_io_error(
                ErrorStage::Validation,
                "failed to resolve the export output directory identity",
                error,
            )
        })?;
        let identity = namespace_identity(&canonical_output);
        let parent = canonical_output.parent().unwrap_or_else(|| Path::new("."));
        let path = parent.join(format!(".pageknot-transactions-{identity}"));
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(journal_error(
                    ErrorStage::Validation,
                    "PageKnot transaction namespace must be a directly addressed directory",
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                create_directory(&path, ErrorStage::Validation)?;
            }
            Err(error) => {
                return Err(journal_io_error(
                    ErrorStage::Validation,
                    "failed to inspect the PageKnot transaction namespace",
                    error,
                ));
            }
        }
        let lock_path = path.join("lock");
        let existed = fs::symlink_metadata(&lock_path).is_ok();
        if existed {
            let metadata = fs::symlink_metadata(&lock_path).map_err(|error| {
                journal_io_error(
                    ErrorStage::Validation,
                    "failed to inspect the export transaction lock",
                    error,
                )
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(journal_error(
                    ErrorStage::Validation,
                    "export transaction lock must be a directly addressed regular file",
                ));
            }
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| {
                journal_io_error(
                    ErrorStage::Validation,
                    "failed to open the export transaction lock",
                    error,
                )
            })?;
        let opened_metadata = lock.metadata().map_err(|error| {
            journal_io_error(
                ErrorStage::Validation,
                "failed to inspect the opened export transaction lock",
                error,
            )
        })?;
        if !opened_metadata.is_file()
            || fs::symlink_metadata(&lock_path)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err(journal_error(
                ErrorStage::Validation,
                "export transaction lock changed while it was opened",
            ));
        }
        if !existed {
            lock.sync_all().map_err(|error| {
                journal_io_error(
                    ErrorStage::Validation,
                    "failed to synchronize the export transaction lock",
                    error,
                )
            })?;
            sync_directory(&path, ErrorStage::Validation)?;
        }
        lock.try_lock_exclusive().map_err(|error| {
            journal_error(
                ErrorStage::Commit,
                "another PageKnot export transaction owns this output directory",
            )
            .retryable(true)
            .with_detail("ioKind", format!("{:?}", error.kind()))
            .with_detail("outputDirectory", output_directory.display().to_string())
        })?;
        Ok(Self {
            output_directory: canonical_output,
            path,
            identity,
            _lock: lock,
        })
    }

    pub(super) fn create_transaction_root(&self) -> Result<PathBuf> {
        let directory = tempfile::Builder::new()
            .prefix(TRANSACTION_PREFIX)
            .tempdir_in(&self.path)
            .map_err(|error| {
                journal_io_error(
                    ErrorStage::Encoding,
                    "failed to create an export transaction directory",
                    error,
                )
            })?;
        let path = directory.keep();
        sync_directory(&path, ErrorStage::Encoding)?;
        sync_directory(&self.path, ErrorStage::Encoding)?;
        Ok(path)
    }

    pub(super) fn transaction_roots(&self) -> Result<Vec<PathBuf>> {
        let mut roots = Vec::new();
        for entry in fs::read_dir(&self.path).map_err(|error| {
            journal_io_error(
                ErrorStage::Commit,
                "failed to enumerate the PageKnot transaction namespace",
                error,
            )
        })? {
            let entry = entry.map_err(|error| {
                journal_io_error(
                    ErrorStage::Commit,
                    "failed to enumerate the PageKnot transaction namespace",
                    error,
                )
            })?;
            let name = entry.file_name();
            if name == "lock" {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
                journal_io_error(
                    ErrorStage::Commit,
                    "failed to inspect an export transaction entry",
                    error,
                )
            })?;
            if !name.to_string_lossy().starts_with(TRANSACTION_PREFIX)
                || metadata.file_type().is_symlink()
                || !metadata.is_dir()
            {
                return Err(journal_error(
                    ErrorStage::Commit,
                    "PageKnot transaction namespace contains an unrecognized entry",
                )
                .with_detail("path", entry.path().display().to_string()));
            }
            roots.push(entry.path());
        }
        roots.sort();
        Ok(roots)
    }

    pub(super) fn output_directory(&self) -> &Path {
        &self.output_directory
    }

    pub(super) fn identity(&self) -> &str {
        &self.identity
    }
}

#[derive(Debug)]
pub(super) struct JournalStore {
    root: PathBuf,
    snapshot: TransactionJournal,
    next_sequence: u32,
}

impl JournalStore {
    pub(super) fn new(
        root: &Path,
        output_identity: String,
        mode: CommitMode,
        entries: Vec<JournalEntry>,
    ) -> Result<Self> {
        let transaction_id = root
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| name.starts_with(TRANSACTION_PREFIX))
            .ok_or_else(|| {
                journal_error(
                    ErrorStage::Internal,
                    "export transaction directory has an invalid identity",
                )
            })?
            .to_owned();
        validate_entries(&entries)?;
        Ok(Self {
            root: root.to_owned(),
            snapshot: TransactionJournal {
                schema_version: JOURNAL_SCHEMA_VERSION,
                transaction_id,
                output_identity,
                sequence: 0,
                phase: JournalPhase::Staging,
                mode,
                entries,
            },
            next_sequence: 0,
        })
    }

    pub(super) fn from_snapshot(root: &Path, snapshot: TransactionJournal) -> Self {
        let next_sequence = snapshot.sequence.saturating_add(1);
        Self {
            root: root.to_owned(),
            snapshot,
            next_sequence,
        }
    }

    pub(super) fn persist(&mut self, phase: JournalPhase) -> Result<()> {
        self.snapshot.phase = phase;
        self.snapshot.sequence = self.next_sequence;
        let bytes = serde_json::to_vec(&self.snapshot).map_err(|error| {
            journal_error(
                ErrorStage::Commit,
                format!("failed to encode the export transaction journal: {error}"),
            )
        })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_JOURNAL_BYTES {
            return Err(journal_error(
                ErrorStage::Commit,
                "export transaction journal exceeds its byte limit",
            ));
        }
        let path = self
            .root
            .join(format!("journal-{:08}.json", self.snapshot.sequence));
        write_new_file(&path, &bytes, ErrorStage::Commit)?;
        self.next_sequence = self.next_sequence.saturating_add(1);
        Ok(())
    }

    pub(super) fn snapshot(&self) -> &TransactionJournal {
        &self.snapshot
    }
}

pub(super) fn load_latest_journal(
    root: &Path,
    output_identity: &str,
) -> Result<Option<TransactionJournal>> {
    let entries = fs::read_dir(root).map_err(|error| {
        journal_io_error(
            ErrorStage::Commit,
            "failed to enumerate an export transaction directory",
            error,
        )
    })?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            journal_io_error(
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
            paths.push(entry.path());
        }
    }
    if paths.len() > MAXIMUM_JOURNAL_FILES {
        return Err(journal_error(
            ErrorStage::Commit,
            "export transaction contains too many journal snapshots",
        ));
    }
    paths.sort();
    let transaction_id = root.file_name().and_then(|name| name.to_str());
    let mut latest = None;
    for path in paths {
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            journal_io_error(
                ErrorStage::Commit,
                "failed to inspect an export transaction journal",
                error,
            )
        })?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAXIMUM_JOURNAL_BYTES
        {
            continue;
        }
        let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
        File::open(&path)
            .and_then(|file| {
                file.take(MAXIMUM_JOURNAL_BYTES.saturating_add(1))
                    .read_to_end(&mut bytes)
            })
            .map_err(|error| {
                journal_io_error(
                    ErrorStage::Commit,
                    "failed to read an export transaction journal",
                    error,
                )
            })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_JOURNAL_BYTES {
            continue;
        }
        let Ok(snapshot) = serde_json::from_slice::<TransactionJournal>(&bytes) else {
            continue;
        };
        if snapshot.schema_version != JOURNAL_SCHEMA_VERSION
            || snapshot.output_identity != output_identity
            || Some(snapshot.transaction_id.as_str()) != transaction_id
            || validate_entries(&snapshot.entries).is_err()
        {
            continue;
        }
        let file_sequence = path
            .file_stem()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix("journal-"))
            .and_then(|value| value.parse::<u32>().ok());
        if file_sequence != Some(snapshot.sequence) {
            continue;
        }
        if latest
            .as_ref()
            .is_none_or(|current: &TransactionJournal| snapshot.sequence > current.sequence)
        {
            latest = Some(snapshot);
        }
    }
    Ok(latest)
}

fn validate_entries(entries: &[JournalEntry]) -> Result<()> {
    if entries.is_empty() || entries.len() > MAXIMUM_JOURNAL_VARIANTS {
        return Err(journal_error(
            ErrorStage::Commit,
            "export transaction journal has an invalid variant count",
        ));
    }
    let mut names = BTreeSet::new();
    for entry in entries {
        let mut components = Path::new(&entry.destination_name).components();
        if !matches!(components.next(), Some(Component::Normal(_)))
            || components.next().is_some()
            || !names.insert(entry.destination_name.as_str())
        {
            return Err(journal_error(
                ErrorStage::Commit,
                "export transaction journal contains an invalid destination name",
            ));
        }
    }
    Ok(())
}

fn namespace_identity(output_directory: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(output_directory.to_string_lossy().as_bytes());
    ContentDigest::from_bytes(hasher.finalize().into()).to_hex()
}

fn journal_error(stage: ErrorStage, message: impl Into<String>) -> PageKnotError {
    PageKnotError::new("pageknot.export.output", stage, message)
}

fn journal_io_error(
    stage: ErrorStage,
    message: &'static str,
    error: std::io::Error,
) -> PageKnotError {
    journal_error(stage, format!("{message}: {error}"))
        .with_detail("ioKind", format!("{:?}", error.kind()))
}
