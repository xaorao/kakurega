//! Credentials in HTTP headers outside a `curl` command: `Authorization: Bearer ...` in a request dump, a JSON or YAML header,
//! Python or JavaScript code or a sentence, a bare `Bearer` token, and the values of session cookies. The reference scanners read
//! an Authorization header only inside `curl`, so a fresh random token in any other form passed.

use kk_secrets::{looks_secret, scan};
use std::time::Instant;

fn fresh(alpha: &str, n: usize, seed: u64) -> String {
    let c: Vec<char> = alpha.chars().collect();
    let mut s = seed | 1;
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            c[(s % c.len() as u64) as usize]
        })
        .collect()
}

fn flagged(text: &str) -> bool {
    !scan(text).is_empty() || looks_secret(text).is_some()
}

const ALNUM: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

#[test]
fn hd01_a_fresh_random_token_is_flagged_in_every_header_form() {
    let shapes = [
        ("a bare header", "Authorization: Bearer {}"),
        ("lower case", "authorization: bearer {}"),
        ("an HTTP request dump", "GET /v1/items HTTP/1.1\r\nHost: api.example.com\r\nAuthorization: Bearer {}\r\nAccept: */*\r\n"),
        ("a JSON header", "{\"headers\": {\"Authorization\": \"Bearer {}\"}}"),
        ("Python", "requests.get(url, headers={\"Authorization\": \"Bearer {}\"})"),
        ("JavaScript", "fetch(url, { headers: { Authorization: 'Bearer {}' } })"),
        ("YAML", "headers:\n  authorization: Bearer {}\n"),
        ("a Token scheme", "Authorization: Token {}"),
        ("an Api-Token scheme", "Authorization: Api-Token {}"),
        ("a raw key", "Authorization: {}"),
        ("a proxy header", "Proxy-Authorization: Bearer {}"),
        ("a Cookie header", "Cookie: theme=dark; sessionid={}"),
        ("a Set-Cookie header", "Set-Cookie: sessionid={}; Path=/; HttpOnly"),
        ("a cookie in a curl header", "curl -H 'Cookie: session={}' https://example.com"),
        ("a Bearer token in a sentence", "the token is Bearer {} and it expires"),
        ("inside curl, as before", "curl -H 'Authorization: Bearer {}' https://api.example.com"),
    ];
    for (name, shape) in shapes {
        let mut missed = Vec::new();
        for seed in 1..=3000u64 {
            let t = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 8;
            let text = shape.replace("{}", &fresh(ALNUM, 40, t));
            if !flagged(&text) {
                missed.push(text);
            }
        }
        // the sentence form depends on the same filters as every other credential and can lose a few random values
        let allowed = if name == "a Bearer token in a sentence" {
            15
        } else {
            0
        };
        assert!(
            missed.len() <= allowed,
            "{name}: {} of 3000 not flagged, for example {:?}",
            missed.len(),
            missed.iter().take(2).collect::<Vec<_>>()
        );
    }
}

#[test]
fn hd02_other_token_shapes_in_a_header_are_flagged_too() {
    for (name, text) in [
        ("hexadecimal", format!("Authorization: Bearer {}", fresh("0123456789abcdef", 40, 7))),
        ("base64 with symbols", format!("Authorization: Bearer {}", fresh(&format!("{ALNUM}+/="), 44, 9))),
        ("a JWT", "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4ifQ.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U".to_string()),
        ("HTTP Basic with a real-looking password", "Authorization: Basic am9obi5kb2U6czNjcjN0UGFzc3cwcmQh".to_string()),
        ("a session cookie with a symbol", format!("Cookie: PHPSESSID={}", fresh(ALNUM, 26, 3))),
        ("a short token after an explicit Bearer scheme", "Authorization: Bearer k3Jd9Xm2Qp".to_string()),
        ("a short token after an explicit Token scheme", "Authorization: Token k3Jd9Xm2Qp".to_string()),
        ("a short token after an explicit Api-Token scheme", "Authorization: Api-Token k3Jd9Xm2Qp".to_string()),
        ("an equals sign instead of a colon", "Authorization=Bearer k3Jd9Xm2Qp".to_string()),
        ("a rocket instead of a colon", "'Authorization' => 'Bearer k3Jd9Xm2Qp'".to_string()),
        ("a remember-me cookie", format!("Set-Cookie: remember_me={}; Max-Age=999", fresh(ALNUM, 32, 5))),
        ("an auth cookie among others", format!("Cookie: a=1; b=2; auth_token={}; c=3", fresh(ALNUM, 36, 11))),
    ] {
        assert!(flagged(&text), "{name} not flagged: {text}");
    }
}

