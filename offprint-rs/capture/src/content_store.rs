use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bytes::Bytes;
use futures_util::{Stream, StreamExt as _};
use offprint_model::{ContentDigest, ErrorStage, OffprintError, Result};
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredContent {
    digest: ContentDigest,
    bytes: u64,
    path: PathBuf,
}

impl StoredContent {
    #[must_use]
    pub const fn digest(&self) -> ContentDigest {
        self.digest
    }

    #[must_use]
    pub const fn bytes(&self) -> u64 {
        self.bytes
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn read(&self, maximum_bytes: u64) -> Result<Vec<u8>> {
        if self.bytes > maximum_bytes {
            return Err(content_error(
                "offprint.resource.limit",
                "stored content exceeds the requested read limit",
            )
            .with_detail("bytes", self.bytes)
            .with_detail("limit", maximum_bytes));
        }
        let file = tokio::fs::File::open(&self.path).await.map_err(|error| {
            content_error(
                "offprint.resource.store",
                format!("failed to read stored resource content: {error}"),
            )
        })?;
        // The exposed path may have grown since insertion. Read one extra byte
        // to detect growth while bounding allocation by the recorded size.
        let mut bytes = Vec::new();
        file.take(self.bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .await
            .map_err(|error| {
                content_error(
                    "offprint.resource.store",
                    format!("failed to read stored resource content: {error}"),
                )
            })?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != self.bytes {
            return Err(content_error(
                "offprint.resource.store",
                "stored resource content changed after insertion",
            ));
        }
        Ok(bytes)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentInsertion {
    pub content: StoredContent,
    pub new_content: bool,
}

#[derive(Debug)]
pub struct ContentStore {
    directory: TempDir,
    entries: BTreeMap<ContentDigest, StoredContent>,
    received_bytes: u64,
    unique_bytes: u64,
}

impl ContentStore {
    pub fn new() -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("offprint-content-")
            .tempdir()
            .map_err(|error| {
                content_error(
                    "offprint.resource.store",
                    format!("failed to create the temporary content store: {error}"),
                )
            })?;
        Ok(Self {
            directory,
            entries: BTreeMap::new(),
            received_bytes: 0,
            unique_bytes: 0,
        })
    }

    pub fn new_in(parent: &Path) -> Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("offprint-content-")
            .tempdir_in(parent)
            .map_err(|error| {
                content_error(
                    "offprint.resource.store",
                    format!("failed to create the injected temporary content store: {error}"),
                )
            })?;
        Ok(Self {
            directory,
            entries: BTreeMap::new(),
            received_bytes: 0,
            unique_bytes: 0,
        })
    }

    pub async fn insert_bytes(
        &mut self,
        bytes: Vec<u8>,
        maximum_resource_bytes: u64,
        maximum_total_bytes: u64,
    ) -> Result<ContentInsertion> {
        self.insert_stream(
            futures_util::stream::once(async move { Ok(Bytes::from(bytes)) }),
            maximum_resource_bytes,
            maximum_total_bytes,
        )
        .await
    }

    pub async fn insert_stream<S>(
        &mut self,
        stream: S,
        maximum_resource_bytes: u64,
        maximum_total_bytes: u64,
    ) -> Result<ContentInsertion>
    where
        S: Stream<Item = Result<Bytes>> + Send,
    {
        futures_util::pin_mut!(stream);
        let temporary = tempfile::Builder::new()
            .prefix(".resource-")
            .tempfile_in(self.directory.path())
            .map_err(|error| {
                content_error(
                    "offprint.resource.store",
                    format!("failed to create a temporary content entry: {error}"),
                )
            })?;
        let (file, path) = temporary.into_parts();
        let mut file = tokio::fs::File::from_std(file);
        let mut digest = Sha256::new();
        let mut resource_bytes = 0_u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let chunk_bytes = u64::try_from(chunk.len()).unwrap_or(u64::MAX);
            resource_bytes = resource_bytes.checked_add(chunk_bytes).ok_or_else(|| {
                content_error("offprint.resource.limit", "resource byte count overflowed")
            })?;
            let total_bytes = self
                .received_bytes
                .checked_add(chunk_bytes)
                .ok_or_else(|| {
                    content_error(
                        "offprint.resource.limit",
                        "total resource byte count overflowed",
                    )
                })?;
            self.received_bytes = total_bytes;
            if resource_bytes > maximum_resource_bytes || total_bytes > maximum_total_bytes {
                return Err(content_error(
                    "offprint.resource.limit",
                    "resource content exceeds the configured content-store limit",
                )
                .with_detail("resourceBytes", resource_bytes)
                .with_detail("totalBytes", total_bytes)
                .with_detail("resourceLimit", maximum_resource_bytes)
                .with_detail("totalLimit", maximum_total_bytes));
            }
            digest.update(&chunk);
            file.write_all(&chunk).await.map_err(|error| {
                content_error(
                    "offprint.resource.store",
                    format!("failed to write temporary resource content: {error}"),
                )
            })?;
        }
        file.flush().await.map_err(|error| {
            content_error(
                "offprint.resource.store",
                format!("failed to flush temporary resource content: {error}"),
            )
        })?;
        file.sync_all().await.map_err(|error| {
            content_error(
                "offprint.resource.store",
                format!("failed to sync temporary resource content: {error}"),
            )
        })?;
        drop(file);
        let digest = ContentDigest::from_bytes(digest.finalize().into());
        if let Some(content) = self.entries.get(&digest) {
            return Ok(ContentInsertion {
                content: content.clone(),
                new_content: false,
            });
        }
        let destination = self.directory.path().join(digest.to_hex());
        path.persist(&destination).map_err(|error| {
            content_error(
                "offprint.resource.store",
                format!("failed to commit resource content in the temporary store: {error}"),
            )
        })?;
        let content = StoredContent {
            digest,
            bytes: resource_bytes,
            path: destination,
        };
        self.unique_bytes = self
            .unique_bytes
            .checked_add(resource_bytes)
            .ok_or_else(|| {
                content_error(
                    "offprint.resource.limit",
                    "unique resource byte count overflowed",
                )
            })?;
        self.entries.insert(digest, content.clone());
        Ok(ContentInsertion {
            content,
            new_content: true,
        })
    }

    #[must_use]
    pub const fn received_bytes(&self) -> u64 {
        self.received_bytes
    }

    #[must_use]
    pub const fn unique_bytes(&self) -> u64 {
        self.unique_bytes
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn content_error(code: &'static str, message: impl Into<String>) -> OffprintError {
    OffprintError::new(code, ErrorStage::Resource, message)
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use offprint_model::{ErrorStage, OffprintError};
    use proptest::prelude::*;
    use tempfile::TempDir;

    use super::ContentStore;

    fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
        tokio::runtime::Builder::new_current_thread().build()
    }

    #[test]
    fn equal_content_reuses_one_digest_entry() -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            let mut store = ContentStore::new()?;

            let first = store.insert_bytes(b"same".to_vec(), 16, 32).await?;
            let second = store.insert_bytes(b"same".to_vec(), 16, 32).await?;

            assert!(first.new_content);
            assert!(!second.new_content);
            assert_eq!(first.content.digest(), second.content.digest());
            assert_eq!(store.len(), 1);
            assert_eq!(store.received_bytes(), 8);
            assert_eq!(store.unique_bytes(), 4);
            Ok(())
        })
    }

    #[test]
    fn content_limit_stops_a_stream_before_commit() -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            let mut store = ContentStore::new()?;

            let error = store.insert_bytes(b"oversized".to_vec(), 4, 32).await;

            assert_eq!(
                error.as_ref().map_err(|error| error.code.as_str()),
                Err("offprint.resource.limit")
            );
            assert!(store.is_empty());
            Ok(())
        })
    }

    #[test]
    fn stored_content_read_enforces_limits_and_detects_changed_length()
    -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            let mut store = ContentStore::new()?;
            let inserted = store.insert_bytes(b"page".to_vec(), 16, 32).await?;
            assert_eq!(inserted.content.read(4).await?, b"page");
            assert_eq!(
                inserted
                    .content
                    .read(3)
                    .await
                    .map_err(|error| error.code.to_string()),
                Err("offprint.resource.limit".to_owned())
            );

            for changed in [b"longer".as_slice(), b"x".as_slice()] {
                std::fs::write(inserted.content.path(), changed)?;
                assert_eq!(
                    inserted
                        .content
                        .read(4)
                        .await
                        .map_err(|error| error.code.to_string()),
                    Err("offprint.resource.store".to_owned())
                );
            }
            Ok(())
        })
    }

    #[test]
    fn failed_stream_bytes_still_consume_the_receive_budget()
    -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            let mut store = ContentStore::new()?;
            let stream = futures_util::stream::iter([
                Ok(Bytes::from_static(b"abc")),
                Err(OffprintError::new(
                    "offprint.resource.stream",
                    ErrorStage::Resource,
                    "fixture stream failed",
                )),
            ]);

            assert!(store.insert_stream(stream, 16, 16).await.is_err());
            assert_eq!(store.received_bytes(), 3);
            assert!(store.is_empty());
            Ok(())
        })
    }

    #[test]
    fn injected_store_removes_uncommitted_content_on_drop() -> Result<(), Box<dyn std::error::Error>>
    {
        let parent = TempDir::new()?;
        let store = ContentStore::new_in(parent.path())?;
        let directory = store.directory.path().to_owned();

        drop(store);

        assert!(!directory.exists());
        Ok(())
    }

    proptest! {
        #![proptest_config({
            let mut config = ProptestConfig::default();
            if std::env::var_os("PROPTEST_CASES").is_none() {
                config.cases = 64;
            }
            config
        })]

        #[test]
        fn arbitrary_equal_content_has_one_unique_entry(
            content in proptest::collection::vec(any::<u8>(), 0..4096),
        ) {
            let runtime = runtime()
                .map_err(|error| TestCaseError::fail(error.to_string()))?;
            runtime.block_on(async move {
                let mut store = ContentStore::new()
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                let first = store
                    .insert_bytes(content.clone(), 4096, 8192)
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;
                let second = store
                    .insert_bytes(content.clone(), 4096, 8192)
                    .await
                    .map_err(|error| TestCaseError::fail(error.to_string()))?;

                prop_assert!(first.new_content);
                prop_assert!(!second.new_content);
                prop_assert_eq!(first.content.digest(), second.content.digest());
                prop_assert_eq!(store.len(), 1);
                prop_assert_eq!(
                    store.received_bytes(),
                    u64::try_from(content.len()).unwrap_or(u64::MAX) * 2,
                );
                prop_assert_eq!(
                    store.unique_bytes(),
                    u64::try_from(content.len()).unwrap_or(u64::MAX),
                );
                Ok(())
            })?;
        }
    }
}
