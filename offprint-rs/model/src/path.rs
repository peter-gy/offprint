use std::fmt;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use camino::{Utf8Path, Utf8PathBuf};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{ErrorStage, OffprintError, Result};

#[derive(
    Clone, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
#[schemars(with = "String")]
pub struct PortablePath(Utf8PathBuf);

impl PortablePath {
    #[must_use]
    pub fn new(value: impl Into<Utf8PathBuf>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_utf8_path(&self) -> &Utf8Path {
        &self.0
    }

    #[must_use]
    pub fn into_utf8_path_buf(self) -> Utf8PathBuf {
        self.0
    }

    pub fn from_path_buf(path: PathBuf) -> Result<Self> {
        Utf8PathBuf::from_path_buf(path).map(Self).map_err(|path| {
            OffprintError::new(
                "offprint.input.path_encoding",
                ErrorStage::Validation,
                format!("path is not valid UTF-8: {}", path.display()),
            )
        })
    }
}

impl Deref for PortablePath {
    type Target = Utf8Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<Path> for PortablePath {
    fn as_ref(&self) -> &Path {
        self.0.as_std_path()
    }
}

impl AsRef<Utf8Path> for PortablePath {
    fn as_ref(&self) -> &Utf8Path {
        &self.0
    }
}

impl fmt::Display for PortablePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<Utf8PathBuf> for PortablePath {
    fn from(value: Utf8PathBuf) -> Self {
        Self(value)
    }
}

impl From<String> for PortablePath {
    fn from(value: String) -> Self {
        Self(Utf8PathBuf::from(value))
    }
}

impl From<&str> for PortablePath {
    fn from(value: &str) -> Self {
        Self(Utf8PathBuf::from(value))
    }
}
