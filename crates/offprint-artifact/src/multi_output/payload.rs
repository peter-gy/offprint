use std::collections::{BTreeMap, BTreeSet};

use offprint_model::{ContentDigest, ErrorStage, FormatVerification, OffprintError, Result};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Per-payload limits applied while staging and recovering an artifact transaction.
pub struct ArtifactTransactionLimits {
    /// Maximum file count and maximum directory count in one payload.
    pub maximum_files: usize,
    /// Maximum encoded bytes in one payload.
    pub maximum_bytes: u64,
}

impl ArtifactTransactionLimits {
    /// Creates the limits used for staging and recovery validation.
    #[must_use]
    pub const fn new(maximum_files: usize, maximum_bytes: u64) -> Self {
        Self {
            maximum_files,
            maximum_bytes,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A verified directory payload expressed as portable relative files.
pub struct ArtifactDirectory {
    files: BTreeMap<String, Vec<u8>>,
    directories: usize,
    bytes: u64,
    sha256: ContentDigest,
}

impl ArtifactDirectory {
    /// Validates a portable file tree and computes its aggregate digest.
    pub fn new(files: BTreeMap<String, Vec<u8>>) -> Result<Self> {
        let directories = validate_directory_files(&files)?;
        let (bytes, sha256) = directory_digest(&files);
        Ok(Self {
            files,
            directories,
            bytes,
            sha256,
        })
    }

    /// Returns the number of files in this directory payload.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Returns the aggregate byte size of every file.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Returns the digest of the ordered relative paths and file contents.
    #[must_use]
    pub const fn sha256(&self) -> ContentDigest {
        self.sha256
    }

    pub(super) fn files(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.files
    }

    pub(super) fn contains(&self, relative_path: &str) -> bool {
        self.files.contains_key(relative_path)
    }

    pub(super) const fn directory_count(&self) -> usize {
        self.directories
    }
}

#[derive(Debug)]
/// A verified file or directory ready for transactional delivery.
pub struct PreparedArtifact {
    name: String,
    payload: PreparedPayload,
    verification: FormatVerification,
}

#[derive(Debug)]
pub(super) enum PreparedPayload {
    File(Vec<u8>),
    Directory {
        entrypoint: String,
        directory: ArtifactDirectory,
    },
}

impl PreparedArtifact {
    /// Prepares one verified file payload.
    pub fn file(
        name: impl Into<String>,
        bytes: Vec<u8>,
        verification: FormatVerification,
    ) -> Result<Self> {
        let name = name.into();
        validate_output_name(&name)?;
        validate_verification(
            &verification,
            u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            ContentDigest::sha256(&bytes),
        )?;
        Ok(Self {
            name,
            payload: PreparedPayload::File(bytes),
            verification,
        })
    }

    /// Prepares one verified directory payload and its relative entrypoint.
    pub fn directory(
        name: impl Into<String>,
        entrypoint: impl Into<String>,
        directory: ArtifactDirectory,
        verification: FormatVerification,
    ) -> Result<Self> {
        let name = name.into();
        let entrypoint = entrypoint.into();
        validate_output_name(&name)?;
        validate_relative_file(&entrypoint)?;
        if !directory.contains(&entrypoint) {
            return Err(payload_error(
                "artifact directory entrypoint is absent from its prepared files",
            ));
        }
        validate_verification(&verification, directory.bytes(), directory.sha256())?;
        Ok(Self {
            name,
            payload: PreparedPayload::Directory {
                entrypoint,
                directory,
            },
            verification,
        })
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn verification(&self) -> &FormatVerification {
        &self.verification
    }

    pub(super) const fn output_kind(&self) -> super::journal::OutputKind {
        match &self.payload {
            PreparedPayload::File(_) => super::journal::OutputKind::File,
            PreparedPayload::Directory { .. } => super::journal::OutputKind::Directory,
        }
    }

    /// Returns the number of files that delivery will create.
    #[must_use]
    pub fn file_count(&self) -> usize {
        match &self.payload {
            PreparedPayload::File(_) => 1,
            PreparedPayload::Directory { directory, .. } => directory.file_count(),
        }
    }

    /// Returns the aggregate byte size of the prepared payload.
    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.verification.bytes
    }

    pub(super) fn entrypoint(&self) -> Option<&str> {
        match &self.payload {
            PreparedPayload::File(_) => None,
            PreparedPayload::Directory { entrypoint, .. } => Some(entrypoint),
        }
    }

    pub(super) fn directory_count(&self) -> usize {
        match &self.payload {
            PreparedPayload::File(_) => 0,
            PreparedPayload::Directory { directory, .. } => directory.directory_count(),
        }
    }

    pub(super) fn into_payload(self) -> PreparedPayload {
        self.payload
    }
}

fn validate_directory_files(files: &BTreeMap<String, Vec<u8>>) -> Result<usize> {
    if files.is_empty() {
        return Err(payload_error(
            "artifact directory requires at least one prepared file",
        ));
    }
    let mut directories = BTreeSet::new();
    for path in files.keys() {
        validate_relative_file(path)?;
        let mut prefix = String::new();
        let mut components = path.split('/').peekable();
        while let Some(component) = components.next() {
            if components.peek().is_none() {
                break;
            }
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            if files.contains_key(&prefix) {
                return Err(payload_error(
                    "artifact directory contains a file and directory at the same path",
                ));
            }
            directories.insert(prefix.clone());
        }
    }
    Ok(directories.len())
}

fn validate_output_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(payload_error(
            "artifact transaction destination name must be one portable path component",
        ));
    }
    Ok(())
}

fn validate_relative_file(path: &str) -> Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.ends_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(payload_error(
            "artifact directory file must use a portable relative path",
        ));
    }
    Ok(())
}

