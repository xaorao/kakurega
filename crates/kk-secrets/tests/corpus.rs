use kk_secrets::{credential_value, entropy, looks_secret, pieces, scan, variants};

fn j(parts: &[&str]) -> String {
    parts.concat()
}

fn body(n: usize, alphabet: &str, seed: usize) -> String {
    let a: Vec<char> = alphabet.chars().collect();
    (0..n)
        .map(|i| a[(i * 7 + seed * 13 + i * i) % a.len()])
        .collect()
}

const ALNUM: &str = "aB3dE5gH7jK9mN2pQ4rS6tU8vW1xY0zCfLhJkMnPqRsTuVwXyZ";
const B32: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const B58: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn b64(s: &str) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in s.as_bytes().chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn positives() -> Vec<(&'static str, String)> {
    let gh = j(&["ghp", "_", &body(36, ALNUM, 1)]);
    let pw = "Zk9fQ2x8Lm4vPq7R";
    vec![
        ("github token", format!("export GH={gh}\n")),
        ("github fine-grained", j(&["github", "_pat_", &body(82, ALNUM, 3)])),
        ("aws id", j(&["AK", "IA", &body(16, B32, 4)])),
        ("aws temp id", j(&["AS", "IA", &body(16, B32, 5)])),
        ("stripe live", j(&["sk", "_live_", &body(26, ALNUM, 6)])),
        ("stripe restricted", j(&["rk", "_live_", &body(26, ALNUM, 7)])),
        ("slack bot", j(&["xox", "b-", "123456789012-1234567890123-", &body(24, ALNUM, 8)])),
        ("google api", j(&["AI", "za", &body(35, ALNUM, 9)])),
        ("anthropic", j(&["sk", "-ant-", "api03-", &body(80, ALNUM, 10)])),
        ("openai project", j(&["sk", "-proj-", &body(60, ALNUM, 11)])),
        ("npm", j(&["npm", "_", &body(36, ALNUM, 12)])),
        ("gitlab", j(&["glpat", "-", &body(20, ALNUM, 13)])),
        ("huggingface", j(&["hf", "_", &body(34, ALNUM, 14)])),
        ("sendgrid", j(&["SG", ".", &body(22, ALNUM, 15), ".", &body(43, ALNUM, 16)])),
        ("private key block", "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n".to_string()),
        ("jwt", j(&["eyJ", "hbGciOiJIUzI1NiJ9", ".", "eyJ", "zdWIiOiIxMjM0NTY3ODkwIn0", ".", &body(30, ALNUM, 17)])),
        ("url password", format!("DATABASE_URL=postgres://app:{pw}@db.internal:5432/main")),
        ("env assignment", format!("PAYMENTS_API_KEY={pw}\n")),
        ("yaml password", format!("database:\n  password: \"{pw}\"\n")),
        ("python assignment", format!("client_secret = '{pw}'")),
        ("near key", j(&["ed25519", ":", &body(88, B58, 18)])),
        ("solana keypair", format!("[{}]", (0..64).map(|i| ((i * 37 + 11) % 256).to_string()).collect::<Vec<_>>().join(","))),
        ("hex private key", j(&["private", "_key = 0x", &body(64, "0123456789abcdef", 19)])),
        ("recovery phrase", "my seed phrase: apple river stone cloud night angle brave candy dwarf eagle fable giant".to_string()),
        ("commented credential", format!("# password = {pw}\n")),
        ("base64 of a token", format!("config: {}", b64(&gh))),
        ("docker auth", format!("{{\"auths\":{{\"r.io\":{{\"auth\":\"{}\"}}}}}}", b64(&format!("deploy:{pw}")))),
        ("basic auth header", format!("Authorization: Basic {}", b64(&format!("admin:{pw}")))),
        ("base64 twice", format!("blob={}", b64(&b64(&j(&["AK", "IA", &body(16, B32, 20)]))))),
    ]
}

fn negatives() -> Vec<(&'static str, String)> {
    vec![
        ("uuid", "id = 123e4567-e89b-12d3-a456-426614174000".into()),
        ("sha256", "digest: 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08".into()),
        ("git sha", "commit 4b825dc642cb6eb9a060e54bf8d69288fbee4904".into()),
        ("counter", "inputTokens += x;".into()),
        ("increment", "token = ++n;".into()),
        ("config lookup", "tokens = settings.get(\"t\")".into()),
        ("environ", "password = os.environ[\"DB_PASSWORD\"]".into()),
        ("template", "api_key = \"${API_KEY}\"".into()),
        ("placeholder", "secret: <your-secret-here>".into()),
        ("count", "TOKEN_COUNT = 1000".into()),
        ("limit", "max_tokens: 4096".into()),
        ("type", "token_type: \"bearer\"".into()),
        ("file name", "password_file = \"/etc/service/pw\"".into()),
        ("prose", "Passwords must be at least 12 characters and tokens expire in ten minutes.".into()),
        ("comparison", "if token == expected_token:".into()),
        ("identifier", "const SECRET_NAMES: &[&str] = &[\"a\"];".into()),
        ("integrity", "integrity: sha512-Zm9vYmFyYmF6cXV4Zm9vYmFyYmF6cXV4Zm9vYmFyYmF6cXV4Zm9vYmFyYmF6cXV4".into()),
        ("image data", "src=\"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==\"".into()),
        ("short word", "password = hunter".into()),
        ("lower word", "secret = production".into()),
        ("twelve words not a phrase", "my seed phrase: the quick brown fox jumps over the lazy dog and runs far".into()),
        ("ordinary code", "def login(user_id):\n    token = tokens.issue(user_id, _counter)\n    ACTIVE_TOKENS[token] = user_id\n    return token\n".into()),
        ("random long identifier", "BuildConfigurationValidatorFactoryInstance".into()),
    ]
}

