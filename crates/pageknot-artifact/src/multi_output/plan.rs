use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use pageknot_model::{ConflictPolicy, ContentDigest, ErrorStage, PageKnotError, Result};
use sha2::{Digest as _, Sha256};

use super::durable::{create_directory, sync_directory, write_new_file};
use super::journal::{CommitMode, JournalEntry, OutputKind};
use super::payload::{
    ArtifactDirectory, ArtifactTransactionLimits, PreparedArtifact, PreparedPayload,
};

const FILE_READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub(super) struct DestinationPlan {
    pub(super) destination: PathBuf,
    pub(super) entry: JournalEntry,
}

#[derive(Clone, Debug)]
pub(super) struct StagedArtifact {
    pub(super) destination: PathBuf,
    pub(super) staged_path: PathBuf,
    pub(super) entry: JournalEntry,
}

pub(super) fn validate_prepared_artifacts(
    prepared: &[PreparedArtifact],
    limits: ArtifactTransactionLimits,
) -> Result<()> {
    for artifact in prepared {
        if artifact.file_count() > limits.maximum_files {
            return Err(PageKnotError::new(
                "pageknot.export.files",
                ErrorStage::Encoding,
                "artifact payload exceeds the file count limit",
            ));
        }
        if artifact.directory_count() > limits.maximum_files {
            return Err(PageKnotError::new(
                "pageknot.export.files",
                ErrorStage::Encoding,
                "artifact payload exceeds the directory count limit",
            ));
        }
        if artifact.verification().bytes > limits.maximum_bytes {
            return Err(PageKnotError::new(
                "pageknot.export.size",
                ErrorStage::Encoding,
                "artifact payload exceeds the export byte limit",
            ));
        }
    }
    Ok(())
}

pub(super) fn plan_destinations(
    output_directory: &Path,
    conflict: ConflictPolicy,
    prepared: &[PreparedArtifact],
) -> Result<(CommitMode, Vec<DestinationPlan>)> {
    let mut plans = Vec::with_capacity(prepared.len());
    for artifact in prepared {
        let name = artifact.name();
        let output_kind = artifact.output_kind();
        let (destination, existed, previous_kind) =
            resolve_destination(output_directory, name, output_kind, conflict)?;
        let destination_name = destination
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .ok_or_else(|| {
                plan_error(
                    ErrorStage::Validation,
                    "artifact export destination name is not valid UTF-8",
                )
            })?;
        plans.push(DestinationPlan {
            destination,
            entry: JournalEntry {
                destination_name,
                kind: artifact.verification().kind,
                output_kind,
                existed,
                previous_kind,
                verification: artifact.verification().clone(),
                directory_files: artifact.file_count(),
                entrypoint: artifact.entrypoint().map(str::to_owned),
            },
        });
    }
    let mode = if output_directory_is_empty(output_directory, ErrorStage::Validation)? {
        CommitMode::DirectorySwap
    } else {
        CommitMode::Entries
    };
    Ok((mode, plans))
}

pub(super) fn stage_artifacts(
    root: &Path,
    prepared: Vec<PreparedArtifact>,
    plans: &[DestinationPlan],
    limits: ArtifactTransactionLimits,
) -> Result<Vec<StagedArtifact>> {
    if prepared.len() != plans.len() {
        return Err(plan_error(
            ErrorStage::Internal,
            "artifact transaction plan does not match its prepared artifacts",
        ));
    }
    let staged_output = root.join("staged-output");
    create_directory(&staged_output, ErrorStage::Encoding)?;
    let mut staged = Vec::with_capacity(plans.len());
    for (artifact, plan) in prepared.into_iter().zip(plans) {
        let staged_path = staged_output.join(&plan.entry.destination_name);
        match artifact.into_payload() {
            PreparedPayload::File(bytes) => {
                write_new_file(&staged_path, &bytes, ErrorStage::Encoding)?;
            }
            PreparedPayload::Directory { directory, .. } => {
                validate_directory_limits(&directory, limits, ErrorStage::Encoding)?;
                stage_directory(&staged_path, &directory)?;
            }
        }
        staged.push(StagedArtifact {
            destination: plan.destination.clone(),
            staged_path,
            entry: plan.entry.clone(),
        });
    }
    sync_directory(&staged_output, ErrorStage::Encoding)?;
    Ok(staged)
}

pub(super) fn validate_staged_artifacts(
    artifacts: &[StagedArtifact],
    limits: ArtifactTransactionLimits,
) -> Result<()> {
    for artifact in artifacts {
        validate_entry_path(&artifact.staged_path, &artifact.entry, limits)?;
    }
    Ok(())
}