fn validate_verification(
    verification: &FormatVerification,
    bytes: u64,
    sha256: ContentDigest,
) -> Result<()> {
    if verification.bytes != bytes || verification.sha256 != sha256 {
        return Err(payload_error(
            "prepared artifact payload does not match its verification record",
        ));
    }
    Ok(())
}

fn directory_digest(files: &BTreeMap<String, Vec<u8>>) -> (u64, ContentDigest) {
    let mut ordered = files.iter().collect::<Vec<_>>();
    ordered.sort_by(|(left, _), (right, _)| {
        left.matches('/')
            .count()
            .cmp(&right.matches('/').count())
            .then_with(|| left.cmp(right))
    });
    let mut hasher = Sha256::new();
    let mut bytes = 0_u64;
    for (path, content) in ordered {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(
            u64::try_from(content.len())
                .unwrap_or(u64::MAX)
                .to_le_bytes(),
        );
        hasher.update(content);
        bytes = bytes.saturating_add(u64::try_from(content.len()).unwrap_or(u64::MAX));
    }
    (bytes, ContentDigest::from_bytes(hasher.finalize().into()))
}

fn payload_error(message: impl Into<String>) -> OffprintError {
    OffprintError::new("offprint.export.output", ErrorStage::Encoding, message)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::ArtifactDirectory;

    #[test]
    fn directory_digest_orders_root_files_before_nested_files() -> offprint_model::Result<()> {
        let directory = ArtifactDirectory::new(BTreeMap::from([
            ("assets/image.bin".to_owned(), b"image".to_vec()),
            ("index.md".to_owned(), b"# capture\n".to_vec()),
        ]))?;

        let reordered = ArtifactDirectory::new(BTreeMap::from([
            ("index.md".to_owned(), b"# capture\n".to_vec()),
            ("assets/image.bin".to_owned(), b"image".to_vec()),
        ]))?;

        assert_eq!(directory.bytes(), reordered.bytes());
        assert_eq!(directory.sha256(), reordered.sha256());
        Ok(())
    }

    #[test]
    fn directory_rejects_file_directory_collisions() {
        let error = ArtifactDirectory::new(BTreeMap::from([
            ("assets".to_owned(), b"file".to_vec()),
            ("assets/image.bin".to_owned(), b"image".to_vec()),
        ]))
        .err();

        assert!(error.is_some());
    }
}
