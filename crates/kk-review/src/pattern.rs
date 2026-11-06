//! A second opinion from pattern rules, if the owner has installed Semgrep and pointed `KAKU_SEMGREP` at it.
//!
//! This is not verification. A pattern match says "code of this shape is on this line"; it does not say the code can be
//! attacked, and an absence says nothing about logic errors. Results are always labelled as pattern matches, never as
//! reproduced, and they never change whether a finding is confirmed.
//!
//! Semgrep reads the project; it does not run it. Still, it is a large program handed an untrusted project, so it runs the way
//! the other readers do: a copy of the files the model is also allowed to see (nothing labelled restricted), a fresh folder, an
//! empty environment, a hard time limit, a cap on its output, and a result that is checked line by line against what we asked
//! for (only our own rule ids, only files we gave it) before any of it is believed. Rules are ours (`rules/patterns.yml`);
//! nothing is downloaded, metrics and version checks are switched off, and comments in the project that say "skip this line"
//! (`nosem`) are ignored.

use crate::import::ImportedFile;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const RULES: &str = include_str!("../rules/patterns.yml");

pub const MAX_FILES: usize = 5000;
pub const MAX_FILE_BYTES: usize = 1_000_000;
pub const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_MATCHES: usize = 500;
pub const TIME_LIMIT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternMatch {
    pub rule: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternScan {
    /// Nothing was run, and why (not asked for, not installed).
    NotRun(String),
    Ran {
        matches: Vec<PatternMatch>,
        files: usize,
        truncated: bool,
    },
    Failed(String),
}

impl Default for PatternScan {
    fn default() -> Self {
        PatternScan::NotRun("pattern rules were not asked for".into())
    }
}

/// The rule ids in `RULES` with the fixed sentence each one carries. The sentences are ours and are what the owner is shown;
/// text that comes back from the scanner is never shown.
pub fn rules() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut id: Option<String> = None;
    for line in RULES.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("- id:") {
            id = Some(rest.trim().to_string());
        } else if let Some(m) = t.strip_prefix("message:") {
            if let Some(i) = id.take() {
                out.push((i, m.trim().to_string()));
            }
        }
    }
    out
}

pub fn rule_message(id: &str) -> Option<String> {
    rules().into_iter().find(|(i, _)| i == id).map(|(_, m)| m)
}

/// The path in `KAKU_SEMGREP`, if it names a file this user can run. Nothing is looked up on `PATH`: running a scanner is a
/// choice the owner makes by naming it.
pub fn from_setting(value: Option<&str>) -> Result<PathBuf, String> {
    let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) else {
        return Err("pattern rules were not asked for (KAKU_SEMGREP is not set)".into());
    };
    let p = PathBuf::from(v);
    if !p.is_absolute() {
        return Err("KAKU_SEMGREP must be the full path to semgrep".into());
    }
    let meta = std::fs::metadata(&p)
        .map_err(|_| "KAKU_SEMGREP names a file that does not exist".to_string())?;
    if !meta.is_file() {
        return Err("KAKU_SEMGREP does not name a file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return Err("KAKU_SEMGREP names a file that cannot be run".into());
        }
    }
    Ok(p)
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

