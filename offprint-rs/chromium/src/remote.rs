use std::collections::BTreeSet;
use std::time::Duration;

use offprint_model::{
    BrowserInfo, BrowserProduct, BrowserSource, ErrorStage, OffprintError, RedactedUrl,
    RedactionPolicy, Result,
};
use reqwest::{Client, Response};
use serde_json::{Value, json};
use tokio::time::timeout;
use url::Url;

use crate::CdpClient;
use crate::cdp::generated::cdp_browser::{GetVersionCommand, GetVersionParams};

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(2);
const MAXIMUM_DISCOVERY_BYTES: usize = 1024 * 1024;

pub async fn resolve_remote_endpoint(endpoint: &Url) -> Result<Url> {
    if matches!(endpoint.scheme(), "ws" | "wss") {
        return Ok(endpoint.clone());
    }
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(OffprintError::new(
            "offprint.input.cdp_url",
            ErrorStage::Validation,
            "remote browser endpoint must use http, https, ws, or wss",
        ));
    }

    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(DISCOVERY_TIMEOUT)
        .timeout(DISCOVERY_TIMEOUT)
        .build()
        .map_err(|_| discovery_error("failed to create the remote discovery client"))?;

    let version_url = discovery_url(endpoint, "json/version")?;
    let version_attempt = discover_from_version(&client, &version_url, endpoint).await;
    if let Ok(websocket) = version_attempt {
        return Ok(websocket);
    }
    let version_failure = version_attempt
        .err()
        .unwrap_or_else(|| "unknown discovery failure".to_owned());

    let list_url = discovery_url(endpoint, "json/list")?;
    let list_attempt = discover_from_list(&client, &list_url, endpoint).await;
    if let Ok(websocket) = list_attempt {
        return Ok(websocket);
    }
    let list_failure = list_attempt
        .err()
        .unwrap_or_else(|| "unknown discovery failure".to_owned());

    let direct = direct_websocket_url(endpoint)?;
    let websocket_failure = match probe_websocket(&direct).await {
        Ok(()) => return Ok(direct),
        Err(failure) => failure,
    };

    Err(
        discovery_error("remote browser discovery exhausted every supported endpoint").with_detail(
            "attempts",
            json!({
                "jsonVersion": version_failure,
                "jsonList": list_failure,
                "webSocket": websocket_failure,
            }),
        ),
    )
}

/// Connects to a remote CDP endpoint and returns its browser identity.
///
/// The probe connection is closed before this function returns.
pub async fn probe_remote_browser(endpoint: &Url) -> Result<BrowserInfo> {
    let (client, info) = connect_remote_browser(endpoint).await?;
    client.close().await?;
    Ok(info)
}

pub(crate) async fn connect_remote_browser(
    requested_endpoint: &Url,
) -> Result<(CdpClient, BrowserInfo)> {
    let websocket_endpoint = resolve_remote_endpoint(requested_endpoint).await?;
    let client = CdpClient::connect(websocket_endpoint).await?;
    let version = client
        .command("Browser.getVersion", json!({}), None)
        .await?;
    let product_value = version
        .get("product")
        .and_then(Value::as_str)
        .unwrap_or("Chromium/unknown");
    let (product, version_number) = parse_remote_product(product_value);
    let protocol_version = version
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let info = BrowserInfo {
        product,
        version: version_number,
        source: BrowserSource::Remote,
        executable_path: None,
        endpoint: Some(RedactedUrl::from_url(
            requested_endpoint,
            &RedactionPolicy::default(),
        )),
        revision: None,
        protocol_version,
    };
    Ok((client, info))
}

fn parse_remote_product(value: &str) -> (BrowserProduct, String) {
    let (name, version) = value.split_once('/').unwrap_or((value, "unknown"));
    let product = if name.contains("Edge") || name.contains("Edg") {
        BrowserProduct::Edge
    } else if name.contains("Chrome") {
        BrowserProduct::Chrome
    } else {
        BrowserProduct::Chromium
    };
    (product, version.to_owned())
}

async fn discover_from_version(
    client: &Client,
    discovery_url: &Url,
    requested_endpoint: &Url,
) -> std::result::Result<Url, String> {
    let payload = fetch_json(client, discovery_url, "/json/version").await?;
    let websocket = payload
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .ok_or_else(|| "/json/version omitted webSocketDebuggerUrl".to_owned())?;
    normalize_websocket_url(websocket, requested_endpoint)
}

async fn discover_from_list(
    client: &Client,
    discovery_url: &Url,
    requested_endpoint: &Url,
) -> std::result::Result<Url, String> {
    let payload = fetch_json(client, discovery_url, "/json/list").await?;
    let targets = payload
        .as_array()
        .ok_or_else(|| "/json/list returned a non-array response".to_owned())?;
    let selected = targets
        .iter()
        .find(|target| target.get("type").and_then(Value::as_str) == Some("browser"))
        .or_else(|| {
            targets.iter().find(|target| {
                target
                    .get("webSocketDebuggerUrl")
                    .and_then(Value::as_str)
                    .is_some()
            })
        })
        .ok_or_else(|| "/json/list contained no WebSocket target".to_owned())?;
    let websocket = selected
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .ok_or_else(|| "/json/list target omitted webSocketDebuggerUrl".to_owned())?;
    normalize_websocket_url(websocket, requested_endpoint)
}

