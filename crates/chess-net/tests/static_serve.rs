//! Static SPA serving behavior for `chess-net --static-dir`.
//!
//! These tests lock in the production web path: release assets should be
//! compressed for remote users, while `index.html`/SPA fallbacks stay
//! revalidatable so deploys can roll forward.

use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use anyhow::{Context, Result};
use chess_core::rules::RuleSet;
use chess_net::ServeOpts;

struct HttpResponse {
    status: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn http_get(addr: SocketAddr, path: &str, accept_encoding: Option<&str>) -> Result<HttpResponse> {
    let mut stream = TcpStream::connect(addr)?;
    let accept_encoding =
        accept_encoding.map(|value| format!("Accept-Encoding: {value}\r\n")).unwrap_or_default();
    let req =
        format!("GET {path} HTTP/1.1\r\nHost: x\r\n{accept_encoding}Connection: close\r\n\r\n");
    stream.write_all(req.as_bytes())?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .context("HTTP response did not contain header/body split")?;

    let head = String::from_utf8_lossy(&raw[..split]);
    let mut lines = head.split("\r\n");
    let status = lines.next().unwrap_or_default().to_string();
    let headers = lines
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect();
    let body = raw[split + 4..].to_vec();
    Ok(HttpResponse { status, headers, body })
}

#[tokio::test(flavor = "multi_thread")]
async fn spa_fallback_serves_index_with_no_cache() -> Result<()> {
    let dist = tempfile::tempdir()?;
    fs::write(dist.path().join("index.html"), "<!doctype html><main>INDEX</main>")?;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let opts =
        ServeOpts::new(RuleSet::xiangqi_casual()).with_static_dir(Some(dist.path().to_path_buf()));
    let server = tokio::spawn(chess_net::serve_with(listener, opts));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let res = http_get(addr, "/play/demo-room", None)?;
    assert!(res.status.starts_with("HTTP/1.1 200"), "status: {}", res.status);
    assert_eq!(res.header("cache-control"), Some("no-cache"));
    assert!(
        String::from_utf8_lossy(&res.body).contains("INDEX"),
        "expected index body, got {:?}",
        String::from_utf8_lossy(&res.body)
    );

    server.abort();
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn hashed_wasm_asset_is_gzip_compressed_and_immutable() -> Result<()> {
    let dist = tempfile::tempdir()?;
    fs::write(dist.path().join("index.html"), "<!doctype html><main>INDEX</main>")?;
    fs::write(dist.path().join("chess-web-deadbeef_bg.wasm"), vec![0_u8; 8192])?;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let opts =
        ServeOpts::new(RuleSet::xiangqi_casual()).with_static_dir(Some(dist.path().to_path_buf()));
    let server = tokio::spawn(chess_net::serve_with(listener, opts));
    tokio::time::sleep(Duration::from_millis(50)).await;

    let res = http_get(addr, "/chess-web-deadbeef_bg.wasm", Some("gzip"))?;
    assert!(res.status.starts_with("HTTP/1.1 200"), "status: {}", res.status);
    assert_eq!(res.header("content-encoding"), Some("gzip"));
    assert_eq!(res.header("cache-control"), Some("public, max-age=31536000, immutable"));

    server.abort();
    Ok(())
}