fn temp_dir(base: &Path) -> Result<TempDir, String> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = base.join(format!(
        "kk-pattern-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir.join("src")).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(TempDir(dir))
}

/// Runs the scanner over `files` and returns what it matched. `files` should be the ones the model may also see. Its working
/// folder is made inside `scratch` and removed afterwards.
pub fn scan(
    semgrep: &Path,
    files: &[&ImportedFile],
    scratch: &Path,
    limit: Duration,
) -> PatternScan {
    let tmp = match temp_dir(scratch) {
        Ok(t) => t,
        Err(e) => return PatternScan::Failed(format!("could not prepare a folder: {e}")),
    };
    let src = tmp.0.join("src");
    let mut given: BTreeSet<String> = BTreeSet::new();
    for f in files.iter().take(MAX_FILES) {
        if !safe_relative(&f.path) || f.bytes.len() > MAX_FILE_BYTES {
            continue;
        }
        let target = src.join(&f.path);
        if let Some(parent) = target.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                continue;
            }
        }
        if std::fs::write(&target, &f.bytes).is_ok() {
            given.insert(f.path.clone());
        }
    }
    // an empty ignore file stops Semgrep skipping test folders by default, and stands in for any the project carried
    let _ = std::fs::write(src.join(".semgrepignore"), b"");
    let rules_path = tmp.0.join("rules.yml");
    if std::fs::write(&rules_path, RULES).is_err() {
        return PatternScan::Failed("could not write the rules".into());
    }
    let mut cmd = Command::new(semgrep);
    cmd.arg("--metrics=off")
        .arg("--disable-version-check")
        .arg("--disable-nosem")
        .arg("--no-git-ignore")
        .arg("--quiet")
        .arg("--json")
        .arg("--timeout")
        .arg("10")
        .arg("--max-target-bytes")
        .arg(MAX_FILE_BYTES.to_string())
        .arg("--config")
        .arg(&rules_path)
        .arg(".")
        .current_dir(&src)
        .env_clear()
        .env("HOME", tmp.0.join("home"))
        .env("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin")
        .env("SEMGREP_SEND_METRICS", "off")
        .env("SEMGREP_ENABLE_VERSION_CHECK", "0")
        .env("SEMGREP_FORCE_COLOR", "0")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return PatternScan::Failed(format!("semgrep could not be started: {e}")),
    };
    let mut stdout = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 65536];
        let mut over = false;
        loop {
            match stdout.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if buf.len() + n > MAX_OUTPUT_BYTES {
                        over = true;
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
            }
        }
        (buf, over)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if started.elapsed() > limit || reader.is_finished() => {
                #[cfg(unix)]
                {
                    // the whole group: Semgrep starts helper processes
                    let _ = Command::new("kill")
                        .args(["-KILL", &format!("-{}", child.id())])
                        .status();
                }
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => break None,
        }
    };
    let (out, over) = reader.join().unwrap_or_default();
    if over {
        return PatternScan::Failed("semgrep wrote more output than is accepted".into());
    }
    let Some(status) = status else {
        return PatternScan::Failed(if started.elapsed() > limit {
            "semgrep did not finish in time".into()
        } else {
            "semgrep stopped unexpectedly".into()
        });
    };
    if !status.success() {
        return PatternScan::Failed(format!(
            "semgrep ended with {}",
            status
                .code()
                .map_or("a signal".to_string(), |c| format!("code {c}"))
        ));
    }
    parse(&out, &given)
}

