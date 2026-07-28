use std::collections::BTreeMap;
use std::fmt;
use std::io::Write as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use flate2::Compression;
use flate2::write::GzEncoder;
use pageknot_model::{ErrorStage, PageKnotError, Result};
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::ServerConfig;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use sha1::{Digest as _, Sha1};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::task::{JoinHandle, JoinSet};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use url::Url;

const MAXIMUM_REQUEST_HEAD_BYTES: usize = 16 * 1024;
const MAXIMUM_CONNECTIONS: usize = 256;
const MAXIMUM_WEBSOCKET_MESSAGE_BYTES: usize = 64 * 1024;
const WEBSOCKET_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

#[derive(Clone, Debug)]
pub struct FixtureResponse {
    pub status: u16,
    pub content_type: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureRequest {
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub headers: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub enum FixtureDelivery {
    Complete,
    Delayed {
        delay: Duration,
    },
    Chunked {
        chunk_bytes: usize,
        interval: Duration,
    },
    Partial {
        declared_bytes: usize,
    },
    EventStream {
        events: Vec<String>,
        interval: Duration,
    },
    WebSocket {
        messages: Vec<String>,
        interval: Duration,
    },
}

#[derive(Clone, Debug)]
pub struct FixtureRoute {
    pub response: FixtureResponse,
    pub delivery: FixtureDelivery,
}

type FixtureHandler = Arc<dyn Fn(&FixtureRequest) -> FixtureRoute + Send + Sync + 'static>;

#[derive(Clone)]
enum RegisteredRoute {
    Static(FixtureRoute),
    Dynamic(FixtureHandler),
}

impl fmt::Debug for RegisteredRoute {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Static(route) => formatter.debug_tuple("Static").field(route).finish(),
            Self::Dynamic(_) => formatter.write_str("Dynamic(<handler>)"),
        }
    }
}

impl FixtureResponse {
    #[must_use]
    pub fn html(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8".to_owned(),
            headers: BTreeMap::new(),
            body: body.into(),
        }
    }

    #[must_use]
    pub fn text(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            content_type: "text/plain; charset=utf-8".to_owned(),
            headers: BTreeMap::new(),
            body: body.into(),
        }
    }

    #[must_use]
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    #[must_use]
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    pub fn gzip(mut self) -> Result<Self> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&self.body)
            .and_then(|()| encoder.finish())
            .map(|body| {
                self.body = body;
                self.headers
                    .insert("Content-Encoding".to_owned(), "gzip".to_owned());
                self
            })
            .map_err(|error| {
                fixture_error(format!("failed to compress a fixture response: {error}"))
            })
    }
}

impl From<FixtureResponse> for FixtureRoute {
    fn from(response: FixtureResponse) -> Self {
        Self {
            response,
            delivery: FixtureDelivery::Complete,
        }
    }
}

impl FixtureRoute {
    #[must_use]
    pub fn delayed(response: FixtureResponse, delay: Duration) -> Self {
        Self {
            response,
            delivery: FixtureDelivery::Delayed { delay },
        }
    }

    pub fn chunked(
        response: FixtureResponse,
        chunk_bytes: usize,
        interval: Duration,
    ) -> Result<Self> {
        if chunk_bytes == 0 {
            return Err(fixture_error(
                "fixture chunk size must be greater than zero",
            ));
        }
        Ok(Self {
            response,
            delivery: FixtureDelivery::Chunked {
                chunk_bytes,
                interval,
            },
        })
    }

    pub fn partial(response: FixtureResponse, declared_bytes: usize) -> Result<Self> {
        if declared_bytes <= response.body.len() {
            return Err(fixture_error(
                "a partial fixture must declare more bytes than it sends",
            ));
        }
        Ok(Self {
            response,
            delivery: FixtureDelivery::Partial { declared_bytes },
        })
    }

