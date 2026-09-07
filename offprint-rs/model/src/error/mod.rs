mod catalog;

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

pub use catalog::{ERROR_CODE_REGISTRY, ErrorCodeDefinition};

pub type Result<T, E = OffprintError> = std::result::Result<T, E>;

#[derive(
    Clone, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct ErrorCode(String);

impl ErrorCode {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_error_code(&value)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn from_static(value: &'static str) -> Self {
        debug_assert!(validate_error_code(value).is_ok());
        Self(value.to_owned())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ErrorCode {
    type Err = OffprintError;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

fn validate_error_code(value: &str) -> Result<()> {
    let valid = value.starts_with("offprint.")
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'_'
        })
        && value.split('.').all(|segment| !segment.is_empty());
    if valid {
        Ok(())
    } else {
        Err(OffprintError::new(
            "offprint.input.error_code",
            ErrorStage::Validation,
            format!("invalid Offprint error code `{value}`"),
        ))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorStage {
    Validation,
    Browser,
    Navigation,
    Readiness,
    Collection,
    Resource,
    Transform,
    Encoding,
    Verification,
    Commit,
    Shutdown,
    Internal,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OffprintError {
    pub code: ErrorCode,
    pub message: String,
    pub stage: ErrorStage,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics_path: Option<crate::PortablePath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Box<OffprintError>>,
}

impl OffprintError {
    #[must_use]
    pub fn new(code: &'static str, stage: ErrorStage, message: impl Into<String>) -> Self {
        let (stage, retryable) = catalog::definition(code).map_or((stage, false), |definition| {
            (definition.stage, definition.retryable)
        });
        Self {
            code: ErrorCode::from_static(code),
            message: message.into(),
            stage,
            retryable,
            details: BTreeMap::new(),
            diagnostics_path: None,
            source: None,
        }
    }

    #[must_use]
    pub const fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    #[must_use]
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<JsonValue>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_source(mut self, source: OffprintError) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    #[must_use]
    pub fn with_diagnostics_path(mut self, path: crate::PortablePath) -> Self {
        self.diagnostics_path = Some(path);
        self
    }
}

impl fmt::Display for OffprintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl Error for OffprintError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

#[cfg(test)]
mod tests;
