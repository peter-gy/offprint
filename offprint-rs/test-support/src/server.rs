use std::collections::BTreeMap;
use std::fmt;
use std::io::Write as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use flate2::Compression;
use flate2::write::GzEncoder;
use offprint_model::{ErrorStage, OffprintError, Result};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use url::Url;

mod http;
#[cfg(test)]
mod tests;

use http::{fixture_tls_acceptor, serve_http, serve_https};

const MAXIMUM_WEBSOCKET_MESSAGE_BYTES: usize = 64 * 1024;

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

fn normalize_path(path: String) -> Result<String> {
    if !path.starts_with('/') || path.contains('\r') || path.contains('\n') {
        return Err(fixture_error(
            "fixture route must start with `/` and contain no line breaks",
        ));
    }
    Ok(path)
}

fn fixture_error(message: impl Into<String>) -> OffprintError {
    OffprintError::new("offprint.internal.fixture", ErrorStage::Internal, message)
}