pub(super) fn validate_entry_path(
    path: &Path,
    entry: &JournalEntry,
    limits: ArtifactTransactionLimits,
) -> Result<()> {
    match entry.output_kind {
        OutputKind::File => validate_file(
            path,
            entry.verification.bytes,
            entry.verification.sha256,
            limits.maximum_bytes,
        ),
        OutputKind::Directory => validate_directory(
            path,
            entry.directory_files,
            entry.verification.bytes,
            entry.verification.sha256,
            limits,
        ),
    }
}

pub(super) fn path_matches_entry(
    path: &Path,
    entry: &JournalEntry,
    limits: ArtifactTransactionLimits,
) -> bool {
    validate_entry_path(path, entry, limits).is_ok()
}

pub(super) fn output_matches_complete_set(
    output_directory: &Path,
    entries: &[JournalEntry],
    limits: ArtifactTransactionLimits,
) -> bool {
    validate_complete_output_set(output_directory, entries, limits).is_ok()
}

pub(super) fn validate_complete_output_set(
    output_directory: &Path,
    entries: &[JournalEntry],
    limits: ArtifactTransactionLimits,
) -> Result<()> {
    if !direct_directory(output_directory, ErrorStage::Commit)? {
        return Err(plan_error(
            ErrorStage::Commit,
            "committed artifact output must be a directly addressed directory",
        ));
    }
    let expected = entries
        .iter()
        .map(|entry| entry.destination_name.as_str())
        .collect::<BTreeSet<_>>();
    let actual = fs::read_dir(output_directory)
        .map_err(|error| {
            plan_io_error(
                ErrorStage::Commit,
                "failed to enumerate the committed artifact output",
                error,
            )
        })?
        .map(|entry| {
            entry
                .map_err(|error| {
                    plan_io_error(
                        ErrorStage::Commit,
                        "failed to enumerate the committed artifact output",
                        error,
                    )
                })
                .and_then(|entry| {
                    entry.file_name().into_string().map_err(|_| {
                        plan_error(
                            ErrorStage::Commit,
                            "committed artifact output contains a non-UTF-8 name",
                        )
                    })
                })
        })
        .collect::<Result<BTreeSet<_>>>()?;
    if actual.iter().map(String::as_str).collect::<BTreeSet<_>>() != expected {
        return Err(plan_error(
            ErrorStage::Commit,
            "committed artifact output set does not match its transaction journal",
        ));
    }
    for entry in entries {
        validate_entry_path(
            &output_directory.join(&entry.destination_name),
            entry,
            limits,
        )?;
    }
    Ok(())
}

pub(super) fn recheck_destinations(
    conflict: ConflictPolicy,
    artifacts: &[StagedArtifact],
) -> Result<()> {
    for artifact in artifacts {
        let current = direct_metadata(&artifact.destination)?;
        match conflict {
            ConflictPolicy::Fail | ConflictPolicy::Uniquify => {
                if current.is_some() {
                    return Err(output_exists_error(&artifact.destination));
                }
            }
            ConflictPolicy::Replace => match (artifact.entry.existed, current) {
                (false, None) => {}
                (false, Some(_)) => return Err(output_exists_error(&artifact.destination)),
                (true, Some(metadata)) => {
                    let current_kind = metadata_kind(&metadata).ok_or_else(|| {
                        plan_error(
                            ErrorStage::Commit,
                            "artifact export replacement changed to an unsupported file type",
                        )
                    })?;
                    if Some(current_kind) != artifact.entry.previous_kind {
                        return Err(plan_error(
                            ErrorStage::Commit,
                            "artifact export destination kind changed before commit",
                        ));
                    }
                }
                (true, None) => {
                    return Err(plan_error(
                        ErrorStage::Commit,
                        "artifact export destination disappeared before commit",
                    ));
                }
            },
        }
    }
    Ok(())
}

