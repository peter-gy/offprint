use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use offprint_export::{MarkdownBundle, markdown_byte_limit_error, markdown_file_limit_error};
use offprint_model::{ErrorStage, PortablePath, Result};

use super::export::{MAXIMUM_EXPORT_BYTES, MAXIMUM_MARKDOWN_ASSETS, export_error};
use super::{BoundedFileReadError, read_bounded_file};

pub(super) async fn read_markdown_bundle(path: &PortablePath) -> Result<MarkdownBundle> {
    read_markdown_bundle_with_limits(path, MAXIMUM_MARKDOWN_ASSETS, MAXIMUM_EXPORT_BYTES).await
}

pub(super) async fn read_markdown_bundle_with_limits(
    path: &PortablePath,
    maximum_assets: usize,
    maximum_bytes: u64,
) -> Result<MarkdownBundle> {
    let initial_root = markdown_root_entries(path.as_std_path()).await?;
    let index_path = path.join("index.md");
    let markdown = read_markdown_entrypoint(index_path.as_std_path(), maximum_bytes).await?;
    let mut total_bytes = u64::try_from(markdown.len()).unwrap_or(u64::MAX);
    let mut assets = BTreeMap::new();

    if initial_root.contains("assets") {
        let asset_directory = path.join("assets");
        require_direct_directory(
            asset_directory.as_std_path(),
            "Markdown assets",
            "Markdown assets must be stored in a directly addressed directory",
        )
        .await?;
        let asset_names = directory_names(asset_directory.as_std_path(), "Markdown assets").await?;
        if asset_names.is_empty() {
            return Err(export_error(
                "offprint.export.verify",
                ErrorStage::Verification,
                "Markdown assets directory must contain at least one asset",
            ));
        }
        if asset_names.len() > maximum_assets {
            return Err(markdown_file_limit_error(ErrorStage::Verification));
        }
        for name in &asset_names {
            let remaining = maximum_bytes.saturating_sub(total_bytes);
            let content =
                read_markdown_asset(asset_directory.join(name).as_std_path(), remaining).await?;
            total_bytes =
                total_bytes.saturating_add(u64::try_from(content.len()).unwrap_or(u64::MAX));
            if total_bytes > maximum_bytes {
                return Err(markdown_byte_limit_error(ErrorStage::Verification));
            }
            assets.insert(format!("assets/{name}"), content);
        }
        require_direct_directory(
            asset_directory.as_std_path(),
            "Markdown assets",
            "Markdown assets must be stored in a directly addressed directory",
        )
        .await?;
        if directory_names(asset_directory.as_std_path(), "Markdown assets").await? != asset_names {
            return Err(export_error(
                "offprint.export.verify",
                ErrorStage::Verification,
                "Markdown asset entries changed during verification",
            ));
        }
    }

    if markdown_root_entries(path.as_std_path()).await? != initial_root {
        return Err(export_error(
            "offprint.export.verify",
            ErrorStage::Verification,
            "Markdown root entries changed during verification",
        ));
    }
    let bundle = MarkdownBundle { markdown, assets };
    bundle.validate_limits(maximum_assets, maximum_bytes, ErrorStage::Verification)?;
    Ok(bundle)
}

async fn markdown_root_entries(path: &Path) -> Result<BTreeSet<String>> {
    require_direct_directory(
        path,
        "Markdown directory",
        "Markdown verification requires a directly addressed directory",
    )
    .await?;
    let entries = directory_names(path, "Markdown directory").await?;
    let has_index = entries.contains("index.md");
    let has_assets = entries.contains("assets");
    let expected_entries = 1_usize.saturating_add(usize::from(has_assets));
    if !has_index || entries.len() != expected_entries {
        return Err(export_error(
            "offprint.export.verify",
            ErrorStage::Verification,
            "Markdown directory must contain exactly index.md and referenced assets",
        ));
    }
    Ok(entries)
}

async fn directory_names(path: &Path, subject: &'static str) -> Result<BTreeSet<String>> {
    let mut directory = tokio::fs::read_dir(path).await.map_err(|error| {
        export_error(
            "offprint.export.verify",
            ErrorStage::Verification,
            format!("failed to enumerate {subject}: {error}"),
        )
    })?;
    let mut names = BTreeSet::new();
    while let Some(entry) = directory.next_entry().await.map_err(|error| {
        export_error(
            "offprint.export.verify",
            ErrorStage::Verification,
            format!("failed to enumerate {subject}: {error}"),
        )
    })? {
        let name = entry.file_name().into_string().map_err(|_| {
            export_error(
                "offprint.export.verify",
                ErrorStage::Verification,
                format!("{subject} contains a non-UTF-8 entry name"),
            )
        })?;
        names.insert(name);
    }
    Ok(names)
}

