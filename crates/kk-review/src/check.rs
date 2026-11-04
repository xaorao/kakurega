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
    for _ in 0..2 {
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
    let status = if stdout.lines().any(|l| l.trim() == "REPRODUCED") {
        CheckStatus::Reproduced
    } else {
        CheckStatus::NotReproduced
    };
    CheckResult {
        status,
        stdout,
        sample_paths,
    }
}
