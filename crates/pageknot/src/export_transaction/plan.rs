use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};

use pageknot_export::MarkdownBundle;
use pageknot_model::{
    ArtifactVariantKind, ConflictPolicy, ContentDigest, ErrorStage, PageKnotError, Result,
};
use sha2::{Digest as _, Sha256};

use super::PreparedVariant;
use super::durable::{create_directory, sync_directory, write_new_file};
use super::journal::{CommitMode, JournalEntry, OutputKind};

const FILE_READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub(super) struct DestinationPlan {
    pub(super) destination: PathBuf,
    pub(super) entry: JournalEntry,
}

#[derive(Clone, Debug)]
pub(super) struct StagedVariant {
    pub(super) destination: PathBuf,
    pub(super) staged_path: PathBuf,
    pub(super) entry: JournalEntry,
}

pub(super) fn plan_destinations(
    output_directory: &Path,
    conflict: ConflictPolicy,
    prepared: &[PreparedVariant],
) -> Result<(CommitMode, Vec<DestinationPlan>)> {
    let mut plans = Vec::with_capacity(prepared.len());
    for variant in prepared {
        let (name, kind, output_kind, verification, markdown_assets) = match variant {
            PreparedVariant::File {
                name,
                kind,
                verification,
                ..
            } => (
                name.as_str(),
                *kind,
                OutputKind::File,
                verification.clone(),
                0,
            ),
            PreparedVariant::Markdown {
                name,
                bundle,
                verification,
            } => (
                name.as_str(),
                ArtifactVariantKind::Markdown,
                OutputKind::Directory,
                verification.clone(),
                bundle.assets.len(),
            ),
        };
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
                kind,
                output_kind,
                existed,
                previous_kind,
                verification,
                markdown_assets,
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

pub(super) fn stage_variants(
    root: &Path,
    prepared: Vec<PreparedVariant>,
    plans: &[DestinationPlan],
    maximum_assets: usize,
    maximum_bytes: u64,
) -> Result<Vec<StagedVariant>> {
    if prepared.len() != plans.len() {
        return Err(plan_error(
            ErrorStage::Internal,
            "artifact export plan does not match its prepared variants",
        ));
    }
    let staged_output = root.join("staged-output");
    create_directory(&staged_output, ErrorStage::Encoding)?;
    let mut staged = Vec::with_capacity(plans.len());
    for (variant, plan) in prepared.into_iter().zip(plans) {
        let staged_path = staged_output.join(&plan.entry.destination_name);
        match variant {
            PreparedVariant::File { bytes, .. } => {
                write_new_file(&staged_path, &bytes, ErrorStage::Encoding)?;
            }
            PreparedVariant::Markdown { bundle, .. } => {
                validate_markdown_bundle_limits(
                    &bundle,
                    maximum_assets,
                    maximum_bytes,
                    ErrorStage::Encoding,
                )?;
                stage_markdown_bundle(&staged_path, &bundle)?;
            }
        }
        staged.push(StagedVariant {
            destination: plan.destination.clone(),
            staged_path,
            entry: plan.entry.clone(),
        });
    }
    sync_directory(&staged_output, ErrorStage::Encoding)?;
    Ok(staged)
}

pub(super) fn validate_staged_variants(
    variants: &[StagedVariant],
    maximum_assets: usize,
    maximum_bytes: u64,
) -> Result<()> {
    for variant in variants {
        validate_entry_path(
            &variant.staged_path,
            &variant.entry,
            maximum_assets,
            maximum_bytes,
        )?;
    }
    Ok(())
}

pub(super) fn validate_entry_path(
    path: &Path,
    entry: &JournalEntry,
    maximum_assets: usize,
    maximum_bytes: u64,
) -> Result<()> {
    match entry.output_kind {
        OutputKind::File => validate_file(
            path,
            entry.verification.bytes,
            entry.verification.sha256,
            maximum_bytes,
        ),
        OutputKind::Directory => validate_markdown(
            path,
            entry.markdown_assets,
            entry.verification.bytes,
            entry.verification.sha256,
            maximum_assets,
            maximum_bytes,
        ),
    }
}

pub(super) fn path_matches_entry(
    path: &Path,
    entry: &JournalEntry,
    maximum_assets: usize,
    maximum_bytes: u64,
) -> bool {
    validate_entry_path(path, entry, maximum_assets, maximum_bytes).is_ok()
}

pub(super) fn output_matches_complete_set(
    output_directory: &Path,
    entries: &[JournalEntry],
    maximum_assets: usize,
    maximum_bytes: u64,
) -> bool {
    validate_complete_output_set(output_directory, entries, maximum_assets, maximum_bytes).is_ok()
}

pub(super) fn validate_complete_output_set(
    output_directory: &Path,
    entries: &[JournalEntry],
    maximum_assets: usize,
    maximum_bytes: u64,
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
            maximum_assets,
            maximum_bytes,
        )?;
    }
    Ok(())
}

