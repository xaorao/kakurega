//! Choosing the NEAR route for `kaku serve`: `--route near --near-model NAME` and the account key, read from the environment
//! (`NEAR_AI_API_KEY`) or from a file the owner names (`--near-key-file`, a line `NEAR_AI_API_KEY=...`, readable by the owner only).
//! The key is never printed, never logged and never part of an error text.

use kk_near::route::{NearConfig, NearRoute};
use kk_near::NearPolicy;
use std::path::Path;
use std::sync::Arc;

const KEY_VAR: &str = "NEAR_AI_API_KEY";

/// A key that is plain: one token of printable characters, not empty, not absurdly long.
fn plain_key(k: &str) -> bool {
    !k.is_empty() && k.len() <= 256 && k.chars().all(|c| c.is_ascii_graphic())
}

/// The key from a file of `NAME=value` lines. The file must be readable by its owner only.
pub fn key_from_file(path: &Path) -> Result<String, String> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path)
        .map_err(|_| format!("the key file {} could not be read", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(format!(
            "the key file {} can be read by other users: run chmod 600 on it",
            path.display()
        ));
    }
    if meta.len() > 4096 {
        return Err(format!("{} is too large to be a key file", path.display()));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|_| format!("the key file {} could not be read", path.display()))?;
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line
            .strip_prefix(KEY_VAR)
            .and_then(|r| r.trim_start().strip_prefix('='))
        {
            let v = v.trim().trim_matches('"').trim_matches('\'');
            return if plain_key(v) {
                Ok(v.to_string())
            } else {
                Err(format!(
                    "the {KEY_VAR} line in {} is empty or not a plain key",
                    path.display()
                ))
            };
        }
    }
    Err(format!("{} has no {KEY_VAR} line", path.display()))
}

/// The base address: https, or plain http only to this computer (a test gateway).
fn plain_url(u: &str) -> bool {
    if let Some(rest) = u.strip_prefix("https://") {
        return !rest.is_empty() && !rest.contains(['@', ' ', '\\']);
    }
    u.strip_prefix("http://").is_some_and(|rest| {
        let host = rest.split(['/', ':']).next().unwrap_or("");
        matches!(host, "127.0.0.1" | "localhost" | "[::1]")
    })
}

/// The NEAR route the flags ask for, or `None` for the local model. A route that is asked for and cannot be made is an error:
/// the service never starts on a different route than the one that was chosen.
pub fn remote_from_flags(args: &[String]) -> Result<Option<Arc<NearRoute>>, String> {
    match crate::flag(args, "--route").as_deref() {
        None | Some("local") => return Ok(None),
        Some("near") => {}
        Some(other) => return Err(format!("--route is local or near, not {other:?}")),
    }
    let model = crate::flag(args, "--near-model")
        .ok_or("--route near needs --near-model NAME, a model NEAR AI Cloud lists as confidential (full TEE)")?;
    let base_url = crate::flag(args, "--near-url");
    if let Some(u) = &base_url {
        if !plain_url(u) {
            return Err("--near-url must be an https address (or http to this computer, for a test gateway)".into());
        }
    }
    let key = match (
        crate::flag(args, "--near-key-file"),
        std::env::var(KEY_VAR).ok(),
    ) {
        (Some(path), _) => key_from_file(Path::new(&path))?,
        (None, Some(k)) if plain_key(&k) => k,
        (None, Some(_)) => return Err(format!("{KEY_VAR} is empty or not a plain key")),
        (None, None) => {
            return Err(format!(
                "--route near needs the account key: set {KEY_VAR}, or give --near-key-file FILE"
            ))
        }
    };
    NearRoute::new(NearConfig {
        base_url,
        api_key: key,
        model,
        policy: NearPolicy::default(),
    })
    .map(|r| Some(Arc::new(r)))
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn file(tag: &str, text: &str, mode: u32) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("kk-near-key-{tag}-{}", std::process::id()));
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn nk01_a_key_file_is_read_only_when_private_and_plain_and_the_key_is_never_in_an_error() {
        let secret = "sk-test-0123456789abcdef";
        let good = file(
            "good",
            &format!("# the account key\nOTHER=1\nNEAR_AI_API_KEY = \"{secret}\"\n"),
            0o600,
        );
        assert_eq!(key_from_file(&good).unwrap(), secret);
        let open = file("open", &format!("NEAR_AI_API_KEY={secret}\n"), 0o644);
        let e = key_from_file(&open).unwrap_err();
        assert!(e.contains("chmod 600") && !e.contains(secret), "{e}");
        for (tag, text, why) in [
            ("none", "OTHER=1\n", "has no NEAR_AI_API_KEY line"),
            ("empty", "NEAR_AI_API_KEY=\n", "empty or not a plain key"),
            ("space", "NEAR_AI_API_KEY=a b\n", "empty or not a plain key"),
            (
                "ctrl",
                "NEAR_AI_API_KEY=ab\u{7}\n",
                "empty or not a plain key",
            ),
            (
                "long",
                &format!("NEAR_AI_API_KEY={}\n", "k".repeat(300)),
                "empty or not a plain key",
            ),
        ] {
            let f = file(tag, text, 0o600);
            let e = key_from_file(&f).unwrap_err();
            assert!(e.contains(why), "{tag}: {e}");
            assert!(
                !e.contains(&"k".repeat(20)),
                "{tag}: the key is in the error: {e}"
            );
        }
        assert!(key_from_file(Path::new("/nonexistent/key")).is_err());
        assert!(key_from_file(&std::env::temp_dir())
            .unwrap_err()
            .contains("not a file"));
        let big = file("big", &"x".repeat(5000), 0o600);
        assert!(key_from_file(&big).unwrap_err().contains("too large"));
    }

    #[test]
    fn nk02_the_route_flags_choose_local_by_default_and_refuse_what_is_half_given() {
        assert!(remote_from_flags(&args(&["--model", "x"]))
            .unwrap()
            .is_none());
        assert!(remote_from_flags(&args(&["--route", "local"]))
            .unwrap()
            .is_none());
        let e = remote_from_flags(&args(&["--route", "other"]))
            .err()
            .unwrap();
        assert!(e.contains("local or near"), "{e}");
        let e = remote_from_flags(&args(&["--route", "near"]))
            .err()
            .unwrap();
        assert!(e.contains("--near-model"), "{e}");
        let key = file("flagkey", "NEAR_AI_API_KEY=abc123\n", 0o600);
        let k = key.to_str().unwrap();
        for bad in [
            "http://evil.example/v1",
            "ftp://x",
            "https://",
            "https://a b",
            "https://u@h/",
            "http://127.0.0.1.evil.example/",
        ] {
            let e = remote_from_flags(&args(&[
                "--route",
                "near",
                "--near-model",
                "m",
                "--near-key-file",
                k,
                "--near-url",
                bad,
            ]))
            .err()
            .unwrap();
            assert!(e.contains("--near-url"), "{bad}: {e}");
        }
        // a route that is asked for and made: no network call yet, so no account is needed to build it
        let r = remote_from_flags(&args(&[
            "--route",
            "near",
            "--near-model",
            "vendor/model",
            "--near-key-file",
            k,
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(r.model(), "vendor/model");
        let r = remote_from_flags(&args(&[
            "--route",
            "near",
            "--near-model",
            "m",
            "--near-key-file",
            k,
            "--near-url",
            "http://127.0.0.1:9/v1",
        ]))
        .unwrap()
        .unwrap();
        assert!(r.base_url().starts_with("http://127.0.0.1:9"));
    }
}
