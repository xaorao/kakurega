#[test]
fn tls01_the_cli_http_client_can_speak_https() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in listener.incoming().flatten() {
            drop(s);
        }
    });
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(3)))
        .build()
        .into();
    let err = agent
        .get(format!("https://127.0.0.1:{port}/"))
        .call()
        .err()
        .map(|e| e.to_string())
        .unwrap_or_default();
    assert!(
        !err.to_lowercase().contains("tls required") && !err.to_lowercase().contains("unsecured"),
        "an https request must be attempted, not refused as unsecured: {err}"
    );
}