async fn require_direct_directory(
    path: &Path,
    subject: &'static str,
    invalid_message: &'static str,
) -> Result<()> {
    let metadata = tokio::fs::symlink_metadata(path).await.map_err(|error| {
        export_error(
            "offprint.export.verify",
            ErrorStage::Verification,
            format!("failed to inspect {subject}: {error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(export_error(
            "offprint.export.verify",
            ErrorStage::Verification,
            invalid_message,
        ));
    }
    Ok(())
}

async fn read_markdown_entrypoint(path: &Path, maximum_bytes: u64) -> Result<Vec<u8>> {
    match read_bounded_file(path, maximum_bytes, true).await {
        Ok(markdown) => Ok(markdown),
        Err(BoundedFileReadError::TooLarge) => {
            Err(markdown_byte_limit_error(ErrorStage::Verification))
        }
        Err(BoundedFileReadError::NotDirectFile) => Err(export_error(
            "offprint.export.verify",
            ErrorStage::Verification,
            "Markdown entrypoint must be a directly addressed regular file",
        )),
        Err(BoundedFileReadError::Io(error)) => Err(export_error(
            "offprint.export.verify",
            ErrorStage::Verification,
            format!("failed to read the Markdown entrypoint: {error}"),
        )),
    }
}

async fn read_markdown_asset(path: &Path, maximum_bytes: u64) -> Result<Vec<u8>> {
    match read_bounded_file(path, maximum_bytes, true).await {
        Ok(content) => Ok(content),
        Err(BoundedFileReadError::TooLarge) => {
            Err(markdown_byte_limit_error(ErrorStage::Verification))
        }
        Err(BoundedFileReadError::NotDirectFile) => Err(export_error(
            "offprint.export.verify",
            ErrorStage::Verification,
            "Markdown asset must be a directly addressed regular file",
        )),
        Err(BoundedFileReadError::Io(error)) => Err(export_error(
            "offprint.export.verify",
            ErrorStage::Verification,
            format!("failed to read a Markdown asset: {error}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use offprint_model::PortablePath;

    use super::read_markdown_bundle_with_limits;

    type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

    fn markdown_path(directory: &tempfile::TempDir) -> TestResult<PortablePath> {
        std::fs::write(directory.path().join("index.md"), b"# capture")?;
        Ok(PortablePath::from_path_buf(directory.path().to_owned())?)
    }

    #[tokio::test]
    async fn reader_enforces_the_aggregate_byte_limit() -> TestResult {
        let directory = tempfile::tempdir()?;
        let assets = directory.path().join("assets");
        std::fs::create_dir(&assets)?;
        std::fs::write(directory.path().join("index.md"), b"1234")?;
        std::fs::write(assets.join("image.bin"), b"5678")?;
        let path = PortablePath::from_path_buf(directory.path().to_owned())?;

        let error = read_markdown_bundle_with_limits(&path, 4, 7)
            .await
            .err()
            .ok_or("oversized Markdown bundle was accepted")?;

        assert_eq!(error.code.as_str(), "offprint.export.size");
        Ok(())
    }

    #[tokio::test]
    async fn reader_enforces_the_asset_count_limit() -> TestResult {
        let directory = tempfile::tempdir()?;
        let assets = directory.path().join("assets");
        std::fs::create_dir(&assets)?;
        std::fs::write(directory.path().join("index.md"), b"# capture")?;
        std::fs::write(assets.join("first.bin"), b"1")?;
        std::fs::write(assets.join("second.bin"), b"2")?;
        let path = PortablePath::from_path_buf(directory.path().to_owned())?;

        let error = read_markdown_bundle_with_limits(&path, 1, 64)
            .await
            .err()
            .ok_or("Markdown asset count limit was not enforced")?;

        assert_eq!(error.code.as_str(), "offprint.export.files");
        Ok(())
    }

    #[tokio::test]
    async fn reader_rejects_an_extra_root_file() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = markdown_path(&directory)?;
        std::fs::write(directory.path().join("notes.txt"), b"not digested")?;

        assert!(
            read_markdown_bundle_with_limits(&path, 4, 64)
                .await
                .is_err()
        );
        Ok(())
    }

    #[tokio::test]
    async fn reader_rejects_an_extra_root_directory() -> TestResult {
        let directory = tempfile::tempdir()?;
        let path = markdown_path(&directory)?;
        std::fs::create_dir(directory.path().join("extra"))?;

        assert!(
            read_markdown_bundle_with_limits(&path, 4, 64)
                .await
                .is_err()
        );
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reader_rejects_an_extra_root_symlink() -> TestResult {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir()?;
        let path = markdown_path(&directory)?;
        symlink("index.md", directory.path().join("extra-link"))?;

        assert!(
            read_markdown_bundle_with_limits(&path, 4, 64)
                .await
                .is_err()
        );
        Ok(())
    }
}