#[test]
fn sk01_every_provider_format_and_wallet_key_in_the_corpus_is_found() {
    let mut missed = Vec::new();
    for (name, text) in positives() {
        if looks_secret(&text).is_none() {
            missed.push(name);
        }
    }
    assert!(missed.is_empty(), "missed: {missed:?}");
}

#[test]
fn sk02_nothing_in_the_benign_corpus_is_flagged() {
    let mut flagged = Vec::new();
    for (name, text) in negatives() {
        if let Some(r) = looks_secret(&text) {
            flagged.push((name, r));
        }
    }
    assert!(flagged.is_empty(), "false positives: {flagged:?}");
}

#[test]
fn sk03_decoded_hits_say_so_and_a_decode_bomb_finishes_inside_its_budget() {
    let tok = j(&["ghp", "_", &body(36, ALNUM, 1)]);
    let hits = scan(&b64(&tok));
    assert!(
        hits.iter().any(|h| h.decoded && h.rule == "github token"),
        "{hits:?}"
    );
    // 40 levels of base64 around a token: only the first levels are opened
    let mut nested = tok.clone();
    for _ in 0..40 {
        nested = b64(&nested);
    }
    let t0 = std::time::Instant::now();
    let _ = scan(&nested);
    assert!(t0.elapsed().as_millis() < 2000, "bounded work");
    // two megabytes of base64 noise
    let noise = "QUJD".repeat(500_000);
    let t1 = std::time::Instant::now();
    let _ = scan(&noise);
    assert!(
        t1.elapsed().as_millis() < 3000,
        "bounded work on large input: {:?}",
        t1.elapsed()
    );
}

#[test]
fn sk04_credential_values_need_randomness_not_just_length() {
    for v in [
        "Zk9fQ2x8Lm4vPq7R",
        "s3cr3t!Pass99",
        "hunter2hunter2X9",
        "password123456",
    ] {
        assert!(credential_value(v), "{v}");
    }
    for v in [
        "",
        "short",
        "production",
        "PRODUCTION",
        "abcdefgh",
        "11111111",
        "${TOKEN_VALUE}",
        "<token>",
        "get_token()",
        "os.environ['X']",
        "123e4567-e89b-12d3-a456-426614174000",
    ] {
        assert!(!credential_value(v), "{v}");
    }
    assert!(entropy("aaaaaaaa") < 0.1 && entropy("aB3dE5gH7jK9") > 3.0);
}

#[test]
fn sk05_variants_cover_the_transformations_a_leak_can_take() {
    let v = variants("Zk9fQ2x8Lm4vPq7R", 6);
    let raw = "Zk9fQ2x8Lm4vPq7R";
    let rev: String = raw.chars().rev().collect();
    for (name, form) in [
        ("raw", raw.to_string()),
        ("reversed", rev),
        ("base64", b64(raw)),
        (
            "hex",
            raw.bytes().map(|b| format!("{b:02x}")).collect::<String>(),
        ),
        ("percent", "Zk9fQ2x8Lm4vPq7R".to_string()),
    ] {
        assert!(v.contains(&form), "{name}");
    }
    assert!(
        variants("abc", 6).is_empty(),
        "short values are not expanded"
    );
    let p = pieces(
        "postgres://app:Zk9fQ2x8Lm@db.internal:5432/main?sslmode=require",
        6,
    );
    assert!(
        p.contains("Zk9fQ2x8Lm")
            && p.contains("app:Zk9fQ2x8Lm")
            && !p.contains("db.internal")
            && !p.contains("main"),
        "{p:?}"
    );
}

#[test]
fn the_extension_carries_the_same_format_table_as_this_crate() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/extension/formats.js");
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(have, kk_secrets::formats_js(), "regenerate with: cargo run -p kk-secrets --example export_formats > apps/extension/formats.js");
}

/// FNV-1a, written out so the number does not depend on a library's version.
fn fnv(parts: &[String]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in parts {
        for b in p.bytes().chain(std::iter::once(0xff)) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// The detector's answers over its own corpus, as one number. Stores that kept text under an older detector are checked again when
/// `DETECTOR_VERSION` changes; this test makes sure the version is bumped when the answers do.
#[test]
fn dv01_the_detector_version_is_bumped_whenever_its_answers_over_the_corpus_change() {
    let mut parts: Vec<String> = Vec::new();
    for (name, text) in positives() {
        parts.push(format!("{name}={:?}", looks_secret(&text)));
    }
    for (name, text) in negatives() {
        parts.push(format!("{name}={:?}", looks_secret(&text)));
    }
    let got = fnv(&parts);
    const EXPECTED: (u32, u64) = (1, 5040826748400983632);
    assert_eq!(
        (kk_secrets::DETECTOR_VERSION, got),
        EXPECTED,
        "the detector's answers changed: bump DETECTOR_VERSION in crates/kk-secrets/src/lib.rs and put (new version, {got}) here"
    );
}
