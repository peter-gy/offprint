use futures_util::StreamExt as _;
use offprint_browser::LoadedResource;
use offprint_model::{ErrorStage, OffprintError, ResourceRetrievalSource, Result};
use serde_json::{Value, json};
use url::Url;

use super::ChromiumPage;
use crate::resources::{resource_body_stream, with_resource_url};

impl ChromiumPage {
    pub async fn load_resource(
        &self,
        frame_id: &str,
        url: &Url,
        maximum_bytes: u64,
    ) -> Result<LoadedResource> {
        self.load_resource_in_session(&self.session_id, frame_id, url, maximum_bytes)
            .await
    }

    pub async fn load_resource_in_session(
        &self,
        session_id: &str,
        frame_id: &str,
        url: &Url,
        maximum_bytes: u64,
    ) -> Result<LoadedResource> {
        match self
            .observed_resources
            .load(session_id, frame_id, url, maximum_bytes)
            .await
        {
            Ok(Some(resource)) => return Ok(resource),
            Ok(None) => {}
            Err(error)
                if matches!(
                    error.code.as_str(),
                    "offprint.resource.limit"
                        | "offprint.resource.load"
                        | "offprint.browser.cdp_event_lag"
                ) =>
            {
                return Err(error);
            }
            Err(_) => {}
        }
        let response = self
            .client
            .command(
                "Network.loadNetworkResource",
                json!({
                    "frameId": frame_id,
                    "url": url.as_str(),
                    "options": {
                        "disableCache": true,
                        "includeCredentials": true,
                    },
                }),
                Some(session_id),
            )
            .await
            .map_err(|error| with_resource_url(error, url))?;
        let resource = response.get("resource").ok_or_else(|| {
            with_resource_url(
                OffprintError::new(
                    "offprint.resource.protocol_shape",
                    ErrorStage::Resource,
                    "resource response has no result record",
                ),
                url,
            )
        })?;
        if !resource
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            let reason = resource
                .get("netErrorName")
                .and_then(Value::as_str)
                .unwrap_or("browser resource load failed");
            return Err(with_resource_url(
                OffprintError::new("offprint.resource.load", ErrorStage::Resource, reason)
                    .retryable(true),
                url,
            ));
        }
        let status = resource
            .get("httpStatusCode")
            .and_then(Value::as_u64)
            .and_then(|status| u16::try_from(status).ok())
            .unwrap_or(200);
        let media_type = resource
            .get("headers")
            .and_then(Value::as_object)
            .and_then(|headers| {
                headers
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                    .and_then(|(_, value)| value.as_str())
            })
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_owned());
        let encoded_length = resource.get("encodedDataLength").and_then(Value::as_u64);
        let stream = resource
            .get("stream")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                with_resource_url(
                    OffprintError::new(
                        "offprint.resource.stream",
                        ErrorStage::Resource,
                        "browser resource load returned no body stream",
                    ),
                    url,
                )
            })?;
        if encoded_length.is_some_and(|bytes| bytes > maximum_bytes) {
            let _ignored = self
                .client
                .command("IO.close", json!({"handle": stream}), Some(session_id))
                .await;
            return Err(with_resource_url(
                OffprintError::new(
                    "offprint.resource.limit",
                    ErrorStage::Resource,
                    "declared resource body length exceeds the configured byte limit",
                )
                .with_detail("attempted", encoded_length.unwrap_or(u64::MAX))
                .with_detail("limit", maximum_bytes),
                url,
            ));
        }
        let resource_url = url.clone();
        let body = resource_body_stream(
            self.client.clone(),
            session_id.to_owned(),
            stream.to_owned(),
            maximum_bytes,
            self.resource_cancellation.child_token(),
        )
        .map(move |chunk| chunk.map_err(|error| with_resource_url(error, &resource_url)));
        Ok(LoadedResource {
            final_url: url.clone(),
            redirects: Vec::new(),
            status,
            media_type,
            encoded_length,
            body: Box::pin(body),
            source: ResourceRetrievalSource::BrowserContextFetch,
        })
    }
}
