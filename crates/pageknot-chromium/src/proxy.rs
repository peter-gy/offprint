use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use pageknot_browser::NetworkGuard;
use pageknot_model::{ErrorStage, NetworkPolicy, PageKnotError, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{RwLock, Semaphore};
use tokio::task::JoinSet;
use tokio::time::{Instant, timeout};
use tokio_util::sync::CancellationToken;
use url::{Host, Url};

const MAXIMUM_REQUEST_HEAD_BYTES: usize = 64 * 1024;
const REQUEST_HEAD_TIMEOUT: Duration = Duration::from_secs(10);
const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub(crate) struct ValidatingProxy {
    address: SocketAddr,
    policy: Arc<RwLock<ProxyPolicy>>,
    cancellation: CancellationToken,
    traffic_cancellation: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Clone, Debug)]
enum ProxyPolicy {
    Pending,
    Guarded(NetworkGuard),
    DenyAll,
}

impl ValidatingProxy {
    pub(crate) async fn start(connection_budget: Arc<Semaphore>) -> Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|error| proxy_error(format!("failed to bind the network guard: {error}")))?;
        let address = listener.local_addr().map_err(|error| {
            proxy_error(format!("failed to read the network guard address: {error}"))
        })?;
        let policy = Arc::new(RwLock::new(ProxyPolicy::Pending));
        let cancellation = CancellationToken::new();
        let traffic_cancellation = CancellationToken::new();
        let task_policy = Arc::clone(&policy);
        let task_cancellation = cancellation.clone();
        let task_traffic_cancellation = traffic_cancellation.clone();
        let task = tokio::spawn(async move {
            run_proxy(
                listener,
                task_policy,
                task_cancellation,
                task_traffic_cancellation,
                connection_budget,
            )
            .await;
        });
        Ok(Self {
            address,
            policy,
            cancellation,
            traffic_cancellation,
            task: Some(task),
        })
    }

    #[must_use]
    pub(crate) fn browser_address(&self) -> String {
        format!("http://{}", self.address)
    }

    pub(crate) async fn set_guard(&self, guard: NetworkGuard) {
        let mut policy = self.policy.write().await;
        if !matches!(*policy, ProxyPolicy::DenyAll) {
            *policy = ProxyPolicy::Guarded(guard);
        }
    }

    pub(crate) async fn ensure_standard_guard(&self, initial_url: &Url) -> Result<()> {
        let mut policy = self.policy.write().await;
        if matches!(*policy, ProxyPolicy::Pending) {
            *policy =
                ProxyPolicy::Guarded(NetworkGuard::new(NetworkPolicy::Standard, initial_url)?);
        }
        Ok(())
    }

    pub(crate) async fn deny_all(&self) {
        *self.policy.write().await = ProxyPolicy::DenyAll;
        self.traffic_cancellation.cancel();
    }

    pub(crate) async fn close(mut self) {
        self.traffic_cancellation.cancel();
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            let _ignored = task.await;
        }
    }
}

