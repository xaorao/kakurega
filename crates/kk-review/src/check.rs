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

/// The language a finding's reproducer is written in. Python is the default so older findings and reports are unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    Python,
    JavaScript,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Language::Python => "python",
            Language::JavaScript => "javascript",
        }
    }

    /// Only the names this program knows. Anything else is refused, never guessed.
    pub fn parse(name: &str) -> Option<Language> {
        match name {
            "python" => Some(Language::Python),
            "javascript" => Some(Language::JavaScript),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub title: String,
    pub file: String,
    pub line: i64,
    pub explanation: String,
    pub reproducer: String,
    pub language: Language,
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
    if o.keys().any(|k| {
        ![
            "title",
            "file",
            "line",
            "explanation",
            "reproducer",
            "language",
        ]
        .contains(&k.as_str())
    }) {
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
    let language = match o.get("language") {
        None => Language::default(),
        Some(v) => Language::parse(v.as_str()?)?,
    };
    Some(Finding {
        language,
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

fn js_specifiers(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes: Vec<char> = source.chars().collect();
    let text: String = bytes.iter().collect();
    for marker in ["require(", "import(", "from ", "import "] {
        let mut rest = text.as_str();
        while let Some(i) = rest.find(marker) {
            rest = &rest[i + marker.len()..];
            let t = rest.trim_start();
            let mut chars = t.chars();
            let q = match chars.next() {
                Some(q @ ('\'' | '"')) => q,
                _ => continue,
            };
            let spec: String = chars.take_while(|c| *c != q && *c != '\n').collect();
            if !spec.is_empty() {
                out.push(spec);
            }
        }
    }
    out
}

fn js_resolve(from: &str, spec: &str, files: &BTreeMap<String, &ImportedFile>) -> Option<String> {
    let dir = Path::new(from).parent().unwrap_or(Path::new(""));
    let mut parts: Vec<String> = dir
        .components()
        .filter_map(|c| c.as_os_str().to_str().map(String::from))
        .collect();
    for seg in spec.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s.to_string()),
        }
    }
    let base = parts.join("/");
    let mut candidates = vec![base.clone()];
    for ext in [".js", ".mjs", ".cjs"] {
        candidates.push(format!("{base}{ext}"));
    }
    for ext in [".js", ".mjs", ".cjs"] {
        candidates.push(format!("{base}/index{ext}"));
    }
    candidates.into_iter().find(|c| files.contains_key(c))
}

/// The cited file plus every file it reaches through literal relative imports. Bare package names are not followed:
/// there is no `node_modules` in the sample, so a package the code needs is named in the error.
pub fn js_closure(
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
        for spec in js_specifiers(&source) {
            if spec.starts_with("./") || spec.starts_with("../") {
                if let Some(target) = js_resolve(&path, &spec, files) {
                    stack.push(target);
                }
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
    pub node: &'a str,
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
    let closure = match finding.language {
        Language::Python => python_closure(&finding.file, files),
        Language::JavaScript => js_closure(&finding.file, files),
    };
    let sample_paths = match closure {
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
    let (script_name, interpreter) = match finding.language {
        Language::Python => ("repro.py", env.python),
        Language::JavaScript => ("repro.mjs", env.node),
    };
    let script = dir.join(script_name);
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
        let mut spec = SandboxSpec::new(interpreter);
        match finding.language {
            Language::Python => {
                spec.args = vec!["-B".into(), "/repro/repro.py".into()];
                spec.env = vec![("PYTHONPATH".into(), "/sample".into())];
            }
            Language::JavaScript => {
                spec.args = vec!["/repro/repro.mjs".into()];
            }
        }
        spec.binds.push(Bind {
            host: sample.clone(),
            inside: "/sample".into(),
            writable: false,
        });
        spec.binds.push(Bind {
            host: script.clone(),
            inside: format!("/repro/{script_name}").into(),
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
    /// The first change. A fix may carry up to three more in other files (`also`); all of them apply or none does.
    pub file: String,
    pub old_text: String,
    pub new_text: String,
    pub also: Vec<Edit>,
    pub explanation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub file: String,
    pub old_text: String,
    pub new_text: String,
}

pub const MAX_ALSO: usize = 3;

impl Fix {
    /// Every change in this fix, the first one first.
    pub fn edits(&self) -> Vec<Edit> {
        let mut v = vec![Edit {
            file: self.file.clone(),
            old_text: self.old_text.clone(),
            new_text: self.new_text.clone(),
        }];
        v.extend(self.also.iter().cloned());
        v
    }
}

fn clean_edit(
    file: &str,
    old_text: &str,
    new_text: &str,
    known: &BTreeSet<String>,
) -> Option<Edit> {
    if !known.contains(file)
        || old_text.is_empty()
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
    Some(Edit {
        file: file.to_string(),
        old_text: old_text.to_string(),
        new_text: new_text.to_string(),
    })
}

pub fn parse_fix(bytes: &[u8], known_files: &BTreeSet<String>) -> Option<Fix> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let o = v.as_object()?;
    if o.keys().any(|k| {
        ![
            "title",
            "file",
            "old_text",
            "new_text",
            "explanation",
            "also",
        ]
        .contains(&k.as_str())
    }) {
        return None;
    }
    let first = clean_edit(
        o.get("file")?.as_str()?,
        o.get("old_text")?.as_str()?,
        o.get("new_text")?.as_str()?,
        known_files,
    )?;
    let mut also = Vec::new();
    if let Some(list) = o.get("also") {
        let list = list.as_array()?;
        if list.len() > MAX_ALSO {
            return None;
        }
        for item in list {
            let m = item.as_object()?;
            if m.keys()
                .any(|k| !["file", "old_text", "new_text"].contains(&k.as_str()))
            {
                return None;
            }
            let e = clean_edit(
                m.get("file")?.as_str()?,
                m.get("old_text")?.as_str()?,
                m.get("new_text")?.as_str()?,
                known_files,
            )?;
            // one change per file keeps every change independent and every diff and undo exact
            if e.file == first.file || also.iter().any(|x: &Edit| x.file == e.file) {
                return None;
            }
            also.push(e);
        }
    }
    Some(Fix {
        title: clean_text(o.get("title")?.as_str()?, MAX_TITLE),
        file: first.file,
        old_text: first.old_text,
        new_text: first.new_text,
        also,
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

/// Applies every change of a fix to the files it names. All of them apply or the result is `None`.
pub fn apply_all(
    files: &BTreeMap<String, &ImportedFile>,
    fix: &Fix,
) -> Option<BTreeMap<String, Vec<u8>>> {
    let mut out = BTreeMap::new();
    for e in fix.edits() {
        let orig = files.get(&e.file)?;
        let single = Fix {
            title: String::new(),
            file: e.file.clone(),
            old_text: e.old_text,
            new_text: e.new_text,
            also: vec![],
            explanation: String::new(),
        };
        out.insert(e.file, apply_fix(&orig.bytes, &single)?);
    }
    Some(out)
}

/// The unified diff of every change in a fix, one file after another.
pub fn unified_diff_all(files: &BTreeMap<String, &ImportedFile>, fix: &Fix) -> String {
    let mut out = String::new();
    for e in fix.edits() {
        let Some(orig) = files.get(&e.file) else {
            continue;
        };
        let single = Fix {
            title: String::new(),
            file: e.file.clone(),
            old_text: e.old_text,
            new_text: e.new_text,
            also: vec![],
            explanation: String::new(),
        };
        out.push_str(&unified_diff(&orig.bytes, &single));
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixStatus {
    /// The reproducer printed REPRODUCED (with CONTROL_OK) before the change and does not after it.
    FixesTheReproducer,
    StillReproduced,
    CouldNotApply(String),
    NotChecked(String),
    /// The reproducer stops reproducing, but the project's own tests passed before the change and fail after it.
    BreaksTests(String),
}

/// What running the project's own `unittest` tests said, in the sandbox, with no network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestRun {
    Passed(u32),
    Failed(String),
    NoTests,
    Unavailable(String),
}

impl TestRun {
    pub fn describe(&self) -> String {
        match self {
            TestRun::Passed(n) => format!("{n} test(s) passed"),
            TestRun::Failed(m) => format!("tests failed: {m}"),
            TestRun::NoTests => "the project has no unittest tests that could run".into(),
            TestRun::Unavailable(m) => format!("the tests could not be run: {m}"),
        }
    }
}

/// Reads the end of a `python -m unittest` run. Anything not clearly a pass is not a pass.
pub fn parse_unittest(exit_ok: bool, stderr: &str) -> TestRun {
    let ran = stderr.lines().rev().find_map(|l| {
        l.strip_prefix("Ran ")
            .and_then(|r| r.split_whitespace().next())
            .and_then(|n| n.parse::<u32>().ok())
    });
    let tail: String = stderr
        .lines()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" ");
    match ran {
        None => TestRun::Failed(format!(
            "no test summary: {}",
            tail.chars().take(160).collect::<String>()
        )),
        Some(0) => TestRun::NoTests,
        Some(n) if exit_ok && stderr.lines().rev().take(3).any(|l| l.trim() == "OK") => {
            TestRun::Passed(n)
        }
        Some(_) => TestRun::Failed(tail.chars().take(160).collect()),
    }
}

fn looks_like_test(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.ends_with(".py") && (name.starts_with("test") || name.ends_with("_test.py"))
}

/// Runs the project's own `unittest` tests over every file the review may see. Only called when a test file exists.
pub fn run_project_tests(files: &BTreeMap<String, &ImportedFile>, env: &CheckEnv<'_>) -> TestRun {
    if !files.keys().any(|p| looks_like_test(p)) {
        return TestRun::NoTests;
    }
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir: PathBuf = env
        .scratch
        .join(format!("tests-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let project = dir.join("project");
    for (p, f) in files {
        let dest = project.join(p);
        if dest.parent().map(fs::create_dir_all).transpose().is_err()
            || fs::write(&dest, &f.bytes).is_err()
        {
            let _ = fs::remove_dir_all(&dir);
            return TestRun::Unavailable("could not prepare the project".into());
        }
    }
    let mut spec = SandboxSpec::new(env.python);
    spec.args = vec![
        "-B".into(),
        "-m".into(),
        "unittest".into(),
        "discover".into(),
        "-s".into(),
        "/project".into(),
        "-t".into(),
        "/project".into(),
    ];
    spec.env = vec![("PYTHONPATH".into(), "/project".into())];
    spec.binds.push(Bind {
        host: project,
        inside: "/project".into(),
        writable: false,
    });
    spec.chdir = Some("/project".into());
    spec.limits = Limits {
        wall: Duration::from_secs(60),
        cpu_secs: 30,
        ..Limits::default()
    };
    let result = run_with(&spec, env.transport);
    let _ = fs::remove_dir_all(&dir);
    match result {
        Ok(o) => parse_unittest(o.exit == Exit::Code(0), &String::from_utf8_lossy(&o.stderr)),
        Err(SandboxError::Unsupported(m)) => TestRun::Unavailable(m.to_string()),
        Err(e) => TestRun::Unavailable(e.to_string()),
    }
}

/// `verify_fix`, then the project's own tests before and after. A fix that makes passing tests fail is not accepted.
pub fn verify_fix_with_tests(
    finding: &Finding,
    before: &CheckResult,
    fix: &Fix,
    files: &BTreeMap<String, &ImportedFile>,
    env: &CheckEnv<'_>,
) -> (FixStatus, String) {
    let status = verify_fix(finding, before, fix, files, env);
    if status != FixStatus::FixesTheReproducer {
        return (status, String::new());
    }
    let before_tests = run_project_tests(files, env);
    let TestRun::Passed(_) = before_tests else {
        return (
            status,
            format!("before the change: {}", before_tests.describe()),
        );
    };
    let Some(new_bytes) = apply_all(files, fix) else {
        return (status, String::new());
    };
    let patched: Vec<ImportedFile> = new_bytes
        .into_iter()
        .map(|(f, bytes)| {
            let mut p = (*files[&f]).clone();
            p.bytes = bytes;
            p
        })
        .collect();
    let mut map = files.clone();
    for p in &patched {
        map.insert(p.path.clone(), p);
    }
    let after_tests = run_project_tests(&map, env);
    match after_tests {
        TestRun::Passed(n) => (
            status,
            format!("the project's {n} test(s) passed before and after this change"),
        ),
        TestRun::Failed(m) => (
            FixStatus::BreaksTests(m.clone()),
            format!("the project's tests passed before this change and fail after it: {m}"),
        ),
        other => (status, format!("after the change: {}", other.describe())),
    }
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
    if fix.edits().iter().any(|e| !files.contains_key(&e.file)) {
        return FixStatus::CouldNotApply("a file the fix names is not in the project".into());
    }
    let Some(new_bytes) = apply_all(files, fix) else {
        return FixStatus::CouldNotApply(
            "the text to replace must appear exactly once in each file".into(),
        );
    };
    let patched: Vec<ImportedFile> = new_bytes
        .into_iter()
        .map(|(f, bytes)| {
            let mut p = (*files[&f]).clone();
            p.bytes = bytes;
            p
        })
        .collect();
    let mut map: BTreeMap<String, &ImportedFile> = files.clone();
    for p in &patched {
        map.insert(p.path.clone(), p);
    }
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
    // A change inside a line is shown as the whole line before and after, the way a code review shows it.
    let at = text.find(fix.old_text.as_str()).unwrap_or(0);
    let end = (at + fix.old_text.len()).min(text.len());
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line_end = if end > 0 && text.as_bytes()[end - 1] == b'\n' {
        end
    } else {
        text[end..].find('\n').map_or(text.len(), |i| end + i + 1)
    };
    let whole = Fix {
        old_text: text[line_start..line_end].to_string(),
        new_text: format!(
            "{}{}{}",
            &text[line_start..at],
            fix.new_text,
            &text[end..line_end]
        ),
        ..fix.clone()
    };
    let fix = &whole;
    let at = line_start;
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
