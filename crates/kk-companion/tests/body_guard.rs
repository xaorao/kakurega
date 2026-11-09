//! A request body that a handler never reads must not be left unread in the connection. When the server closes a connection
//! with unread bytes in it, the operating system resets it, and the reset throws away the part of a large reply that the
//! client has not read yet. The guard therefore reads small bodies itself before a handler runs.

use axum::body::{Body, Bytes};
use axum::routing::post;
use axum::Router;
use kk_companion::app::{AppState, Extension};
use kk_companion::{start, Companion, Options};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

const BIG: usize = 2 * 1024 * 1024;

struct Routes;

impl Extension for Routes {
    fn routes(&self, _state: AppState) -> Router<AppState> {
        Router::new()
            // answers with a large reply and never touches the request body
            .route("/admin/test/ignore", post(|| async { "x".repeat(BIG) }))
            .route("/ext/test/ignore", post(|| async { "x".repeat(BIG) }))
            // reads the whole body and says how long it was and what the first and last bytes were
            .route(
                "/admin/test/echo",
                post(|body: Bytes| async move {
                    format!(
                        "{}:{}:{}",
                        body.len(),
                        body.first().copied().unwrap_or(0),
                        body.last().copied().unwrap_or(0)
                    )
                }),
            )
            // reads a body larger than the guard buffers, as an upload does
            .route(
                "/admin/test/stream",
                post(|body: Body| async move {
                    match axum::body::to_bytes(body, 8 * 1024 * 1024).await {
                        Ok(b) => b.len().to_string(),
                        Err(e) => format!("error: {e}"),
                    }
                }),
            )
    }
}

fn server() -> (Companion, String) {
    let c = start(Options {
        extension: Arc::new(Routes),
        ..Options::default()
    })
    .unwrap();
    let token = c.security.admin_token().to_string();
    (c, token)
}

struct Parts {
    status: u16,
    declared: Option<usize>,
    body: Vec<u8>,
}

/// Sends a request in pieces with pauses, then reads the whole answer slowly until the connection ends.
fn exchange(
    c: &Companion,
    token: &str,
    path: &str,
    head_extra: &str,
    pieces: &[(&[u8], u64)],
    read_pause_ms: u64,
) -> Result<Parts, String> {
    let mut s = TcpStream::connect(c.addr).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n{head_extra}\r\n",
        c.addr.port()
    );
    s.write_all(head.as_bytes()).map_err(|e| e.to_string())?;
    for (bytes, pause_after_ms) in pieces {
        let _ = s.write_all(bytes);
        std::thread::sleep(Duration::from_millis(*pause_after_ms));
    }
    std::thread::sleep(Duration::from_millis(read_pause_ms));
    let mut all = Vec::new();
    let mut buf = vec![0u8; 16384];
    loop {
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => all.extend_from_slice(&buf[..n]),
            Err(e) => return Err(format!("after {} bytes: {e}", all.len())),
        }
    }
    let split = all
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("no response head")?
        + 4;
    let text = String::from_utf8_lossy(&all[..split]).to_ascii_lowercase();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let declared = text
        .lines()
        .find_map(|l| l.strip_prefix("content-length: "))
        .and_then(|v| v.trim().parse().ok());
    Ok(Parts {
        status,
        declared,
        body: all[split..].to_vec(),
    })
}

#[test]
fn bg01_a_large_reply_arrives_whole_when_the_small_body_is_late_and_the_handler_ignores_it() {
    let (c, token) = server();
    for round in 0..4 {
        let r = exchange(
            &c,
            &token,
            "/admin/test/ignore",
            "Content-Length: 2\r\n",
            &[(b"{}", 0)],
            300,
        );
        // headers first, the two bytes 400 ms later
        let r2 = exchange(
            &c,
            &token,
            "/admin/test/ignore",
            "Content-Length: 2\r\n",
            &[(b"", 400), (b"{}", 0)],
            300,
        );
        for (label, r) in [("together", r), ("late", r2)] {
            let r =
                r.unwrap_or_else(|e| panic!("round {round} {label}: the reply was cut off {e}"));
            assert_eq!(r.status, 200);
            assert_eq!(r.declared, Some(BIG));
            assert_eq!(
                r.body.len(),
                BIG,
                "round {round} {label}: the reply arrived whole"
            );
        }
    }
}

#[test]
fn bg02_a_small_body_reaches_the_handler_intact_at_the_edges_of_the_limit() {
    let (c, token) = server();
    for n in [1usize, 2, 10, 65_535, 65_536] {
        let body: Vec<u8> = (0..n).map(|i| (i % 251) as u8 + 1).collect();
        let r = exchange(
            &c,
            &token,
            "/admin/test/echo",
            &format!("Content-Length: {n}\r\n"),
            &[(&body, 0)],
            0,
        )
        .unwrap();
        assert_eq!(r.status, 200, "{n}");
        assert_eq!(
            String::from_utf8_lossy(&r.body),
            format!("{n}:{}:{}", body[0], body[n - 1]),
            "{n} bytes"
        );
    }
}