impl Drop for ValidatingProxy {
    fn drop(&mut self) {
        self.traffic_cancellation.cancel();
        self.cancellation.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn run_proxy(
    listener: TcpListener,
    policy: Arc<RwLock<ProxyPolicy>>,
    cancellation: CancellationToken,
    traffic_cancellation: CancellationToken,
    connection_budget: Arc<Semaphore>,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            () = cancellation.cancelled() => break,
            accepted = listener.accept() => {
                let Ok((stream, _peer)) = accepted else {
                    break;
                };
                let Ok(permit) = Arc::clone(&connection_budget).try_acquire_owned() else {
                    drop(stream);
                    continue;
                };
                let connection_policy = Arc::clone(&policy);
                let connection_cancellation = traffic_cancellation.child_token();
                connections.spawn(async move {
                    let _permit = permit;
                    let _ignored =
                        serve_connection(stream, connection_policy, connection_cancellation).await;
                });
            }
            Some(_finished) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
}

async fn serve_connection(
    mut browser: TcpStream,
    policy: Arc<RwLock<ProxyPolicy>>,
    cancellation: CancellationToken,
) -> Result<()> {
    let request = match read_request_head(&mut browser).await {
        Ok(request) => request,
        Err(error) => {
            write_proxy_failure(&mut browser, 400, "Bad Request").await;
            return Err(error);
        }
    };
    let destination = request.destination()?;
    let active_guard = match policy.read().await.clone() {
        ProxyPolicy::Guarded(guard) => guard,
        ProxyPolicy::Pending => {
            write_proxy_failure(&mut browser, 403, "Forbidden").await;
            return Err(proxy_error(
                "the network guard rejected a request before policy activation",
            ));
        }
        ProxyPolicy::DenyAll => {
            write_proxy_failure(&mut browser, 403, "Forbidden").await;
            return Err(proxy_error("the network guard denied external traffic"));
        }
    };
    let addresses = tokio::select! {
        () = cancellation.cancelled() => return Ok(()),
        result = resolve_validated_addresses(&active_guard, &destination) => result?,
    };
    let mut upstream = match tokio::select! {
        () = cancellation.cancelled() => return Ok(()),
        result = connect_pinned(&addresses) => result,
    } {
        Ok(stream) => stream,
        Err(error) => {
            write_proxy_failure(&mut browser, 502, "Bad Gateway").await;
            return Err(error);
        }
    };

    if request.method.eq_ignore_ascii_case("CONNECT") {
        browser
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .map_err(|error| proxy_error(format!("failed to accept the proxy tunnel: {error}")))?;
    } else {
        upstream
            .write_all(&request.origin_form())
            .await
            .map_err(|error| {
                proxy_error(format!("failed to forward the guarded request: {error}"))
            })?;
    }

    tokio::select! {
        () = cancellation.cancelled() => Ok(()),
        result = tokio::io::copy_bidirectional(&mut browser, &mut upstream) => {
            result
                .map(|_| ())
                .map_err(|error| proxy_error(format!("guarded connection failed: {error}")))
        }
    }
}

#[derive(Debug)]
struct ProxyRequest {
    method: String,
    target: String,
    version: String,
    remainder: Vec<u8>,
}

impl ProxyRequest {
    fn destination(&self) -> Result<Url> {
        if self.method.eq_ignore_ascii_case("CONNECT") {
            return Url::parse(&format!("https://{}/", self.target)).map_err(|error| {
                proxy_error(format!("proxy tunnel destination is invalid: {error}"))
            });
        }
        let destination = Url::parse(&self.target).map_err(|error| {
            proxy_error(format!("proxy request destination is invalid: {error}"))
        })?;
        if !matches!(destination.scheme(), "http" | "https") {
            return Err(proxy_error("proxy request scheme is blocked"));
        }
        Ok(destination)
    }

    fn origin_form(&self) -> Vec<u8> {
        let path = Url::parse(&self.target)
            .ok()
            .map(|url| {
                let mut path = url.path().to_owned();
                if path.is_empty() {
                    path.push('/');
                }
                if let Some(query) = url.query() {
                    path.push('?');
                    path.push_str(query);
                }
                path
            })
            .unwrap_or_else(|| self.target.clone());
        let first_line = format!("{} {path} {}\r\n", self.method, self.version);
        let header_end = find_header_end(&self.remainder).unwrap_or(self.remainder.len());
        let header_lines_end = header_end.saturating_sub(2);
        let mut request = Vec::with_capacity(
            first_line
                .len()
                .saturating_add(self.remainder.len())
                .saturating_add(19),
        );
        request.extend_from_slice(first_line.as_bytes());
        for line in self.remainder[..header_lines_end].split_inclusive(|byte| *byte == b'\n') {
            let name = line
                .split(|byte| *byte == b':')
                .next()
                .unwrap_or_default()
                .trim_ascii();
            if name.eq_ignore_ascii_case(b"connection")
                || name.eq_ignore_ascii_case(b"proxy-connection")
            {
                continue;
            }
            request.extend_from_slice(line);
        }
        request.extend_from_slice(b"Connection: close\r\n\r\n");
        request.extend_from_slice(&self.remainder[header_end..]);
        request
    }
}

async fn read_request_head(stream: &mut TcpStream) -> Result<ProxyRequest> {
    timeout(REQUEST_HEAD_TIMEOUT, async {
        let mut bytes = Vec::with_capacity(1024);
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer).await.map_err(|error| {
                proxy_error(format!("failed to read the proxy request: {error}"))
            })?;
            if read == 0 {
                return Err(proxy_error("proxy client closed before sending a request"));
            }
            if bytes.len().saturating_add(read) > MAXIMUM_REQUEST_HEAD_BYTES {
                return Err(proxy_error("proxy request head exceeds the byte limit"));
            }
            bytes.extend_from_slice(&buffer[..read]);
            if find_header_end(&bytes).is_some() {
                let request_line_end = bytes
                    .windows(2)
                    .position(|window| window == b"\r\n")
                    .ok_or_else(|| proxy_error("proxy request line is incomplete"))?;
                let request_line = std::str::from_utf8(&bytes[..request_line_end])
                    .map_err(|_| proxy_error("proxy request line is not UTF-8"))?;
                let mut fields = request_line.split_whitespace();
                let method = fields
                    .next()
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| proxy_error("proxy request omitted its method"))?;
                let target = fields
                    .next()
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| proxy_error("proxy request omitted its destination"))?;
                let version = fields
                    .next()
                    .filter(|value| matches!(*value, "HTTP/1.0" | "HTTP/1.1"))
                    .ok_or_else(|| proxy_error("proxy request used an unsupported HTTP version"))?;
                if fields.next().is_some() {
                    return Err(proxy_error("proxy request line has unexpected fields"));
                }
                return Ok(ProxyRequest {
                    method: method.to_owned(),
                    target: target.to_owned(),
                    version: version.to_owned(),
                    remainder: bytes[request_line_end + 2..].to_vec(),
                });
            }
        }
    })
    .await
    .map_err(|_| proxy_error("proxy request head exceeded its deadline"))?
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
}

