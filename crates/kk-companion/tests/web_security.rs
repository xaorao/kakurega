use kk_companion::security::Security;
use kk_companion::{start, Companion, Options};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

impl Reply {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn raw(
    c: &Companion,
    method: &str,
    path: &str,
    host: Option<&str>,
    extra: &[(&str, &str)],
    body: Option<&str>,
) -> Reply {
    let mut s = TcpStream::connect(c.addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let default_host = format!("127.0.0.1:{}", c.addr.port());
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
        host.unwrap_or(&default_host)
    );
    for (k, v) in extra {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if let Some(b) = body {
        req.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            b.len()
        ));
    }
    req.push_str("\r\n");
    if let Some(b) = body {
        req.push_str(b);
    }
    let _ = s.write_all(req.as_bytes());
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let headers = lines
        .filter_map(|l| l.split_once(": "))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    Reply {
        status,
        headers,
        body: body.to_string(),
    }
}

fn origin(c: &Companion) -> String {
    format!("http://127.0.0.1:{}", c.addr.port())
}

fn pair_with(c: &Companion, code: &str) -> Reply {
    let o = origin(c);
    raw(
        c,
        "POST",
        "/api/pair",
        None,
        &[("Origin", &o)],
        Some(&format!("{{\"code\":\"{code}\"}}")),
    )
}

fn cookie_of(r: &Reply) -> String {
    r.header("set-cookie")
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string()
}

fn csrf_of(r: &Reply) -> String {
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    v["csrf"].as_str().unwrap().to_string()
}

fn paired(c: &Companion) -> (String, String) {
    let r = pair_with(c, &c.security.issue_pairing_code());
    assert_eq!(r.status, 200, "{}", r.body);
    (cookie_of(&r), csrf_of(&r))
}

#[test]
fn ws12_a_request_for_any_other_host_is_refused_before_routing() {
    let c = start(Options::default()).unwrap();
    for host in [
        "evil.example",
        "localhost",
        &format!("localhost:{}", c.addr.port()),
        &format!("127.0.0.1:{}", c.addr.port() + 1),
        "127.0.0.1",
        "[::1]:80",
    ] {
        let r = raw(&c, "GET", "/api/health", Some(host), &[], None);
        assert_eq!(r.status, 421, "Host {host}");
    }
    assert_eq!(raw(&c, "GET", "/api/health", None, &[], None).status, 200);
}

#[test]
fn ws11_other_origins_cannot_use_the_service_even_from_localhost() {
    let c = start(Options::default()).unwrap();
    let code = c.security.issue_pairing_code();
    let body = format!("{{\"code\":\"{code}\"}}");
    for o in [
        "http://evil.example",
        "http://127.0.0.1:1",
        &format!("http://localhost:{}", c.addr.port()),
        "null",
        "http://127.0.0.1",
    ] {
        let r = raw(&c, "POST", "/api/pair", None, &[("Origin", o)], Some(&body));
        assert_eq!(r.status, 403, "Origin {o}: {}", r.body);
    }
    let none = raw(&c, "POST", "/api/pair", None, &[], Some(&body));
    assert_eq!(
        none.status, 403,
        "a state-changing request without an Origin is refused"
    );
    let ok = pair_with(&c, &code);
    assert_eq!(
        ok.status, 200,
        "the code was not consumed by the refused attempts"
    );
    let (cookie, _) = paired(&c);
    let cross = raw(
        &c,
        "GET",
        "/api/whoami",
        None,
        &[("Cookie", &cookie), ("Sec-Fetch-Site", "cross-site")],
        None,
    );
    assert_eq!(cross.status, 403);
    let same_site_other_port = raw(
        &c,
        "GET",
        "/api/whoami",
        None,
        &[("Cookie", &cookie), ("Origin", "http://127.0.0.1:9")],
        None,
    );
    assert_eq!(same_site_other_port.status, 403);
}

