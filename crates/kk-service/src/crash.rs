//! A record of a crash that the owner can read and decide to attach to a bug report.
//!
//! When the program panics, a small file is written into the data folder: the version, the time, the thread's name and the
//! place in the code (a path inside the project or a dependency, with every personal part of the path removed). It never
//! holds the panic message, because a message can quote a file, a note or a key. Nothing is sent anywhere; the report is read
//! by `kaku diagnostics`, and a file that does not look exactly like this is not shown.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// The most reports kept; the oldest go first.
pub const KEEP: usize = 10;
const HEADER: &str = "kakurega-crash 1";
const MAX_FILE: u64 = 2048;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub version: String,
    pub at: u64,
    pub thread: String,
    pub place: String,
    pub message_kind: String,
}

fn folder(data: &Path) -> PathBuf {
    data.join("crashes")
}

/// A source path with everything personal taken off: a path inside this project starts at `crates/`, one inside a
/// downloaded dependency at its name and version, and anything else is a bare file name.
pub fn scrub_place(file: &str, line: u32) -> String {
    let normal = file.replace('\\', "/");
    let kept = if let Some(i) = normal.rfind("/crates/") {
        normal[i + 1..].to_string()
    } else if normal.starts_with("crates/") {
        normal.clone()
    } else if let Some(i) = normal.find("/registry/src/") {
        // …/registry/src/index.crates.io-xxxx/name-1.2.3/src/lib.rs
        let rest = &normal[i + "/registry/src/".len()..];
        rest.split_once('/')
            .map(|(_, r)| r.to_string())
            .unwrap_or_default()
    } else if let Some(i) = normal.find("/rustlib/") {
        // the standard library: library/core/src/…
        let rest = &normal[i..];
        rest.find("/library/")
            .map(|j| rest[j + 1..].to_string())
            .unwrap_or_else(|| "std".to_string())
    } else {
        normal.rsplit('/').next().unwrap_or("").to_string()
    };
    let kept: String = kept
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '+'))
        .take(110)
        .collect();
    if kept.is_empty() {
        format!("unknown:{line}")
    } else {
        format!("{kept}:{line}")
    }
}

fn plain(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | ':' | '/'))
        .take(max)
        .collect()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Writes one report. Never panics and never fails loudly: a crash that cannot be recorded is simply not recorded.
pub fn record(data: &Path, thread: &str, file: &str, line: u32, message_kind: &str) {
    let dir = folder(data);
    if crate::keys::ensure_private_dir(&dir).is_err() {
        return;
    }
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let at = now();
    let body = format!(
        "{HEADER}\nversion {}\nat {at}\nthread {}\nplace {}\nmessage {}\n",
        plain(env!("CARGO_PKG_VERSION"), 40),
        plain(thread, 40),
        scrub_place(file, line),
        if message_kind == "text" {
            "text"
        } else {
            "other"
        },
    );
    let path = dir.join(format!("crash-{at:012}-{n:04}.txt"));
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        let _ = f.write_all(body.as_bytes());
        let _ = f.sync_all();
    }
    prune(&dir);
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("crash-") && n.ends_with(".txt"))
        .collect();
    v.sort();
    v
}

fn prune(dir: &Path) {
    let all = names(dir);
    for old in all.iter().take(all.len().saturating_sub(KEEP)) {
        let _ = std::fs::remove_file(dir.join(old));
    }
}

/// Makes every panic of this process leave a report in `data`, then runs whatever hook was there before (which prints the
/// usual message to the terminal).
pub fn install(data: &Path) {
    let data = data.to_path_buf();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_string();
        let (file, line) = info
            .location()
            .map(|l| (l.file().to_string(), l.line()))
            .unwrap_or_default();
        let kind = if info.payload().downcast_ref::<&str>().is_some()
            || info.payload().downcast_ref::<String>().is_some()
        {
            "text"
        } else {
            "other"
        };
        record(&data, &thread, &file, line, kind);
        previous(info);
    }));
}

