use crate::import::ImportedFile;
use crate::rust::{self, BuildProblem};
use kk_sandbox::{run_with, Bind, Exit, Limits, SandboxError, SandboxSpec, Transport};
use kk_types::label::Level;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The wording the checks use, as names, so that what is told to a model (`feedback.rs`) is chosen by these and not by text that
/// could drift. Messages that end in something variable are prefixes; the rest are the whole message.
pub mod msg {
    pub const NO_REPRODUCER: &str = "the finding has no reproducer";
    pub const SANDBOX_PATH: &str = "the script names a path that exists only inside the sandbox";
    pub const WITHHELD_FILE: &str = "the code depends on a withheld file";
    pub const NOT_IN_PROJECT: &str = "the cited file is not in the project";
    pub const NEEDS_PACKAGE: &str = "the script needs the package '";
    pub const ENDED: &str = "the reproducer ended with ";
    pub const NO_CONTROL: &str = "the script printed REPRODUCED without a CONTROL_OK line: it must first show, and assert, that the normal behaviour works";
    pub const CONTROL_UNTESTED: &str = "the script prints CONTROL_OK without testing anything first: the line is not under a condition and nothing before it can fail";
    pub const CONTROL_INDEPENDENT: &str = "the script still prints CONTROL_OK when the cited file is emptied, so its control does not exercise the code it claims to test";
    pub const INDEPENDENT: &str = "the script still prints REPRODUCED when the cited file is emptied, so its result does not depend on the code it claims to test";
    pub const FINDING_NOT_REPRODUCED: &str = "the finding it belongs to was not reproduced";
    pub const FIX_FILE_MISSING: &str = "a file the fix names is not in the project";
    pub const FIX_TEST_FILE: &str = "a fix may not change a test file";
    pub const FIX_ONCE: &str = "the text to replace must appear exactly once in each file";
    pub const NO_CONTROL_AFTER: &str =
        "after the change the script no longer shows the normal behaviour working";
    pub const NO_VERDICT_AFTER: &str =
        "after the change the script ended without printing its NOT REPRODUCED verdict";
    pub const UNTRUSTED_AFTER: &str = "the check after the change could not be trusted";
    pub const DOES_NOT_COMPILE: &str = "the script did not compile";
}

/// The line of a script's error output that says what went wrong: the last one with text in it (a Python traceback ends
/// with the exception, most tools end with their message), at most 300 characters. The lines before it are where it
/// happened, inside the sandbox, which tells a reader little.
pub fn error_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("it printed no error")
        .chars()
        .take(300)
        .collect()
}

pub const MAX_TITLE: usize = 200;
pub const MAX_EXPLANATION: usize = 4000;
pub const MAX_REPRODUCER: usize = 16_000;