    #[must_use]
    pub fn event_stream(events: Vec<String>, interval: Duration) -> Self {
        Self {
            response: FixtureResponse {
                status: 200,
                content_type: "text/event-stream".to_owned(),
                headers: BTreeMap::from([("Cache-Control".to_owned(), "no-cache".to_owned())]),
                body: Vec::new(),
            },
            delivery: FixtureDelivery::EventStream { events, interval },
        }
    }

    pub fn websocket(messages: Vec<String>, interval: Duration) -> Result<Self> {
        if messages
            .iter()
            .any(|message| message.len() > MAXIMUM_WEBSOCKET_MESSAGE_BYTES)
        {
            return Err(fixture_error(
                "fixture WebSocket message exceeds its byte limit",
            ));
        }
        Ok(Self {
            response: FixtureResponse::text(Vec::new()),
            delivery: FixtureDelivery::WebSocket { messages, interval },
        })
    }

    fn validate(&self) -> Result<()> {
        if self.response.content_type.contains(['\r', '\n']) {
            return Err(fixture_error(
                "fixture content type must contain no line breaks",
            ));
        }
        for (name, value) in &self.response.headers {
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
                || value.contains(['\r', '\n'])
            {
                return Err(fixture_error("fixture response header is invalid"));
            }
        }
        match &self.delivery {
            FixtureDelivery::Chunked { chunk_bytes: 0, .. } => Err(fixture_error(
                "fixture chunk size must be greater than zero",
            )),
            FixtureDelivery::Partial { declared_bytes }
                if *declared_bytes <= self.response.body.len() =>
            {
                Err(fixture_error(
                    "a partial fixture must declare more bytes than it sends",
                ))
            }
            FixtureDelivery::WebSocket { messages, .. }
                if messages
                    .iter()
                    .any(|message| message.len() > MAXIMUM_WEBSOCKET_MESSAGE_BYTES) =>
            {
                Err(fixture_error(
                    "fixture WebSocket message exceeds its byte limit",
                ))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug)]
pub struct FixtureServer {
    address: SocketAddr,
    scheme: &'static str,
    certificate_der: Option<Vec<u8>>,
    routes: Arc<RwLock<BTreeMap<String, RegisteredRoute>>>,
    requests: Arc<RwLock<Vec<FixtureRequest>>>,
    shutdown: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl FixtureServer {
    pub async fn start() -> Result<Self> {
        Self::start_http().await
    }

    pub async fn start_http() -> Result<Self> {
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .map_err(|error| {
                fixture_error(format!("failed to bind the fixture server: {error}"))
            })?;
        let address = listener.local_addr().map_err(|error| {
            fixture_error(format!(
                "failed to read the fixture server address: {error}"
            ))
        })?;
        let routes = Arc::new(RwLock::new(BTreeMap::new()));
        let requests = Arc::new(RwLock::new(Vec::new()));
        let shutdown = CancellationToken::new();
        let task_routes = Arc::clone(&routes);
        let task_requests = Arc::clone(&requests);
        let task_shutdown = shutdown.clone();
        let task = tokio::spawn(async move {
            serve_http(listener, task_routes, task_requests, task_shutdown).await;
        });
        Ok(Self {
            address,
            scheme: "http",
            certificate_der: None,
            routes,
            requests,
            shutdown,
            task: Some(task),
        })
    }

    pub async fn start_https() -> Result<Self> {
        let listener = TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .map_err(|error| {
                fixture_error(format!("failed to bind the TLS fixture server: {error}"))
            })?;
        let address = listener.local_addr().map_err(|error| {
            fixture_error(format!(
                "failed to read the TLS fixture server address: {error}"
            ))
        })?;
        let (acceptor, certificate_der) = fixture_tls_acceptor()?;
        let routes = Arc::new(RwLock::new(BTreeMap::new()));
        let requests = Arc::new(RwLock::new(Vec::new()));
        let shutdown = CancellationToken::new();
        let task_routes = Arc::clone(&routes);
        let task_requests = Arc::clone(&requests);
        let task_shutdown = shutdown.clone();
        let task = tokio::spawn(async move {
            serve_https(
                listener,
                acceptor,
                task_routes,
                task_requests,
                task_shutdown,
            )
            .await;
        });
        Ok(Self {
            address,
            scheme: "https",
            certificate_der: Some(certificate_der),
            routes,
            requests,
            shutdown,
            task: Some(task),
        })
    }

    pub async fn register(&self, path: impl Into<String>, response: FixtureResponse) -> Result<()> {
        self.register_route(path, response.into()).await
    }

    pub async fn register_route(&self, path: impl Into<String>, route: FixtureRoute) -> Result<()> {
        let path = normalize_path(path.into())?;
        route.validate()?;
        self.routes
            .write()
            .await
            .insert(path, RegisteredRoute::Static(route));
        Ok(())
    }

    pub async fn register_handler<F>(&self, path: impl Into<String>, handler: F) -> Result<()>
    where
        F: Fn(&FixtureRequest) -> FixtureRoute + Send + Sync + 'static,
    {
        let path = normalize_path(path.into())?;
        self.routes
            .write()
            .await
            .insert(path, RegisteredRoute::Dynamic(Arc::new(handler)));
        Ok(())
    }

    pub fn url(&self, path: &str) -> Result<Url> {
        let path = normalize_path(path.to_owned())?;
        Url::parse(&format!("{}://{}{}", self.scheme, self.address, path))
            .map_err(|error| fixture_error(format!("failed to construct a fixture URL: {error}")))
    }

    pub fn url_with_host(&self, path: &str, host: &str) -> Result<Url> {
        let mut url = self.url(path)?;
        url.set_host(Some(host))
            .map_err(|error| fixture_error(format!("failed to set a fixture URL host: {error}")))?;
        Ok(url)
    }

    #[must_use]
    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    #[must_use]
    pub fn certificate_der(&self) -> Option<&[u8]> {
        self.certificate_der.as_deref()
    }

    pub async fn requests(&self) -> Vec<FixtureRequest> {
        self.requests.read().await.clone()
    }

    pub async fn close(mut self) {
        self.shutdown.cancel();
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.shutdown.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

#[derive(Debug)]
pub struct FixtureCluster {
    pub http_a: FixtureServer,
    pub http_b: FixtureServer,
    pub https_a: FixtureServer,
    pub https_b: FixtureServer,
}

impl FixtureCluster {
    pub async fn start() -> Result<Self> {
        let http_a = FixtureServer::start_http().await?;
        let http_b = match FixtureServer::start_http().await {
            Ok(server) => server,
            Err(error) => {
                http_a.close().await;
                return Err(error);
            }
        };
        let https_a = match FixtureServer::start_https().await {
            Ok(server) => server,
            Err(error) => {
                http_a.close().await;
                http_b.close().await;
                return Err(error);
            }
        };
        let https_b = match FixtureServer::start_https().await {
            Ok(server) => server,
            Err(error) => {
                http_a.close().await;
                http_b.close().await;
                https_a.close().await;
                return Err(error);
            }
        };
        Ok(Self {
            http_a,
            http_b,
            https_a,
            https_b,
        })
    }

    pub async fn close(self) {
        self.http_a.close().await;
        self.http_b.close().await;
        self.https_a.close().await;
        self.https_b.close().await;
    }
}

async fn serve_http(
    listener: TcpListener,
    routes: Arc<RwLock<BTreeMap<String, RegisteredRoute>>>,
    requests: Arc<RwLock<Vec<FixtureRequest>>>,
    shutdown: CancellationToken,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
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

async fn serve_https(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    routes: Arc<RwLock<BTreeMap<String, RegisteredRoute>>>,
    requests: Arc<RwLock<Vec<FixtureRequest>>>,
    shutdown: CancellationToken,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
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
    loop {
        let read = stream.read(&mut buffer).await?;
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

fn fixture_tls_acceptor() -> Result<(TlsAcceptor, Vec<u8>)> {
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

fn normalize_path(path: String) -> Result<String> {
    if !path.starts_with('/') || path.contains('\r') || path.contains('\n') {
        return Err(fixture_error(
            "fixture route must start with `/` and contain no line breaks",
        ));
    }
    Ok(path)
}

fn fixture_error(message: impl Into<String>) -> PageKnotError {
    PageKnotError::new("pageknot.internal.fixture", ErrorStage::Internal, message)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::error::Error;
    use std::io::Read as _;
    use std::time::Duration;

    use flate2::read::GzDecoder;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    use tokio::net::TcpStream;

    use super::{FixtureCluster, FixtureResponse, FixtureRoute, FixtureServer};

    #[tokio::test]
    async fn fixture_server_serves_registered_routes_on_loopback()
    -> std::result::Result<(), Box<dyn Error + Send + Sync>> {
        let server = FixtureServer::start().await?;
        server
            .register("/", FixtureResponse::html("<h1>PageKnot</h1>"))
            .await?;
        let response =
            exchange(server.address(), b"GET / HTTP/1.1\r\nHost: fixture\r\n\r\n").await?;

        let expected = b"<h1>PageKnot</h1>";
        assert!(
            response
                .windows(expected.len())
                .any(|window| window == expected)
        );
        server.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn dynamic_route_receives_query_and_headers()
    -> std::result::Result<(), Box<dyn Error + Send + Sync>> {
        let server = FixtureServer::start().await?;
        server
            .register_handler("/echo", |request| {
                let query = request.query.clone().unwrap_or_default();
                let authorization = request
                    .headers
                    .get("authorization")
                    .cloned()
                    .unwrap_or_default();
                FixtureResponse::text(format!("{query}|{authorization}")).into()
            })
            .await?;

        let response = exchange(
            server.address(),
            b"GET /echo?color=blue HTTP/1.1\r\nHost: fixture\r\nAuthorization: Bearer test-token\r\n\r\n",
        )
        .await?;
        assert_eq!(response_body(&response)?, b"color=blue|Bearer test-token");
        let requests = server.requests().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/echo");
        assert_eq!(requests[0].query.as_deref(), Some("color=blue"));
        server.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn gzip_response_preserves_compressed_bytes()
    -> std::result::Result<(), Box<dyn Error + Send + Sync>> {
        let server = FixtureServer::start().await?;
        server
            .register(
                "/compressed",
                FixtureResponse::text("compressed fixture").gzip()?,
            )
            .await?;

        let response = exchange(
            server.address(),
            b"GET /compressed HTTP/1.1\r\nHost: fixture\r\n\r\n",
        )
        .await?;
        let head = response_head_bytes(&response)?;
        assert!(
            String::from_utf8_lossy(head)
                .to_ascii_lowercase()
                .contains("content-encoding: gzip")
        );
        let mut decoder = GzDecoder::new(response_body(&response)?);
        let mut decoded = String::new();
        decoder.read_to_string(&mut decoded)?;
        assert_eq!(decoded, "compressed fixture");
        server.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn chunked_route_reassembles_for_http_clients()
    -> std::result::Result<(), Box<dyn Error + Send + Sync>> {
        let server = FixtureServer::start().await?;
        server
            .register_route(
                "/chunked",
                FixtureRoute::chunked(
                    FixtureResponse::text("three chunks"),
                    4,
                    Duration::from_millis(1),
                )?,
            )
            .await?;

        let body = reqwest::get(server.url("/chunked")?).await?.text().await?;
        assert_eq!(body, "three chunks");
        server.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn partial_route_closes_before_declared_content_length()
    -> std::result::Result<(), Box<dyn Error + Send + Sync>> {
        let server = FixtureServer::start().await?;
        server
            .register_route(
                "/partial",
                FixtureRoute::partial(FixtureResponse::text("short"), 20)?,
            )
            .await?;

        let response = exchange(
            server.address(),
            b"GET /partial HTTP/1.1\r\nHost: fixture\r\n\r\n",
        )
        .await?;
        let head = String::from_utf8_lossy(response_head_bytes(&response)?);
        assert!(head.contains("Content-Length: 20"));
        assert_eq!(response_body(&response)?, b"short");
        server.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn https_server_accepts_its_exported_certificate()
    -> std::result::Result<(), Box<dyn Error + Send + Sync>> {
        let server = FixtureServer::start_https().await?;
        server
            .register("/", FixtureResponse::text("secure fixture"))
            .await?;
        let certificate_der = server
            .certificate_der()
            .ok_or_else(|| std::io::Error::other("TLS fixture omitted its certificate"))?;
        let certificate = reqwest::Certificate::from_der(certificate_der)?;
        let client = reqwest::Client::builder()
            .add_root_certificate(certificate)
            .build()?;

        let body = client
            .get(server.url_with_host("/", "localhost")?)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        assert_eq!(body, "secure fixture");
        server.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn fixture_cluster_provides_four_distinct_origins()
    -> std::result::Result<(), Box<dyn Error + Send + Sync>> {
        let cluster = FixtureCluster::start().await?;
        let origins = [
            cluster.http_a.url("/")?,
            cluster.http_b.url("/")?,
            cluster.https_a.url("/")?,
            cluster.https_b.url("/")?,
        ]
        .into_iter()
        .map(|url| url.origin().ascii_serialization())
        .collect::<BTreeSet<_>>();
        assert_eq!(origins.len(), 4);
        cluster.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn closing_server_stops_an_open_websocket()
    -> std::result::Result<(), Box<dyn Error + Send + Sync>> {
        let server = FixtureServer::start().await?;
        server
            .register_route(
                "/socket",
                FixtureRoute::websocket(vec!["ready".to_owned()], Duration::ZERO)?,
            )
            .await?;
        let mut stream = TcpStream::connect(server.address()).await?;
        stream
            .write_all(
                b"GET /socket HTTP/1.1\r\nHost: fixture\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
            )
            .await?;
        let head = read_until(&mut stream, b"\r\n\r\n").await?;
        assert!(String::from_utf8_lossy(&head).starts_with("HTTP/1.1 101"));
        let mut frame_head = [0_u8; 2];
        stream.read_exact(&mut frame_head).await?;
        assert_eq!(frame_head, [0x81, 5]);
        let mut payload = [0_u8; 5];
        stream.read_exact(&mut payload).await?;
        assert_eq!(&payload, b"ready");

        tokio::time::timeout(Duration::from_secs(1), server.close()).await?;
        Ok(())
    }

    async fn exchange(address: std::net::SocketAddr, request: &[u8]) -> std::io::Result<Vec<u8>> {
        let mut stream = TcpStream::connect(address).await?;
        stream.write_all(request).await?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        Ok(response)
    }

    async fn read_until(stream: &mut TcpStream, delimiter: &[u8]) -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        while !bytes.ends_with(delimiter) {
            if bytes.len() >= super::MAXIMUM_REQUEST_HEAD_BYTES {
                return Err(std::io::Error::other("response head is too large"));
            }
            let mut byte = [0_u8; 1];
            let read = stream.read(&mut byte).await?;
            if read == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
            }
            bytes.push(byte[0]);
        }
        Ok(bytes)
    }

    fn response_head_bytes(response: &[u8]) -> std::io::Result<&[u8]> {
        let boundary = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or_else(|| std::io::Error::other("response head is incomplete"))?;
        Ok(&response[..boundary])
    }

    fn response_body(response: &[u8]) -> std::io::Result<&[u8]> {
        let boundary = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .ok_or_else(|| std::io::Error::other("response head is incomplete"))?;
        Ok(&response[boundary + 4..])
    }
}
