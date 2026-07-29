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
    let response = exchange(server.address(), b"GET / HTTP/1.1\r\nHost: fixture\r\n\r\n").await?;

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

#[tokio::test]
async fn closing_server_stops_a_partial_request_head()
-> std::result::Result<(), Box<dyn Error + Send + Sync>> {
    let server = FixtureServer::start().await?;
    let mut stream = TcpStream::connect(server.address()).await?;
    stream.write_all(b"GET / HTTP/1.1\r\nHost: fixture").await?;

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
        if bytes.len() >= super::http::MAXIMUM_REQUEST_HEAD_BYTES {
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