/// Reads the scanner's JSON, keeping only what can be checked against what was asked: a rule id of ours and a file we gave it.
pub fn parse(out: &[u8], given: &BTreeSet<String>) -> PatternScan {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(out) else {
        return PatternScan::Failed("semgrep's answer could not be read".into());
    };
    let Some(results) = v.get("results").and_then(|r| r.as_array()) else {
        return PatternScan::Failed("semgrep's answer had no results list".into());
    };
    let ours: BTreeSet<String> = rules().into_iter().map(|(i, _)| i).collect();
    let mut matches = Vec::new();
    let mut truncated = false;
    for r in results {
        // the id comes back prefixed with the folder the rules were read from
        let rule = r
            .get("check_id")
            .and_then(|c| c.as_str())
            .and_then(|c| c.rsplit('.').next())
            .filter(|c| ours.contains(*c));
        let file = r
            .get("path")
            .and_then(|p| p.as_str())
            .map(|p| p.strip_prefix("./").unwrap_or(p))
            .filter(|p| given.contains(*p));
        let line = r
            .pointer("/start/line")
            .and_then(|l| l.as_u64())
            .filter(|l| (1..=10_000_000).contains(l));
        let (Some(rule), Some(file), Some(line)) = (rule, file, line) else {
            continue;
        };
        if matches.len() >= MAX_MATCHES {
            truncated = true;
            break;
        }
        let m = PatternMatch {
            rule: rule.to_string(),
            file: file.to_string(),
            line: line as u32,
        };
        if !matches.contains(&m) {
            matches.push(m);
        }
    }
    matches.sort_by(|a, b| (&a.file, a.line, &a.rule).cmp(&(&b.file, b.line, &b.rule)));
    PatternScan::Ran {
        matches,
        files: given.len(),
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn given(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn pt01_the_rules_file_lists_every_rule_with_one_sentence_of_ours() {
        let r = rules();
        assert!(r.len() >= 12);
        let ids: BTreeSet<_> = r.iter().map(|(i, _)| i.as_str()).collect();
        assert_eq!(ids.len(), r.len(), "ids are unique");
        for (id, msg) in &r {
            assert!(id.starts_with("kk-") && !id.contains('.'), "{id}");
            assert!(msg.len() > 10 && msg.len() < 120, "{id}: {msg}");
        }
        assert_eq!(
            rule_message("kk-py-pickle-load").unwrap(),
            "pickle load, which runs code from its input"
        );
        assert_eq!(rule_message("nope"), None);
    }

    #[test]
    fn pt02_only_a_full_path_to_a_runnable_file_is_accepted_and_nothing_is_searched_for() {
        assert!(from_setting(None).is_err());
        assert!(from_setting(Some("  ")).is_err());
        assert!(from_setting(Some("semgrep")).is_err(), "no PATH lookup");
        assert!(from_setting(Some("/no/such/semgrep")).is_err());
        assert!(from_setting(Some("/tmp")).is_err(), "a folder");
        let f = std::env::temp_dir().join(format!("kk-sg-notexec-{}", std::process::id()));
        std::fs::write(&f, "x").unwrap();
        assert!(
            from_setting(Some(f.to_str().unwrap())).is_err(),
            "not executable"
        );
        let _ = std::fs::remove_file(&f);
        assert!(from_setting(Some("/bin/sh")).is_ok());
    }

    #[test]
    fn pt03_the_answer_is_believed_only_where_it_names_our_rules_and_files_we_gave() {
        let g = given(&["a.py", "dir/b.js"]);
        let out = br#"{"results":[
          {"check_id":"tmp.x.rules.kk-py-eval-exec","path":"a.py","start":{"line":3}},
          {"check_id":"kk-js-eval","path":"./dir/b.js","start":{"line":9}},
          {"check_id":"tmp.x.rules.kk-py-eval-exec","path":"a.py","start":{"line":3}},
          {"check_id":"tmp.x.rules.not-ours","path":"a.py","start":{"line":1}},
          {"check_id":"tmp.x.rules.kk-py-eval-exec","path":"/etc/passwd","start":{"line":1}},
          {"check_id":"tmp.x.rules.kk-py-eval-exec","path":"../a.py","start":{"line":1}},
          {"check_id":"tmp.x.rules.kk-py-eval-exec","path":"c.py","start":{"line":1}},
          {"check_id":"tmp.x.rules.kk-py-eval-exec","path":"a.py","start":{"line":0}},
          {"check_id":"tmp.x.rules.kk-py-eval-exec","path":"a.py","start":{"line":-4}},
          {"check_id":"tmp.x.rules.kk-py-eval-exec","path":"a.py","start":{"line":99999999999}},
          {"check_id":7,"path":"a.py","start":{"line":1}},
          {"path":"a.py"},
          "junk", null
        ]}"#;
        let PatternScan::Ran {
            matches,
            files,
            truncated,
        } = parse(out, &g)
        else {
            panic!()
        };
        assert_eq!(files, 2);
        assert!(!truncated);
        assert_eq!(
            matches,
            vec![
                PatternMatch {
                    rule: "kk-py-eval-exec".into(),
                    file: "a.py".into(),
                    line: 3
                },
                PatternMatch {
                    rule: "kk-js-eval".into(),
                    file: "dir/b.js".into(),
                    line: 9
                },
            ]
        );
    }

    #[test]
    fn pt04_unreadable_answers_are_failures_and_a_flood_is_cut_off() {
        let g = given(&["a.py"]);
        for bad in [&b""[..], b"not json", b"[]", b"{}", b"{\"results\": 3}"] {
            assert!(
                matches!(parse(bad, &g), PatternScan::Failed(_)),
                "{:?}",
                String::from_utf8_lossy(bad)
            );
        }
        let one = r#"{"check_id":"kk-py-eval-exec","path":"a.py","start":{"line":LINE}}"#;
        let many: Vec<String> = (1..=MAX_MATCHES + 50)
            .map(|n| one.replace("LINE", &n.to_string()))
            .collect();
        let doc = format!("{{\"results\":[{}]}}", many.join(","));
        let PatternScan::Ran {
            matches, truncated, ..
        } = parse(doc.as_bytes(), &g)
        else {
            panic!()
        };
        assert_eq!(matches.len(), MAX_MATCHES);
        assert!(truncated);
    }

    #[test]
    fn pt05_paths_that_could_leave_the_folder_are_never_written() {
        for bad in ["", "/abs", "../x", "a/../../x", "./"] {
            assert!(!safe_relative(bad), "{bad:?}");
        }
        for good in ["a.py", "dir/sub/b.js", "x y.py"] {
            assert!(safe_relative(good), "{good:?}");
        }
    }
}
