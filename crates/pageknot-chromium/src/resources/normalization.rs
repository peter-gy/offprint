use pageknot_model::{ErrorStage, PageKnotError, Result};
use serde_json::Value;

pub(crate) const RENDERED_RESPONSE_RESOURCE_TYPES: &[&str] =
    &["Stylesheet", "Image", "Media", "Font", "TextTrack", "Other"];

pub(crate) fn captures_rendered_response(resource_type: Option<&str>) -> bool {
    resource_type
        .is_some_and(|resource_type| RENDERED_RESPONSE_RESOURCE_TYPES.contains(&resource_type))
}

pub(super) fn fulfilled_response_headers(parameters: &Value) -> Vec<Value> {
    const BODY_HEADERS: &[&str] = &[
        "content-encoding",
        "content-length",
        "content-md5",
        "content-digest",
        "digest",
        "repr-digest",
        "transfer-encoding",
    ];

    parameters
        .get("responseHeaders")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|header| {
            header
                .get("name")
                .and_then(Value::as_str)
                .is_none_or(|name| {
                    !BODY_HEADERS
                        .iter()
                        .any(|owned| name.eq_ignore_ascii_case(owned))
                })
        })
        .cloned()
        .collect()
}

pub(super) fn validate_intercepted_body(parameters: &Value, body_bytes: usize) -> Result<()> {
    let content_encoded = response_header_values(parameters, "content-encoding")
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|encoding| !encoding.is_empty() && !encoding.eq_ignore_ascii_case("identity"));
    if content_encoded {
        return Ok(());
    }
    let lengths = response_header_values(parameters, "content-length")
        .map(|value| {
            value.trim().parse::<u64>().map_err(|_| {
                PageKnotError::new(
                    "pageknot.resource.load",
                    ErrorStage::Resource,
                    "intercepted response contains an invalid Content-Length",
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let Some(expected) = lengths.first().copied() else {
        return Ok(());
    };
    if lengths.iter().any(|length| *length != expected) {
        return Err(PageKnotError::new(
            "pageknot.resource.load",
            ErrorStage::Resource,
            "intercepted response contains conflicting Content-Length values",
        ));
    }
    let received = u64::try_from(body_bytes).unwrap_or(u64::MAX);
    if received != expected {
        return Err(PageKnotError::new(
            "pageknot.resource.load",
            ErrorStage::Resource,
            "intercepted response body does not match its declared Content-Length",
        )
        .with_detail("declaredBytes", expected)
        .with_detail("receivedBytes", received));
    }
    Ok(())
}

fn response_header_values<'a>(
    parameters: &'a Value,
    name: &'a str,
) -> impl Iterator<Item = &'a str> {
    parameters
        .get("responseHeaders")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(move |header| {
            header
                .get("name")
                .and_then(Value::as_str)
                .filter(|candidate| candidate.eq_ignore_ascii_case(name))
                .and_then(|_| header.get("value").and_then(Value::as_str))
        })
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use serde_json::json;

    use super::*;

    type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

    #[test]
    fn intercepted_identity_body_must_match_its_declared_length() -> TestResult {
        let parameters = json!({
            "responseHeaders": [
                {"name": "Content-Type", "value": "image/svg+xml"},
                {"name": "Content-Length", "value": "512"}
            ]
        });

        let Err(error) = validate_intercepted_body(&parameters, 43) else {
            return Err(std::io::Error::other("the truncated response was accepted").into());
        };

        assert_eq!(error.code.as_str(), "pageknot.resource.load");
        assert_eq!(
            error.details.get("declaredBytes"),
            Some(&serde_json::Value::from(512))
        );
        assert_eq!(
            error.details.get("receivedBytes"),
            Some(&serde_json::Value::from(43))
        );
        Ok(())
    }

    #[test]
    fn intercepted_decoded_body_does_not_use_the_encoded_length() -> TestResult {
        let parameters = json!({
            "responseHeaders": [
                {"name": "Content-Encoding", "value": "gzip"},
                {"name": "Content-Length", "value": "12"},
                {"name": "Content-Type", "value": "image/svg+xml"}
            ]
        });

        validate_intercepted_body(&parameters, 64)?;

        let retained = fulfilled_response_headers(&parameters);
        assert_eq!(
            retained,
            vec![json!({
                "name": "Content-Type",
                "value": "image/svg+xml"
            })]
        );
        Ok(())
    }
}