#[test]
fn hd03_documentation_placeholders_prose_and_ordinary_cookies_are_left_alone() {
    for text in [
        "Authorization: Bearer <token>",
        "Authorization: Bearer ${TOKEN}",
        "Authorization: Bearer $TOKEN",
        "Authorization: Bearer {{ token }}",
        "Authorization: Bearer YOUR_API_KEY",
        "Authorization: Bearer your-token-here",
        "Authorization: Bearer xxxxxxxxxxxxxxxxxxxx",
        "Authorization: Bearer",
        "Authorization:",
        "Authorization: Basic <base64(user:password)>",
        "Authorization: Basic dXNlcjpwYXNzd29yZA==",
        "fetch(url, { headers: { Authorization: `Bearer ${token}` } })",
        "Bearer tokens are sent in the Authorization header of each request.",
        "The authorization code flow returns a short-lived code.",
        "grant_type=authorization_code",
        "Authorization header: required",
        "Cookie: theme=dark; lang=en; consent=yes",
        "Set-Cookie: theme=dark; Path=/",
        "Cookie: session=<value>",
        "Cookie: session=${SESSION_ID}",
        "qa-f19-http-bearer qa-f19-http-oauth qa-f19-http-none",
        "is_bearer abcdef0123456789abcdef",
        "bearer: required, scheme: bearer",
        "the Bearer a1b2c3d4e5 scheme",
        "uses Bearer authenticationmechanismvalidation for requests",
        "the Bearer thequickbrownfoxjumpsoverthelazydog scheme",
        "Authorization: abc123def45",
        "Authorization: Bearer abc12",
    ] {
        assert!(!flagged(text), "withheld by mistake: {text:?}");
    }
}

#[test]
fn hd04_text_with_non_ascii_letters_around_the_keywords_never_panics_and_still_finds_the_token() {
    let token = fresh(ALNUM, 40, 21);
    for pad in 0..9 {
        let lead = "ậ".repeat(pad);
        for shape in [
            "Cookie: sid={}",
            "Set-Cookie: sid={}; Path=/",
            "Authorization: Bearer {}",
            "\"Authorization\": \"Bearer {}\"",
            "Bearer {}",
        ] {
            let text = format!("{lead}{}", shape.replace("{}", &token));
            assert!(
                flagged(&text),
                "not flagged after {pad} non-ASCII letters: {text}"
            );
            // and the keyword cut off by multi-byte text on both sides
            let around = format!("{lead}é{}é{lead}", shape.replace("{}", &token));
            let _ = scan(&around);
        }
    }
    for text in [
        "ậậcookie",
        "ậậset-cookie:",
        "ậậậcookie: a=b",
        "é\u{0301}authorization:é",
        "authorizationậ: x",
        "bearerậ x",
    ] {
        let _ = scan(text);
    }
}

#[test]
fn hd05_a_hostile_text_is_scanned_in_linear_time_whatever_repeats() {
    // each of these once made, or could make, the work grow with the square of the size (`Cookie: ` repeated 60,000 times took
    // twelve seconds before the lines and tokens were bounded); each is now a few tens of milliseconds
    let inputs: Vec<(&str, String)> = vec![
        (
            "authorization: bearer, repeated",
            "authorization: bearer ".repeat(40_000),
        ),
        ("Cookie: repeated", "Cookie: ".repeat(60_000)),
        ("Set-Cookie: repeated", "Set-Cookie: ".repeat(40_000)),
        ("authorization= repeated", "authorization=".repeat(50_000)),
        (
            "Authorization: a repeated",
            "Authorization: a".repeat(50_000),
        ),
        ("bearer x1 repeated", "bearer x1".repeat(60_000)),
        (
            "one very long bearer value",
            format!("Authorization: Bearer {}", "a1".repeat(500_000)),
        ),
        (
            "a cookie line of semicolons",
            format!("Cookie: sid={}", ";".repeat(500_000)),
        ),
        (
            "one cookie line of 70,000 pairs",
            format!("Cookie: {}", "a=bbbbbbbbbbbb;".repeat(70_000)),
        ),
        ("non-ASCII around the keyword", "ậCookie: é".repeat(60_000)),
    ];
    for (name, text) in &inputs {
        let start = Instant::now();
        let _ = scan(text);
        assert!(
            start.elapsed().as_secs_f64() < 3.0,
            "{name}: {} bytes took {:?}",
            text.len(),
            start.elapsed()
        );
    }
}