async fn resolve_validated_addresses(
    guard: &NetworkGuard,
    destination: &Url,
) -> Result<Vec<SocketAddr>> {
    guard.validate_url(destination)?;
    let port = destination
        .port_or_known_default()
        .ok_or_else(|| proxy_error("network destination has no usable port"))?;
    let addresses = match destination.host() {
        Some(Host::Ipv4(address)) => vec![SocketAddr::new(IpAddr::V4(address), port)],
        Some(Host::Ipv6(address)) => vec![SocketAddr::new(IpAddr::V6(address), port)],
        Some(Host::Domain(host)) => tokio::net::lookup_host((host, port))
            .await
            .map_err(|error| {
                PageKnotError::new(
                    "pageknot.navigation.dns",
                    ErrorStage::Navigation,
                    format!("failed to resolve guarded destination: {error}"),
                )
                .retryable(true)
            })?
            .collect(),
        None => return Err(proxy_error("network destination omitted its host")),
    };
    validate_resolved_addresses(guard, destination, addresses)
}

fn validate_resolved_addresses(
    guard: &NetworkGuard,
    destination: &Url,
    addresses: Vec<SocketAddr>,
) -> Result<Vec<SocketAddr>> {
    let mut deduplicated = addresses;
    deduplicated.sort_unstable();
    deduplicated.dedup();
    guard.validate_resolved(destination, deduplicated.iter().map(SocketAddr::ip))?;
    Ok(deduplicated)
}

async fn connect_pinned(addresses: &[SocketAddr]) -> Result<TcpStream> {
    let deadline = Instant::now() + UPSTREAM_CONNECT_TIMEOUT;
    let mut last_error = None;
    for address in addresses {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match timeout(remaining, TcpStream::connect(*address)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(error)) => last_error = Some(error.kind()),
            Err(_) => break,
        }
    }
    let reason = last_error.map_or_else(
        || "connection deadline elapsed".to_owned(),
        |kind| format!("I/O error ({kind:?})"),
    );
    Err(proxy_error(format!(
        "failed to connect to a validated address: {reason}"
    ))
    .retryable(true))
}

async fn write_proxy_failure(stream: &mut TcpStream, status: u16, reason: &str) {
    let response =
        format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    let _ignored = stream.write_all(response.as_bytes()).await;
}