/// The language a finding's reproducer is written in. Python is the default so older findings and reports are unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    #[default]
    Python,
    JavaScript,
    /// Run by Node's own type stripping: plain type annotations only, no build step.
    TypeScript,
    /// Built with `rustc` against the project's library crate, which is built the same way: no cargo, no packages.
    Rust,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Language::Python => "python",
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
            Language::Rust => "rust",
        }
    }

    /// Only the names this program knows. Anything else is refused, never guessed.
    pub fn parse(name: &str) -> Option<Language> {
        match name {
            "python" => Some(Language::Python),
            "javascript" => Some(Language::JavaScript),
            "typescript" => Some(Language::TypeScript),
            "rust" => Some(Language::Rust),
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

/// A script that arrives wrapped in one markdown code fence (a line of three backticks with at most a short language word, the code,
/// and a closing line of three backticks) is the script without the fence: a small model very often wraps its code like this, and
/// the fence is a syntax error in every language the checks run. Only exactly one fenced block is unwrapped. Text around it, a
/// second block, a longer fence, a fence line inside the code or a fence with anything but a short word after it is left exactly
/// as it came, so nothing is guessed.
pub fn unfence(text: &str) -> String {
    let lines: Vec<&str> = text.trim().lines().collect();
    if lines.len() < 2 {
        return text.to_string();
    }
    let (first, last) = (lines[0].trim(), lines[lines.len() - 1].trim());
    let word = first.strip_prefix("```").unwrap_or("x`");
    let word_ok = word.len() <= 12
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'#');
    if !word_ok || last != "```" {
        return text.to_string();
    }
    let body = &lines[1..lines.len() - 1];
    if body.iter().any(|l| l.trim_start().starts_with("```")) {
        return text.to_string();
    }
    let mut out = body.join("\n");
    out.push('\n');
    out
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
        reproducer: unfence(
            &o.get("reproducer")
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .take(MAX_REPRODUCER)
                .collect::<String>(),
        ),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosureError {
    NeedsRestrictedFile(String),
    EntryMissing,
    /// The check cannot be done here, for a reason that has one fixed sentence.
    Unavailable(&'static str),
}

/// What a withheld Python module is replaced by in a check's sample: a module with nothing in it. Importing a name from it
/// succeeds, so the files around it load; using that name in any way (calling it, testing it, comparing it, reading from
/// it) raises, so a check whose code path needs the withheld file fails instead of running on a guess. Its text is fixed
/// and the same everywhere: nothing of the withheld file is in it.
pub const STAND_IN: &str = "# Kakurega: this file is withheld (it holds a key or is marked private). An empty stand-in takes its place so that\n# the files importing it can load; nothing of the real file is here. Any use of a name from it stops the check.\nclass _Withheld:\n    def __init__(self, name):\n        object.__setattr__(self, '_name', name)\n    def _refuse(self, *args, **kwargs):\n        import os\n        for fd in (1, 2):\n            os.write(fd, b'\\nKAKUREGA_WITHHELD_USED\\n')\n        raise RuntimeError('this needs ' + object.__getattribute__(self, '_name') + ', which is in a withheld file')\n    __call__ = __getattr__ = __setattr__ = __delattr__ = __bool__ = __eq__ = __ne__ = __lt__ = __le__ = __gt__ = __ge__ = __iter__ = __next__ = __len__ = __getitem__ = __setitem__ = __delitem__ = __contains__ = __reversed__ = __str__ = __bytes__ = __format__ = __int__ = __float__ = __complex__ = __index__ = __round__ = __trunc__ = __floor__ = __ceil__ = __neg__ = __pos__ = __abs__ = __invert__ = __enter__ = __exit__ = __await__ = __aiter__ = __anext__ = __fspath__ = __hash__ = __copy__ = __deepcopy__ = __reduce__ = __reduce_ex__ = __add__ = __radd__ = __iadd__ = __sub__ = __rsub__ = __isub__ = __mul__ = __rmul__ = __imul__ = __matmul__ = __rmatmul__ = __imatmul__ = __truediv__ = __rtruediv__ = __itruediv__ = __floordiv__ = __rfloordiv__ = __ifloordiv__ = __mod__ = __rmod__ = __imod__ = __divmod__ = __rdivmod__ = __idivmod__ = __pow__ = __rpow__ = __ipow__ = __lshift__ = __rlshift__ = __ilshift__ = __rshift__ = __rrshift__ = __irshift__ = __and__ = __rand__ = __iand__ = __xor__ = __rxor__ = __ixor__ = __or__ = __ror__ = __ior__ = _refuse\ndef __getattr__(name):\n    return _Withheld(name)\n";

/// The line a stand-in writes when the code under check uses it.
const WITHHELD_USED: &str = "KAKUREGA_WITHHELD_USED";

/// Whether a script catches errors: Python's `except`, or JavaScript's `catch`.
fn catches_errors(script: &str) -> bool {
    script.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("except")
            || t.contains("} catch")
            || t.starts_with("catch")
            || t.contains(".catch(")
            || t.contains("suppress(")
    })
}

/// The second stand-in: every name in the withheld module is None.
const STAND_IN_NONE: &str = "# Kakurega: this file is withheld. A second empty stand-in, in which every name is None, checks that the result\n# does not depend on what the real file holds.\ndef __getattr__(name):\n    return None\n";

/// The bytes a sample holds for a project file: the file itself, or the stand-in for a withheld one.
pub fn sample_bytes(f: &ImportedFile) -> &[u8] {
    if f.label.level >= Level::Restricted {
        STAND_IN.as_bytes()
    } else {
        &f.bytes
    }
}

/// Where a project keeps its importable code: at its top, or in a `src` folder (the src layout).
const PYTHON_ROOTS: [&str; 2] = ["", "src/"];

fn module_paths(module: &str) -> Vec<String> {
    let base = module.replace('.', "/");
    PYTHON_ROOTS
        .iter()
        .flat_map(|r| [format!("{r}{base}.py"), format!("{r}{base}/__init__.py")])
        .collect()
}

/// The modules a project file imports, with its relative imports (`from .x import y`, `from .. import z`) resolved
/// against the package the file is in, so the sample holds the sibling modules a package needs to load.
fn imports_in(source: &str, path: &str) -> Vec<String> {
    let mut out = imports_of(source);
    let inside = PYTHON_ROOTS
        .iter()
        .filter(|r| !r.is_empty())
        .find_map(|r| path.strip_prefix(r))
        .unwrap_or(path);
    // the package a file is in: its folder, as a dotted name
    let package: Vec<&str> = inside
        .rsplit_once('/')
        .map_or(Vec::new(), |(d, _)| d.split('/').collect());
    for line in source.lines() {
        let Some(rest) = line.trim().strip_prefix("from ") else {
            continue;
        };
        let mut it = rest.split_whitespace();
        let module = it.next().unwrap_or("");
        if !module.starts_with('.') || it.next() != Some("import") {
            continue;
        }
        let dots = module.chars().take_while(|c| *c == '.').count();
        if dots > package.len() + 1 || dots == 0 {
            continue;
        }
        let mut base: Vec<String> = package[..package.len() + 1 - dots]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let tail = &module[dots..];
        if !tail.is_empty() {
            base.extend(tail.split('.').map(str::to_string));
        }
        if !base.is_empty() {
            out.push(base.join("."));
        }
        // `from . import x` names a module x of that package
        for name in it {
            let n = name.trim_matches(|c| c == ',' || c == '(' || c == ')');
            if !n.is_empty() && n != "*" && n != "as" {
                let mut m = base.clone();
                m.push(n.to_string());
                out.push(m.join("."));
            }
        }
    }
    out
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
            // The file the finding cites, or one the script names, cannot be replaced: the check would not be about it.
            // A withheld module that another file merely imports (a package's __init__ that imports every backend) is
            // stood in for by an empty module (see STAND_IN): if the code that runs needs it, the check fails; nothing
            // of the withheld file is copied, read or released.
            if path == entry {
                return Err(ClosureError::NeedsRestrictedFile(path));
            }
            continue;
        }
        let source = String::from_utf8_lossy(&f.bytes).to_string();
        let mut wanted: Vec<String> = Vec::new();
        for m in imports_in(&source, &path) {
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

const JS_EXTENSIONS: [&str; 3] = [".js", ".mjs", ".cjs"];
const TS_EXTENSIONS: [&str; 6] = [".ts", ".mts", ".cts", ".js", ".mjs", ".cjs"];

/// The file a relative import names, among the project's files. For TypeScript the build tool's convention is followed as well:
/// `./x.js` stands for `./x.ts` (and `.mjs` for `.mts`, `.cjs` for `.cts`), so the file is part of the sample even though only
/// code that names its own extension runs under Node's type stripping.
fn script_resolve(
    from: &str,
    spec: &str,
    files: &BTreeMap<String, &ImportedFile>,
    exts: &[&str],
) -> Option<String> {
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
    for ext in exts {
        candidates.push(format!("{base}{ext}"));
    }
    if exts.contains(&".ts") {
        for (written, real) in [(".js", ".ts"), (".mjs", ".mts"), (".cjs", ".cts")] {
            if let Some(stem) = base.strip_suffix(written) {
                candidates.push(format!("{stem}{real}"));
            }
        }
    }
    for ext in exts {
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
    script_closure(entry, files, &JS_EXTENSIONS)
}

/// The same for TypeScript: the cited file plus every file it reaches through literal relative imports, `.ts` family first.
pub fn ts_closure(
    entry: &str,
    files: &BTreeMap<String, &ImportedFile>,
) -> Result<Vec<String>, ClosureError> {
    script_closure(entry, files, &TS_EXTENSIONS)
}

/// How the files a cited file reaches are found, for one language.
type Closure = fn(&str, &BTreeMap<String, &ImportedFile>) -> Result<Vec<String>, ClosureError>;

/// The files put in the sandbox for a finding's check: the cited file's closure, joined with the closure of every project file
/// the reproducer itself imports by a literal name. A check that goes through another part of the project (the handler in
/// front of the cited code, say) then finds that part. A withheld file is refused whichever way it is reached, and a name
/// that is not a project file adds nothing. Rust builds the whole crate already.
pub fn reproducer_sample(
    finding: &Finding,
    files: &BTreeMap<String, &ImportedFile>,
) -> Result<Vec<String>, ClosureError> {
    let (closure, named): (Closure, Vec<String>) = match finding.language {
        Language::Python => (python_closure, {
            let mut named = Vec::new();
            for m in imports_of(&finding.reproducer) {
                let parts: Vec<&str> = m.split('.').collect();
                for n in 1..=parts.len() {
                    named.extend(
                        module_paths(&parts[..n].join("."))
                            .into_iter()
                            .filter(|p| files.contains_key(p)),
                    );
                }
            }
            named
        }),
        Language::JavaScript | Language::TypeScript => {
            let exts: &[&str] = if finding.language == Language::JavaScript {
                &JS_EXTENSIONS
            } else {
                &TS_EXTENSIONS
            };
            // the reproducer runs from the project's top folder, so its relative names start there
            let named = js_specifiers(&finding.reproducer)
                .iter()
                .filter(|s| s.starts_with('.'))
                .filter_map(|s| script_resolve("repro", s, files, exts))
                .collect();
            let closure: Closure = if finding.language == Language::JavaScript {
                js_closure
            } else {
                ts_closure
            };
            (closure, named)
        }
        Language::Rust => return rust_closure(&finding.file, files),
    };
    let mut all: BTreeSet<String> = closure(&finding.file, files)?.into_iter().collect();
    for path in named {
        if !all.contains(&path) {
            all.extend(closure(&path, files)?);
        }
    }
    Ok(all.into_iter().collect())
}

/// The files the compiler needs to build the crate a Rust finding is in: its whole module tree.
pub fn rust_closure(
    entry: &str,
    files: &BTreeMap<String, &ImportedFile>,
) -> Result<Vec<String>, ClosureError> {
    match rust::rust_crate(entry, files) {
        Ok(c) => Ok(c.sample),
        Err(rust::Problem::Restricted(p)) => Err(ClosureError::NeedsRestrictedFile(p)),
        Err(rust::Problem::EntryMissing) => Err(ClosureError::EntryMissing),
        Err(rust::Problem::Unavailable(why)) => Err(ClosureError::Unavailable(why.sentence())),
    }
}

/// Builds the project's library crate (in the edition its manifest names), then the reproducer against it as `project` (always in
/// the 2021 edition: the script is not the project's code, and `use project::...` needs 2018 or later), then runs the reproducer, all in
/// one sandbox:
/// the only place a sandbox can write is its own `/tmp`, so a program built in one run could not be found by the next. The four
/// words after the script are the compiler, the edition, the crate root and the crate's name; they are passed as arguments, never
/// put into the script, and each was checked before. `gcc` is named where a program is linked (not for a library, which is never
/// linked) because `cc` is a link into `/etc`, which the sandbox lacks.
/// A failed build says so on the first line of its error stream; the program's own output is never read as that.
const RUST_BUILD_AND_RUN: &str = r#"set -u
rustc="$1"; edition="$2"; root="$3"; name="$4"
"$rustc" --edition "$edition" --crate-type lib --crate-name "$name" --cap-lints allow -C opt-level=0 -C debuginfo=0 -o "/tmp/lib$name.rlib" "/sample/$root" 2>/tmp/e1 || { echo "KK_BUILD_FAILED crate" >&2; head -c 4000 /tmp/e1 >&2; exit 97; }
"$rustc" -C linker=/usr/bin/gcc --edition 2021 --cap-lints allow -C opt-level=0 -C debuginfo=0 --extern "project=/tmp/lib$name.rlib" -o /tmp/repro /repro/repro.rs 2>/tmp/e2 || { echo "KK_BUILD_FAILED script" >&2; head -c 4000 /tmp/e2 >&2; exit 97; }
exec /tmp/repro
"#;

/// The same builds for the crate's own tests: the crate with its test harness, then the harness with one thread.
pub(crate) const RUST_BUILD_AND_TEST: &str = r#"set -u
rustc="$1"; edition="$2"; root="$3"; name="$4"
"$rustc" -C linker=/usr/bin/gcc --edition "$edition" --test --crate-name "$name" --cap-lints allow -C opt-level=0 -C debuginfo=0 -o /tmp/tests "/sample/$root" 2>/tmp/e1 || { echo "KK_BUILD_FAILED crate" >&2; head -c 4000 /tmp/e1 >&2; exit 97; }
exec /tmp/tests --test-threads=1
"#;

/// Asks the sandbox, once, whether it can build and run a Rust program at all: a program is built and run, and the answer is the
/// result, not a version number. The error says which part is missing (the compiler, the linker, the edition).
fn rust_unavailable(rustc: &str, transport: &Transport) -> Option<String> {
    const PROBE: &str = r#"set -u
echo 'fn main() { println!("ok"); }' > /tmp/probe.rs
"$1" -C linker=/usr/bin/gcc --edition 2021 -o /tmp/probe /tmp/probe.rs 2>/tmp/e || { echo "KK_BUILD_FAILED script" >&2; head -c 2000 /tmp/e >&2; exit 97; }
exec /tmp/probe
"#;
    let mut spec = SandboxSpec::new("/usr/bin/sh");
    spec.args = vec!["-c".into(), PROBE.into(), "sh".into(), rustc.into()];
    spec.limits = Limits {
        wall: Duration::from_secs(30),
        cpu_secs: 20,
        ..Limits::default()
    };
    match run_with(&spec, transport) {
        Ok(o) if o.exit == Exit::Code(0) && String::from_utf8_lossy(&o.stdout) == "ok\n" => None,
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            if let Some((stage, rest)) = rust::build_failure(&stderr) {
                if let Some(BuildProblem::Environment(why)) =
                    rust::classify_build_failure(stage, rest)
                {
                    if why != rust::CRATE_SENTENCE {
                        return Some(why.into());
                    }
                }
            }
            Some("the sandbox cannot build a Rust program (the Rust compiler and gcc are needed under /usr/bin), so this cannot be checked here".into())
        }
        Err(SandboxError::Unsupported(m)) => Some(m.to_string()),
        Err(e) => Some(e.to_string()),
    }
}

fn script_closure(
    entry: &str,
    files: &BTreeMap<String, &ImportedFile>,
    exts: &[&str],
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
                if let Some(target) = script_resolve(&path, &spec, files, exts) {
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
    /// The sandbox could not start the check: nothing is known about the finding, and the setup needs fixing.
    NotRun(String),
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
    /// The Rust compiler as seen inside the sandbox.
    pub rustc: &'a str,
}

/// The major version in the answer of `node --version` (`v22.22.1`), or nothing if the answer is anything else. The answer is
/// read strictly: it is shown to the owner and compared against the version the JavaScript checks need.
pub fn node_major(text: &str) -> Option<u32> {
    let body = text.strip_suffix('\n').unwrap_or(text).strip_prefix('v')?;
    let mut parts = body.split('.');
    let mut next = || {
        let p = parts.next()?;
        (!p.is_empty() && p.len() <= 4 && p.bytes().all(|b| b.is_ascii_digit())).then_some(p)
    };
    let (major, minor, patch) = (next()?, next()?, next()?);
    let _ = (minor, patch);
    if parts.next().is_some() {
        return None;
    }
    major.parse().ok()
}

/// Whether a script names a path that exists only inside our sandbox (`/sample` is the project, `/repro` the script). A check
/// that does can only be re-run here, so it is not accepted: the script must use the current folder instead. A path counts
/// only when it starts at a root (not `src/sample`, `https://x/sample`, `../sample`, `~/sample`) and the name ends there
/// (not `/samples`, `/sample_data`).
pub fn names_sandbox_path(text: &str) -> bool {
    let bytes = text.as_bytes();
    for name in ["/sample", "/repro"] {
        let mut from = 0;
        while let Some(i) = text[from..].find(name) {
            let at = from + i;
            let before = if at == 0 { None } else { Some(bytes[at - 1]) };
            let after = bytes.get(at + name.len()).copied();
            let root_start = before.is_none_or(|b| {
                !(b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'/' | b'~' | b':'))
            });
            let name_ends =
                after.is_none_or(|b| !(b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')));
            if root_start && name_ends {
                return true;
            }
            from = at + name.len();
        }
    }
    false
}

pub fn run_reproducer(
    finding: &Finding,
    files: &BTreeMap<String, &ImportedFile>,
    env: &CheckEnv<'_>,
) -> CheckResult {
    if finding.reproducer.trim().is_empty() {
        return CheckResult {
            status: CheckStatus::Unavailable(msg::NO_REPRODUCER.into()),
            stdout: String::new(),
            sample_paths: vec![],
        };
    }
    if names_sandbox_path(&finding.reproducer) {
        return CheckResult {
            status: CheckStatus::Unavailable(
                format!("{} (/sample or /repro), so no one else could re-run it; it must find the project through the current folder", msg::SANDBOX_PATH),
            ),
            stdout: String::new(),
            sample_paths: vec![],
        };
    }
    let sample_paths = match reproducer_sample(finding, files) {
        Ok(p) => p,
        Err(ClosureError::NeedsRestrictedFile(p)) => {
            return CheckResult {
                status: CheckStatus::Unavailable(format!("{} ({p})", msg::WITHHELD_FILE)),
                stdout: String::new(),
                sample_paths: vec![],
            }
        }
        Err(ClosureError::EntryMissing) => {
            return CheckResult {
                status: CheckStatus::Unavailable(msg::NOT_IN_PROJECT.into()),
                stdout: String::new(),
                sample_paths: vec![],
            }
        }
        Err(ClosureError::Unavailable(why)) => {
            return CheckResult {
                status: CheckStatus::Unavailable(why.into()),
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
        if fs::write(&dest, sample_bytes(files[p])).is_err() {
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
        Language::TypeScript => ("repro.mts", env.node),
        Language::Rust => ("repro.rs", env.rustc),
    };
    let rust_plan = if finding.language == Language::Rust {
        rust::rust_crate(&finding.file, files).ok()
    } else {
        None
    };
    let script = dir.join(script_name);
    if fs::write(&script, finding.reproducer.as_bytes()).is_err() {
        return CheckResult {
            status: CheckStatus::Failed("could not write the reproducer".into()),
            stdout: String::new(),
            sample_paths,
        };
    }
    if finding.language == Language::TypeScript {
        if let Some(why) = typescript_unavailable(interpreter, env.transport) {
            let _ = fs::remove_dir_all(&dir);
            return CheckResult {
                status: CheckStatus::Unavailable(why),
                stdout: String::new(),
                sample_paths,
            };
        }
    }
    if finding.language == Language::Rust {
        if let Some(why) = rust_unavailable(interpreter, env.transport) {
            let _ = fs::remove_dir_all(&dir);
            return CheckResult {
                status: CheckStatus::Unavailable(why),
                stdout: String::new(),
                sample_paths,
            };
        }
    }
    let mut outputs = Vec::new();
    // Round 2 is the negative control: the cited file is emptied. A script whose verdict does not depend on that
    // code cannot be evidence about it.
    let mut control_stdout = String::new();
    // A withheld module stood in for gets one more run, with a second stand-in whose every name is None (so `is None`
    // tests turn the other way and every use breaks differently). A result that depends on what the withheld file holds
    // changes between the two; only one that comes out the same can be confirmed.
    let stand_ins: Vec<&String> = sample_paths
        .iter()
        .filter(|p| files[p.as_str()].label.level >= Level::Restricted)
        .collect();
    // a script that catches errors could turn a stand-in's refusal into its verdict: with a stand-in in the sample, such a
    // script is not run at all
    if !stand_ins.is_empty() && catches_errors(&finding.reproducer) {
        let _ = fs::remove_dir_all(&dir);
        return CheckResult {
            status: CheckStatus::Unavailable(format!(
                "{} (the script catches errors, and a withheld file had to be stood in for, so its verdict could come from the stand-in)",
                msg::WITHHELD_FILE
            )),
            stdout: String::new(),
            sample_paths,
        };
    }
    let rounds = if stand_ins.is_empty() { 3 } else { 4 };
    let mut variant: Option<(Exit, String)> = None;
    for round in 0..rounds {
        if round == 2 {
            let _ = fs::write(sample.join(&finding.file), b"");
        }
        if round == 3 {
            let _ = fs::write(
                sample.join(&finding.file),
                sample_bytes(files[finding.file.as_str()]),
            );
            for p in &stand_ins {
                let _ = fs::write(sample.join(p.as_str()), STAND_IN_NONE);
            }
        }
        let mut spec = SandboxSpec::new(if finding.language == Language::Rust {
            "/usr/bin/sh"
        } else {
            interpreter
        });
        match finding.language {
            Language::Rust => {
                let Some(plan) = rust_plan.as_ref() else {
                    let _ = fs::remove_dir_all(&dir);
                    return CheckResult {
                        status: CheckStatus::Failed("could not prepare the crate".into()),
                        stdout: String::new(),
                        sample_paths,
                    };
                };
                spec.args = vec![
                    "-c".into(),
                    RUST_BUILD_AND_RUN.into(),
                    "sh".into(),
                    interpreter.into(),
                    plan.edition.clone().into(),
                    plan.root.clone().into(),
                    plan.crate_name.clone().into(),
                ];
            }
            Language::Python => {
                spec.args = vec!["-B".into(), "/repro/repro.py".into()];
                // a project in the src layout imports its package from src/
                spec.env = vec![("PYTHONPATH".into(), "/sample:/sample/src".into())];
            }
            Language::JavaScript => {
                spec.args = vec!["/repro/repro.mjs".into()];
            }
            Language::TypeScript => {
                spec.args = vec!["/repro/repro.mts".into()];
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
        spec.limits = if finding.language == Language::Rust {
            // building the crate and the script takes part of the same budget as running the script
            Limits {
                wall: Duration::from_secs(60),
                cpu_secs: 45,
                ..Limits::default()
            }
        } else {
            Limits {
                wall: Duration::from_secs(30),
                cpu_secs: 15,
                ..Limits::default()
            }
        };
        let run = run_with(&spec, env.transport);
        // A run that touched a withheld file's stand-in ran on a guess, whatever it printed after (a script can catch the
        // error). The stand-in says so on both output channels before it raises, and such a check is never confirmed.
        if let Ok(o) = &run {
            if [&o.stdout, &o.stderr]
                .iter()
                .any(|b| String::from_utf8_lossy(b).contains(WITHHELD_USED))
            {
                let _ = fs::remove_dir_all(&dir);
                return CheckResult {
                    status: CheckStatus::Unavailable(format!(
                        "{} (the script reached it while running, through the stand-in that took its place)",
                        msg::WITHHELD_FILE
                    )),
                    stdout: String::new(),
                    sample_paths,
                };
            }
        }
        match run {
            Ok(o) if round == 3 => {
                variant = Some((
                    o.exit.clone(),
                    String::from_utf8_lossy(&o.stdout).into_owned(),
                ));
            }
            Ok(o) if round == 2 => {
                if o.exit == Exit::Code(0) {
                    control_stdout = String::from_utf8_lossy(&o.stdout).into_owned();
                }
            }
            Ok(o) => {
                if o.exit != Exit::Code(0) {
                    let _ = fs::remove_dir_all(&dir);
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    if finding.language == Language::Rust {
                        if let Some((stage, rest)) = rust::build_failure(&stderr) {
                            let status = match rust::classify_build_failure(stage, rest) {
                                Some(BuildProblem::Environment(why)) => {
                                    CheckStatus::Unavailable(why.into())
                                }
                                Some(BuildProblem::Package(pkg)) => CheckStatus::Unavailable(format!(
                                    "{}{pkg}', which is not in the sandbox (it has no installed packages and no network), so this cannot be checked here",
                                    msg::NEEDS_PACKAGE
                                )),
                                None => CheckStatus::Failed(format!(
                                    "{}: {}",
                                    msg::DOES_NOT_COMPILE,
                                    rest.chars().take(300).collect::<String>()
                                )),
                            };
                            return CheckResult {
                                status,
                                stdout: String::new(),
                                sample_paths,
                            };
                        }
                    }
                    if finding.language == Language::TypeScript {
                        if let Some(why) = typescript_environment_problem(&stderr) {
                            return CheckResult {
                                status: CheckStatus::Unavailable(why.into()),
                                stdout: String::new(),
                                sample_paths,
                            };
                        }
                    }
                    if let Some(pkg) = missing_package(finding.language, &stderr) {
                        let in_project = files.keys().any(|p| {
                            PYTHON_ROOTS.iter().any(|r| {
                                p == &format!("{r}{pkg}.py") || p.starts_with(&format!("{r}{pkg}/"))
                            })
                        });
                        if !in_project {
                            return CheckResult {
                                status: CheckStatus::Unavailable(format!(
                                    "{}{pkg}', which is not in the sandbox (it has no installed packages and no network), so this cannot be checked here",
                                    msg::NEEDS_PACKAGE
                                )),
                                stdout: String::new(),
                                sample_paths,
                            };
                        }
                    }
                    return CheckResult {
                        status: CheckStatus::Failed(format!(
                            "{}{}: {}",
                            msg::ENDED,
                            o.exit,
                            error_line(&o.stderr)
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
            Err(SandboxError::NotStarted(m)) => {
                let _ = fs::remove_dir_all(&dir);
                return CheckResult {
                    status: CheckStatus::NotRun(m),
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
        CheckStatus::Failed(msg::NO_CONTROL.into())
    } else if control_untested(&finding.reproducer) {
        CheckStatus::Failed(msg::CONTROL_UNTESTED.into())
    } else if marker(&control_stdout, "REPRODUCED") {
        CheckStatus::Failed(msg::INDEPENDENT.into())
    } else if marker(&control_stdout, "CONTROL_OK") {
        CheckStatus::Failed(msg::CONTROL_INDEPENDENT.into())
    } else if !stand_ins.is_empty()
        && variant
            .as_ref()
            .is_none_or(|(exit, out)| *exit != Exit::Code(0) || *out != stdout)
    {
        CheckStatus::Unavailable(format!(
            "{} (the result changes with what the withheld file holds, tried with two different stand-ins)",
            msg::WITHHELD_FILE
        ))
    } else {
        CheckStatus::Reproduced
    };
    CheckResult {
        status,
        stdout,
        sample_paths,
    }
}

/// Whether the script says CONTROL_OK without having tested anything: the line that prints it stands at the start of a
/// line (so it is under no `if`), carries no condition of its own, and nothing before it can stop the script (an assert,
/// a raise or throw, an exit). Such a line is printed whatever the project does, so it shows nothing about normal
/// behaviour. A script in a language whose statements are always inside a function (Rust) is judged by its runs alone.
fn control_untested(script: &str) -> bool {
    let stops = |l: &str| {
        let t = l.trim_start();
        t.starts_with("assert")
            || t.contains("assert(")
            || t.contains("assert.")
            || t.contains("raise ")
            || t.contains("throw ")
            || t.contains("exit(")
    };
    let mut can_fail_before = false;
    let mut printed = false;
    for line in script.lines() {
        if line.contains("CONTROL_OK") {
            printed = true;
            let at_start = !line.starts_with([' ', '\t']);
            let conditional = line.contains(" if ")
                || line.trim_start().starts_with("if ")
                || line.trim_start().starts_with("if(")
                || line.contains('?')
                || line.contains("&&")
                || line.contains(" and ");
            if !at_start || conditional || can_fail_before {
                return false;
            }
        }
        if stops(line) {
            can_fail_before = true;
        }
    }
    printed
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

/// Changes in one fix, however they are spread over files. Several may be in the same file.
pub const MAX_EDITS: usize = 8;
/// Different files one fix may change.
pub const MAX_FILES: usize = 4;
/// The text from the first to the last change in one file, which several changes there are joined into.
pub const MAX_SPAN: usize = 200_000;
/// How much longer than the original one file may become.
const MAX_GROWTH: usize = MAX_FIX_TEXT * MAX_EDITS;

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
        if list.len() + 1 > MAX_EDITS {
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
            also.push(e);
        }
    }
    let mut files: Vec<&str> = std::iter::once(first.file.as_str())
        .chain(also.iter().map(|e| e.file.as_str()))
        .collect();
    files.sort_unstable();
    files.dedup();
    if files.len() > MAX_FILES {
        return None;
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
    let at = match occurrences(text, fix.old_text.as_str()).as_slice() {
        [one] => *one,
        _ => return None,
    };
    let out = format!(
        "{}{}{}",
        &text[..at],
        fix.new_text,
        &text[at + fix.old_text.len()..]
    );
    (out.len() <= original.len() + MAX_GROWTH).then(|| out.into_bytes())
}

/// Where `pat` starts in `text`, every place, including places that overlap each other: "aa" is in "aaa" twice, so a change
/// to "aa" there is ambiguous.
pub fn occurrences(text: &str, pat: &str) -> Vec<usize> {
    let mut at = Vec::new();
    if pat.is_empty() {
        return at;
    }
    let mut from = 0;
    while let Some(i) = text[from..].find(pat) {
        let start = from + i;
        at.push(start);
        if at.len() > 2 {
            break;
        }
        from = start + text[start..].chars().next().map_or(1, char::len_utf8);
    }
    at
}

/// Joins several changes in one file into one change of the span they cover, so that everything after the model's answer can
/// keep working with one exact change per file (checksums, applying, undoing, the saved copy). Each change must be in the
/// file exactly once as it is now and no two may share any text; the order they were written in does not matter. The result
/// is the same text a person would get by making the changes one by one.
pub fn consolidate(files: &BTreeMap<String, &ImportedFile>, fix: Fix) -> Result<Fix, String> {
    let edits = fix.edits();
    let mut order: Vec<String> = Vec::new();
    let mut groups: BTreeMap<String, Vec<Edit>> = BTreeMap::new();
    for e in edits {
        if !groups.contains_key(&e.file) {
            order.push(e.file.clone());
        }
        groups.entry(e.file.clone()).or_default().push(e);
    }
    if groups.len() > MAX_FILES {
        return Err(format!("a fix may change at most {MAX_FILES} files"));
    }
    if groups.values().all(|g| g.len() == 1) {
        return Ok(fix);
    }
    let mut merged: Vec<Edit> = Vec::new();
    for file in &order {
        let group = &groups[file];
        if group.len() == 1 {
            merged.push(group[0].clone());
            continue;
        }
        let orig = files
            .get(file)
            .ok_or_else(|| format!("{file} is not in the project"))?;
        let text = std::str::from_utf8(&orig.bytes)
            .map_err(|_| format!("{file} is not text, so several changes cannot be made in it"))?;
        let mut spans: Vec<(usize, usize, &Edit)> = Vec::new();
        for e in group {
            match occurrences(text, e.old_text.as_str()).as_slice() {
                [at] => spans.push((*at, *at + e.old_text.len(), e)),
                [] => return Err(format!("text to replace is not in {file}")),
                _ => return Err(format!("text to replace is in {file} more than once")),
            }
        }
        spans.sort_by_key(|(start, _, _)| *start);
        for pair in spans.windows(2) {
            if pair[1].0 < pair[0].1 {
                return Err(format!("two changes in {file} share some of the same text"));
            }
        }
        let (first, last) = (spans[0].0, spans[spans.len() - 1].1);
        if last - first > MAX_SPAN {
            return Err(format!(
                "the changes in {file} are too far apart to be made as one"
            ));
        }
        let mut new_text = String::new();
        for (i, (_, end, e)) in spans.iter().enumerate() {
            new_text.push_str(&e.new_text);
            if let Some(next) = spans.get(i + 1) {
                new_text.push_str(&text[*end..next.0]);
            }
        }
        merged.push(Edit {
            file: file.clone(),
            old_text: text[first..last].to_string(),
            new_text,
        });
    }
    let mut it = merged.into_iter();
    let head = it.next().expect("a fix has at least one change");
    Ok(Fix {
        title: fix.title,
        file: head.file,
        old_text: head.old_text,
        new_text: head.new_text,
        also: it.collect(),
        explanation: fix.explanation,
    })
}

/// Applies every change of a fix to the files it names. All of them apply or the result is `None`.
pub fn apply_all(
    files: &BTreeMap<String, &ImportedFile>,
    fix: &Fix,
) -> Option<BTreeMap<String, Vec<u8>>> {
    let mut out = BTreeMap::new();
    for e in fix.edits() {
        // one change per file: several in a file are joined by `consolidate` first, never applied one over another
        if out.contains_key(&e.file) {
            return None;
        }
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
            TestRun::Passed(n) => kk_types::words::counted(&format!("{n} test(s) passed")),
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

/// Whether a path is part of a project's tests: a test file by name, a helper such as `conftest.py`, or anything inside a
/// folder named `tests`, `test` or `__tests__`. A fix may not change these, because a fix that edits the tests it must pass
/// proves nothing.
pub fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let mut parts: Vec<&str> = lower.split('/').collect();
    let name = parts.pop().unwrap_or_default();
    if parts
        .iter()
        .any(|d| matches!(*d, "tests" | "test" | "__tests__"))
    {
        return true;
    }
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    match ext {
        "py" => stem.starts_with("test") || stem.ends_with("_test") || stem == "conftest",
        "js" | "mjs" | "cjs" | "ts" | "mts" | "cts" | "jsx" | "tsx" => {
            stem.ends_with(".test") || stem.ends_with(".spec") || stem.ends_with("_test")
        }
        "rs" => {
            matches!(stem, "tests" | "test") || stem.ends_with("_test") || stem.ends_with("_tests")
        }
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestVerdict {
    /// The tests that passed before still run and pass.
    Maintained(String),
    /// A test that passed before no longer runs or no longer passes.
    Broken(String),
    /// The run after the change could not be done, so nothing can be said.
    Inconclusive(String),
    /// The tests did not pass before the change, so there is nothing to keep.
    NotComparable(String),
}

/// The project's tests before and after a change. Passing is not enough: a change that makes fewer tests run (a deleted
/// test, a module that no longer loads, a suite that stops early) has not kept the tests passing.
pub fn compare_test_runs(before: &TestRun, after: &TestRun) -> TestVerdict {
    let TestRun::Passed(had) = before else {
        return TestVerdict::NotComparable(format!("before the change: {}", before.describe()));
    };
    match after {
        TestRun::Passed(now) if now >= had => TestVerdict::Maintained(kk_types::words::counted(
            &format!("the project's {now} test(s) passed before and after this change"),
        )),
        TestRun::Passed(now) => TestVerdict::Broken(kk_types::words::counted(&format!(
            "{had} test(s) passed before this change but only {now} ran after it"
        ))),
        TestRun::Failed(m) => TestVerdict::Broken(format!(
            "the project's tests passed before this change and fail after it: {m}"
        )),
        TestRun::NoTests => TestVerdict::Broken(
            "the project's tests passed before this change and none ran after it".into(),
        ),
        TestRun::Unavailable(_) => {
            TestVerdict::Inconclusive(format!("after the change: {}", after.describe()))
        }
    }
}

/// Whether a failed TypeScript run failed because of what Node's type stripping cannot do, not because of the code: the answer is
/// one of three fixed sentences chosen from Node's own error codes, never text taken from the error. Node refuses `enum`,
/// `namespace` with code and parameter properties (ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX), refuses an import without its file
/// extension (ERR_MODULE_NOT_FOUND on a path), and before 22.18 does not know the `.mts` extension at all.
pub fn typescript_environment_problem(stderr: &str) -> Option<&'static str> {
    if stderr.contains("ERR_UNKNOWN_FILE_EXTENSION") || stderr.contains("Unknown file extension") {
        return Some("Node.js in the sandbox is too old to run TypeScript (22.18 or newer is needed), so this cannot be checked here");
    }
    if stderr.contains("ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX") {
        return Some("the code uses TypeScript syntax that needs a build step (an enum, a namespace with code or a parameter property), and only plain type annotations can be checked here");
    }
    if stderr.contains("ERR_MODULE_NOT_FOUND")
        && stderr.lines().any(|l| {
            l.find("Cannot find module '/")
                .is_some_and(|_| l.contains("imported from"))
        })
    {
        return Some("a project file imports another without its file extension (for example './util' instead of './util.ts'), which needs a build step, so this cannot be checked here");
    }
    None
}

/// Asks the sandbox's Node.js, once, whether it can run TypeScript at all. The error a Node.js without the parser gives depends on
/// the shape of the script, so the answer is not read from a failed run: `node -p process.features.typescript` is run first, and
/// a Node.js that does not say `strip` or `transform` makes the check unavailable instead of failed.
fn typescript_unavailable(node: &str, transport: &Transport) -> Option<String> {
    let mut spec = SandboxSpec::new(node);
    spec.args = vec!["-p".into(), "process.features.typescript".into()];
    spec.limits = Limits {
        wall: Duration::from_secs(20),
        cpu_secs: 10,
        ..Limits::default()
    };
    match run_with(&spec, transport) {
        Ok(o) if o.exit == Exit::Code(0) => {
            if node_has_typescript(&String::from_utf8_lossy(&o.stdout)) {
                None
            } else {
                Some("the Node.js in the sandbox has no TypeScript support (the official build from nodejs.org, 22.18 or newer, has it; some packaged builds leave it out), so this cannot be checked here".into())
            }
        }
        Ok(_) => Some("the Node.js in the sandbox could not be asked whether it runs TypeScript (it may be older than 22.10), so this cannot be checked here".into()),
        Err(SandboxError::Unsupported(m)) => Some(m.to_string()),
        Err(e) => Some(e.to_string()),
    }
}

/// Whether this Node.js can run TypeScript, from its own answer to `node -p process.features.typescript`: `strip` or `transform`.
/// The version number is not a safe test: the official 22.22.1 answers `strip`, but the package Debian and Ubuntu ship with the
/// same number is built without the TypeScript parser and answers `false`. An older Node that has no such property prints
/// `undefined`, which is also a no.
pub fn node_has_typescript(feature_text: &str) -> bool {
    matches!(feature_text.trim_end_matches('\n'), "strip" | "transform")
}

/// The package a failed script was missing, read from the interpreter's own error. Only a plain package name is ever
/// returned (never text taken from the error), so it is safe to show.
pub fn missing_package(language: Language, stderr: &str) -> Option<String> {
    // a Python module name is identifier segments joined by dots; only the top segment is repeated
    let python_name = |s: &str| -> Option<String> {
        let segments_ok = !s.is_empty()
            && s.len() <= 120
            && s.split('.').all(|seg| {
                !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            });
        segments_ok.then(|| s.split('.').next().unwrap_or_default().to_string())
    };
    // an npm package name: letters, digits and `_ - .`, optionally `@scope/name`; never a path
    let npm_name = |s: &str| -> Option<String> {
        let body = s.strip_prefix('@').map_or(s, |r| r);
        let parts: Vec<&str> = body.split('/').collect();
        let part_ok = |p: &&str| {
            !p.is_empty()
                && !p.starts_with('.')
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        };
        let shape_ok = if s.starts_with('@') {
            parts.len() == 2
        } else {
            parts.len() == 1
        };
        (s.len() <= 80 && shape_ok && parts.iter().all(part_ok)).then(|| s.to_string())
    };
    for line in stderr.lines() {
        match language {
            Language::Python => {
                if let Some(rest) = line
                    .trim()
                    .strip_prefix("ModuleNotFoundError: No module named '")
                {
                    let Some((name, _)) = rest.split_once('\'') else {
                        continue;
                    };
                    if let Some(p) = python_name(name) {
                        return Some(p);
                    }
                }
            }
            Language::Rust => {}
            Language::JavaScript | Language::TypeScript => {
                for marker in ["Cannot find package '", "Cannot find module '"] {
                    if let Some(at) = line.find(marker) {
                        let rest = &line[at + marker.len()..];
                        let Some((name, _)) = rest.split_once('\'') else {
                            continue;
                        };
                        if let Some(p) = npm_name(name) {
                            return Some(p);
                        }
                    }
                }
            }
        }
    }
    None
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

/// The project's own tests for one language. Python runs `unittest`; JavaScript runs Node's built-in `node --test`. Rust needs to
/// know which crate, so it is run by `run_project_tests_of`; asked here, it says so.
pub fn run_project_tests_for(
    language: Language,
    files: &BTreeMap<String, &ImportedFile>,
    env: &CheckEnv<'_>,
) -> TestRun {
    match language {
        Language::Python => run_project_tests(files, env),
        Language::JavaScript => run_node_tests(files, env, false),
        Language::TypeScript => run_node_tests(files, env, true),
        Language::Rust => TestRun::Unavailable("Rust tests are run for a particular crate".into()),
    }
}

/// The project's own tests for the language of a finding, and for Rust the crate the finding is in.
pub fn run_project_tests_of(
    finding: &Finding,
    files: &BTreeMap<String, &ImportedFile>,
    env: &CheckEnv<'_>,
) -> TestRun {
    match finding.language {
        Language::Rust => run_rust_tests(&finding.file, files, env),
        other => run_project_tests_for(other, files, env),
    }
}

/// The crate's own unit tests, built and run in the sandbox with the same strict reading of the runner's summary as the others.
/// Tests in a `tests` folder need the crate as a dependency and are not run; a crate with no `#[test]` has none to run.
fn run_rust_tests(
    entry: &str,
    files: &BTreeMap<String, &ImportedFile>,
    env: &CheckEnv<'_>,
) -> TestRun {
    let plan = match rust::rust_crate(entry, files) {
        Ok(c) => c,
        Err(rust::Problem::Unavailable(why)) => return TestRun::Unavailable(why.sentence().into()),
        Err(rust::Problem::Restricted(_)) => {
            return TestRun::Unavailable(msg::WITHHELD_FILE.into());
        }
        Err(rust::Problem::EntryMissing) => return TestRun::NoTests,
    };
    let holds_tests = plan.sample.iter().any(|p| {
        p.ends_with(".rs")
            && files
                .get(p)
                .is_some_and(|f| rust::has_inline_tests(&String::from_utf8_lossy(&f.bytes)))
    });
    if !holds_tests {
        return TestRun::NoTests;
    }
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir: PathBuf = env
        .scratch
        .join(format!("rstests-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let sample = dir.join("sample");
    for p in &plan.sample {
        let dest = sample.join(p);
        if dest.parent().map(fs::create_dir_all).transpose().is_err()
            || fs::write(&dest, &files[p].bytes).is_err()
        {
            let _ = fs::remove_dir_all(&dir);
            return TestRun::Unavailable("could not prepare the crate".into());
        }
    }
    let mut spec = SandboxSpec::new("/usr/bin/sh");
    spec.args = vec![
        "-c".into(),
        RUST_BUILD_AND_TEST.into(),
        "sh".into(),
        env.rustc.into(),
        plan.edition.clone().into(),
        plan.root.clone().into(),
        plan.crate_name.clone().into(),
    ];
    spec.binds.push(Bind {
        host: sample,
        inside: "/sample".into(),
        writable: false,
    });
    spec.chdir = Some("/sample".into());
    spec.limits = Limits {
        wall: Duration::from_secs(90),
        cpu_secs: 60,
        ..Limits::default()
    };
    let result = run_with(&spec, env.transport);
    let _ = fs::remove_dir_all(&dir);
    match result {
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            if let Some((stage, rest)) = rust::build_failure(&stderr) {
                return match rust::classify_build_failure(stage, rest) {
                    Some(BuildProblem::Environment(why)) => TestRun::Unavailable(why.into()),
                    _ => TestRun::Unavailable(
                        "the crate's tests do not build in this environment (they may need packages or dev-dependencies)".into(),
                    ),
                };
            }
            rust::parse_rust_tests(&o.exit, &String::from_utf8_lossy(&o.stdout))
        }
        Err(SandboxError::Unsupported(m)) => TestRun::Unavailable(m.to_string()),
        Err(e) => TestRun::Unavailable(e.to_string()),
    }
}

fn is_js_test_file(path: &str, typescript: bool) -> bool {
    if !is_test_path(path) {
        return false;
    }
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "js" | "mjs" | "cjs" => true,
        "ts" | "mts" | "cts" => typescript,
        _ => false,
    }
}

fn run_node_tests(
    files: &BTreeMap<String, &ImportedFile>,
    env: &CheckEnv<'_>,
    typescript: bool,
) -> TestRun {
    if !files.keys().any(|p| is_js_test_file(p, typescript)) {
        return TestRun::NoTests;
    }
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir: PathBuf = env
        .scratch
        .join(format!("jstests-{}-{n}", std::process::id()));
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
    let mut spec = SandboxSpec::new(env.node);
    spec.args = vec![
        "--test".into(),
        "--test-reporter=tap".into(),
        "--test-concurrency=1".into(),
    ];
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
        Ok(o) => parse_node_test(&o.exit, &String::from_utf8_lossy(&o.stdout)),
        Err(SandboxError::Unsupported(m)) => TestRun::Unavailable(m.to_string()),
        Err(e) => TestRun::Unavailable(e.to_string()),
    }
}

/// Reads a `node --test --test-reporter=tap` run. Anything not clearly a pass is not a pass: the runner's own summary must be
/// present, complete and add up, its failure counts must agree with the `not ok` lines and with the exit status, and a run
/// where nothing passed (nothing ran, or everything was skipped) is no evidence. Text a test file prints is escaped by Node
/// (`# \# tests 99`), so only the runner's own column-0 summary is read, and the last one wins.
pub fn parse_node_test(exit: &Exit, stdout: &str) -> TestRun {
    let hint = missing_package(Language::JavaScript, stdout)
        .map(|p| format!(" (it needs the package '{p}', which is not in the sandbox)"))
        .unwrap_or_default();
    let lines: Vec<&str> = stdout.lines().collect();
    let count = |l: &str, key: &str| -> Option<u32> {
        l.strip_prefix(key)
            .and_then(|r| r.trim().parse::<u32>().ok())
    };
    // the runner's own summary: `# tests N` directly after its plan line `1..N` (text a test prints is prefixed by Node, so a
    // printed plan line reads `# 1..5` and can not stand in for it)
    let is_plan = |l: &str| {
        l.strip_prefix("1..")
            .is_some_and(|r| !r.is_empty() && r.bytes().all(|b| b.is_ascii_digit()))
    };
    let Some(at) = lines
        .iter()
        .enumerate()
        .rposition(|(i, l)| count(l, "# tests ").is_some() && i > 0 && is_plan(lines[i - 1]))
    else {
        return TestRun::Failed(format!("no test summary{hint}"));
    };
    let field = |key: &str| lines[at..].iter().take(8).find_map(|l| count(l, key));
    let (Some(tests), Some(pass), Some(fail), Some(cancelled), Some(skipped), Some(todo)) = (
        field("# tests "),
        field("# pass "),
        field("# fail "),
        field("# cancelled "),
        field("# skipped "),
        field("# todo "),
    ) else {
        return TestRun::Failed(format!("the test summary is incomplete{hint}"));
    };
    if tests != pass + fail + cancelled + skipped + todo {
        return TestRun::Failed("the test summary does not add up".into());
    }
    if fail > 0 || cancelled > 0 {
        return TestRun::Failed(kk_types::words::counted(&format!(
            "{fail} test(s) failed and {cancelled} were cancelled{hint}"
        )));
    }
    let uncounted_failure = lines.iter().any(|l| {
        let l = l.trim_start();
        l.starts_with("not ok ") && !l.contains("# TODO") && !l.contains("# SKIP")
    });
    if uncounted_failure {
        return TestRun::Failed("a test failed that the summary does not count".into());
    }
    if *exit != Exit::Code(0) {
        return TestRun::Failed(format!("the test run ended with {exit}{hint}"));
    }
    if tests == 0 || pass == 0 {
        return TestRun::NoTests;
    }
    TestRun::Passed(pass)
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
    let before_tests = run_project_tests_of(finding, files, env);
    let Some(new_bytes) = apply_all(files, fix) else {
        return (status, String::new());
    };
    if !matches!(before_tests, TestRun::Passed(_)) {
        return (
            status,
            format!("before the change: {}", before_tests.describe()),
        );
    }
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
    let after_tests = run_project_tests_of(finding, &map, env);
    match compare_test_runs(&before_tests, &after_tests) {
        TestVerdict::Maintained(note) => (status, note),
        TestVerdict::Broken(note) => (FixStatus::BreaksTests(note.clone()), note),
        TestVerdict::Inconclusive(note) | TestVerdict::NotComparable(note) => (status, note),
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
        return FixStatus::NotChecked(msg::FINDING_NOT_REPRODUCED.into());
    }
    if fix.edits().iter().any(|e| !files.contains_key(&e.file)) {
        return FixStatus::CouldNotApply(msg::FIX_FILE_MISSING.into());
    }
    if let Some(e) = fix.edits().iter().find(|e| is_test_path(&e.file)) {
        return FixStatus::CouldNotApply(format!(
            "{} ({}): a change to the tests it must pass proves nothing",
            msg::FIX_TEST_FILE,
            e.file
        ));
    }
    if finding.language == Language::Rust {
        // Rust keeps its unit tests in the file they test, so a change is also refused when it touches a test inside a file
        if let Some(e) = fix.edits().iter().find(|e| {
            files.get(&e.file).is_some_and(|f| {
                rust::change_touches_tests(
                    &String::from_utf8_lossy(&f.bytes),
                    &e.old_text,
                    &e.new_text,
                )
            })
        }) {
            return FixStatus::CouldNotApply(format!(
                "{} ({}): a change to the tests it must pass proves nothing",
                msg::FIX_TEST_FILE,
                e.file
            ));
        }
    }
    let Some(new_bytes) = apply_all(files, fix) else {
        return FixStatus::CouldNotApply(msg::FIX_ONCE.into());
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
    let verdict_after = after.stdout.lines().any(|l| l.trim() == "NOT REPRODUCED");
    match after.status {
        CheckStatus::NotReproduced if !control_after => FixStatus::NotChecked(format!(
            "{} (no CONTROL_OK), so this cannot be told apart from removing the feature",
            msg::NO_CONTROL_AFTER
        )),
        CheckStatus::NotReproduced if !verdict_after => FixStatus::NotChecked(format!(
            "{}, so it cannot be told apart from the code ending the program early",
            msg::NO_VERDICT_AFTER
        )),
        CheckStatus::NotReproduced => FixStatus::FixesTheReproducer,
        CheckStatus::Reproduced => FixStatus::StillReproduced,
        other => FixStatus::NotChecked(format!("{}: {other:?}", msg::UNTRUSTED_AFTER)),
    }
}

/// A plain unified diff of what one change does to a file, for people to read; applying uses `apply_fix`, never this text.
/// Lines that are the same before and after are context, so a change that rewrites a block shows only the lines that differ,
/// and changes far apart are separate hunks.
pub fn unified_diff(original: &[u8], fix: &Fix) -> String {
    let before = String::from_utf8_lossy(original).into_owned();
    let after = match apply_fix(original, fix) {
        Some(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        // not applicable: still show what was meant, as a plain replacement of the first place it is found
        None => before.replacen(fix.old_text.as_str(), &fix.new_text, 1),
    };
    diff_text(&fix.file, &before, &after)
}

fn split_lines(text: &str) -> Vec<&str> {
    text.strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Op {
    Same,
    Del,
    Add,
}

/// The line-by-line difference of two texts: the common start and end are trimmed, and what is left is compared with a
/// longest-common-subsequence table (or shown as one replacement if that would be too large).
fn line_ops<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<(Op, &'a str)> {
    let mut start = 0;
    while start < a.len() && start < b.len() && a[start] == b[start] {
        start += 1;
    }
    let (mut ea, mut eb) = (a.len(), b.len());
    while ea > start && eb > start && a[ea - 1] == b[eb - 1] {
        ea -= 1;
        eb -= 1;
    }
    let mut ops: Vec<(Op, &str)> = a[..start].iter().map(|l| (Op::Same, *l)).collect();
    let (ma, mb) = (&a[start..ea], &b[start..eb]);
    if ma.len().saturating_mul(mb.len()) > 4_000_000 {
        ops.extend(ma.iter().map(|l| (Op::Del, *l)));
        ops.extend(mb.iter().map(|l| (Op::Add, *l)));
    } else {
        let (n, m) = (ma.len(), mb.len());
        let mut table = vec![0u32; (n + 1) * (m + 1)];
        let at = |i: usize, j: usize| i * (m + 1) + j;
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                table[at(i, j)] = if ma[i] == mb[j] {
                    table[at(i + 1, j + 1)] + 1
                } else {
                    table[at(i + 1, j)].max(table[at(i, j + 1)])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && ma[i] == mb[j] {
                ops.push((Op::Same, ma[i]));
                i += 1;
                j += 1;
            } else if i < n && (j == m || table[at(i + 1, j)] >= table[at(i, j + 1)]) {
                ops.push((Op::Del, ma[i]));
                i += 1;
            } else {
                ops.push((Op::Add, mb[j]));
                j += 1;
            }
        }
    }
    ops.extend(a[ea..].iter().map(|l| (Op::Same, *l)));
    ops
}

pub(crate) fn diff_text(file: &str, before: &str, after: &str) -> String {
    const CONTEXT: usize = 3;
    let (a, b) = (split_lines(before), split_lines(after));
    let ops = line_ops(&a, &b);
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, (op, _))| *op != Op::Same)
        .map(|(i, _)| i)
        .collect();
    if changed.is_empty() {
        return String::new();
    }
    // runs of changes that are close enough to share their context become one hunk
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for &i in &changed {
        match ranges.last_mut() {
            Some((_, end)) if i - *end <= 2 * CONTEXT + 1 => *end = i,
            _ => ranges.push((i, i)),
        }
    }
    let mut out = format!("--- a/{file}\n+++ b/{file}\n");
    for (first, last) in ranges {
        let from = first.saturating_sub(CONTEXT);
        let to = (last + 1 + CONTEXT).min(ops.len());
        let old_before = ops[..from].iter().filter(|(op, _)| *op != Op::Add).count();
        let new_before = ops[..from].iter().filter(|(op, _)| *op != Op::Del).count();
        let old_len = ops[from..to]
            .iter()
            .filter(|(op, _)| *op != Op::Add)
            .count();
        let new_len = ops[from..to]
            .iter()
            .filter(|(op, _)| *op != Op::Del)
            .count();
        let old_start = if old_len == 0 {
            old_before
        } else {
            old_before + 1
        };
        let new_start = if new_len == 0 {
            new_before
        } else {
            new_before + 1
        };
        out.push_str(&format!(
            "@@ -{old_start},{old_len} +{new_start},{new_len} @@\n"
        ));
        for (op, line) in &ops[from..to] {
            let mark = match op {
                Op::Same => ' ',
                Op::Del => '-',
                Op::Add => '+',
            };
            out.push(mark);
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod control_tests {
    use super::control_untested;

    #[test]
    fn ck_use01_a_control_line_that_tests_nothing_is_told_apart_from_one_that_does() {
        // printed whatever the project does
        for script in [
            "from shop import shipping_cost\ncost = shipping_cost(15)\nprint('CONTROL_OK')\nprint('REPRODUCED' if cost == 0 else 'NOT REPRODUCED')",
            "const { f } = require('./f.js');\nconsole.log('CONTROL_OK');\nconsole.log(f(1) ? 'REPRODUCED' : 'NOT REPRODUCED');",
            "print(\"CONTROL_OK\")\nprint(\"REPRODUCED\")",
        ] {
            assert!(control_untested(script), "{script}");
        }
        // under a condition, or after something that can fail
        for script in [
            "from shop.cart import Cart\ncart = Cart()\ncart.add('apple', 2)\nif cart.total() == 2:\n    print('CONTROL_OK')\nprint('REPRODUCED')",
            "cart.add('apple', 2)\nassert cart.total() == 2\nprint('CONTROL_OK')\nprint('REPRODUCED')",
            "print('CONTROL_OK' if total == 2 else 'CONTROL_FAILED')\nprint('REPRODUCED')",
            "if (cart.total() === 2) console.log('CONTROL_OK');\nconsole.log('REPRODUCED');",
            "const ok = cart.total() === 2;\nok && console.log('CONTROL_OK');",
            "if (cart.total() !== 2) throw new Error('control');\nconsole.log('CONTROL_OK');",
            "fn main() {\n    let mut cart = Cart::new();\n    println!(\"CONTROL_OK\");\n}",
            "print('REPRODUCED')",
        ] {
            assert!(!control_untested(script), "{script}");
        }
    }
}

#[cfg(test)]
mod python_import_tests {
    use super::*;

    #[test]
    fn ck_use02_relative_imports_and_the_src_layout_are_followed() {
        let src = "from .money import Money\nfrom ..util import helpers\nfrom . import locale\nimport os\n";
        let got = imports_in(src, "src/moneyed/core/classes.py");
        for want in [
            "moneyed.core.money",
            "moneyed.core.money.Money",
            "moneyed.util",
            "moneyed.util.helpers",
            "moneyed.core.locale",
            "os",
        ] {
            assert!(got.iter().any(|m| m == want), "{want} in {got:?}");
        }
        // a file at the top of a package
        let got = imports_in("from .x import y\n", "pkg/__init__.py");
        assert!(got.iter().any(|m| m == "pkg.x"), "{got:?}");
        // too many dots for where the file is: nothing invented
        assert!(imports_in("from ...x import y\n", "pkg/a.py")
            .iter()
            .all(|m| !m.contains('x')));
        assert!(module_paths("moneyed").contains(&"src/moneyed/__init__.py".to_string()));
    }

    #[test]
    fn ck_use03_a_script_that_catches_errors_is_recognised_and_the_stand_in_holds_nothing_real() {
        for script in [
            "try:\n    f()\nexcept Exception:\n    print('REPRODUCED')\n",
            "try { f() } catch (e) { console.log('REPRODUCED') }\n",
            "p.catch(() => 0)\n",
            "with suppress(ValueError):\n    f()\n",
        ] {
            assert!(catches_errors(script), "{script}");
        }
        assert!(!catches_errors(
            "print('REPRODUCED' if f() else 'NOT REPRODUCED')\n"
        ));
        // the stand-ins are fixed text: no project name, no value, the same for every file
        assert!(
            STAND_IN.contains("def __getattr__(name):")
                && STAND_IN.contains(WITHHELD_USED)
                && STAND_IN.contains("__radd__")
        );
        assert!(STAND_IN_NONE.contains("return None"));
    }
}
