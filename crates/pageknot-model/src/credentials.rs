use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize, Serializer};
use url::Url;
use zeroize::Zeroize as _;

#[derive(Clone, Default, Deserialize, Eq, JsonSchema, PartialEq)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

impl Serialize for SecretString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str("[REDACTED]")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestHeader {
    pub name: String,
    pub value: SecretString,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CookieSameSite {
    Strict,
    Lax,
    None,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserCookie {
    pub name: String,
    pub value: SecretString,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<Url>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secure: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_site: Option<CookieSameSite>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureCredentials {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<RequestHeader>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cookies: Vec<BrowserCookie>,
}

impl CaptureCredentials {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty() && self.cookies.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::SecretString;

    #[test]
    fn secret_debug_output_redacts_its_value() {
        let secret = SecretString::new("credential-value");

        assert_eq!(format!("{secret:?}"), "SecretString([REDACTED])");
    }

    #[test]
    fn secret_serialization_redacts_its_value() {
        let secret = SecretString::new("credential-value");
        let serialized = serde_json::to_string(&secret);

        assert_eq!(serialized.ok().as_deref(), Some("\"[REDACTED]\""));
    }
}