#[test]
fn ws13_pairing_codes_are_single_use_expire_and_throttle_guessers() {
    let c = start(Options::default()).unwrap();
    let code = c.security.issue_pairing_code();
    assert_eq!(pair_with(&c, &code).status, 200);
    assert_eq!(pair_with(&c, &code).status, 401, "single use");
    let short = Arc::new(Security::with_pairing_ttl(Duration::from_millis(50)));
    let c2 = start(Options {
        security: short,
        ..Options::default()
    })
    .unwrap();
    let expiring = c2.security.issue_pairing_code();
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(pair_with(&c2, &expiring).status, 401, "expired");
    let c3 = start(Options::default()).unwrap();
    for _ in 0..5 {
        assert_eq!(pair_with(&c3, "guess").status, 401);
    }
    assert_eq!(pair_with(&c3, "guess").status, 429);
}

#[test]
fn ws13b_wrong_guesses_are_throttled_but_they_can_never_keep_the_owner_out() {
    let c = start(Options::default()).unwrap();
    let valid = c.security.issue_pairing_code();
    for _ in 0..5 {
        assert_eq!(pair_with(&c, "guess").status, 401);
    }
    assert_eq!(pair_with(&c, "guess").status, 429, "guessers are throttled");
    assert_eq!(pair_with(&c, "").status, 429);
    let r = pair_with(&c, &valid);
    assert_eq!(
        r.status, 200,
        "the right code works even while guessers are throttled: {}",
        r.body
    );
    assert_eq!(
        pair_with(&c, &valid).status,
        401,
        "and it is still single use"
    );
    for _ in 0..4 {
        assert_eq!(
            pair_with(&c, "guess").status,
            401,
            "a good pairing clears the count"
        );
    }
    assert_eq!(pair_with(&c, "guess").status, 429);
    // a flood of wrong codes, however long, never stops a code that was issued meanwhile
    for _ in 0..200 {
        let _ = pair_with(&c, "flood");
    }
    let later = c.security.issue_pairing_code();
    assert_eq!(pair_with(&c, &later).status, 200);
}

#[test]
fn ws13c_the_throttle_is_safe_only_because_a_pairing_code_cannot_be_guessed() {
    let c = start(Options::default()).unwrap();
    let codes: Vec<String> = (0..20).map(|_| c.security.issue_pairing_code()).collect();
    for code in &codes {
        assert!(code.len() >= 22, "{code}: at least 128 bits of base64");
        assert!(code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
    }
    let mut unique = codes.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), codes.len());
    // a part of a valid code is not a valid code
    assert_eq!(pair_with(&c, &codes[0][..codes[0].len() - 1]).status, 401);
    assert_eq!(pair_with(&c, &codes[1].to_uppercase()).status, 401);
}

#[test]
fn ws14_sessions_live_on_the_server_and_can_be_revoked_at_once() {
    let c = start(Options::default()).unwrap();
    let (cookie, csrf) = paired(&c);
    let (cookie2, csrf2) = paired(&c);
    assert_eq!(
        raw(&c, "GET", "/api/whoami", None, &[("Cookie", &cookie)], None).status,
        200
    );
    assert_eq!(raw(&c, "GET", "/api/whoami", None, &[], None).status, 401);
    assert_eq!(
        raw(
            &c,
            "GET",
            "/api/whoami",
            None,
            &[("Cookie", "kk_session_1=abc")],
            None
        )
        .status,
        401
    );
    let o = origin(&c);
    let no_csrf = raw(
        &c,
        "POST",
        "/api/logout",
        None,
        &[("Cookie", &cookie), ("Origin", &o)],
        None,
    );
    assert_eq!(no_csrf.status, 403);
    let wrong_csrf = raw(
        &c,
        "POST",
        "/api/logout",
        None,
        &[("Cookie", &cookie), ("Origin", &o), ("X-KK-CSRF", &csrf2)],
        None,
    );
    assert_eq!(
        wrong_csrf.status, 403,
        "another session's CSRF token does not work"
    );
    let out = raw(
        &c,
        "POST",
        "/api/logout",
        None,
        &[("Cookie", &cookie), ("Origin", &o), ("X-KK-CSRF", &csrf)],
        None,
    );
    assert_eq!(out.status, 204);
    assert_eq!(
        raw(&c, "GET", "/api/whoami", None, &[("Cookie", &cookie)], None).status,
        401,
        "revoked at once"
    );
    assert_eq!(
        raw(
            &c,
            "GET",
            "/api/whoami",
            None,
            &[("Cookie", &cookie2)],
            None
        )
        .status,
        200
    );
    let all = raw(
        &c,
        "POST",
        "/api/sessions/revoke-all",
        None,
        &[("Cookie", &cookie2), ("Origin", &o), ("X-KK-CSRF", &csrf2)],
        None,
    );
    assert_eq!(all.status, 204);
    assert_eq!(
        raw(
            &c,
            "GET",
            "/api/whoami",
            None,
            &[("Cookie", &cookie2)],
            None
        )
        .status,
        401
    );
    assert_eq!(c.security.active_sessions(), 0);
}