fn resolve_destination(
    output_directory: &Path,
    requested_name: &str,
    output_kind: OutputKind,
    conflict: ConflictPolicy,
) -> Result<(PathBuf, bool, Option<OutputKind>)> {
    let requested = output_directory.join(requested_name);
    match conflict {
        ConflictPolicy::Fail => {
            if direct_metadata(&requested)?.is_some() {
                return Err(output_exists_error(&requested));
            }
            Ok((requested, false, None))
        }
        ConflictPolicy::Replace => {
            let metadata = direct_metadata(&requested)?;
            let previous_kind = metadata.as_ref().and_then(metadata_kind);
            if metadata.is_some() && previous_kind.is_none() {
                return Err(plan_error(
                    ErrorStage::Validation,
                    "artifact export replacement must be a regular file or directory",
                ));
            }
            if output_kind == OutputKind::File && previous_kind == Some(OutputKind::Directory) {
                return Err(plan_error(
                    ErrorStage::Validation,
                    "file artifact replacement requires a regular file",
                ));
            }
            Ok((requested, metadata.is_some(), previous_kind))
        }
        ConflictPolicy::Uniquify => {
            for suffix in 0..=10_000_u32 {
                let candidate = unique_candidate(&requested, suffix, output_kind);
                if direct_metadata(&candidate)?.is_none() {
                    return Ok((candidate, false, None));
                }
            }
            Err(PageKnotError::new(
                "pageknot.output.uniquify",
                ErrorStage::Validation,
                "portable artifact variant filename suffixes are exhausted",
            ))
        }
    }
}

