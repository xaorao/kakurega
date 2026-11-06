//! A model on this computer must never be reached through a proxy named in the environment: the prompt would travel to it.

use kk_route::{ChatMessage, CompletionRequest, OllamaRoute, Route};
use std::net::TcpListener;
use std::time::Duration;

#[test]
fn px01_a_call_to_the_local_model_does_not_go_through_an_environment_proxy() {
    let proxy = TcpListener::bind("127.0.0.1:0").unwrap();
    proxy.set_nonblocking(true).unwrap();
    let url = format!("http://{}", proxy.local_addr().unwrap());
    for k in [
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        std::env::set_var(k, &url);
    }
    std::env::remove_var("NO_PROXY");
    std::env::remove_var("no_proxy");
    let model = TcpListener::bind("127.0.0.1:0").unwrap();
    model.set_nonblocking(true).unwrap();
    let req = CompletionRequest {
        messages: vec![ChatMessage::new("user", "SECRET-PROMPT")],
        tools: vec![],
        max_tokens: 5,
        temperature: 0.0,
    };
    let route = OllamaRoute::new(&format!("http://{}", model.local_addr().unwrap()), "m")
        .unwrap()
        .with_retry(kk_route::retry::Retry::none())
        .with_timeout(Duration::from_millis(300));
    let _ = route.complete(&req);
    let _ = route.stream(&req, &mut |_| true);
    std::thread::sleep(Duration::from_millis(200));
    assert!(proxy.accept().is_err(), "the proxy was contacted");
    assert!(
        model.accept().is_ok(),
        "the model's own address was contacted directly"
    );
}