/// The reports on disk, newest first. A file that is too big, or whose every line is not exactly what this module writes,
/// is skipped: the owner is never shown text this program did not write.
pub fn reports(data: &Path) -> Vec<Report> {
    let dir = folder(data);
    let mut out = Vec::new();
    // files that are not reports do not use up the places of real ones; at most 200 names are looked at
    for name in names(&dir).into_iter().rev().take(200) {
        if out.len() >= KEEP {
            break;
        }
        let path = dir.join(&name);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.len() > MAX_FILE {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(r) = parse(&text) {
            out.push(r);
        }
    }
    out
}

fn parse(text: &str) -> Option<Report> {
    let mut lines = text.lines();
    if lines.next()? != HEADER {
        return None;
    }
    let field =
        |l: Option<&str>, key: &str, max: usize, allowed: fn(char) -> bool| -> Option<String> {
            let v = l?.strip_prefix(key)?.strip_prefix(' ')?;
            (!v.is_empty() && v.chars().count() <= max && v.chars().all(allowed))
                .then(|| v.to_string())
        };
    let version = field(lines.next(), "version", 40, |c| {
        c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-')
    })?;
    let at = field(lines.next(), "at", 12, |c| c.is_ascii_digit())?
        .parse()
        .ok()?;
    let thread = field(lines.next(), "thread", 40, |c| {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+' | ':' | '/')
    })?;
    let place = field(lines.next(), "place", 130, |c| {
        c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | '+' | ':')
    })?;
    // a place is a relative path as `scrub_place` writes it: never absolute, never climbing, never empty
    if place.starts_with('/')
        || place.contains("..")
        || place.contains("//")
        || !place.contains(':')
    {
        return None;
    }
    let message_kind = field(lines.next(), "message", 5, |c| c.is_ascii_lowercase())?;
    if message_kind != "text" && message_kind != "other" {
        return None;
    }
    if lines.next().is_some() {
        return None;
    }
    Some(Report {
        version,
        at,
        thread,
        place,
        message_kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kk-crash-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn cr01_a_place_loses_every_personal_part_of_its_path() {
        assert_eq!(
            scrub_place(
                "/Users/alice/work/near/kakurega/crates/kk-service/src/service.rs",
                42
            ),
            "crates/kk-service/src/service.rs:42"
        );
        assert_eq!(
            scrub_place("crates/kk-cli/src/main.rs", 7),
            "crates/kk-cli/src/main.rs:7"
        );
        assert_eq!(
            scrub_place("C:\\Users\\bob\\kakurega\\crates\\kk-parse\\src\\pdf.rs", 9),
            "crates/kk-parse/src/pdf.rs:9"
        );
        assert_eq!(scrub_place("/Users/alice/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/serde_json-1.0.140/src/de.rs", 3), "serde_json-1.0.140/src/de.rs:3");
        assert_eq!(scrub_place("/Users/alice/.rustup/toolchains/stable/lib/rustlib/src/rust/library/core/src/option.rs", 1), "library/core/src/option.rs:1");
        assert_eq!(
            scrub_place("/home/carol/secret-project/build.rs", 5),
            "build.rs:5"
        );
        for p in ["", "/", "///", "\u{202e}\u{0}"] {
            assert!(scrub_place(p, 1).ends_with(":1"), "{p:?}");
        }
        for bad in ["alice", "bob", "carol", "/Users", "/home"] {
            for p in [
                "/Users/alice/work/crates/kk-service/src/x.rs",
                "C:\\Users\\bob\\crates\\a.rs",
                "/home/carol/x/y.rs",
            ] {
                assert!(
                    !scrub_place(p, 1).contains(bad),
                    "{p} -> {}",
                    scrub_place(p, 1)
                );
            }
        }
    }

    #[test]
    fn cr02_a_report_holds_only_the_fixed_fields_and_never_the_message() {
        let d = temp("fields");
        let hook_data = d.clone();
        let result = std::thread::Builder::new()
            .name("worker-1".into())
            .spawn(move || {
                let previous = std::panic::take_hook();
                install(&hook_data);
                let r = std::panic::catch_unwind(|| {
                    let key = format!("{}{}", "sk_li", "ve_abcdefghijklmnop0123");
                    panic!("could not read /Users/alice/notes.txt: {key} and the note said hello");
                });
                let _ = std::panic::take_hook();
                std::panic::set_hook(previous);
                r.is_err()
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(result);
        let all = reports(&d);
        assert_eq!(all.len(), 1, "{all:?}");
        assert_eq!(all[0].thread, "worker-1");
        assert!(
            all[0].place.starts_with("crates/kk-service/src/crash.rs:"),
            "{}",
            all[0].place
        );
        assert_eq!(all[0].message_kind, "text");
        let file = std::fs::read_dir(d.join("crashes"))
            .unwrap()
            .flatten()
            .next()
            .unwrap()
            .path();
        let raw = std::fs::read_to_string(&file).unwrap();
        for leak in ["alice", "notes.txt", "sk_live", "hello", "/Users"] {
            assert!(!raw.contains(leak), "{leak} reached the report: {raw}");
        }
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cr03_only_the_newest_reports_are_kept() {
        let d = temp("keep");
        for i in 0..(KEEP + 7) {
            record(
                &d,
                &format!("t{i}"),
                "crates/kk-service/src/a.rs",
                i as u32,
                "text",
            );
        }
        let all = reports(&d);
        assert_eq!(all.len(), KEEP);
        assert_eq!(all[0].thread, format!("t{}", KEEP + 6), "newest first");
        assert_eq!(std::fs::read_dir(d.join("crashes")).unwrap().count(), KEEP);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cr04_recording_never_fails_loudly_when_the_folder_cannot_be_written() {
        let d = temp("blocked");
        std::fs::write(d.join("crashes"), b"a file where the folder should be").unwrap();
        record(&d, "t", "a.rs", 1, "text");
        assert!(reports(&d).is_empty());
        record(&PathBuf::from("/nonexistent/place"), "t", "a.rs", 1, "text");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cr05_a_file_that_is_not_exactly_a_report_is_never_shown() {
        let d = temp("hostile");
        let dir = d.join("crashes");
        std::fs::create_dir_all(&dir).unwrap();
        let good = "kakurega-crash 1\nversion 0.1.0\nat 1700000000\nthread main\nplace crates/a/src/b.rs:3\nmessage text\n";
        std::fs::write(dir.join("crash-000000000001-0001.txt"), good).unwrap();
        let cases: Vec<(&str, String)> = vec![
            ("wrong header", good.replace("kakurega-crash 1", "other")),
            ("extra line", format!("{good}extra secret line\n")),
            (
                "path in place",
                good.replace("crates/a/src/b.rs:3", "/Users/alice/x.rs:3"),
            ),
            (
                "spaces in thread",
                good.replace("thread main", "thread main thread"),
            ),
            ("control in place", good.replace("b.rs", "b\u{1b}[31m.rs")),
            (
                "bidi in thread",
                good.replace("thread main", "thread ma\u{202e}in"),
            ),
            (
                "long place",
                good.replace("crates/a/src/b.rs:3", &"a".repeat(500)),
            ),
            (
                "bad kind",
                good.replace("message text", "message the secret"),
            ),
            ("huge", format!("{good}{}", "x".repeat(5000))),
        ];
        for (i, (name, body)) in cases.iter().enumerate() {
            std::fs::write(
                dir.join(format!("crash-00000000{:04}-9999.txt", i + 2)),
                body,
            )
            .unwrap();
            let _ = name;
        }
        std::os::unix::fs::symlink("/etc/hosts", dir.join("crash-999999999999-0000.txt")).unwrap();
        // a link to a well-formed report kept somewhere else is not followed either
        let elsewhere = d.join("elsewhere.txt");
        std::fs::write(
            &elsewhere,
            good.replace("crates/a/src/b.rs:3", "crates/linked/src/c.rs:9"),
        )
        .unwrap();
        std::os::unix::fs::symlink(&elsewhere, dir.join("crash-999999999998-0000.txt")).unwrap();
        let shown = reports(&d);
        assert_eq!(shown.len(), 1, "{shown:?}");
        assert_eq!(shown[0].place, "crates/a/src/b.rs:3");
        let _ = std::fs::remove_dir_all(&d);
    }
}
