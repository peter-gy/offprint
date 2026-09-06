use std::collections::BTreeSet;
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::ContentDigest;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedactionPolicy {
    secret_query_keys: BTreeSet<String>,
}

impl RedactionPolicy {
    #[must_use]
    pub fn new(secret_query_keys: impl IntoIterator<Item = String>) -> Self {
        Self {
            secret_query_keys: secret_query_keys
                .into_iter()
                .map(|key| key.to_ascii_lowercase())
                .collect(),
        }
    }

    #[must_use]
    pub fn redact(&self, url: &Url) -> RedactedUrl {
        if url.scheme() == "data" {
            return RedactedUrl("data:[redacted]".to_owned());
        }
        let mut redacted = url.clone();
        if !redacted.username().is_empty() {
            let _ = redacted.set_username("<redacted>");
        }
        if redacted.password().is_some() {
            let _ = redacted.set_password(Some("<redacted>"));
        }

        if redacted.query().is_some() {
            let pairs = redacted
                .query_pairs()
                .map(|(key, value)| {
                    let value = if self.is_secret_query_key(&key) {
                        "<redacted>".into()
                    } else {
                        value
                    };
                    (key.into_owned(), value.into_owned())
                })
                .collect::<Vec<_>>();
            redacted.set_query(None);
            redacted.query_pairs_mut().extend_pairs(pairs);
        }

        RedactedUrl(redacted.to_string())
    }

    fn is_secret_query_key(&self, key: &str) -> bool {
        let key = key.to_ascii_lowercase();
        self.secret_query_keys.contains(&key)
            || key
                .split(|character: char| !character.is_ascii_alphanumeric())
                .any(|part| self.secret_query_keys.contains(part))
            || [
                "accesskeyid",
                "apikey",
                "authorization",
                "credential",
                "password",
                "secret",
                "signature",
                "token",
            ]
            .iter()
            .any(|suffix| key.ends_with(suffix))
    }
}

impl Default for RedactionPolicy {
    fn default() -> Self {
        Self::new(
            [
                "access_token",
                "api_key",
                "apikey",
                "auth",
                "authorization",
                "awsaccesskeyid",
                "credential",
                "key",
                "key-pair-id",
                "password",
                "policy",
                "secret",
                "signature",
                "sig",
                "token",
            ]
            .into_iter()
            .map(str::to_owned),
        )
    }
}

#[derive(
    Clone, Debug, Deserialize, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct RedactedUrl(String);

impl RedactedUrl {
    #[must_use]
    pub fn from_url(url: &Url, policy: &RedactionPolicy) -> Self {
        policy.redact(url)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RedactedUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSummary {
    pub requested_url: RedactedUrl,
    pub requested_url_sha256: ContentDigest,
    pub final_url: RedactedUrl,
    pub final_url_sha256: ContentDigest,
}

impl SourceSummary {
    #[must_use]
    pub fn new(requested: &Url, final_url: &Url, policy: &RedactionPolicy) -> Self {
        Self {
            requested_url: RedactedUrl::from_url(requested, policy),
            requested_url_sha256: ContentDigest::sha256(requested.as_str()),
            final_url: RedactedUrl::from_url(final_url, policy),
            final_url_sha256: ContentDigest::sha256(final_url.as_str()),
        }
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{RedactionPolicy, SourceSummary};

    #[test]
    fn source_summary_redacts_credentials_and_secret_query_values() -> Result<(), url::ParseError> {
        let url = Url::parse("https://user:pass@example.com/a?token=secret&view=full")?;
        let summary = SourceSummary::new(&url, &url, &RedactionPolicy::default());

        assert_eq!(
            summary.requested_url.as_str(),
            "https://%3Credacted%3E:%3Credacted%3E@example.com/a?token=%3Credacted%3E&view=full"
        );
        assert_ne!(
            summary.requested_url_sha256.to_string(),
            crate::ContentDigest::sha256(summary.requested_url.as_str()).to_string()
        );
        Ok(())
    }

    #[test]
    fn redaction_recognizes_prefixed_signed_url_fields() -> Result<(), url::ParseError> {
        let url = Url::parse(
            "https://example.com/a?X-Amz-Credential=credential-value&X-Amz-Signature=signature-value&view=full",
        )?;
        let summary = SourceSummary::new(&url, &url, &RedactionPolicy::default());

        assert!(!summary.requested_url.as_str().contains("credential-value"));
        assert!(!summary.requested_url.as_str().contains("signature-value"));
        assert!(summary.requested_url.as_str().contains("view=full"));
        Ok(())
    }
}
