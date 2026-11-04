const MARKERS: &[&str] = &[
    "-----BEGIN RSA PRIVATE KEY-----",
    "-----BEGIN PRIVATE KEY-----",
    "-----BEGIN OPENSSH PRIVATE KEY-----",
    "-----BEGIN EC PRIVATE KEY-----",
    "-----BEGIN PGP PRIVATE KEY BLOCK-----",
];
const KEY_WORDS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "secret",
    "token",
    "api_key",
    "apikey",
    "api key",
    "private_key",
    "private key",
    "access_key",
    "seed phrase",
    "recovery phrase",
    "mnemonic",
];
const PREFIXES: &[&str] = &[
    "sk_live_",
    "sk_test_",
    "sk-",
    "ghp_",
    "gho_",
    "ghs_",
    "github_pat_",
    "xoxb-",
    "xoxp-",
    "AIza",
    "AKIA",
    "ASIA",
    "glpat-",
    "npm_",
];

fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '_' | '=' | '-' | '.')
}

fn literal_value(v: &str) -> bool {
    let v = v
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`' || c == '.' || c == ',');
    v.len() >= 8
        && !v.contains(char::is_whitespace)
        && !v.starts_with("<")
        && !v.starts_with("$")
        && !v.starts_with("{")
}

pub fn looks_secret(text: &str) -> Option<&'static str> {
    if MARKERS.iter().any(|m| text.contains(m)) {
        return Some("a private key block");
    }
    let lower = text.to_ascii_lowercase();
    for w in text.split(|c: char| !is_token_char(c)) {
        if w.len() >= 12 && PREFIXES.iter().any(|p| w.starts_with(p)) {
            return Some("a known credential format");
        }
        if w.len() >= 20 && w.matches('.').count() == 2 && w.starts_with("eyJ") {
            return Some("a signed token");
        }
        if w.len() >= 32
            && w.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '_' | '=' | '-'))
        {
            let digits = w.chars().filter(|c| c.is_ascii_digit()).count();
            let letters = w.chars().filter(|c| c.is_ascii_alphabetic()).count();
            if digits >= 4 && letters >= 4 {
                return Some("a long random-looking token");
            }
        }
    }
    for line in lower.lines() {
        for kw in KEY_WORDS {
            let mut from = 0;
            while let Some(i) = line[from..].find(kw) {
                let after = &line[from + i + kw.len()..];
                let rest = after.trim_start_matches([' ', '"', '\'', '`']);
                let tail = rest
                    .strip_prefix(':')
                    .or_else(|| rest.strip_prefix('='))
                    .or_else(|| rest.strip_prefix("is "))
                    .or_else(|| rest.strip_prefix("was "));
                if let Some(t) = tail {
                    if literal_value(t.split_whitespace().next().unwrap_or("")) {
                        return Some("a credential with its value");
                    }
                }
                from += i + kw.len();
            }
        }
    }
    for l in text.lines() {
        if let Some(rest) = l.split("://").nth(1) {
            let authority = rest.split('/').next().unwrap_or("");
            if authority.contains('@')
                && authority.split('@').next().is_some_and(|u| u.contains(':'))
            {
                return Some("a URL with a password in it");
            }
        }
    }
    None
}
