use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use camino::{Utf8Path, Utf8PathBuf};
use offprint_model::{
    CaptureArtifact, ConflictPolicy, ContentDigest, ERROR_CODE_REGISTRY, ErrorStage, OffprintError,
    Result,
};
use sha2::{Digest as _, Sha256};
use tempfile::{Builder, NamedTempFile};

#[derive(Debug)]
pub struct FileArtifactWriter {
    destination: Utf8PathBuf,
    conflict: ConflictPolicy,
    staging: NamedTempFile,
    bytes: u64,
    hasher: Sha256,
}

impl FileArtifactWriter {
    pub fn create(destination: impl Into<Utf8PathBuf>, conflict: ConflictPolicy) -> Result<Self> {
        let destination = destination.into();
        validate_destination(&destination, conflict)?;
        let parent = destination
            .parent()
            .filter(|path| !path.as_str().is_empty())
            .unwrap_or(Utf8Path::new("."));
        let file_name = destination.file_name().ok_or_else(|| {
            output_error(
                "offprint.output.path",
                ErrorStage::Validation,
                "output path must contain a file name",
            )
        })?;
        let staging = Builder::new()
            .prefix(&format!(".{file_name}.offprint-"))
            .tempfile_in(parent)
            .map_err(|error| {
                registered_output_io_error(
                    "offprint.output.staging",
                    "failed to create the staging artifact",
                    error,
                )
            })?;
        Ok(Self {
            destination,
            conflict,
            staging,
            bytes: 0,
            hasher: Sha256::new(),
        })
    }

    pub fn finish(mut self) -> Result<StagedFileArtifact> {
        self.staging.as_file_mut().flush().map_err(|error| {
            output_io_error(
                "offprint.output.flush",
                ErrorStage::Encoding,
                "failed to flush the staging artifact",
                error,
            )
        })?;
        self.staging.as_file().sync_all().map_err(|error| {
            output_io_error(
                "offprint.output.sync",
                ErrorStage::Encoding,
                "failed to synchronize the staging artifact",
                error,
            )
        })?;
        Ok(StagedFileArtifact {
            destination: self.destination,
            conflict: self.conflict,
            staging: self.staging,
            bytes: self.bytes,
            sha256: ContentDigest::from_bytes(self.hasher.finalize().into()),
        })
    }
}

impl Write for FileArtifactWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.staging.as_file_mut().write(buffer)?;
        self.bytes = self
            .bytes
            .saturating_add(u64::try_from(written).unwrap_or(u64::MAX));
        self.hasher.update(&buffer[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.staging.as_file_mut().flush()
    }
}

#[derive(Debug)]
pub struct StagedFileArtifact {
    destination: Utf8PathBuf,
    conflict: ConflictPolicy,
    staging: NamedTempFile,
    bytes: u64,
    sha256: ContentDigest,
}

impl StagedFileArtifact {
    #[must_use]
    pub fn path(&self) -> &Path {
        self.staging.path()
    }

    #[must_use]
    pub fn destination(&self) -> &Utf8Path {
        &self.destination
    }

    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    #[must_use]
    pub const fn sha256(&self) -> ContentDigest {
        self.sha256
    }

    pub fn read_all(&mut self, maximum_bytes: u64) -> Result<Vec<u8>> {
        if self.bytes > maximum_bytes {
            return Err(output_error(
                "offprint.artifact.size",
                ErrorStage::Verification,
                "staging artifact exceeds the verification byte limit",
            ));
        }
        let file = self.staging.as_file_mut();
        file.seek(SeekFrom::Start(0)).map_err(|error| {
            output_io_error(
                "offprint.output.read",
                ErrorStage::Verification,
                "failed to rewind the staging artifact",
                error,
            )
        })?;
        let capacity = usize::try_from(self.bytes).map_err(|error| {
            output_error(
                "offprint.artifact.size",
                ErrorStage::Verification,
                format!("staging artifact cannot fit in memory: {error}"),
            )
        })?;
        let mut bytes = Vec::with_capacity(capacity);
        file.take(maximum_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| {
                output_io_error(
                    "offprint.output.read",
                    ErrorStage::Verification,
                    "failed to read the staging artifact",
                    error,
                )
            })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != self.bytes {
            return Err(output_error(
                "offprint.artifact.size",
                ErrorStage::Verification,
                "staging artifact byte count changed before verification",
            ));
        }
        Ok(bytes)
    }

