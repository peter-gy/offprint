use std::io::{self, Write};

use pageknot_model::{ArtifactResult, ContentDigest, ErrorStage, PageKnotError, Result};
use sha2::{Digest as _, Sha256};

#[derive(Debug)]
pub struct MemoryArtifactWriter {
    bytes: Vec<u8>,
    maximum_bytes: u64,
    hasher: Sha256,
}

impl MemoryArtifactWriter {
    #[must_use]
    pub fn new(maximum_bytes: u64) -> Self {
        Self {
            bytes: Vec::new(),
            maximum_bytes,
            hasher: Sha256::new(),
        }
    }

    pub fn finish(self) -> Result<MemoryArtifact> {
        let bytes = u64::try_from(self.bytes.len()).map_err(|error| {
            PageKnotError::new(
                "pageknot.artifact.size",
                ErrorStage::Encoding,
                "artifact byte count exceeds the supported range",
            )
            .with_detail("reason", error.to_string())
        })?;
        Ok(MemoryArtifact {
            content: self.bytes,
            bytes,
            sha256: ContentDigest::from_bytes(self.hasher.finalize().into()),
        })
    }
}

impl Write for MemoryArtifactWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let next = u64::try_from(self.bytes.len())
            .unwrap_or(u64::MAX)
            .saturating_add(u64::try_from(buffer.len()).unwrap_or(u64::MAX));
        if next > self.maximum_bytes {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "artifact exceeds the in-memory byte limit",
            ));
        }
        self.bytes.extend_from_slice(buffer);
        self.hasher.update(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryArtifact {
    content: Vec<u8>,
    bytes: u64,
    sha256: ContentDigest,
}

impl MemoryArtifact {
    #[must_use]
    pub fn content(&self) -> &[u8] {
        &self.content
    }

    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    #[must_use]
    pub const fn sha256(&self) -> ContentDigest {
        self.sha256
    }

    #[must_use]
    pub fn into_result(self) -> ArtifactResult {
        ArtifactResult::Bytes {
            bytes: self.bytes,
            sha256: self.sha256,
            content: self.content,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::MemoryArtifactWriter;

    #[test]
    fn memory_writer_enforces_the_limit_before_growing() {
        let mut writer = MemoryArtifactWriter::new(4);

        assert!(writer.write_all(b"page").is_ok());
        assert_eq!(
            writer
                .write_all(b"knot")
                .as_ref()
                .map_err(std::io::Error::kind),
            Err(std::io::ErrorKind::FileTooLarge)
        );
        assert_eq!(
            writer.finish().as_ref().map(|artifact| artifact.content()),
            Ok(b"page".as_slice())
        );
    }
}