fn proxy_error(message: impl Into<String>) -> PageKnotError {
    PageKnotError::new("pageknot.navigation.proxy", ErrorStage::Navigation, message)
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::time::Duration;

    use pageknot_browser::NetworkGuard;
    use pageknot_model::NetworkPolicy;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::Semaphore;
    use url::Url;

    use super::{ProxyRequest, ValidatingProxy, validate_resolved_addresses};

    type TestResult<T = ()> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

    #[test]
    fn absolute_proxy_requests_are_rewritten_to_origin_form() {
        let request = ProxyRequest {
            method: "GET".to_owned(),
            target: "http://example.test/path?q=1".to_owned(),
            version: "HTTP/1.1".to_owned(),
            remainder: b"Host: example.test\r\n\r\n".to_vec(),
        };

        assert_eq!(
            request.origin_form(),
            b"GET /path?q=1 HTTP/1.1\r\nHost: example.test\r\nConnection: close\r\n\r\n"
        );
    }

    #[test]
    fn mixed_dns_answers_are_rejected_before_a_socket_is_opened() -> TestResult {
        let destination = Url::parse("https://example.test/")?;
        let guard = NetworkGuard::new(NetworkPolicy::Server, &destination)?;
        let addresses = vec![
            SocketAddr::from(([93, 184, 216, 34], 443)),
            SocketAddr::from(([127, 0, 0, 1], 443)),
        ];

        let result = validate_resolved_addresses(&guard, &destination, addresses);

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.navigation.address_blocked")
        );
        Ok(())
    }

    #[tokio::test]
    async fn proxies_share_the_browser_connection_budget() -> TestResult {
        let connection_budget = Arc::new(Semaphore::new(1));
        let first = ValidatingProxy::start(Arc::clone(&connection_budget)).await?;
        let second = ValidatingProxy::start(Arc::clone(&connection_budget)).await?;
        let permit = Arc::clone(&connection_budget).try_acquire_owned()?;
        let mut client = TcpStream::connect(second.address).await?;
        let mut response = [0_u8; 1];

        let read =
            tokio::time::timeout(Duration::from_secs(1), client.read(&mut response)).await??;

        assert_eq!(read, 0);
        drop(permit);
        first.close().await;
        second.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn proxy_connects_to_the_address_that_passed_validation() -> TestResult {
        let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let upstream_address = upstream.local_addr()?;
        let upstream_task = tokio::spawn(async move {
            let (mut stream, _) = upstream.accept().await?;
            let mut request = vec![0_u8; 1024];
            let read = stream.read(&mut request).await?;
            assert!(request[..read].starts_with(b"GET /resource HTTP/1.1\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await?;
            TestResult::Ok(())
        });
        let proxy = start_test_proxy().await?;
        let initial = Url::parse(&format!("http://127.0.0.1:{}/", upstream_address.port()))?;
        proxy
            .set_guard(NetworkGuard::new(NetworkPolicy::Standard, &initial)?)
            .await;
        let proxy_address = proxy
            .browser_address()
            .trim_start_matches("http://")
            .parse::<SocketAddr>()?;
        let mut client = TcpStream::connect(proxy_address).await?;
        client
            .write_all(
                format!(
                    "GET http://127.0.0.1:{}/resource HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
                    upstream_address.port()
                )
                .as_bytes(),
            )
            .await?;
        let mut response = Vec::new();
        client.read_to_end(&mut response).await?;

        assert!(response.ends_with(b"\r\n\r\nok"));
        upstream_task.await??;
        proxy.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn deny_all_rejects_new_requests_before_upstream_connection() -> TestResult {
        let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let upstream_address = upstream.local_addr()?;
        let proxy = start_test_proxy().await?;
        let initial = Url::parse(&format!("http://127.0.0.1:{}/", upstream_address.port()))?;
        proxy
            .set_guard(NetworkGuard::new(NetworkPolicy::Standard, &initial)?)
            .await;
        proxy.deny_all().await;

        let proxy_address = proxy
            .browser_address()
            .trim_start_matches("http://")
            .parse::<SocketAddr>()?;
        let mut client = TcpStream::connect(proxy_address).await?;
        client
            .write_all(
                format!(
                    "GET http://127.0.0.1:{}/resource HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
                    upstream_address.port()
                )
                .as_bytes(),
            )
            .await?;
        let mut response = Vec::new();
        client.read_to_end(&mut response).await?;

        assert!(response.starts_with(b"HTTP/1.1 403 Forbidden\r\n"));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), upstream.accept())
                .await
                .is_err()
        );
        proxy.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn deny_all_cancels_an_established_tunnel() -> TestResult {
        let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let upstream_address = upstream.local_addr()?;
        let upstream_task = tokio::spawn(async move {
            let (_stream, _) = upstream.accept().await?;
            tokio::time::sleep(Duration::from_secs(5)).await;
            TestResult::Ok(())
        });
        let proxy = start_test_proxy().await?;
        let initial = Url::parse(&format!("http://127.0.0.1:{}/", upstream_address.port()))?;
        proxy
            .set_guard(NetworkGuard::new(NetworkPolicy::Standard, &initial)?)
            .await;
        let proxy_address = proxy
            .browser_address()
            .trim_start_matches("http://")
            .parse::<SocketAddr>()?;
        let mut client = TcpStream::connect(proxy_address).await?;
        client
            .write_all(
                format!(
                    "CONNECT 127.0.0.1:{} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                    upstream_address.port()
                )
                .as_bytes(),
            )
            .await?;
        let mut response = [0_u8; 39];
        client.read_exact(&mut response).await?;
        assert_eq!(&response, b"HTTP/1.1 200 Connection Established\r\n\r\n");

        proxy.deny_all().await;
        let mut remaining = Vec::new();
        tokio::time::timeout(Duration::from_secs(1), client.read_to_end(&mut remaining)).await??;

        upstream_task.abort();
        proxy.close().await;
        Ok(())
    }

    #[test]
    fn literal_address_validation_keeps_the_original_port() -> TestResult {
        let destination = Url::parse("http://127.0.0.1:8123/")?;
        let guard = NetworkGuard::new(NetworkPolicy::Standard, &destination)?;
        let addresses = vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8123)];

        let validated = validate_resolved_addresses(&guard, &destination, addresses)?;

        assert_eq!(validated.first().map(SocketAddr::port), Some(8123));
        Ok(())
    }

    async fn start_test_proxy() -> TestResult<ValidatingProxy> {
        Ok(ValidatingProxy::start(Arc::new(Semaphore::new(8))).await?)
    }
}