#[test]
fn ws05_cookies_are_httponly_samesite_strict_and_named_for_the_port() {
    let c = start(Options::default()).unwrap();
    let r = pair_with(&c, &c.security.issue_pairing_code());
    let sc = r.header("set-cookie").unwrap();
    assert!(
        sc.starts_with(&format!("kk_session_{}=", c.addr.port())),
        "{sc}"
    );
    assert!(
        sc.contains("HttpOnly") && sc.contains("SameSite=Strict") && sc.contains("Path=/"),
        "{sc}"
    );
}

#[test]
fn ws06_every_response_carries_the_security_headers_even_errors() {
    let c = start(Options::default()).unwrap();
    let replies = [
        raw(&c, "GET", "/api/health", None, &[], None),
        raw(&c, "GET", "/api/whoami", None, &[], None),
        raw(&c, "GET", "/api/health", Some("evil.example"), &[], None),
        raw(&c, "GET", "/nothing", None, &[], None),
    ];
    for r in &replies {
        assert!(
            r.header("content-security-policy")
                .unwrap()
                .contains("default-src 'none'"),
            "{}",
            r.status
        );
        assert!(r
            .header("content-security-policy")
            .unwrap()
            .contains("frame-ancestors 'none'"));
        assert_eq!(r.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(r.header("referrer-policy"), Some("no-referrer"));
        assert_eq!(r.header("cache-control"), Some("no-store"));
        assert_eq!(r.header("x-frame-options"), Some("DENY"));
        assert!(!r
            .header("content-security-policy")
            .unwrap()
            .contains("unsafe-inline"));
    }
}

#[test]
fn ws07_the_admin_interface_is_for_local_tools_only() {
    let c = start(Options::default()).unwrap();
    let token = c.security.admin_token().to_string();
    assert_eq!(raw(&c, "GET", "/admin/ping", None, &[], None).status, 401);
    assert_eq!(
        raw(
            &c,
            "GET",
            "/admin/ping",
            None,
            &[("Authorization", "Bearer wrong")],
            None
        )
        .status,
        401
    );
    assert_eq!(
        raw(
            &c,
            "GET",
            "/admin/ping",
            None,
            &[("Authorization", &format!("Bearer {token}"))],
            None
        )
        .status,
        200
    );
    let o = origin(&c);
    assert_eq!(
        raw(
            &c,
            "GET",
            "/admin/ping",
            None,
            &[
                ("Authorization", &format!("Bearer {token}")),
                ("Origin", &o)
            ],
            None
        )
        .status,
        403,
        "a web page never reaches the admin interface"
    );
    let (cookie, _) = paired(&c);
    assert_eq!(
        raw(&c, "GET", "/admin/ping", None, &[("Cookie", &cookie)], None).status,
        403
    );
    assert_eq!(
        raw(
            &c,
            "GET",
            "/api/whoami",
            None,
            &[("Authorization", &format!("Bearer {token}"))],
            None
        )
        .status,
        401,
        "the admin token is not a session"
    );
}

#[test]
fn ws08_static_files_cannot_escape_the_ui_directory_or_follow_links() {
    let dir = std::env::temp_dir().join(format!("kk-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("index.html"), "<html>ok</html>").unwrap();
    std::fs::write(dir.join("assets/app.js"), "console.log(1)").unwrap();
    std::fs::write(dir.join(".hidden"), "hidden").unwrap();
    let outside = std::env::temp_dir().join(format!("kk-outside-{}.txt", std::process::id()));
    std::fs::write(&outside, "TOP SECRET").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join("assets/link.txt")).unwrap();
    let c = start(Options {
        ui_dir: Some(dir.clone()),
        ..Options::default()
    })
    .unwrap();
    assert!(raw(&c, "GET", "/", None, &[], None).body.contains("ok"));
    let js = raw(&c, "GET", "/assets/app.js", None, &[], None);
    assert_eq!(js.status, 200);
    assert!(js
        .header("content-type")
        .unwrap()
        .starts_with("text/javascript"));
    for p in [
        "/../Cargo.toml",
        "/%2e%2e/%2e%2e/etc/passwd",
        "/..%2f..%2fetc/passwd",
        "/assets/../../etc/passwd",
        "/.hidden",
        "/assets/link.txt",
        "//etc/passwd",
        "/assets/%00app.js",
        "/assets\\..\\x",
    ] {
        let r = raw(&c, "GET", p, None, &[], None);
        assert!(
            !r.body.contains("TOP SECRET")
                && !r.body.contains("root:")
                && !r.body.contains("hidden"),
            "{p}: {} {}",
            r.status,
            r.body
        );
        assert!(
            r.status != 200 || r.body == "<html>ok</html>",
            "{p}: only the app shell may be served for unknown paths, got {}",
            r.body
        );
    }
}

#[test]
fn ws09_oversized_bodies_are_refused() {
    let c = start(Options::default()).unwrap();
    let o = origin(&c);
    let big = format!("{{\"code\":\"{}\"}}", "a".repeat(2 << 20));
    let r = raw(&c, "POST", "/api/pair", None, &[("Origin", &o)], Some(&big));
    assert!(
        r.status == 413 || r.status == 0,
        "refused either with 413 or by closing the connection, got {}",
        r.status
    );
    assert_eq!(
        pair_with(&c, &c.security.issue_pairing_code()).status,
        200,
        "the service is still healthy"
    );
}

#[test]
fn ws15_the_session_status_endpoint_never_errors_and_never_leaks_a_token_without_a_session() {
    let c = start(Options::default()).unwrap();
    let anon = raw(&c, "GET", "/api/session", None, &[], None);
    assert_eq!(anon.status, 200);
    assert!(
        anon.body.contains("\"paired\":false") && !anon.body.contains("csrf"),
        "{}",
        anon.body
    );
    let (cookie, csrf) = paired(&c);
    let with = raw(
        &c,
        "GET",
        "/api/session",
        None,
        &[("Cookie", &cookie)],
        None,
    );
    assert!(
        with.body.contains("\"paired\":true") && with.body.contains(&csrf),
        "{}",
        with.body
    );
    let cross = raw(
        &c,
        "GET",
        "/api/session",
        None,
        &[("Cookie", &cookie), ("Sec-Fetch-Site", "cross-site")],
        None,
    );
    assert_eq!(
        cross.status, 403,
        "the status endpoint is still protected from cross-site pages"
    );
}

const SHELL: &str = "k7Zp2QwL9xVbN4mRtY8uHc3JdFg6SaE1oPi5KeXrTwA";

fn shell_companion() -> Companion {
    start(Options {
        shell_secret: Some(SHELL.to_string()),
        ..Options::default()
    })
    .unwrap()
}

#[test]
fn ws20_with_a_shell_secret_nothing_but_the_shell_gets_any_answer_from_the_app() {
    let c = shell_companion();
    let o = origin(&c);
    let paths = [
        "/",
        "/index.html",
        "/api/health",
        "/api/session",
        "/api/nothing",
        "/assets/x.js",
    ];
    for p in paths {
        let r = raw(&c, "GET", p, None, &[], None);
        assert_eq!(r.status, 403, "{p}: {}", r.body);
        assert!(r.body.contains("SHELL_ONLY"), "{p}: {}", r.body);
        assert!(
            r.header("content-security-policy").is_some(),
            "{p}: refusals carry the security headers"
        );
    }
    let r = raw(
        &c,
        "POST",
        "/api/pair",
        None,
        &[("Origin", &o)],
        Some(r#"{"code":"x"}"#),
    );
    assert_eq!(r.status, 403);
    // wrong, near and empty secrets are refused alike
    let near = format!("{SHELL}x");
    let short = &SHELL[..SHELL.len() - 1];
    let upper = SHELL.to_uppercase();
    for bad in ["", "x", short, near.as_str(), upper.as_str()] {
        let r = raw(&c, "GET", "/api/health", None, &[("x-kk-shell", bad)], None);
        assert_eq!(r.status, 403, "{bad:?}");
    }
    // the right one gets the normal service, including pairing and a session
    let r = raw(
        &c,
        "GET",
        "/api/health",
        None,
        &[("x-kk-shell", SHELL)],
        None,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let code = c.security.issue_pairing_code();
    let r = raw(
        &c,
        "POST",
        "/api/pair",
        None,
        &[("Origin", &o), ("x-kk-shell", SHELL)],
        Some(&format!("{{\"code\":\"{code}\"}}")),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    // the secret is never echoed
    for p in ["/api/health", "/api/session", "/"] {
        for with in [true, false] {
            let extra: Vec<(&str, &str)> = if with {
                vec![("x-kk-shell", SHELL)]
            } else {
                vec![]
            };
            let r = raw(&c, "GET", p, None, &extra, None);
            let all = format!("{:?}{}", r.headers, r.body);
            assert!(!all.contains(SHELL), "{p}");
        }
    }
}

#[test]
fn ws21_the_secret_does_not_touch_the_doors_that_have_their_own_proof() {
    let c = shell_companion();
    // the admin door still needs its bearer token and still refuses anything from a page
    let r = raw(&c, "POST", "/admin/pairing", None, &[], Some("{}"));
    assert_eq!(r.status, 401, "{}", r.body);
    assert!(r.body.contains("ADMIN_AUTH"), "{}", r.body);
    let bearer = format!("Bearer {}", c.security.admin_token());
    let r = raw(
        &c,
        "POST",
        "/admin/pairing",
        None,
        &[("Authorization", &bearer)],
        Some("{}"),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    // the shell secret does not stand in for the admin token
    let r = raw(
        &c,
        "POST",
        "/admin/pairing",
        None,
        &[("x-kk-shell", SHELL)],
        Some("{}"),
    );
    assert_eq!(r.status, 401);
    // the extension door answers by its own rule, not the shell's
    let r = raw(&c, "GET", "/ext/anything", None, &[], None);
    assert!(!r.body.contains("SHELL_ONLY"), "{}", r.body);
    // a wrong host is still refused first
    let r = raw(
        &c,
        "GET",
        "/api/health",
        Some("evil.test"),
        &[("x-kk-shell", SHELL)],
        None,
    );
    assert_eq!(r.status, 421);
}

#[test]
fn ws22_without_a_shell_secret_nothing_changes_and_a_weak_one_is_refused_at_start() {
    let c = start(Options::default()).unwrap();
    assert_eq!(raw(&c, "GET", "/api/health", None, &[], None).status, 200);
    for weak in ["", "short", &"a".repeat(31)] {
        let r = start(Options {
            shell_secret: Some(weak.to_string()),
            ..Options::default()
        });
        assert!(r.is_err(), "{weak:?} must not be accepted");
    }
    assert!(start(Options {
        shell_secret: Some("a".repeat(32)),
        ..Options::default()
    })
    .is_ok());
}
