use std::collections::BTreeMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use offprint_model::Result;
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::ServerConfig;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use sha1::{Digest as _, Sha1};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

use super::{
    FixtureDelivery, FixtureRequest, FixtureResponse, FixtureRoute, RegisteredRoute, fixture_error,
};

const MAXIMUM_CONNECTIONS: usize = 256;
pub(super) const MAXIMUM_REQUEST_HEAD_BYTES: usize = 16 * 1024;
const REQUEST_HEAD_DEADLINE: Duration = Duration::from_secs(10);
const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

pub(super) async fn serve_http(
    listener: TcpListener,
    routes: Arc<RwLock<BTreeMap<String, RegisteredRoute>>>,
    requests: Arc<RwLock<Vec<FixtureRequest>>>,
    shutdown: CancellationToken,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            completed = connections.join_next(), if !connections.is_empty() => {
                let _ignored = completed;
            }
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else {
                    break;
                };
                if connections.len() >= MAXIMUM_CONNECTIONS {
                    drop(stream);
                    continue;
                }
                let routes = Arc::clone(&routes);
                let requests = Arc::clone(&requests);
                let connection_shutdown = shutdown.clone();
                connections.spawn(async move {
                    let _ignored =
                        handle_connection(stream, routes, requests, connection_shutdown).await;
                });
            }
        }
    }
    while connections.join_next().await.is_some() {}
}

pub(super) async fn serve_https(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    routes: Arc<RwLock<BTreeMap<String, RegisteredRoute>>>,
    requests: Arc<RwLock<Vec<FixtureRequest>>>,
    shutdown: CancellationToken,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            completed = connections.join_next(), if !connections.is_empty() => {
                let _ignored = completed;
            }
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else {
                    break;
                };
                if connections.len() >= MAXIMUM_CONNECTIONS {
                    drop(stream);
                    continue;
                }
                let acceptor = acceptor.clone();
                let routes = Arc::clone(&routes);
                let requests = Arc::clone(&requests);
                let connection_shutdown = shutdown.clone();
                connections.spawn(async move {
                    let stream = tokio::select! {
                        () = connection_shutdown.cancelled() => return,
                        stream = acceptor.accept(stream) => stream,
                    };
                    if let Ok(stream) = stream {
                        let _ignored =
                            handle_connection(stream, routes, requests, connection_shutdown).await;
                    }
                });
            }
        }
    }
    while connections.join_next().await.is_some() {}
}

async fn handle_connection<S>(
    mut stream: S,
    routes: Arc<RwLock<BTreeMap<String, RegisteredRoute>>>,
    requests: Arc<RwLock<Vec<FixtureRequest>>>,
    shutdown: CancellationToken,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut request = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 1024];
    let deadline = tokio::time::Instant::now() + REQUEST_HEAD_DEADLINE;
    loop {
        let read = tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            result = tokio::time::timeout_at(deadline, stream.read(&mut buffer)) => {
                result.map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "fixture request head deadline elapsed",
                    )
                })??
            }
        };
        if read == 0 {
            return Ok(());
        }
        if request.len().saturating_add(read) > MAXIMUM_REQUEST_HEAD_BYTES {
            return write_response(
                &mut stream,
                &FixtureResponse {
                    status: 431,
                    content_type: "text/plain; charset=utf-8".to_owned(),
                    headers: BTreeMap::new(),
                    body: b"request head is too large".to_vec(),
                },
                &shutdown,
            )
            .await;
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }

    let parsed = parse_request(&request);
    if let Some(parsed) = parsed.as_ref() {
        requests.write().await.push(parsed.clone());
    }
    let path = parsed.as_ref().map_or("/", |request| request.path.as_str());
    let registered = routes.read().await.get(path).cloned();
    let route = match (registered, parsed.as_ref()) {
        (Some(RegisteredRoute::Static(route)), _) => route,
        (Some(RegisteredRoute::Dynamic(handler)), Some(request)) => handler(request),
        _ => not_found().into(),
    };
    if route.validate().is_err() {
        return write_response(
            &mut stream,
            &FixtureResponse::text("invalid fixture route").with_status(500),
            &shutdown,
        )
        .await;
    }
    write_route(&mut stream, parsed.as_ref(), &route, &shutdown).await
}