fn stage_directory(path: &Path, directory: &ArtifactDirectory) -> Result<()> {
    create_directory(path, ErrorStage::Encoding)?;
    let mut directories = directory
        .files()
        .keys()
        .flat_map(|relative| {
            let mut parent = Path::new(relative).parent();
            let mut parents = Vec::new();
            while let Some(current) = parent.filter(|current| !current.as_os_str().is_empty()) {
                parents.push(current.to_owned());
                parent = current.parent();
            }
            parents
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    directories.sort_by(|left, right| {
        left.components()
            .count()
            .cmp(&right.components().count())
            .then_with(|| left.cmp(right))
    });
    for relative in &directories {
        create_directory(&path.join(relative), ErrorStage::Encoding)?;
    }
    for (relative, bytes) in directory.files() {
        write_new_file(&path.join(relative), bytes, ErrorStage::Encoding)?;
    }
    for relative in directories.iter().rev() {
        sync_directory(&path.join(relative), ErrorStage::Encoding)?;
    }
    sync_directory(path, ErrorStage::Encoding)
}

fn validate_directory_limits(
    directory: &ArtifactDirectory,
    limits: ArtifactTransactionLimits,
    stage: ErrorStage,
) -> Result<()> {
    if directory.file_count() > limits.maximum_files {
        return Err(PageKnotError::new(
            "pageknot.export.files",
            stage,
            "artifact directory exceeds the file count limit",
        ));
    }
    if directory.bytes() > limits.maximum_bytes {
        return Err(PageKnotError::new(
            "pageknot.export.size",
            stage,
            "artifact directory exceeds the aggregate byte limit",
        ));
    }
    Ok(())
}

fn validate_file(
    path: &Path,
    expected_bytes: u64,
    expected_sha256: ContentDigest,
    maximum_bytes: u64,
) -> Result<()> {
    let metadata = direct_regular_file(path, ErrorStage::Commit)?;
    if metadata.len() > maximum_bytes || metadata.len() != expected_bytes {
        return Err(plan_error(
            ErrorStage::Commit,
            "staged artifact variant byte count changed before commit",
        ));
    }
    let (bytes, sha256) = hash_file(path, maximum_bytes)?;
    if bytes != expected_bytes {
        return Err(plan_error(
            ErrorStage::Commit,
            "staged artifact variant byte count changed before commit",
        ));
    }
    if sha256 != expected_sha256 {
        return Err(PageKnotError::new(
            "pageknot.artifact.digest",
            ErrorStage::Commit,
            "staged artifact variant digest changed before commit",
        ));
    }
    Ok(())
}

fn validate_directory(
    path: &Path,
    expected_files: usize,
    expected_bytes: u64,
    expected_sha256: ContentDigest,
    limits: ArtifactTransactionLimits,
) -> Result<()> {
    if expected_files == 0 || expected_files > limits.maximum_files {
        return Err(PageKnotError::new(
            "pageknot.export.files",
            ErrorStage::Commit,
            "staged artifact directory file count changed before commit",
        ));
    }
    let files = collect_directory_files(path, limits.maximum_files)?;
    if files.len() != expected_files {
        return Err(PageKnotError::new(
            "pageknot.export.files",
            ErrorStage::Commit,
            "staged artifact directory file count changed before commit",
        ));
    }
    let mut hasher = Sha256::new();
    let mut total_bytes = 0_u64;
    for (relative, file_path) in files {
        let remaining = limits.maximum_bytes.saturating_sub(total_bytes);
        hash_directory_path(
            &mut hasher,
            &mut total_bytes,
            &relative,
            &file_path,
            remaining,
        )?;
    }
    if total_bytes != expected_bytes || total_bytes > limits.maximum_bytes {
        return Err(PageKnotError::new(
            "pageknot.export.size",
            ErrorStage::Commit,
            "staged artifact directory byte count changed before commit",
        ));
    }
    if ContentDigest::from_bytes(hasher.finalize().into()) != expected_sha256 {
        return Err(PageKnotError::new(
            "pageknot.artifact.digest",
            ErrorStage::Commit,
            "staged artifact directory digest changed before commit",
        ));
    }
    Ok(())
}

fn collect_directory_files(root: &Path, maximum_files: usize) -> Result<Vec<(String, PathBuf)>> {
    let metadata = fs::symlink_metadata(root).map_err(|error| {
        plan_io_error(
            ErrorStage::Commit,
            "failed to inspect the staged artifact directory",
            error,
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(plan_error(
            ErrorStage::Commit,
            "staged artifact output must be a directly addressed directory",
        ));
    }
    let mut pending = vec![(root.to_owned(), String::new())];
    let mut directories = BTreeSet::new();
    let mut files = Vec::new();
    while let Some((directory, relative_directory)) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|error| {
            plan_io_error(
                ErrorStage::Commit,
                "failed to enumerate a staged artifact directory",
                error,
            )
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                plan_io_error(
                    ErrorStage::Commit,
                    "failed to enumerate a staged artifact directory",
                    error,
                )
            })?;
            let name = entry.file_name().into_string().map_err(|_| {
                plan_error(
                    ErrorStage::Commit,
                    "staged artifact directory contains a non-UTF-8 name",
                )
            })?;
            if name.contains('\\') {
                return Err(plan_error(
                    ErrorStage::Commit,
                    "staged artifact directory contains a non-portable name",
                ));
            }
            let relative = if relative_directory.is_empty() {
                name
            } else {
                format!("{relative_directory}/{name}")
            };
            let metadata = fs::symlink_metadata(entry.path()).map_err(|error| {
                plan_io_error(
                    ErrorStage::Commit,
                    "failed to inspect a staged artifact directory entry",
                    error,
                )
            })?;
            if metadata.file_type().is_symlink() {
                return Err(plan_error(
                    ErrorStage::Commit,
                    "staged artifact directory must not contain symbolic links",
                ));
            }
            if metadata.is_file() {
                files.push((relative, entry.path()));
                if files.len() > maximum_files {
                    return Err(PageKnotError::new(
                        "pageknot.export.files",
                        ErrorStage::Commit,
                        "staged artifact directory exceeds the file count limit",
                    ));
                }
            } else if metadata.is_dir() {
                if !directories.insert(relative.clone()) || directories.len() > maximum_files {
                    return Err(PageKnotError::new(
                        "pageknot.export.files",
                        ErrorStage::Commit,
                        "staged artifact directory exceeds the directory count limit",
                    ));
                }
                pending.push((entry.path(), relative));
            } else {
                return Err(plan_error(
                    ErrorStage::Commit,
                    "staged artifact directory contains an unsupported filesystem entry",
                ));
            }
        }
    }
    let expected_directories = files
        .iter()
        .flat_map(|(relative, _)| {
            let mut parent = Path::new(relative).parent();
            let mut parents = Vec::new();
            while let Some(current) = parent.filter(|current| !current.as_os_str().is_empty()) {
                parents.push(current.to_string_lossy().into_owned());
                parent = current.parent();
            }
            parents
        })
        .collect::<BTreeSet<_>>();
    if directories != expected_directories {
        return Err(plan_error(
            ErrorStage::Commit,
            "staged artifact directory contains an empty directory",
        ));
    }
    files.sort_by(|(left, _), (right, _)| {
        left.matches('/')
            .count()
            .cmp(&right.matches('/').count())
            .then_with(|| left.cmp(right))
    });
    Ok(files)
}

fn hash_directory_path(
    hasher: &mut Sha256,
    total_bytes: &mut u64,
    relative_path: &str,
    path: &Path,
    maximum_bytes: u64,
) -> Result<()> {
    let metadata = direct_regular_file(path, ErrorStage::Commit)?;
    if metadata.len() > maximum_bytes {
        return Err(PageKnotError::new(
            "pageknot.export.size",
            ErrorStage::Commit,
            "staged artifact directory file exceeds the export byte limit",
        ));
    }
    hasher.update(relative_path.as_bytes());
    hasher.update([0]);
    hasher.update(metadata.len().to_le_bytes());
    let mut file = File::open(path).map_err(|error| {
        plan_io_error(
            ErrorStage::Commit,
            "failed to read a staged artifact directory file",
            error,
        )
    })?;
    let read = hash_open_file(&mut file, hasher, maximum_bytes)?;
    if read != metadata.len() {
        return Err(plan_error(
            ErrorStage::Commit,
            "staged artifact directory file byte count changed before commit",
        ));
    }
    *total_bytes = total_bytes.saturating_add(read);
    Ok(())
}

fn hash_file(path: &Path, maximum_bytes: u64) -> Result<(u64, ContentDigest)> {
    let mut file = File::open(path).map_err(|error| {
        plan_io_error(
            ErrorStage::Commit,
            "failed to read a staged artifact variant",
            error,
        )
    })?;
    let mut hasher = Sha256::new();
    let bytes = hash_open_file(&mut file, &mut hasher, maximum_bytes)?;
    Ok((bytes, ContentDigest::from_bytes(hasher.finalize().into())))
}

fn hash_open_file(file: &mut File, hasher: &mut Sha256, maximum_bytes: u64) -> Result<u64> {
    let mut total = 0_u64;
    let mut buffer = [0_u8; FILE_READ_BUFFER_BYTES];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            plan_io_error(
                ErrorStage::Commit,
                "failed to read a staged artifact variant",
                error,
            )
        })?;
        if read == 0 {
            return Ok(total);
        }
        total = total.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        if total > maximum_bytes {
            return Err(PageKnotError::new(
                "pageknot.export.size",
                ErrorStage::Commit,
                "staged artifact variant exceeds the export byte limit",
            ));
        }
        hasher.update(&buffer[..read]);
    }
}

