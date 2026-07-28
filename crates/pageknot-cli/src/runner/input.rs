use std::io::Read;

use pageknot::{ArtifactInput, ErrorStage, PageKnotError, Result};
use serde::de::DeserializeOwned;

pub(super) fn ensure_variant_input_readable(path: &str) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        PageKnotError::new(
            "pageknot.artifact.read",
            ErrorStage::Verification,
            format!("failed to inspect artifact `{path}`: {error}"),
        )
    })?;
    if metadata.is_file() {
        std::fs::File::open(path).map_err(|error| {
            PageKnotError::new(
                "pageknot.artifact.read",
                ErrorStage::Verification,
                format!("failed to open artifact `{path}`: {error}"),
            )
        })?;
    } else if metadata.is_dir() {
        std::fs::read_dir(path).map_err(|error| {
            PageKnotError::new(
                "pageknot.artifact.read",
                ErrorStage::Verification,
                format!("failed to read artifact directory `{path}`: {error}"),
            )
        })?;
    }
    Ok(())
}

pub(super) fn read_artifact(value: String, input: &mut dyn Read) -> Result<ArtifactInput> {
    if value == "-" {
        let mut bytes = Vec::new();
        input
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                PageKnotError::new(
                    "pageknot.artifact.read",
                    ErrorStage::Verification,
                    format!("failed to read artifact from stdin: {error}"),
                )
            })?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(PageKnotError::new(
                "pageknot.artifact.size",
                ErrorStage::Verification,
                "artifact from stdin exceeds the byte limit",
            ));
        }
        Ok(ArtifactInput::Bytes(bytes))
    } else {
        Ok(ArtifactInput::File(value.into()))
    }
}

pub(super) fn read_json_input<T>(value: &str, input: &mut dyn Read, name: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    const MAXIMUM_JSON_BYTES: u64 = 16 * 1024 * 1024;
    let mut bytes = Vec::new();
    if value == "-" {
        input
            .take(MAXIMUM_JSON_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                PageKnotError::new(
                    "pageknot.input.json",
                    ErrorStage::Validation,
                    format!("failed to read {name} from stdin: {error}"),
                )
            })?;
    } else {
        let metadata = std::fs::symlink_metadata(value).map_err(|error| {
            PageKnotError::new(
                "pageknot.input.json",
                ErrorStage::Validation,
                format!("failed to inspect {name}: {error}"),
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(PageKnotError::new(
                "pageknot.input.json",
                ErrorStage::Validation,
                format!("{name} must be a directly addressed regular file"),
            ));
        }
        if metadata.len() > MAXIMUM_JSON_BYTES {
            return Err(PageKnotError::new(
                "pageknot.input.json",
                ErrorStage::Validation,
                format!("{name} exceeds the supported byte limit"),
            ));
        }
        std::fs::File::open(value)
            .map_err(|error| {
                PageKnotError::new(
                    "pageknot.input.json",
                    ErrorStage::Validation,
                    format!("failed to open {name}: {error}"),
                )
            })?
            .take(MAXIMUM_JSON_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| {
                PageKnotError::new(
                    "pageknot.input.json",
                    ErrorStage::Validation,
                    format!("failed to read {name}: {error}"),
                )
            })?;
    }
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_JSON_BYTES {
        return Err(PageKnotError::new(
            "pageknot.input.json",
            ErrorStage::Validation,
            format!("{name} exceeds the supported byte limit"),
        ));
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        PageKnotError::new(
            "pageknot.input.json",
            ErrorStage::Validation,
            format!("{name} is not valid PageKnot JSON: {error}"),
        )
    })
}