    pub fn commit(self) -> Result<CaptureArtifact> {
        let parent = open_parent_directory(&self.destination)?;
        self.commit_with_parent_sync(|| sync_parent_directory(parent))
    }

    fn commit_with_parent_sync(
        mut self,
        sync_parent: impl FnOnce() -> io::Result<()>,
    ) -> Result<CaptureArtifact> {
        self.validate_staged_contents()?;
        let destination = match self.conflict {
            ConflictPolicy::Fail => {
                persist_noclobber(self.staging, self.destination.clone())?;
                self.destination
            }
            ConflictPolicy::Replace => {
                persist_replace(self.staging, &self.destination)?;
                self.destination
            }
            ConflictPolicy::Uniquify => persist_unique(self.staging, self.destination)?,
        };
        // The destination is visible after persist succeeds. A later directory
        // sync failure cannot be reported as an uncommitted transaction.
        let _durability = match sync_parent() {
            Ok(()) => CommitDurability::Synced,
            Err(_error) => CommitDurability::AppliedWithSyncWarning,
        };
        Ok(CaptureArtifact::File {
            path: destination.into(),
            bytes: self.bytes,
            sha256: self.sha256,
        })
    }

    fn validate_staged_contents(&mut self) -> Result<()> {
        validate_staging_path_identity(&self.staging)?;
        self.staging.as_file().sync_all().map_err(|error| {
            output_io_error(
                "offprint.output.sync",
                ErrorStage::Commit,
                "failed to synchronize the verified staging artifact",
                error,
            )
        })?;
        let file = self.staging.as_file_mut();
        file.seek(SeekFrom::Start(0)).map_err(|error| {
            output_io_error(
                "offprint.output.read",
                ErrorStage::Commit,
                "failed to rewind the verified staging artifact",
                error,
            )
        })?;

        let mut hasher = Sha256::new();
        let mut remaining = self.bytes;
        let mut buffer = [0_u8; 64 * 1024];
        while remaining > 0 {
            let maximum =
                usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
            let read = file.read(&mut buffer[..maximum]).map_err(|error| {
                output_io_error(
                    "offprint.output.read",
                    ErrorStage::Commit,
                    "failed to re-read the verified staging artifact",
                    error,
                )
            })?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            remaining = remaining.saturating_sub(u64::try_from(read).unwrap_or(u64::MAX));
        }
        let mut trailing = [0_u8; 1];
        let has_trailing_byte = file.read(&mut trailing).map_err(|error| {
            output_io_error(
                "offprint.output.read",
                ErrorStage::Commit,
                "failed to check the verified staging artifact length",
                error,
            )
        })? != 0;
        if remaining != 0 || has_trailing_byte {
            return Err(output_error(
                "offprint.artifact.size",
                ErrorStage::Commit,
                "staging artifact byte count changed before commit",
            ));
        }
        let sha256 = ContentDigest::from_bytes(hasher.finalize().into());
        if sha256 != self.sha256 {
            return Err(output_error(
                "offprint.artifact.digest",
                ErrorStage::Commit,
                "staging artifact digest changed before commit",
            ));
        }
        validate_staging_path_identity(&self.staging)?;
        Ok(())
    }
}

