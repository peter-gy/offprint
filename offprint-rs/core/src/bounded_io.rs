use std::io;
use std::path::Path;

use tokio::io::AsyncReadExt as _;

const FILE_READ_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub(crate) enum BoundedFileReadError {
    Io(io::Error),
    NotDirectFile,
    TooLarge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FileAddressing {
    FollowLinks,
    Direct,
}

pub(crate) async fn read_bounded_file(
    path: &Path,
    maximum_bytes: u64,
    addressing: FileAddressing,
) -> Result<Vec<u8>, BoundedFileReadError> {
    if addressing == FileAddressing::Direct {
        validate_direct_file_path(path).await?;
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(BoundedFileReadError::Io)?;
    let metadata = file.metadata().await.map_err(BoundedFileReadError::Io)?;
    if !metadata.is_file() {
        return Err(BoundedFileReadError::NotDirectFile);
    }
    if metadata.len() > maximum_bytes {
        return Err(BoundedFileReadError::TooLarge);
    }
    if addressing == FileAddressing::Direct {
        validate_direct_file_path(path).await?;
    }
    read_bounded_open_file(file, maximum_bytes).await
}

async fn validate_direct_file_path(path: &Path) -> Result<(), BoundedFileReadError> {
    let metadata = tokio::fs::symlink_metadata(path)
        .await
        .map_err(BoundedFileReadError::Io)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(BoundedFileReadError::NotDirectFile);
    }
    Ok(())
}

pub(crate) async fn read_bounded_open_file(
    mut file: tokio::fs::File,
    maximum_bytes: u64,
) -> Result<Vec<u8>, BoundedFileReadError> {
    let mut bytes = Vec::new();
    let mut total = 0_u64;
    // Binding runtimes use smaller worker stacks, so keep the chunk on the heap.
    let mut buffer = vec![0_u8; FILE_READ_BUFFER_BYTES];
    loop {
        if total == maximum_bytes {
            let mut trailing = [0_u8; 1];
            if file
                .read(&mut trailing)
                .await
                .map_err(BoundedFileReadError::Io)?
                != 0
            {
                return Err(BoundedFileReadError::TooLarge);
            }
            return Ok(bytes);
        }
        let remaining = maximum_bytes.saturating_sub(total);
        let maximum = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let read = file
            .read(&mut buffer[..maximum])
            .await
            .map_err(BoundedFileReadError::Io)?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&buffer[..read]);
        total = total.saturating_add(read as u64);
    }
}

pub(crate) struct BoundedWriter<W> {
    inner: W,
    remaining: u64,
}

impl<W> BoundedWriter<W> {
    pub(crate) const fn new(inner: W, maximum_bytes: u64) -> Self {
        Self {
            inner,
            remaining: maximum_bytes,
        }
    }

    pub(crate) fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: io::Write> io::Write for BoundedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(io::Error::other(
                "encoded output exceeds the supported byte limit",
            ));
        }
        let written = self.inner.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[tokio::test]
    async fn reader_accepts_exact_limit_and_detects_growth_after_open() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("artifact");
        std::fs::write(&path, b"safe")?;
        assert_eq!(
            read_bounded_file(&path, 4, FileAddressing::Direct)
                .await
                .map_err(|error| format!("{error:?}"))?,
            b"safe"
        );
        let file = tokio::fs::File::open(&path).await?;
        std::fs::OpenOptions::new()
            .append(true)
            .open(path)?
            .write_all(b"-oversized")?;
        assert!(matches!(
            read_bounded_open_file(file, 4).await,
            Err(BoundedFileReadError::TooLarge)
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn directly_addressed_reader_rejects_a_symlink() -> TestResult {
        let directory = tempfile::tempdir()?;
        let target = directory.path().join("target");
        let link = directory.path().join("link");
        std::fs::write(&target, b"content")?;
        std::os::unix::fs::symlink(target, &link)?;
        assert!(matches!(
            read_bounded_file(&link, 64, FileAddressing::Direct).await,
            Err(BoundedFileReadError::NotDirectFile)
        ));
        Ok(())
    }
}
