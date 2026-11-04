use crate::import::ImportedFile;
use kk_sandbox::{run_with, Bind, Exit, Limits, SandboxError, SandboxSpec, Transport};
use kk_types::label::Level;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const MAX_TITLE: usize = 200;
pub const MAX_EXPLANATION: usize = 4000;
pub const MAX_REPRODUCER: usize = 16_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub title: String,
    pub file: String,
    pub line: i64,
    pub explanation: String,
    pub reproducer: String,
}

pub fn clean_text(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .filter(
            |c| !matches!(*c as u32, 0x202a..=0x202e | 0x2066..=0x2069 | 0x200b..=0x200f | 0xfeff),
        )
        .take(max)
        .collect()
}

pub fn parse_finding(bytes: &[u8], known_files: &BTreeSet<String>) -> Option<Finding> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let o = v.as_object()?;
    if o.keys()
        .any(|k| !["title", "file", "line", "explanation", "reproducer"].contains(&k.as_str()))
    {
        return None;
    }
    let file = o.get("file")?.as_str()?.to_string();
    if !known_files.contains(&file) {
        return None;
    }
    let line = o.get("line")?.as_i64()?;
    if !(1..=10_000_000).contains(&line) {
        return None;
    }
    Some(Finding {
        title: clean_text(o.get("title")?.as_str()?, MAX_TITLE),
        file,
        line,
        explanation: clean_text(o.get("explanation")?.as_str()?, MAX_EXPLANATION),
        reproducer: o
            .get("reproducer")
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .take(MAX_REPRODUCER)
            .collect(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosureError {
    NeedsRestrictedFile(String),
    EntryMissing,
}

fn module_paths(module: &str) -> Vec<String> {
    let base = module.replace('.', "/");
    vec![format!("{base}.py"), format!("{base}/__init__.py")]
}

fn imports_of(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("import ") {
            for part in rest.split(',') {
                let name = part.split_whitespace().next().unwrap_or("");
                if !name.is_empty() {
                    out.push(name.to_string());
                }
            }
        } else if let Some(rest) = t.strip_prefix("from ") {
            let mut it = rest.split_whitespace();
            let module = it.next().unwrap_or("");
            if it.next() == Some("import") && !module.starts_with('.') {
                out.push(module.to_string());
                for name in it {
                    let n = name.trim_matches(|c| c == ',' || c == '(' || c == ')');
                    if !n.is_empty() && n != "*" && n != "as" {
                        out.push(format!("{module}.{n}"));
                    }
                }
            }
        }
    }
    out
}

pub fn python_closure(
    entry: &str,
    files: &BTreeMap<String, &ImportedFile>,
) -> Result<Vec<String>, ClosureError> {
    if !files.contains_key(entry) {
        return Err(ClosureError::EntryMissing);
    }
    let mut seen = BTreeSet::new();
    let mut stack = vec![entry.to_string()];
    while let Some(path) = stack.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let f = files[&path];
        if f.label.level >= Level::Restricted {
            return Err(ClosureError::NeedsRestrictedFile(path));
        }
        let source = String::from_utf8_lossy(&f.bytes).to_string();
        let mut wanted: Vec<String> = Vec::new();
        for m in imports_of(&source) {
            let parts: Vec<&str> = m.split('.').collect();
            for n in 1..=parts.len() {
                wanted.extend(module_paths(&parts[..n].join(".")));
            }
        }
        for w in wanted {
            if files.contains_key(&w) {
                stack.push(w);
            }
        }
    }
    Ok(seen.into_iter().collect())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStatus {
    Reproduced,
    NotReproduced,
    Nondeterministic,
    Failed(String),
    Unavailable(String),
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub status: CheckStatus,
    pub stdout: String,
    pub sample_paths: Vec<String>,
}

pub struct CheckEnv<'a> {
    pub transport: &'a Transport,
    pub scratch: &'a Path,
    pub python: &'a str,
}

pub fn run_reproducer(
    finding: &Finding,
    files: &BTreeMap<String, &ImportedFile>,
    env: &CheckEnv<'_>,
) -> CheckResult {
    if finding.reproducer.trim().is_empty() {
        return CheckResult {
            status: CheckStatus::Unavailable("the finding has no reproducer".into()),
            stdout: String::new(),
            sample_paths: vec![],
        };
    }
    let sample_paths = match python_closure(&finding.file, files) {
        Ok(p) => p,
        Err(ClosureError::NeedsRestrictedFile(p)) => {
            return CheckResult {
                status: CheckStatus::Unavailable(format!(
                    "the code depends on a withheld file ({p})"
                )),
                stdout: String::new(),
                sample_paths: vec![],
            }
        }
        Err(ClosureError::EntryMissing) => {
            return CheckResult {
                status: CheckStatus::Unavailable("the cited file is not in the project".into()),
                stdout: String::new(),
                sample_paths: vec![],
            }
        }
    };
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir: PathBuf = env
        .scratch
        .join(format!("check-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let sample = dir.join("sample");
    for p in &sample_paths {
        let dest = sample.join(p);
        if let Some(parent) = dest.parent() {
            if fs::create_dir_all(parent).is_err() {
                return CheckResult {
                    status: CheckStatus::Failed("could not prepare the sample".into()),
                    stdout: String::new(),
                    sample_paths,
                };
            }
        }
        if fs::write(&dest, &files[p].bytes).is_err() {
            return CheckResult {
                status: CheckStatus::Failed("could not prepare the sample".into()),
                stdout: String::new(),
                sample_paths,
            };
        }
    }
    let script = dir.join("repro.py");
    if fs::write(&script, finding.reproducer.as_bytes()).is_err() {
        return CheckResult {
            status: CheckStatus::Failed("could not write the reproducer".into()),
            stdout: String::new(),
            sample_paths,
        };
    }
    let mut outputs = Vec::new();
    // Round 2 is the negative control: the cited file is emptied. A script whose verdict does not depend on that
    // code cannot be evidence about it.
    let mut control_stdout = String::new();
    for round in 0..3 {
        if round == 2 {
            let _ = fs::write(sample.join(&finding.file), b"");
        }
        let mut spec = SandboxSpec::new(env.python);
        spec.args = vec!["-B".into(), "/repro/repro.py".into()];
        spec.env = vec![("PYTHONPATH".into(), "/sample".into())];
        spec.binds.push(Bind {
            host: sample.clone(),
            inside: "/sample".into(),
            writable: false,
        });
        spec.binds.push(Bind {
            host: script.clone(),
            inside: "/repro/repro.py".into(),
            writable: false,
        });
        spec.chdir = Some("/sample".into());
        spec.limits = Limits {
            wall: Duration::from_secs(30),
            cpu_secs: 15,
            ..Limits::default()
        };
        match run_with(&spec, env.transport) {
            Ok(o) if round == 2 => {
                if o.exit == Exit::Code(0) {
                    control_stdout = String::from_utf8_lossy(&o.stdout).into_owned();
                }
            }
            Ok(o) => {
                if o.exit != Exit::Code(0) {
                    let _ = fs::remove_dir_all(&dir);
                    return CheckResult {
                        status: CheckStatus::Failed(format!(
                            "the reproducer ended with {:?}: {}",
                            o.exit,
                            String::from_utf8_lossy(&o.stderr)
                                .chars()
                                .take(300)
                                .collect::<String>()
                        )),
                        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
                        sample_paths,
                    };
                }
                outputs.push(String::from_utf8_lossy(&o.stdout).into_owned());
            }
            Err(SandboxError::Unsupported(m)) => {
                let _ = fs::remove_dir_all(&dir);
                return CheckResult {
                    status: CheckStatus::Unavailable(m.to_string()),
                    stdout: String::new(),
                    sample_paths,
                };
            }
            Err(e) => {
                let _ = fs::remove_dir_all(&dir);
                return CheckResult {
                    status: CheckStatus::Failed(e.to_string()),
                    stdout: String::new(),
                    sample_paths,
                };
            }
        }
    }
    let _ = fs::remove_dir_all(&dir);
    if outputs[0] != outputs[1] {
        return CheckResult {
            status: CheckStatus::Nondeterministic,
            stdout: outputs.remove(0),
            sample_paths,
        };
    }
    let stdout = outputs.remove(0);
    let marker = |text: &str, word: &str| text.lines().any(|l| l.trim() == word);
    let status = if !marker(&stdout, "REPRODUCED") {
        CheckStatus::NotReproduced
    } else if !marker(&stdout, "CONTROL_OK") {
        CheckStatus::Failed("the script printed REPRODUCED without a CONTROL_OK line: it must first show, and assert, that the normal behaviour works".into())
    } else if marker(&control_stdout, "REPRODUCED") {
        CheckStatus::Failed("the script still prints REPRODUCED when the cited file is emptied, so its result does not depend on the code it claims to test".into())
    } else {
        CheckStatus::Reproduced
    };
    CheckResult {
        status,
        stdout,
        sample_paths,
    }
}

pub const MAX_FIX_TEXT: usize = 8_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub title: String,
    pub file: String,
    pub old_text: String,
    pub new_text: String,
    pub explanation: String,
}

pub fn parse_fix(bytes: &[u8], known_files: &BTreeSet<String>) -> Option<Fix> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let o = v.as_object()?;
    if o.keys()
        .any(|k| !["title", "file", "old_text", "new_text", "explanation"].contains(&k.as_str()))
    {
        return None;
    }
    let file = o.get("file")?.as_str()?.to_string();
    if !known_files.contains(&file) {
        return None;
    }
    let old_text = o.get("old_text")?.as_str()?.to_string();
    let new_text = o.get("new_text")?.as_str()?.to_string();
    if old_text.is_empty()
        || old_text == new_text
        || old_text.len() > MAX_FIX_TEXT
        || new_text.len() > MAX_FIX_TEXT
    {
        return None;
    }
    if new_text.chars().any(|c| {
        (c.is_control() && !matches!(c, '\n' | '\t'))
            || matches!(c as u32, 0x202a..=0x202e | 0x2066..=0x2069 | 0x200b..=0x200f | 0xfeff)
    }) {
        return None;
    }
    Some(Fix {
        title: clean_text(o.get("title")?.as_str()?, MAX_TITLE),
        file,
        old_text,
        new_text,
        explanation: clean_text(
            o.get("explanation").and_then(Value::as_str).unwrap_or(""),
            MAX_EXPLANATION,
        ),
    })
}

/// Exactly one occurrence of `old_text` is replaced; anything else is not a fix.
pub fn apply_fix(original: &[u8], fix: &Fix) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(original).ok()?;
    if text.matches(fix.old_text.as_str()).count() != 1 {
        return None;
    }
    let out = text.replacen(fix.old_text.as_str(), &fix.new_text, 1);
    (out.len() <= original.len() + MAX_FIX_TEXT).then(|| out.into_bytes())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixStatus {
    /// The reproducer printed REPRODUCED (with CONTROL_OK) before the change and does not after it.
    FixesTheReproducer,
    StillReproduced,
    CouldNotApply(String),
    NotChecked(String),
}

/// Checks a proposed fix against the finding it claims to fix. This establishes only that this reproducer stops
/// reproducing; it does not run the project's tests and does not prove the change is correct or complete.
pub fn verify_fix(
    finding: &Finding,
    before: &CheckResult,
    fix: &Fix,
    files: &BTreeMap<String, &ImportedFile>,
    env: &CheckEnv<'_>,
) -> FixStatus {
    if before.status != CheckStatus::Reproduced {
        return FixStatus::NotChecked("the finding it belongs to was not reproduced".into());
    }
    let Some(orig) = files.get(&fix.file) else {
        return FixStatus::CouldNotApply("the file is not in the project".into());
    };
    let Some(patched_bytes) = apply_fix(&orig.bytes, fix) else {
        return FixStatus::CouldNotApply(
            "the text to replace must appear exactly once in the file".into(),
        );
    };
    let mut patched = (*orig).clone();
    patched.bytes = patched_bytes;
    let mut map: BTreeMap<String, &ImportedFile> = files.clone();
    map.insert(fix.file.clone(), &patched);
    let after = run_reproducer(finding, &map, env);
    let control_after = after.stdout.lines().any(|l| l.trim() == "CONTROL_OK");
    match after.status {
        CheckStatus::NotReproduced if !control_after => FixStatus::NotChecked("after the change the script no longer shows the normal behaviour working (no CONTROL_OK), so this cannot be told apart from removing the feature".into()),
        CheckStatus::NotReproduced => FixStatus::FixesTheReproducer,
        CheckStatus::Reproduced => FixStatus::StillReproduced,
        other => FixStatus::NotChecked(format!(
            "the check after the change could not be trusted: {other:?}"
        )),
    }
}

/// A plain unified diff of the one replacement with three lines of context on each side, for people to read; applying
/// uses `apply_fix`, never this text.
pub fn unified_diff(original: &[u8], fix: &Fix) -> String {
    let text = String::from_utf8_lossy(original);
    let at = text.find(fix.old_text.as_str()).unwrap_or(0);
    let body = text.strip_suffix('\n').unwrap_or(&text);
    let lines: Vec<&str> = body.split('\n').collect();
    let first = text[..at].matches('\n').count();
    let removed = fix.old_text.matches('\n').count() + usize::from(!fix.old_text.ends_with('\n'));
    let last = (first + removed).min(lines.len());
    let before_start = first.saturating_sub(3);
    let after_end = (last + 3).min(lines.len());
    let old_len = (last - first) + (first - before_start) + (after_end - last);
    let new_lines: Vec<&str> = fix
        .new_text
        .trim_end_matches('\n')
        .split('\n')
        .filter(|_| !fix.new_text.is_empty())
        .collect();
    let new_len = new_lines.len() + (first - before_start) + (after_end - last);
    let mut out = format!(
        "--- a/{}\n+++ b/{}\n@@ -{},{} +{},{} @@\n",
        fix.file,
        fix.file,
        before_start + 1,
        old_len,
        before_start + 1,
        new_len
    );
    for l in &lines[before_start..first] {
        out.push_str(&format!(" {l}\n"));
    }
    for l in fix.old_text.trim_end_matches('\n').split('\n') {
        out.push_str(&format!("-{l}\n"));
    }
    for l in &new_lines {
        out.push_str(&format!("+{l}\n"));
    }
    for l in &lines[last..after_end] {
        out.push_str(&format!(" {l}\n"));
    }
    out
}