fn direct_regular_file(path: &Path, stage: ErrorStage) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        plan_io_error(stage, "failed to inspect an artifact variant file", error)
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(plan_error(
            stage,
            "artifact variant must be a directly addressed regular file",
        ));
    }
    Ok(metadata)
}

fn direct_metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(plan_error(
            ErrorStage::Validation,
            "artifact export destination must not be a symbolic link",
        )),
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(plan_io_error(
            ErrorStage::Validation,
            "failed to inspect an artifact export destination",
            error,
        )),
    }
}

fn metadata_kind(metadata: &fs::Metadata) -> Option<OutputKind> {
    if metadata.is_file() {
        Some(OutputKind::File)
    } else if metadata.is_dir() {
        Some(OutputKind::Directory)
    } else {
        None
    }
}

pub(super) fn output_directory_is_empty(path: &Path, stage: ErrorStage) -> Result<bool> {
    if !direct_directory(path, stage)? {
        return Err(plan_error(
            stage,
            "artifact export output must be a directly addressed directory",
        ));
    }
    let mut entries = fs::read_dir(path).map_err(|error| {
        plan_io_error(
            stage,
            "failed to enumerate the artifact export directory",
            error,
        )
    })?;
    match entries.next() {
        None => Ok(true),
        Some(Ok(_)) => Ok(false),
        Some(Err(error)) => Err(plan_io_error(
            stage,
            "failed to enumerate the artifact export directory",
            error,
        )),
    }
}

fn unique_candidate(requested: &Path, suffix: u32, output_kind: OutputKind) -> PathBuf {
    if suffix == 0 {
        return requested.to_owned();
    }
    let parent = requested.parent().unwrap_or_else(|| Path::new(""));
    if output_kind == OutputKind::Directory {
        let name = requested
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("capture-directory");
        return parent.join(format!("{name}-{suffix}"));
    }
    let stem = requested
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("capture");
    let name = requested
        .extension()
        .and_then(|extension| extension.to_str())
        .map_or_else(
            || format!("{stem}-{suffix}"),
            |extension| format!("{stem}-{suffix}.{extension}"),
        );
    parent.join(name)
}

fn direct_directory(path: &Path, stage: ErrorStage) -> Result<bool> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        plan_io_error(
            stage,
            "failed to inspect an artifact output directory",
            error,
        )
    })?;
    Ok(!metadata.file_type().is_symlink() && metadata.is_dir())
}

fn output_exists_error(path: &Path) -> PageKnotError {
    PageKnotError::new(
        "pageknot.output.exists",
        ErrorStage::Validation,
        format!(
            "artifact export destination already exists: `{}`",
            path.display()
        ),
    )
}

fn plan_error(stage: ErrorStage, message: impl Into<String>) -> PageKnotError {
    PageKnotError::new("pageknot.export.output", stage, message)
}

fn plan_io_error(stage: ErrorStage, message: &'static str, error: std::io::Error) -> PageKnotError {
    plan_error(stage, format!("{message}: {error}"))
        .with_detail("ioKind", format!("{:?}", error.kind()))
}