async fn fetch_json(
    client: &Client,
    endpoint: &Url,
    label: &str,
) -> std::result::Result<Value, String> {
    let response = client
        .get(endpoint.clone())
        .send()
        .await
        .map_err(|error| request_failure(label, &error))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("{label} returned HTTP {}", status.as_u16()));
    }
    let body = read_bounded_response(response, label).await?;
    serde_json::from_slice(&body).map_err(|_| format!("{label} returned invalid JSON"))
}

async fn read_bounded_response(
    mut response: Response,
    label: &str,
) -> std::result::Result<Vec<u8>, String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAXIMUM_DISCOVERY_BYTES as u64)
    {
        return Err(format!("{label} exceeded the response byte limit"));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| request_failure(label, &error))?
    {
        if body.len().saturating_add(chunk.len()) > MAXIMUM_DISCOVERY_BYTES {
            return Err(format!("{label} exceeded the response byte limit"));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn request_failure(label: &str, error: &reqwest::Error) -> String {
    if error.is_timeout() {
        format!("{label} timed out")
    } else if error.is_connect() {
        format!("{label} connection failed")
    } else if error.is_body() || error.is_decode() {
        format!("{label} response could not be read")
    } else {
        format!("{label} request failed")
    }
}

fn discovery_url(endpoint: &Url, document: &str) -> Result<Url> {
    let mut result = endpoint.clone();
    let prefix = discovery_path_prefix(endpoint.path());
    result.set_path(&format!("{prefix}/{document}"));
    result.set_fragment(None);
    Ok(result)
}

fn direct_websocket_url(endpoint: &Url) -> Result<Url> {
    let mut websocket = endpoint.clone();
    websocket
        .set_scheme(websocket_scheme(endpoint.scheme()))
        .map_err(|_| discovery_error("remote browser endpoint has an invalid URL scheme"))?;
    let prefix = discovery_path_prefix(endpoint.path());
    websocket.set_path(&format!("{prefix}/devtools/browser"));
    websocket.set_fragment(None);
    Ok(websocket)
}

fn discovery_path_prefix(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    trimmed
        .strip_suffix("/json/version")
        .or_else(|| trimmed.strip_suffix("/json/list"))
        .unwrap_or(trimmed)
}

fn normalize_websocket_url(
    discovered: &str,
    requested_endpoint: &Url,
) -> std::result::Result<Url, String> {
    let mut websocket =
        Url::parse(discovered).map_err(|_| "discovery returned an invalid WebSocket URL")?;
    if !matches!(websocket.scheme(), "ws" | "wss") {
        return Err("discovery returned a non-WebSocket debugger URL".to_owned());
    }
    let host = requested_endpoint
        .host_str()
        .ok_or_else(|| "remote browser endpoint omitted its host".to_owned())?;
    if requested_endpoint.scheme() == "https" {
        websocket
            .set_scheme("wss")
            .map_err(|_| "discovery returned an invalid WebSocket URL")?;
    }
    websocket
        .set_host(Some(host))
        .map_err(|_| "remote browser endpoint has an invalid host".to_owned())?;
    websocket
        .set_port(requested_endpoint.port())
        .map_err(|_| "remote browser endpoint has an invalid port".to_owned())?;
    if !requested_endpoint.username().is_empty() {
        websocket
            .set_username(requested_endpoint.username())
            .map_err(|_| "remote browser endpoint has invalid credentials".to_owned())?;
        websocket
            .set_password(requested_endpoint.password())
            .map_err(|_| "remote browser endpoint has invalid credentials".to_owned())?;
    }
    merge_query(&mut websocket, requested_endpoint);
    websocket.set_fragment(None);
    Ok(websocket)
}

fn merge_query(websocket: &mut Url, endpoint: &Url) {
    let requested = endpoint.query_pairs().collect::<Vec<_>>();
    if requested.is_empty() {
        return;
    }
    let mut present = websocket
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<BTreeSet<_>>();
    let mut query = websocket.query_pairs_mut();
    for (key, value) in requested {
        if present.insert((key.to_string(), value.to_string())) {
            query.append_pair(&key, &value);
        }
    }
}

async fn probe_websocket(endpoint: &Url) -> std::result::Result<(), String> {
    let client = timeout(DISCOVERY_TIMEOUT, CdpClient::connect(endpoint.clone()))
        .await
        .map_err(|_| "direct WebSocket connection timed out".to_owned())?
        .map_err(|error| format!("direct WebSocket connection failed ({})", error.code))?;
    let result = client
        .execute_with_timeout::<GetVersionCommand>(GetVersionParams::new(), None, DISCOVERY_TIMEOUT)
        .await
        .map(|_| ())
        .map_err(|error| format!("direct WebSocket probe failed ({})", error.code));
    let _ignored = client.close().await;
    result
}

const fn websocket_scheme(http_scheme: &str) -> &'static str {
    if matches!(http_scheme.as_bytes(), b"https") {
        "wss"
    } else {
        "ws"
    }
}

fn discovery_error(message: impl Into<String>) -> OffprintError {
    OffprintError::new("offprint.browser.cdp_connect", ErrorStage::Browser, message).retryable(true)
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use futures_util::{SinkExt, StreamExt};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message;
    use url::Url;

    use super::{normalize_websocket_url, resolve_remote_endpoint};

    type TestResult = std::result::Result<(), Box<dyn Error + Send + Sync>>;

    const NOT_FOUND: &str =
        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

    #[tokio::test]
    async fn version_discovery_rewrites_the_endpoint_and_preserves_query() -> TestResult {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let server = tokio::spawn(async move {
            let request = serve_http(
                &listener,
                &http_ok(
                    r#"{"webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/browser/id?existing=value"}"#,
                ),
            )
            .await?;
            if !request.contains("GET /json/version?token=secret HTTP/1.1") {
                return Err(test_error("discovery query was not sent"));
            }
            Ok::<(), Box<dyn Error + Send + Sync>>(())
        });
        let endpoint = Url::parse(&format!("http://127.0.0.1:{port}/?token=secret"))?;

        let resolved = resolve_remote_endpoint(&endpoint).await?;

        assert_eq!(resolved.host_str(), Some("127.0.0.1"));
        assert_eq!(resolved.port(), Some(port));
        assert_eq!(resolved.path(), "/devtools/browser/id");
        assert_eq!(resolved.query(), Some("existing=value&token=secret"));
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn list_discovery_is_the_first_http_fallback() -> TestResult {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let server = tokio::spawn(async move {
            serve_http(&listener, NOT_FOUND).await?;
            serve_http(
                &listener,
                &http_ok(
                    r#"[{"type":"page","webSocketDebuggerUrl":"ws://127.0.0.1:1/page"},{"type":"browser","webSocketDebuggerUrl":"ws://127.0.0.1:2/devtools/browser/id"}]"#,
                ),
            )
            .await?;
            Ok::<(), Box<dyn Error + Send + Sync>>(())
        });
        let endpoint = Url::parse(&format!("http://127.0.0.1:{port}"))?;

        let resolved = resolve_remote_endpoint(&endpoint).await?;

        assert_eq!(resolved.port(), Some(port));
        assert_eq!(resolved.path(), "/devtools/browser/id");
        server.await??;
        Ok(())
    }

    #[tokio::test]
    async fn direct_websocket_probe_is_the_final_fallback() -> TestResult {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let server = tokio::spawn(async move {
            serve_http(&listener, NOT_FOUND).await?;
            serve_http(&listener, NOT_FOUND).await?;
            let (stream, _) = listener.accept().await?;
            let mut websocket = tokio_tungstenite::accept_async(stream).await?;
            let message = websocket
                .next()
                .await
                .ok_or_else(|| test_error("CDP probe sent no command"))??;
            let Message::Text(text) = message else {
                return Err(test_error("CDP probe sent a non-text command"));
            };
            let request: serde_json::Value = serde_json::from_str(&text)?;
            let id = request
                .get("id")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| test_error("CDP probe command omitted its identifier"))?;
            websocket
                .send(Message::Text(
                    format!(
                        r#"{{"id":{id},"result":{{"product":"Chrome/150","protocolVersion":"1.3","revision":"fixture","userAgent":"fixture","jsVersion":"fixture"}}}}"#
                    )
                    .into(),
                ))
                .await?;
            let _ignored = websocket.close(None).await;
            Ok::<(), Box<dyn Error + Send + Sync>>(())
        });
        let endpoint = Url::parse(&format!("http://127.0.0.1:{port}?token=secret"))?;

        let resolved = resolve_remote_endpoint(&endpoint).await?;

        assert_eq!(
            resolved.as_str(),
            format!("ws://127.0.0.1:{port}/devtools/browser?token=secret")
        );
        server.await??;
        Ok(())
    }

    #[test]
    fn secure_discovery_upgrades_and_retargets_the_websocket() -> TestResult {
        let endpoint = Url::parse("https://browser.example:4443/prefix?token=secret")?;

        let websocket =
            normalize_websocket_url("ws://127.0.0.1:9222/devtools/browser/id", &endpoint)
                .map_err(test_error)?;

        assert_eq!(
            websocket.as_str(),
            "wss://browser.example:4443/devtools/browser/id?token=secret"
        );
        Ok(())
    }

    async fn serve_http(
        listener: &TcpListener,
        response: &str,
    ) -> std::result::Result<String, Box<dyn Error + Send + Sync>> {
        let (mut stream, _) = listener.accept().await?;
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = stream.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
        }
        stream.write_all(response.as_bytes()).await?;
        Ok(String::from_utf8(request)?)
    }

    fn http_ok(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn test_error(message: impl Into<String>) -> Box<dyn Error + Send + Sync> {
        Box::new(std::io::Error::other(message.into()))
    }
}