fn parse_request(request: &[u8]) -> Option<FixtureRequest> {
    let text = std::str::from_utf8(request).ok()?;
    let mut lines = text.split("\r\n");
    let line = lines.next()?;
    let mut parts = line.split_ascii_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?;
    let (path, query) = target
        .split_once('?')
        .map_or((target, None), |(path, query)| {
            (path, (!query.is_empty()).then(|| query.to_owned()))
        });
    let mut headers = BTreeMap::new();
    for line in lines.take_while(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':')?;
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
    }
    Some(FixtureRequest {
        method,
        path: path.to_owned(),
        query,
        headers,
    })
}

async fn write_response<S>(
    stream: &mut S,
    response: &FixtureResponse,
    shutdown: &CancellationToken,
) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    write_complete_response(stream, response, shutdown).await
}

async fn write_route<S>(
    stream: &mut S,
    request: Option<&FixtureRequest>,
    route: &FixtureRoute,
    shutdown: &CancellationToken,
) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    match &route.delivery {
        FixtureDelivery::Complete => {
            write_complete_response(stream, &route.response, shutdown).await
        }
        FixtureDelivery::Delayed { delay } => {
            if wait_or_cancel(*delay, shutdown).await {
                return Ok(());
            }
            write_complete_response(stream, &route.response, shutdown).await
        }
        FixtureDelivery::Chunked {
            chunk_bytes,
            interval,
        } => {
            write_chunked_response(stream, &route.response, *chunk_bytes, *interval, shutdown).await
        }
        FixtureDelivery::Partial { declared_bytes } => {
            write_partial_response(stream, &route.response, *declared_bytes, shutdown).await
        }
        FixtureDelivery::EventStream { events, interval } => {
            write_event_stream(stream, &route.response, events, *interval, shutdown).await
        }
        FixtureDelivery::WebSocket { messages, interval } => {
            write_websocket(stream, request, messages, *interval, shutdown).await
        }
    }
}

async fn write_complete_response<S>(
    stream: &mut S,
    response: &FixtureResponse,
    shutdown: &CancellationToken,
) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let reason = reason_phrase(response.status);
    let head = response_head(response, reason, Some(response.body.len()), false, "close");
    write_all_or_cancel(stream, head.as_bytes(), shutdown).await?;
    write_all_or_cancel(stream, &response.body, shutdown).await?;
    stream.shutdown().await
}

async fn write_chunked_response<S>(
    stream: &mut S,
    response: &FixtureResponse,
    chunk_bytes: usize,
    interval: Duration,
    shutdown: &CancellationToken,
) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let head = response_head(
        response,
        reason_phrase(response.status),
        None,
        true,
        "close",
    );
    write_all_or_cancel(stream, head.as_bytes(), shutdown).await?;
    for chunk in response.body.chunks(chunk_bytes) {
        write_http_chunk(stream, chunk, shutdown).await?;
        if wait_or_cancel(interval, shutdown).await {
            return Ok(());
        }
    }
    write_all_or_cancel(stream, b"0\r\n\r\n", shutdown).await?;
    stream.shutdown().await
}

async fn write_partial_response<S>(
    stream: &mut S,
    response: &FixtureResponse,
    declared_bytes: usize,
    shutdown: &CancellationToken,
) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let head = response_head(
        response,
        reason_phrase(response.status),
        Some(declared_bytes),
        false,
        "close",
    );
    write_all_or_cancel(stream, head.as_bytes(), shutdown).await?;
    write_all_or_cancel(stream, &response.body, shutdown).await?;
    stream.shutdown().await
}

async fn write_event_stream<S>(
    stream: &mut S,
    response: &FixtureResponse,
    events: &[String],
    interval: Duration,
    shutdown: &CancellationToken,
) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let head = response_head(
        response,
        reason_phrase(response.status),
        None,
        true,
        "keep-alive",
    );
    write_all_or_cancel(stream, head.as_bytes(), shutdown).await?;
    for event in events {
        let payload = format!("data: {event}\n\n");
        write_http_chunk(stream, payload.as_bytes(), shutdown).await?;
        if wait_or_cancel(interval, shutdown).await {
            return Ok(());
        }
    }
    shutdown.cancelled().await;
    Ok(())
}

