//! A connection left open and idle by any program on the computer used to hold a descriptor for ever; a program started from
//! the desktop often has only 256, and a few hundred idle connections made the service stop answering. Connections are now
//! limited, one that does not send a request head in time is closed, and a slow handler is not mistaken for an idle connection.

use axum::routing::{get, post};
use axum::Router;
use kk_companion::app::{AppState, Extension};
use kk_companion::{start, Companion, Options};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Routes;

impl Extension for Routes {
    fn routes(&self, _state: AppState) -> Router<AppState> {
        Router::new()
            .route(
                "/admin/test/slow",
                post(|| async {
                    tokio::time::sleep(Duration::from_millis(2500)).await;
                    "done"
                }),
            )
            .route("/admin/test/ping", get(|| async { "pong" }))
            .route("/admin/test/post", post(|| async { "posted" }))
    }
}

fn server(max: usize, timeout_ms: u64) -> (Companion, String) {
    let c = start(Options {
        extension: Arc::new(Routes),
        max_connections: max,
        header_timeout: Duration::from_millis(timeout_ms),
        ..Options::default()
    })
    .unwrap();
    let token = c.security.admin_token().to_string();
    (c, token)
}

fn request(c: &Companion, token: &str, method: &str, path: &str) -> Result<String, String> {
    let mut s = TcpStream::connect(c.addr).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    let body = if method == "POST" {
        "Content-Length: 0\r\n"
    } else {
        ""
    };
    s.write_all(
        format!(
            "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {token}\r\n{body}Connection: close\r\n\r\n",
            c.addr.port()
        )
        .as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    let mut out = String::new();
    s.read_to_string(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}

#[test]
fn cn01_a_connection_that_sends_nothing_is_closed_after_the_header_timeout() {
    let (c, _t) = server(8, 600);
    let mut idle: Vec<TcpStream> = (0..3)
        .map(|_| TcpStream::connect(c.addr).unwrap())
        .collect();
    let started = Instant::now();
    for s in idle.iter_mut() {
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut b = [0u8; 1];
        let r = s.read(&mut b);
        assert!(
            matches!(r, Ok(0)),
            "the server should close an idle connection, got {r:?}"
        );
    }
    let took = started.elapsed();
    assert!(
        took >= Duration::from_millis(500) && took < Duration::from_secs(4),
        "{took:?}"
    );
}

#[test]
fn cn02_more_idle_connections_than_slots_delay_a_real_request_by_a_bounded_time_and_never_wedge_the_service(
) {
    let (c, token) = server(4, 700);
    // twelve idle connections, three times the number of slots
    let idle: Vec<TcpStream> = (0..12)
        .map(|_| TcpStream::connect(c.addr).unwrap())
        .collect();
    let started = Instant::now();
    let r = request(&c, &token, "GET", "/admin/test/ping").expect("the real request is answered");
    assert!(r.starts_with("HTTP/1.1 200") && r.ends_with("pong"), "{r}");
    let waited = started.elapsed();
    assert!(
        waited < Duration::from_secs(10),
        "the real request waited {waited:?}"
    );
    drop(idle);
    // and with the idle ones gone, answers are immediate
    let again = Instant::now();
    request(&c, &token, "GET", "/admin/test/ping").unwrap();
    assert!(
        again.elapsed() < Duration::from_secs(2),
        "{:?}",
        again.elapsed()
    );
}

#[test]
fn cn03_a_slow_head_is_cut_off_but_a_slow_handler_is_not() {
    let (c, token) = server(8, 800);
    // one byte of the head every 300 ms: never complete within the timeout
    let mut s = TcpStream::connect(c.addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let started = Instant::now();
    let head = format!(
        "GET /admin/test/ping HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n",
        c.addr.port()
    );
    let mut closed = false;
    for b in head.bytes() {
        if s.write_all(&[b]).is_err() {
            closed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
        if started.elapsed() > Duration::from_secs(6) {
            break;
        }
    }
    let mut buf = [0u8; 1];
    let ended = closed || matches!(s.read(&mut buf), Ok(0) | Err(_)) || s.write_all(b"x").is_err();
    assert!(
        ended,
        "a head sent a byte at a time was never completed and should be cut off"
    );
    // a handler that works for 2.5 s, longer than the 0.8 s timeout, still answers
    let r = request(&c, &token, "POST", "/admin/test/slow").unwrap();
    assert!(r.starts_with("HTTP/1.1 200") && r.ends_with("done"), "{r}");
}

#[test]
fn cn04_each_connection_serves_one_request_and_is_then_closed() {
    let (c, token) = server(8, 1500);
    let mut s = TcpStream::connect(c.addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(
        format!(
            "GET /admin/test/ping HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAuthorization: Bearer {token}\r\n\r\n",
            c.addr.port()
        )
        .as_bytes(),
    )
    .unwrap();
    let mut got = Vec::new();
    s.read_to_end(&mut got)
        .expect("the server ends the connection after its answer");
    let text = String::from_utf8_lossy(&got).to_ascii_lowercase();
    assert!(
        text.starts_with("http/1.1 200") && text.ends_with("pong"),
        "{text}"
    );
    assert!(
        text.contains("connection: close"),
        "the answer says the connection ends: {text}"
    );
}

#[test]
fn cn05_shutdown_returns_promptly_even_with_an_idle_connection_open() {
    let (c, _t) = server(8, 30_000);
    let _idle = TcpStream::connect(c.addr).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let started = Instant::now();
    drop(c);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "shutdown took {:?}",
        started.elapsed()
    );
}

#[test]
fn cn06_the_limit_holds_one_slot_for_an_idle_connection_until_it_times_out() {
    let (c, token) = server(1, 1500);
    let idle = TcpStream::connect(c.addr).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let started = Instant::now();
    let r = request(&c, &token, "GET", "/admin/test/ping").expect("served once the slot is free");
    let waited = started.elapsed();
    assert!(r.starts_with("HTTP/1.1 200"), "{r}");
    assert!(
        waited >= Duration::from_millis(900),
        "with one slot held by an idle connection the real request has to wait for it: {waited:?}"
    );
    assert!(waited < Duration::from_secs(6), "{waited:?}");
    drop(idle);
}

#[test]
fn cn07_a_client_that_keeps_its_connections_can_use_one_after_the_server_would_have_closed_an_idle_one(
) {
    // the terminal workspace keeps a pooled client for the whole session and pauses between requests; the first request
    // after a pause must not be sent down a connection the server has already closed
    let (c, token) = server(8, 700);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let url = |path: &str| format!("http://127.0.0.1:{}{path}", c.addr.port());
    let auth = format!("Bearer {token}");
    for round in 0..6 {
        let get = agent
            .get(&url("/admin/test/ping"))
            .header("Authorization", &auth)
            .call();
        assert_eq!(get.expect("a GET").status().as_u16(), 200, "round {round}");
        // longer than the server's idle limit, so a connection kept from the GET is one the server has closed
        std::thread::sleep(Duration::from_millis(1100));
        let mut post = agent
            .post(&url("/admin/test/post"))
            .header("Authorization", &auth)
            .send_empty()
            .unwrap_or_else(|e| panic!("round {round}: the POST after a pause failed: {e}"));
        assert_eq!(post.status().as_u16(), 200);
        assert_eq!(post.body_mut().read_to_string().unwrap(), "posted");
    }
}