fn validate_staging_path_identity(staging: &NamedTempFile) -> Result<()> {
    let path_metadata = std::fs::symlink_metadata(staging.path()).map_err(|error| {
        output_io_error(
            "offprint.output.staging",
            ErrorStage::Commit,
            "failed to inspect the verified staging artifact path",
            error,
        )
    })?;
    let matches_path = staging_matches_path(staging, &path_metadata).map_err(|error| {
        output_io_error(
            "offprint.output.staging",
            ErrorStage::Commit,
            "failed to inspect the verified staging artifact",
            error,
        )
    })?;
    if path_metadata.file_type().is_symlink() || !path_metadata.is_file() || !matches_path {
        return Err(output_error(
            "offprint.output.staging",
            ErrorStage::Commit,
            "staging artifact path changed before commit",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn staging_matches_path(staging: &NamedTempFile, path: &std::fs::Metadata) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt as _;

    let file = staging.as_file().metadata()?;
    Ok(path.dev() == file.dev() && path.ino() == file.ino())
}

#[cfg(windows)]
fn staging_matches_path(staging: &NamedTempFile, _path: &std::fs::Metadata) -> io::Result<bool> {
    use std::os::windows::fs::OpenOptionsExt as _;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
    };

    // Keep both handles open while comparing identity and inspect the path's
    // reparse point itself so a replacement cannot redirect the comparison.
    let path_file = File::options()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(staging.path())?;
    let path = winapi_util::file::information(&path_file)?;
    let file = winapi_util::file::information(staging.as_file())?;
    Ok(
        path.file_attributes() & u64::from(FILE_ATTRIBUTE_REPARSE_POINT) == 0
            && path.volume_serial_number() == file.volume_serial_number()
            && path.file_index() == file.file_index(),
    )
}

#[cfg(not(any(unix, windows)))]
fn staging_matches_path(_staging: &NamedTempFile, _path: &std::fs::Metadata) -> io::Result<bool> {
    Ok(true)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CommitDurability {
    Synced,
    AppliedWithSyncWarning,
}

#[cfg(unix)]
fn open_parent_directory(destination: &Utf8Path) -> Result<Option<File>> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_str().is_empty())
        .unwrap_or(Utf8Path::new("."));
    match File::open(parent) {
        Ok(directory) => Ok(Some(directory)),
        Err(error) if directory_sync_is_unavailable(&error) => Ok(None),
        Err(error) => Err(output_io_error(
            "offprint.output.sync",
            ErrorStage::Commit,
            "failed to open the output directory for synchronization",
            error,
        )),
    }
}

#[cfg(not(unix))]
fn open_parent_directory(_destination: &Utf8Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_parent_directory(directory: Option<File>) -> io::Result<()> {
    match directory {
        Some(directory) => directory.sync_all(),
        None => Ok(()),
    }
}

#[cfg(not(unix))]
fn sync_parent_directory(_directory: ()) -> io::Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn directory_sync_is_unavailable(error: &std::io::Error) -> bool {
    // macOS protected folders can permit atomic rename while rejecting the
    // read handle needed for directory fsync with EPERM.
    error.raw_os_error() == Some(1)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn directory_sync_is_unavailable(_error: &std::io::Error) -> bool {
    false
}

fn validate_destination(destination: &Utf8Path, conflict: ConflictPolicy) -> Result<()> {
    if destination.as_str().is_empty() || destination.file_name().is_none() {
        return Err(output_error(
            "offprint.output.path",
            ErrorStage::Validation,
            "output path must contain a file name",
        ));
    }
    let parent = destination
        .parent()
        .filter(|path| !path.as_str().is_empty())
        .unwrap_or(Utf8Path::new("."));
    let metadata = std::fs::symlink_metadata(parent).map_err(|error| {
        output_io_error(
            "offprint.output.directory",
            ErrorStage::Validation,
            "output directory is unavailable",
            error,
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(output_error(
            "offprint.output.directory",
            ErrorStage::Validation,
            "output parent must be a directly addressed directory",
        ));
    }
    match std::fs::symlink_metadata(destination) {
        Ok(_) if conflict == ConflictPolicy::Fail => {
            return Err(output_error(
                "offprint.output.exists",
                ErrorStage::Validation,
                "output already exists",
            ));
        }
        Ok(metadata)
            if conflict == ConflictPolicy::Replace && metadata.file_type().is_symlink() =>
        {
            return Err(output_error(
                "offprint.output.path",
                ErrorStage::Validation,
                "replacement output must not be a symbolic link",
            ));
        }
        Ok(metadata) if conflict == ConflictPolicy::Replace && !metadata.is_file() => {
            return Err(output_error(
                "offprint.output.path",
                ErrorStage::Validation,
                "replacement output must be a regular file",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(output_io_error(
                "offprint.output.path",
                ErrorStage::Validation,
                "failed to inspect the output path",
                error,
            ));
        }
    }
    Ok(())
}

fn persist_noclobber(staging: NamedTempFile, destination: Utf8PathBuf) -> Result<File> {
    staging.persist_noclobber(&destination).map_err(|error| {
        let code = if error.error.kind() == io::ErrorKind::AlreadyExists {
            "offprint.output.exists"
        } else {
            "offprint.output.commit"
        };
        output_io_error(
            code,
            ErrorStage::Commit,
            "failed to commit the verified artifact",
            error.error,
        )
    })
}

fn persist_replace(staging: NamedTempFile, destination: &Utf8Path) -> Result<File> {
    staging.persist(destination).map_err(|error| {
        output_io_error(
            "offprint.output.commit",
            ErrorStage::Commit,
            "failed to atomically replace the destination",
            error.error,
        )
    })
}

fn persist_unique(mut staging: NamedTempFile, requested: Utf8PathBuf) -> Result<Utf8PathBuf> {
    for suffix in 0_u32.. {
        let destination = unique_candidate(&requested, suffix);
        match staging.persist_noclobber(&destination) {
            Ok(_) => return Ok(destination),
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                staging = error.file;
            }
            Err(error) => {
                return Err(output_io_error(
                    "offprint.output.commit",
                    ErrorStage::Commit,
                    "failed to commit the verified artifact",
                    error.error,
                ));
            }
        }
    }
    Err(output_error(
        "offprint.output.uniquify",
        ErrorStage::Commit,
        "portable filename suffixes are exhausted",
    ))
}

fn unique_candidate(requested: &Utf8Path, suffix: u32) -> Utf8PathBuf {
    if suffix == 0 {
        return requested.to_owned();
    }
    let parent = requested.parent().unwrap_or(Utf8Path::new(""));
    let stem = requested.file_stem().unwrap_or("capture");
    let extension = requested.extension();
    let name = match extension {
        Some(extension) => format!("{stem}-{suffix}.{extension}"),
        None => format!("{stem}-{suffix}"),
    };
    parent.join(name)
}

fn output_error(
    code: &'static str,
    stage: ErrorStage,
    message: impl Into<String>,
) -> OffprintError {
    OffprintError::new(code, stage, message)
}

fn output_io_error(
    code: &'static str,
    stage: ErrorStage,
    message: &'static str,
    error: io::Error,
) -> OffprintError {
    output_error(code, stage, format!("{message}: {error}"))
        .with_detail("ioKind", format!("{:?}", error.kind()))
}

fn registered_output_io_error(
    code: &'static str,
    message: &'static str,
    error: io::Error,
) -> OffprintError {
    let kind = error.kind();
    let message = format!("{message}: {error}");
    let error = match ERROR_CODE_REGISTRY
        .iter()
        .find(|definition| definition.code == code)
    {
        Some(definition) => OffprintError::new(definition.code, definition.stage, message)
            .retryable(definition.retryable),
        None => OffprintError::new(code, ErrorStage::Internal, message),
    };
    error.with_detail("ioKind", format!("{kind:?}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write as _;

    use camino::Utf8PathBuf;
    use offprint_model::{ConflictPolicy, ERROR_CODE_REGISTRY, ErrorStage};

    use super::{FileArtifactWriter, registered_output_io_error};

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_protected_folder_sync_is_treated_as_unavailable() {
        assert!(super::directory_sync_is_unavailable(
            &std::io::Error::from_raw_os_error(1)
        ));
        assert!(!super::directory_sync_is_unavailable(
            &std::io::Error::from_raw_os_error(13)
        ));
    }

    fn utf8_path(path: &std::path::Path) -> std::io::Result<Utf8PathBuf> {
        Utf8PathBuf::from_path_buf(path.to_owned()).map_err(|path| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("temporary path is not UTF-8: {}", path.display()),
            )
        })
    }

    #[test]
    fn staging_creation_failure_uses_canonical_registry_metadata() -> std::io::Result<()> {
        let error = registered_output_io_error(
            "offprint.output.staging",
            "failed to create the staging artifact",
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        let definition = ERROR_CODE_REGISTRY
            .iter()
            .find(|definition| definition.code == "offprint.output.staging")
            .ok_or_else(|| std::io::Error::other("staging error is absent from the registry"))?;

        assert_eq!(error.code.as_str(), definition.code);
        assert_eq!(error.stage, definition.stage);
        assert_eq!(error.stage, ErrorStage::Encoding);
        assert_eq!(error.retryable, definition.retryable);
        assert!(error.retryable);
        assert_eq!(
            error.details.get("ioKind").and_then(|value| value.as_str()),
            Some("PermissionDenied")
        );
        Ok(())
    }

    #[test]
    fn destination_stays_unchanged_until_replace_commit() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let destination = utf8_path(&directory.path().join("capture.html"))?;
        fs::write(&destination, b"old")?;
        let mut writer = FileArtifactWriter::create(&destination, ConflictPolicy::Replace);
        assert!(
            writer
                .as_mut()
                .is_ok_and(|writer| writer.write_all(b"new").is_ok())
        );
        assert_eq!(
            fs::read(&destination).ok().as_deref(),
            Some(b"old".as_slice())
        );
        let staged = writer.and_then(FileArtifactWriter::finish);
        assert!(staged.and_then(|artifact| artifact.commit()).is_ok());
        assert_eq!(
            fs::read(&destination).ok().as_deref(),
            Some(b"new".as_slice())
        );
        Ok(())
    }

    #[test]
    fn post_commit_parent_sync_failure_keeps_the_committed_outcome() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let destination = utf8_path(&directory.path().join("capture.html"))?;
        fs::write(&destination, b"old")?;
        let mut writer = FileArtifactWriter::create(&destination, ConflictPolicy::Replace)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        writer.write_all(b"new")?;
        let staged = writer
            .finish()
            .map_err(|error| std::io::Error::other(error.to_string()))?;

        let result = staged.commit_with_parent_sync(|| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected parent sync failure",
            ))
        });

        assert!(result.is_ok());
        assert_eq!(fs::read(destination)?, b"new");
        Ok(())
    }

    #[test]
    fn commit_rejects_same_length_staging_tampering() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let destination = utf8_path(&directory.path().join("capture.html"))?;
        let mut writer = FileArtifactWriter::create(&destination, ConflictPolicy::Fail)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        writer.write_all(b"verified")?;
        let staged = writer
            .finish()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        fs::write(staged.path(), b"modified")?;

        let error = staged
            .commit()
            .err()
            .ok_or_else(|| std::io::Error::other("modified staging artifact was committed"))?;

        assert_eq!(error.code.as_str(), "offprint.artifact.digest");
        assert!(!destination.exists());
        Ok(())
    }

    #[test]
    fn commit_rejects_staging_growth() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let destination = utf8_path(&directory.path().join("capture.html"))?;
        let mut writer = FileArtifactWriter::create(&destination, ConflictPolicy::Fail)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        writer.write_all(b"verified")?;
        let staged = writer
            .finish()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let mut staging_file = fs::OpenOptions::new().append(true).open(staged.path())?;
        staging_file.write_all(b"-changed")?;

        let error = staged
            .commit()
            .err()
            .ok_or_else(|| std::io::Error::other("grown staging artifact was committed"))?;

        assert_eq!(error.code.as_str(), "offprint.artifact.size");
        assert!(!destination.exists());
        Ok(())
    }

    #[test]
    fn commit_rejects_staging_path_replacement() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let destination = utf8_path(&directory.path().join("capture.html"))?;
        let mut writer = FileArtifactWriter::create(&destination, ConflictPolicy::Fail)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        writer.write_all(b"verified")?;
        let staged = writer
            .finish()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let displaced = directory.path().join("displaced.html");
        fs::rename(staged.path(), displaced)?;
        fs::write(staged.path(), b"verified")?;

        let error = staged
            .commit()
            .err()
            .ok_or_else(|| std::io::Error::other("replacement staging path was committed"))?;

        assert_eq!(error.code.as_str(), "offprint.output.staging");
        assert!(!destination.exists());
        Ok(())
    }

    #[test]
    fn dropping_staging_rolls_back_the_requested_destination() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let destination = utf8_path(&directory.path().join("capture.html"))?;
        let mut writer = FileArtifactWriter::create(&destination, ConflictPolicy::Fail);
        assert!(
            writer
                .as_mut()
                .is_ok_and(|writer| writer.write_all(b"partial").is_ok())
        );
        drop(writer);
        assert!(!destination.exists());
        Ok(())
    }

    #[test]
    #[cfg_attr(miri, ignore = "Miri does not support linkat")]
    fn uniquify_commits_to_the_first_available_portable_name() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let destination = utf8_path(&directory.path().join("capture.html"))?;
        fs::write(&destination, b"existing")?;
        let mut writer = FileArtifactWriter::create(&destination, ConflictPolicy::Uniquify);
        assert!(
            writer
                .as_mut()
                .is_ok_and(|writer| writer.write_all(b"new").is_ok())
        );
        let result = writer
            .and_then(FileArtifactWriter::finish)
            .and_then(|artifact| artifact.commit());
        let path = match result.as_ref().ok() {
            Some(offprint_model::CaptureArtifact::File { path, .. }) => Some(path),
            _ => None,
        };
        assert_eq!(
            path.and_then(|path| path.file_name()),
            Some("capture-1.html")
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn staging_artifact_is_owner_readable_and_writable() -> std::io::Result<()> {
        use std::os::unix::fs::MetadataExt as _;

        let directory = tempfile::tempdir()?;
        let destination = utf8_path(&directory.path().join("capture.html"))?;
        let writer = FileArtifactWriter::create(&destination, ConflictPolicy::Fail);
        let mode = writer
            .as_ref()
            .map(|writer| {
                writer
                    .staging
                    .as_file()
                    .metadata()
                    .map(|metadata| metadata.mode())
            })
            .map_err(|error| std::io::Error::other(error.to_string()))??;

        assert_eq!(mode & 0o077, 0);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn output_parent_symlink_is_rejected() -> std::io::Result<()> {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir()?;
        let real_parent = directory.path().join("real");
        fs::create_dir(&real_parent)?;
        let linked_parent = directory.path().join("linked");
        symlink(&real_parent, &linked_parent)?;
        let destination = utf8_path(&linked_parent.join("capture.html"))?;

        let error = FileArtifactWriter::create(destination, ConflictPolicy::Fail)
            .err()
            .ok_or_else(|| std::io::Error::other("symlinked output parent was accepted"))?;

        assert_eq!(error.code.as_str(), "offprint.output.directory");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn replace_rejects_a_symbolic_link_destination() -> std::io::Result<()> {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir()?;
        let target = directory.path().join("target.html");
        fs::write(&target, b"existing")?;
        let destination = directory.path().join("capture.html");
        symlink(&target, &destination)?;
        let destination = utf8_path(&destination)?;

        let error = FileArtifactWriter::create(destination, ConflictPolicy::Replace)
            .err()
            .ok_or_else(|| std::io::Error::other("symlink replacement was accepted"))?;

        assert_eq!(error.code.as_str(), "offprint.output.path");
        assert_eq!(fs::read(target)?, b"existing");
        Ok(())
    }
}