#[test]
fn bg03_bodies_over_the_limit_still_stream_to_the_handler_whole() {
    let (c, token) = server();
    for n in [65_537usize, 200_000, 1_500_000] {
        let body = vec![7u8; n];
        let r = exchange(
            &c,
            &token,
            "/admin/test/stream",
            &format!("Content-Length: {n}\r\n"),
            &[(&body, 0)],
            0,
        )
        .unwrap();
        assert_eq!(r.status, 200, "{n}");
        assert_eq!(
            String::from_utf8_lossy(&r.body),
            n.to_string(),
            "{n} bytes streamed"
        );
    }
}

#[test]
fn bg04_a_chunked_body_and_a_missing_body_are_left_as_they_are() {
    let (c, token) = server();
    let r = exchange(
        &c,
        &token,
        "/admin/test/echo",
        "Transfer-Encoding: chunked\r\n",
        &[(b"5\r\nhello\r\n", 50), (b"6\r\n world\r\n0\r\n\r\n", 0)],
        0,
    )
    .unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(
        String::from_utf8_lossy(&r.body),
        format!("11:{}:{}", b'h', b'd')
    );
    let r = exchange(
        &c,
        &token,
        "/admin/test/echo",
        "Content-Length: 0\r\n",
        &[],
        0,
    )
    .unwrap();
    assert_eq!(String::from_utf8_lossy(&r.body), "0:0:0");
    let r = exchange(&c, &token, "/admin/test/echo", "", &[], 0).unwrap();
    assert_eq!(String::from_utf8_lossy(&r.body), "0:0:0");
}

#[test]
fn bg05_a_body_that_stops_early_is_refused_cleanly_and_the_server_carries_on() {
    let (c, token) = server();
    // 100 bytes promised, 10 sent, then the client closes its side
    {
        let mut s = TcpStream::connect(c.addr).unwrap();
        let head = format!(
            "POST /admin/test/echo HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {token}\r\nContent-Length: 100\r\nConnection: close\r\n\r\n0123456789",
            c.addr.port()
        );
        s.write_all(head.as_bytes()).unwrap();
        s.shutdown(std::net::Shutdown::Write).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        let text = String::from_utf8_lossy(&out).to_string();
        assert!(
            text.starts_with("HTTP/1.1 400"),
            "a body that stops early is refused: {text}"
        );
        assert!(text.contains("BAD_BODY"), "{text}");
    }
    let r = exchange(
        &c,
        &token,
        "/admin/test/echo",
        "Content-Length: 3\r\n",
        &[(b"abc", 0)],
        0,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&r.body),
        format!("3:{}:{}", b'a', b'c'),
        "the server still answers"
    );
}

#[test]
fn bg06_a_slow_body_on_one_connection_does_not_hold_up_another() {
    let (c, token) = server();
    let mut slow = TcpStream::connect(c.addr).unwrap();
    slow.set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let head = format!(
        "POST /admin/test/echo HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {token}\r\nContent-Length: 5\r\nConnection: close\r\n\r\nab",
        c.addr.port()
    );
    slow.write_all(head.as_bytes()).unwrap();
    // while three of its five bytes are missing, another connection is served at once
    let start = std::time::Instant::now();
    let r = exchange(
        &c,
        &token,
        "/admin/test/echo",
        "Content-Length: 2\r\n",
        &[(b"ok", 0)],
        0,
    )
    .unwrap();
    assert_eq!(r.status, 200);
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "{:?}",
        start.elapsed()
    );
    slow.write_all(b"cde").unwrap();
    let mut out = Vec::new();
    slow.read_to_end(&mut out).unwrap();
    assert!(String::from_utf8_lossy(&out).ends_with(&format!("5:{}:{}", b'a', b'e')));
}

#[test]
fn bg07_the_extension_door_reads_small_bodies_too() {
    let (c, token) = server();
    for round in 0..3 {
        let r = exchange(
            &c,
            &token,
            "/ext/test/ignore",
            "Origin: chrome-extension://abcdefghijklmnop\r\nContent-Length: 2\r\n",
            &[(b"", 400), (b"{}", 0)],
            300,
        )
        .unwrap_or_else(|e| panic!("round {round}: the reply was cut off {e}"));
        assert_eq!(r.status, 200, "round {round}");
        assert_eq!(r.body.len(), BIG, "round {round}: the reply arrived whole");
    }
}