async fn write_websocket<S>(
    stream: &mut S,
    request: Option<&FixtureRequest>,
    messages: &[String],
    interval: Duration,
    shutdown: &CancellationToken,
) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let key = request
        .filter(|request| request.method == "GET")
        .and_then(|request| request.headers.get("sec-websocket-key"));
    let upgrade = request
        .and_then(|request| request.headers.get("upgrade"))
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
    let Some(key) = key.filter(|_| upgrade) else {
        return write_complete_response(
            stream,
            &FixtureResponse::text("WebSocket upgrade required").with_status(400),
            shutdown,
        )
        .await;
    };
    let accept = base64::engine::general_purpose::STANDARD
        .encode(Sha1::digest(format!("{key}{WEBSOCKET_GUID}").as_bytes()));
    let head = format!(
        "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    write_all_or_cancel(stream, head.as_bytes(), shutdown).await?;
    for message in messages {
        let frame = websocket_text_frame(message.as_bytes())?;
        write_all_or_cancel(stream, &frame, shutdown).await?;
        if wait_or_cancel(interval, shutdown).await {
            return Ok(());
        }
    }
    shutdown.cancelled().await;
    Ok(())
}

fn websocket_text_frame(payload: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut frame = Vec::with_capacity(payload.len().saturating_add(4));
    frame.push(0x81);
    if payload.len() <= 125 {
        frame.push(u8::try_from(payload.len()).map_err(std::io::Error::other)?);
    } else {
        let length = u16::try_from(payload.len()).map_err(std::io::Error::other)?;
        frame.push(126);
        frame.extend_from_slice(&length.to_be_bytes());
    }
    frame.extend_from_slice(payload);
    Ok(frame)
}

fn response_head(
    response: &FixtureResponse,
    reason: &str,
    content_length: Option<usize>,
    chunked: bool,
    connection: &str,
) -> String {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nConnection: {}\r\n",
        response.status, reason, response.content_type, connection
    );
    if let Some(content_length) = content_length {
        head.push_str(&format!("Content-Length: {content_length}\r\n"));
    }
    if chunked {
        head.push_str("Transfer-Encoding: chunked\r\n");
    }
    for (name, value) in &response.headers {
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "connection" | "content-length" | "content-type" | "transfer-encoding"
        ) {
            continue;
        }
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    head
}

async fn write_http_chunk<S>(
    stream: &mut S,
    payload: &[u8],
    shutdown: &CancellationToken,
) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    let prefix = format!("{:x}\r\n", payload.len());
    write_all_or_cancel(stream, prefix.as_bytes(), shutdown).await?;
    write_all_or_cancel(stream, payload, shutdown).await?;
    write_all_or_cancel(stream, b"\r\n", shutdown).await
}

async fn write_all_or_cancel<S>(
    stream: &mut S,
    bytes: &[u8],
    shutdown: &CancellationToken,
) -> std::io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    tokio::select! {
        () = shutdown.cancelled() => Ok(()),
        result = stream.write_all(bytes) => result,
    }
}

async fn wait_or_cancel(duration: Duration, shutdown: &CancellationToken) -> bool {
    if duration.is_zero() {
        return shutdown.is_cancelled();
    }
    tokio::select! {
        () = shutdown.cancelled() => true,
        () = tokio::time::sleep(duration) => false,
    }
}

pub(super) fn fixture_tls_acceptor() -> Result<(TlsAcceptor, Vec<u8>)> {
    let CertifiedKey { cert, signing_key } = generate_simple_self_signed(vec![
        "localhost".to_owned(),
        Ipv4Addr::LOCALHOST.to_string(),
    ])
    .map_err(|error| fixture_error(format!("failed to generate a fixture certificate: {error}")))?;
    let certificate = cert.der().clone();
    let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key.serialize_der()));
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let builder = ServerConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|error| {
            fixture_error(format!(
                "failed to select fixture TLS protocol versions: {error}"
            ))
        })?;
    let mut configuration = builder
        .with_no_client_auth()
        .with_single_cert(vec![certificate.clone()], private_key)
        .map_err(|error| {
            fixture_error(format!(
                "failed to configure the TLS fixture server: {error}"
            ))
        })?;
    configuration.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok((
        TlsAcceptor::from(Arc::new(configuration)),
        certificate.as_ref().to_vec(),
    ))
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        302 => "Found",
        400 => "Bad Request",
        404 => "Not Found",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        _ => "Fixture Response",
    }
}

fn not_found() -> FixtureResponse {
    FixtureResponse {
        status: 404,
        content_type: "text/plain; charset=utf-8".to_owned(),
        headers: BTreeMap::new(),
        body: b"fixture not found".to_vec(),
    }
}
