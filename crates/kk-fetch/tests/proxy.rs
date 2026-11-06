//! A fetch connects only to the addresses it checked. A proxy named in the environment would connect for it and step round
//! that check, so it is never used.

use kk_fetch::{Transport, UreqTransport};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

#[test]
fn px02_a_fetch_connects_to_the_pinned_address_and_never_to_an_environment_proxy() {
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    proxy.set_nonblocking(true).unwrap();
    let purl = format!("http://{}", proxy.local_addr().unwrap());
    for k in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        std::env::set_var(k, &purl);
    }
    std::env::remove_var("NO_PROXY");
    std::env::remove_var("no_proxy");
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = server.local_addr().unwrap();
    let h = std::thread::spawn(move || {
        let (mut s, _) = server.accept().unwrap();
        let mut b = [0u8; 2048];
        let _ = s.read(&mut b);
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi");
    });
    let url = url::Url::parse("http://pinned.example/page").unwrap();
    let r = UreqTransport {
        timeout: Duration::from_secs(3),
    }
    .get(&url, &[addr], 1000)
    .unwrap();
    assert_eq!(r.status, 200);
    h.join().unwrap();
    assert!(proxy.accept().is_err(), "the proxy was contacted");
}