pub(super) fn recheck_destinations(
    conflict: ConflictPolicy,
    variants: &[StagedVariant],
) -> Result<()> {
    for variant in variants {
        let current = direct_metadata(&variant.destination)?;
        match conflict {
            ConflictPolicy::Fail | ConflictPolicy::Uniquify => {
                if current.is_some() {
                    return Err(output_exists_error(&variant.destination));
                }
            }
            ConflictPolicy::Replace => match (variant.entry.existed, current) {
                (false, None) => {}
                (false, Some(_)) => return Err(output_exists_error(&variant.destination)),
                (true, Some(metadata)) => {
                    let current_kind = metadata_kind(&metadata).ok_or_else(|| {
                        plan_error(
                            ErrorStage::Commit,
                            "artifact export replacement changed to an unsupported file type",
                        )
                    })?;
                    if Some(current_kind) != variant.entry.previous_kind {
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

pub(super) fn markdown_bundle_digest(bundle: &MarkdownBundle) -> (u64, ContentDigest) {
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    hash_bundle_bytes(&mut hasher, &mut bytes, "index.md", &bundle.markdown);
    for (path, content) in &bundle.assets {
        hash_bundle_bytes(&mut hasher, &mut bytes, path, content);
    }
    (bytes, ContentDigest::from_bytes(hasher.finalize().into()))
}

pub(super) fn validate_markdown_bundle_limits(
    bundle: &MarkdownBundle,
    maximum_assets: usize,
    maximum_bytes: u64,
    stage: ErrorStage,
) -> Result<()> {
    if bundle.assets.len() > maximum_assets {
        return Err(markdown_file_limit_error(stage));
    }
    let mut bytes = u64::try_from(bundle.markdown.len()).unwrap_or(u64::MAX);
    for content in bundle.assets.values() {
        bytes = bytes.saturating_add(u64::try_from(content.len()).unwrap_or(u64::MAX));
        if bytes > maximum_bytes {
            return Err(markdown_byte_limit_error(stage));
        }
    }
    if bytes > maximum_bytes {
        return Err(markdown_byte_limit_error(stage));
    }
    Ok(())
}

pub(super) fn markdown_file_limit_error(stage: ErrorStage) -> PageKnotError {
    PageKnotError::new(
        "pageknot.export.files",
        stage,
        "Markdown bundle exceeds the asset count limit",
    )
}

pub(super) fn markdown_byte_limit_error(stage: ErrorStage) -> PageKnotError {
    PageKnotError::new(
        "pageknot.export.size",
        stage,
        "Markdown bundle exceeds the aggregate byte limit",
    )
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

fn stage_markdown_bundle(path: &Path, bundle: &MarkdownBundle) -> Result<()> {
    create_directory(path, ErrorStage::Encoding)?;
    write_new_file(
        &path.join("index.md"),
        &bundle.markdown,
        ErrorStage::Encoding,
    )?;
    if !bundle.assets.is_empty() {
        let assets = path.join("assets");
        create_directory(&assets, ErrorStage::Encoding)?;
        for (relative, bytes) in &bundle.assets {
            let name = markdown_asset_name(relative)?;
            write_new_file(&assets.join(name), bytes, ErrorStage::Encoding)?;
        }
        sync_directory(&assets, ErrorStage::Encoding)?;
    }
    sync_directory(path, ErrorStage::Encoding)
}

fn markdown_asset_name(relative: &str) -> Result<&str> {
    let Some(name) = relative.strip_prefix("assets/") else {
        return Err(PageKnotError::new(
            "pageknot.export.markdown_asset",
            ErrorStage::Encoding,
            "Markdown asset path is outside its asset directory",
        ));
    };
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(PageKnotError::new(
            "pageknot.export.markdown_asset",
            ErrorStage::Encoding,
            "Markdown asset path must be one directly addressed file",
        ));
    }
    Ok(name)
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

fn validate_markdown(
    path: &Path,
    expected_assets: usize,
    expected_bytes: u64,
    expected_sha256: ContentDigest,
    maximum_assets: usize,
    maximum_bytes: u64,
) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        plan_io_error(
            ErrorStage::Commit,
            "failed to inspect the staged Markdown directory",
            error,
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(plan_error(
            ErrorStage::Commit,
            "staged Markdown output must be a directly addressed directory",
        ));
    }
    let mut root_entries = directory_names(path)?;
    root_entries.sort();
    let expected_root = if expected_assets == 0 {
        vec!["index.md".to_owned()]
    } else {
        vec!["assets".to_owned(), "index.md".to_owned()]
    };
    if root_entries != expected_root {
        return Err(plan_error(
            ErrorStage::Commit,
            "staged Markdown output structure changed before commit",
        ));
    }
    let mut hasher = Sha256::new();
    let mut total_bytes = 0_u64;
    hash_bundle_path(
        &mut hasher,
        &mut total_bytes,
        "index.md",
        &path.join("index.md"),
        maximum_bytes,
    )?;
    if expected_assets != 0 {
        let assets_path = path.join("assets");
        let assets_metadata = fs::symlink_metadata(&assets_path).map_err(|error| {
            plan_io_error(
                ErrorStage::Commit,
                "failed to inspect staged Markdown assets",
                error,
            )
        })?;
        if assets_metadata.file_type().is_symlink() || !assets_metadata.is_dir() {
            return Err(plan_error(
                ErrorStage::Commit,
                "staged Markdown assets must be a directly addressed directory",
            ));
        }
        let mut assets = directory_paths(&assets_path)?;
        assets.sort_by(|left, right| left.0.cmp(&right.0));
        if assets.len() != expected_assets || assets.len() > maximum_assets {
            return Err(PageKnotError::new(
                "pageknot.export.files",
                ErrorStage::Commit,
                "staged Markdown asset count changed before commit",
            ));
        }
        for (name, asset_path) in assets {
            let remaining = maximum_bytes.saturating_sub(total_bytes);
            hash_bundle_path(
                &mut hasher,
                &mut total_bytes,
                &format!("assets/{name}"),
                &asset_path,
                remaining,
            )?;
        }
    }
    if total_bytes != expected_bytes || total_bytes > maximum_bytes {
        return Err(PageKnotError::new(
            "pageknot.export.size",
            ErrorStage::Commit,
            "staged Markdown byte count changed before commit",
        ));
    }
    if ContentDigest::from_bytes(hasher.finalize().into()) != expected_sha256 {
        return Err(PageKnotError::new(
            "pageknot.artifact.digest",
            ErrorStage::Commit,
            "staged Markdown digest changed before commit",
        ));
    }
    Ok(())
}

fn hash_bundle_path(
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
            "staged Markdown file exceeds the export byte limit",
        ));
    }
    hasher.update(relative_path.as_bytes());
    hasher.update([0]);
    hasher.update(metadata.len().to_le_bytes());
    let mut file = File::open(path).map_err(|error| {
        plan_io_error(
            ErrorStage::Commit,
            "failed to read a staged Markdown file",
            error,
        )
    })?;
    let read = hash_open_file(&mut file, hasher, maximum_bytes)?;
    if read != metadata.len() {
        return Err(plan_error(
            ErrorStage::Commit,
            "staged Markdown file byte count changed before commit",
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

fn hash_bundle_bytes(hasher: &mut Sha256, bytes: &mut u64, path: &str, content: &[u8]) {
    hasher.update(path.as_bytes());
    hasher.update([0]);
    hasher.update(
        u64::try_from(content.len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    hasher.update(content);
    *bytes = bytes.saturating_add(u64::try_from(content.len()).unwrap_or(u64::MAX));
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
            .unwrap_or("capture-markdown");
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

fn directory_names(path: &Path) -> Result<Vec<String>> {
    fs::read_dir(path)
        .map_err(|error| {
            plan_io_error(
                ErrorStage::Commit,
                "failed to enumerate an artifact variant directory",
                error,
            )
        })?
        .map(|entry| {
            entry
                .map_err(|error| {
                    plan_io_error(
                        ErrorStage::Commit,
                        "failed to enumerate an artifact variant directory",
                        error,
                    )
                })?
                .file_name()
                .into_string()
                .map_err(|_| {
                    plan_error(
                        ErrorStage::Commit,
                        "artifact variant directory contains a non-UTF-8 name",
                    )
                })
        })
        .collect()
}

fn directory_paths(path: &Path) -> Result<Vec<(String, PathBuf)>> {
    fs::read_dir(path)
        .map_err(|error| {
            plan_io_error(
                ErrorStage::Commit,
                "failed to enumerate an artifact variant directory",
                error,
            )
        })?
        .map(|entry| {
            let entry = entry.map_err(|error| {
                plan_io_error(
                    ErrorStage::Commit,
                    "failed to enumerate an artifact variant directory",
                    error,
                )
            })?;
            let name = entry.file_name().into_string().map_err(|_| {
                plan_error(
                    ErrorStage::Commit,
                    "artifact variant directory contains a non-UTF-8 name",
                )
            })?;
            Ok((name, entry.path()))
        })
        .collect()
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
