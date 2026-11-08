//! What is specific to Rust reproducers and needs no sandbox: reading `Cargo.toml`, finding the files of a crate, reading a test
//! run, and sorting a failed build into "the environment cannot do this" and "the script is wrong".
//!
//! A Rust finding is checked by building the project's own library crate with `rustc` and linking the reproducer against it as
//! `project`. There is no `cargo` and no network, so a crate with dependencies, a build script, features or a procedural macro
//! cannot be built here; each of those is said, in a fixed sentence, and the check is `Unavailable`, never failed and never
//! reproduced. The files of the crate are found the way the compiler finds them (the Rust Reference, "Modules", section "Module
//! source filenames"), so the sample holds exactly the module tree and nothing else.
//!
//! Everything read here comes from files of the project under review and is untrusted: every reader is strict and, when it is
//! unsure, answers with a reason instead of a guess.

use crate::check::TestRun;
use crate::import::ImportedFile;
use kk_sandbox::Exit;
use kk_types::label::Level;
use std::collections::{BTreeMap, BTreeSet};

/// The editions this program passes to `rustc`. Anything else in a manifest is refused, never guessed.
pub const EDITIONS: [&str; 4] = ["2015", "2018", "2021", "2024"];
/// The most files, and the most bytes, one crate's module tree may have for a check to build it.
pub const MAX_CRATE_FILES: usize = 400;
pub const MAX_CRATE_BYTES: usize = 8 << 20;
/// The largest source file whose module declarations are read.
const MAX_SCAN_BYTES: usize = 4 << 20;

/// Why a Rust check cannot be done here. Each has one fixed sentence; nothing taken from the project is ever put in them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    NoManifest,
    Complex,
    Dependencies,
    BuildScript,
    NoLibrary,
    ProcMacro,
    DefaultFeatures,
    Edition,
    NotInCrate,
    TooLarge,
    UnusualLayout,
}

impl Why {
    pub fn sentence(self) -> &'static str {
        match self {
            Why::NoManifest => "the Rust code is not inside a package with a Cargo.toml, so its crate cannot be built here",
            Why::Complex => "Cargo.toml uses something this check does not read (a value shared from a workspace, a value over several lines, a target-specific table or another unusual form), so the crate cannot be built here",
            Why::Dependencies => "the crate has dependencies, which need cargo and a package download, so it cannot be built here",
            Why::BuildScript => "the crate has a build script, so it cannot be built here",
            Why::NoLibrary => "the crate has no library to link a reproducer against (it has only a program), so this cannot be checked here",
            Why::ProcMacro => "the crate is a procedural macro crate, which cannot be built here",
            Why::DefaultFeatures => "the crate turns features on by default, which this check does not set, so it cannot be built here",
            Why::Edition => "Cargo.toml names a Rust edition this check does not know, so the crate cannot be built here",
            Why::NotInCrate => "the cited file is not part of the module tree of its crate, so a reproducer cannot depend on it",
            Why::TooLarge => "the crate has too many or too large files to be built here",
            Why::UnusualLayout => "the crate's modules use a file layout this check does not follow (a path attribute inside a module block), so it cannot be built here",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    Unavailable(Why),
    /// A file the crate needs is withheld from the review (labelled Restricted).
    Restricted(String),
    EntryMissing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The name the library is built under: the `[lib]` name, else the package name, with dashes turned into underscores.
    pub crate_name: String,
    pub edition: String,
    pub lib_path: Option<String>,
    /// Whether `[dev-dependencies]` has an entry: the crate builds, but its own tests may not.
    pub dev_dependencies: bool,
}

fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && !s.as_bytes()[0].is_ascii_digit()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// The line without a trailing `#` comment, honouring quotes.
fn strip_comment(line: &str) -> String {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in line.chars() {
        if let Some(q) = quote {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' && q == '"' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '#' => break,
            '"' | '\'' => {
                quote = Some(c);
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Opening minus closing brackets and braces outside quotes.
fn bracket_delta(text: &str) -> i32 {
    let mut d = 0;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in text.chars() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' && q == '"' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '[' | '{' => d += 1,
            ']' | '}' => d -= 1,
            _ => {}
        }
    }
    d
}

/// A plain quoted string with no escapes, else nothing.
fn plain_string(v: &str) -> Option<String> {
    let v = v.trim();
    let q = v.chars().next()?;
    if (q != '"' && q != '\'') || v.len() < 2 || !v.ends_with(q) {
        return None;
    }
    let inner = &v[1..v.len() - 1];
    (!inner.contains(q) && !inner.contains('\\') && !inner.chars().any(char::is_control))
        .then(|| inner.to_string())
}

/// Reads the parts of a `Cargo.toml` that decide whether and how the crate can be built with `rustc` alone. This is a reader of a
/// small subset of TOML, on purpose: a form it does not read is `Why::Complex`, never an assumption.
pub fn parse_manifest(text: &str) -> Result<Manifest, Why> {
    if text.contains("\"\"\"") || text.contains("'''") {
        return Err(Why::Complex);
    }
    let mut section = String::new();
    let mut has_package = false;
    let (mut name, mut edition, mut lib_edition) = (None, None, None);
    let (mut lib_path, mut lib_name) = (None, None);
    let (mut deps, mut dev_deps, mut build) = (false, false, false);
    let (mut proc_macro, mut default_features) = (false, false);
    let mut depth: i32 = 0;
    let reads = |s: &str| {
        matches!(
            s,
            "package" | "lib" | "features" | "dependencies" | "dev-dependencies"
        )
    };
    for raw in text.lines() {
        let line = strip_comment(raw);
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if depth > 0 {
            depth += bracket_delta(t);
            if depth < 0 {
                return Err(Why::Complex);
            }
            continue;
        }
        if t.starts_with('[') {
            let inner = t
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim()
                .to_string();
            if bracket_delta(t) != 0 || inner.is_empty() {
                return Err(Why::Complex);
            }
            section = inner;
            if section.starts_with("target.") || section.starts_with("target ") {
                return Err(Why::Complex);
            }
            if section == "package" {
                has_package = true;
            }
            if section.starts_with("dependencies.") {
                deps = true;
            }
            if section.starts_with("dev-dependencies.") {
                dev_deps = true;
            }
            continue;
        }
        let Some((key, value)) = t.split_once('=') else {
            if reads(&section) {
                return Err(Why::Complex);
            }
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        let delta = bracket_delta(value);
        if delta < 0 {
            return Err(Why::Complex);
        }
        if delta > 0 {
            if reads(&section) {
                return Err(Why::Complex);
            }
            depth = delta;
            continue;
        }
        if key.starts_with('"') || key.starts_with('\'') {
            if reads(&section) {
                return Err(Why::Complex);
            }
            continue;
        }
        match section.as_str() {
            "package" => {
                let first = key.split('.').next().unwrap_or(key);
                if key.contains('.') && matches!(first, "name" | "edition" | "build") {
                    return Err(Why::Complex);
                }
                match key {
                    "name" => name = Some(plain_string(value).ok_or(Why::Complex)?),
                    "edition" => edition = Some(plain_string(value).ok_or(Why::Complex)?),
                    "build" if value != "false" => build = true,
                    _ => {}
                }
            }
            "lib" => match key {
                "path" => lib_path = Some(plain_string(value).ok_or(Why::Complex)?),
                "name" => lib_name = Some(plain_string(value).ok_or(Why::Complex)?),
                "edition" => lib_edition = Some(plain_string(value).ok_or(Why::Complex)?),
                "proc-macro" | "proc_macro" if value != "false" => proc_macro = true,
                "crate-type" | "crate_type" if value.contains("proc-macro") => proc_macro = true,
                _ => {}
            },
            "features" if key == "default" && value != "[]" => default_features = true,
            "dependencies" => deps = true,
            "dev-dependencies" => dev_deps = true,
            _ => {}
        }
    }
    if depth != 0 {
        return Err(Why::Complex);
    }
    if !has_package {
        return Err(Why::NoManifest);
    }
    let package_name = name.ok_or(Why::Complex)?;
    if build {
        return Err(Why::BuildScript);
    }
    if proc_macro {
        return Err(Why::ProcMacro);
    }
    if deps {
        return Err(Why::Dependencies);
    }
    if default_features {
        return Err(Why::DefaultFeatures);
    }
    let edition = lib_edition
        .or(edition)
        .unwrap_or_else(|| "2015".to_string());
    if !EDITIONS.contains(&edition.as_str()) {
        return Err(Why::Edition);
    }
    let crate_name = lib_name.unwrap_or(package_name).replace('-', "_");
    if !is_ident(&crate_name) {
        return Err(Why::Complex);
    }
    Ok(Manifest {
        crate_name,
        edition,
        lib_path,
        dev_dependencies: dev_deps,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Str(String),
    Punct(char),
}

/// The tokens of a Rust source file that matter for finding its other files: identifiers, string literals and punctuation, with
/// comments, character literals and lifetimes left out. Text inside a string or a comment is never read as code.
fn tokens(source: &str) -> Vec<Tok> {
    spanned(source).into_iter().map(|(t, _, _)| t).collect()
}

/// The same tokens with where each starts and ends, in bytes of the source.
fn spanned(source: &str) -> Vec<(Tok, usize, usize)> {
    let c: Vec<char> = source.chars().collect();
    let n = c.len();
    let mut out: Vec<Tok> = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    let at = |i: usize| c.get(i).copied();
    let mut last_start = 0usize;
    while i < n {
        if out.len() > spans.len() {
            spans.push((last_start, i));
        }
        last_start = i;
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch == '/' && at(i + 1) == Some('/') {
            while i < n && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '/' && at(i + 1) == Some('*') {
            let mut depth = 1;
            i += 2;
            while i < n && depth > 0 {
                if c[i] == '/' && at(i + 1) == Some('*') {
                    depth += 1;
                    i += 2;
                } else if c[i] == '*' && at(i + 1) == Some('/') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
        } else if ch == '"' {
            i = read_string(&c, i, &mut out);
        } else if ch == '\'' {
            if at(i + 1) == Some('\\') {
                i += 2;
                let mut k = 0;
                while i < n && c[i] != '\'' && k < 12 {
                    i += 1;
                    k += 1;
                }
                i += 1;
            } else if at(i + 2) == Some('\'') {
                i += 3;
            } else {
                i += 1;
                while i < n && (c[i].is_alphanumeric() || c[i] == '_') {
                    i += 1;
                }
            }
        } else if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < n && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let word: String = c[start..i].iter().collect();
            let raw_prefix = matches!(word.as_str(), "r" | "br" | "cr");
            if raw_prefix && matches!(at(i), Some('"' | '#')) {
                let mut j = i;
                let mut hashes = 0;
                while at(j) == Some('#') {
                    hashes += 1;
                    j += 1;
                }
                if at(j) == Some('"') {
                    i = read_raw_string(&c, j, hashes, &mut out);
                    continue;
                }
            }
            if matches!(word.as_str(), "b" | "c") && at(i) == Some('"') {
                i = read_string(&c, i, &mut out);
                continue;
            }
            out.push(Tok::Ident(word));
        } else if ch.is_ascii_digit() {
            while i < n && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
        } else {
            out.push(Tok::Punct(ch));
            i += 1;
        }
    }
    if out.len() > spans.len() {
        spans.push((last_start, i.min(n)));
    }
    let mut byte_at: Vec<usize> = Vec::with_capacity(n + 1);
    let mut b = 0;
    for ch in &c {
        byte_at.push(b);
        b += ch.len_utf8();
    }
    byte_at.push(b);
    out.into_iter()
        .zip(spans)
        .map(|(t, (a, z))| (t, byte_at[a], byte_at[z.min(n)]))
        .collect()
}

fn read_string(c: &[char], start: usize, out: &mut Vec<Tok>) -> usize {
    let mut i = start + 1;
    let mut text = String::new();
    while i < c.len() {
        match c[i] {
            '"' => {
                out.push(Tok::Str(text));
                return i + 1;
            }
            '\\' if i + 1 < c.len() => {
                match c[i + 1] {
                    '\\' => text.push('\\'),
                    '"' => text.push('"'),
                    other => {
                        text.push('\\');
                        text.push(other);
                    }
                }
                i += 2;
            }
            ch => {
                text.push(ch);
                i += 1;
            }
        }
    }
    c.len()
}

fn read_raw_string(c: &[char], quote_at: usize, hashes: usize, out: &mut Vec<Tok>) -> usize {
    let mut i = quote_at + 1;
    let mut text = String::new();
    while i < c.len() {
        if c[i] == '"' && (0..hashes).all(|k| c.get(i + 1 + k) == Some(&'#')) {
            out.push(Tok::Str(text));
            return i + 1 + hashes;
        }
        text.push(c[i]);
        i += 1;
    }
    c.len()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Ref {
    /// `mod name;` with the names of the inline modules it sits in, and a `#[path]` if one precedes it.
    Mod {
        name: String,
        path: Option<String>,
        inline: Vec<String>,
    },
    /// `include!`, `include_str!` or `include_bytes!` with a literal path.
    Include(String),
    /// A form whose file the scanner cannot place.
    Unusual,
}

fn refs_of(source: &str) -> Vec<Ref> {
    if source.len() > MAX_SCAN_BYTES {
        return vec![Ref::Unusual];
    }
    let t = tokens(source);
    let mut out = Vec::new();
    let mut inline: Vec<(String, usize)> = Vec::new();
    let mut depth = 0usize;
    let mut pending: Option<String> = None;
    let mut i = 0;
    while i < t.len() {
        match &t[i] {
            Tok::Punct('#') => {
                let mut j = i + 1;
                if t.get(j) == Some(&Tok::Punct('!')) {
                    j += 1;
                }
                if t.get(j) == Some(&Tok::Punct('[')) {
                    let mut d = 1;
                    let start = j + 1;
                    j += 1;
                    while j < t.len() && d > 0 {
                        match t[j] {
                            Tok::Punct('[') => d += 1,
                            Tok::Punct(']') => d -= 1,
                            _ => {}
                        }
                        j += 1;
                    }
                    let attr = &t[start..j.saturating_sub(1).max(start)];
                    if let [Tok::Ident(k), Tok::Punct('='), Tok::Str(p)] = attr {
                        if k == "path" {
                            pending = Some(p.clone());
                        }
                    } else if attr.iter().any(|x| x == &Tok::Ident("cfg_attr".into()))
                        && attr.iter().any(|x| x == &Tok::Ident("path".into()))
                    {
                        out.push(Ref::Unusual);
                    }
                    i = j;
                    continue;
                }
            }
            Tok::Ident(w) if w == "mod" => {
                if let (Some(Tok::Ident(name)), Some(next)) = (t.get(i + 1), t.get(i + 2)) {
                    if is_ident(name) {
                        match next {
                            Tok::Punct(';') => {
                                out.push(Ref::Mod {
                                    name: name.clone(),
                                    path: pending.take(),
                                    inline: inline.iter().map(|(n, _)| n.clone()).collect(),
                                });
                                i += 3;
                                continue;
                            }
                            Tok::Punct('{') => {
                                if pending.take().is_some() {
                                    out.push(Ref::Unusual);
                                }
                                inline.push((name.clone(), depth));
                                depth += 1;
                                i += 3;
                                continue;
                            }
                            _ => {}
                        }
                    }
                }
            }
            Tok::Ident(w) if matches!(w.as_str(), "include" | "include_str" | "include_bytes") => {
                if let (
                    Some(Tok::Punct('!')),
                    Some(Tok::Punct('(')),
                    Some(Tok::Str(p)),
                    Some(Tok::Punct(')')),
                ) = (t.get(i + 1), t.get(i + 2), t.get(i + 3), t.get(i + 4))
                {
                    out.push(Ref::Include(p.clone()));
                    i += 5;
                    continue;
                }
            }
            Tok::Punct('{') => {
                depth += 1;
                pending = None;
            }
            Tok::Punct('}') => {
                depth = depth.saturating_sub(1);
                if inline.last().is_some_and(|(_, d)| *d == depth) {
                    inline.pop();
                }
                pending = None;
            }
            Tok::Punct(';') | Tok::Punct('=') => pending = None,
            Tok::Ident(w)
                if !matches!(
                    w.as_str(),
                    "pub" | "crate" | "super" | "self" | "in" | "unsafe"
                ) =>
            {
                pending = None
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The path with `.` and `..` resolved and no leading slash, or nothing if it leaves the project.
fn normal(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

fn join(dir: &str, rest: &str) -> String {
    if dir.is_empty() {
        rest.to_string()
    } else {
        format!("{dir}/{rest}")
    }
}

/// The manifest that governs a file: the nearest `Cargo.toml` in its folder or any folder above.
fn find_manifest(entry: &str, files: &BTreeMap<String, &ImportedFile>) -> Option<String> {
    let mut dir = dir_of(entry).to_string();
    loop {
        let candidate = join(&dir, "Cargo.toml");
        if files.contains_key(&candidate) {
            return Some(candidate);
        }
        if dir.is_empty() {
            return None;
        }
        dir = dir_of(&dir).to_string();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustCrate {
    pub crate_name: String,
    pub edition: String,
    /// The crate root, as a path among the project's files.
    pub root: String,
    /// Every file of the crate's module tree, and files it includes, sorted.
    pub sample: Vec<String>,
    pub dev_dependencies: bool,
    /// The folder of `Cargo.toml`.
    pub dir: String,
}

/// The crate a cited file belongs to, and every file the compiler needs to build its library: the files its modules are loaded
/// from and the files it includes. A withheld file anywhere in that set refuses the check.
pub fn rust_crate(
    entry: &str,
    files: &BTreeMap<String, &ImportedFile>,
) -> Result<RustCrate, Problem> {
    if !files.contains_key(entry) {
        return Err(Problem::EntryMissing);
    }
    let manifest_path = find_manifest(entry, files).ok_or(Problem::Unavailable(Why::NoManifest))?;
    let manifest_file = files[&manifest_path];
    if manifest_file.label.level >= Level::Restricted {
        return Err(Problem::Restricted(manifest_path));
    }
    let manifest = parse_manifest(&String::from_utf8_lossy(&manifest_file.bytes))
        .map_err(Problem::Unavailable)?;
    let dir = dir_of(&manifest_path).to_string();
    if files.contains_key(&join(&dir, "build.rs")) {
        return Err(Problem::Unavailable(Why::BuildScript));
    }
    let root = match &manifest.lib_path {
        Some(p) => {
            if p.starts_with('/') || p.split('/').any(|s| s == "..") {
                return Err(Problem::Unavailable(Why::Complex));
            }
            normal(&join(&dir, p)).ok_or(Problem::Unavailable(Why::Complex))?
        }
        None => join(&dir, "src/lib.rs"),
    };
    if !files.contains_key(&root) {
        return Err(Problem::Unavailable(Why::NoLibrary));
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut bytes = 0usize;
    // (path, whether its child modules are looked up beside it: a crate root or a file named mod.rs, or loaded by a path attribute)
    let mut stack: Vec<(String, bool)> = vec![(root.clone(), true)];
    while let Some((path, mod_rs)) = stack.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let f = files[&path];
        if f.label.level >= Level::Restricted {
            return Err(Problem::Restricted(path));
        }
        bytes += f.bytes.len();
        if seen.len() > MAX_CRATE_FILES || bytes > MAX_CRATE_BYTES {
            return Err(Problem::Unavailable(Why::TooLarge));
        }
        if !path.ends_with(".rs") {
            continue;
        }
        let source = String::from_utf8_lossy(&f.bytes);
        let here = dir_of(&path).to_string();
        let stem = path
            .rsplit('/')
            .next()
            .and_then(|n| n.strip_suffix(".rs"))
            .unwrap_or("")
            .to_string();
        for r in refs_of(&source) {
            match r {
                Ref::Unusual => return Err(Problem::Unavailable(Why::UnusualLayout)),
                Ref::Include(p) => {
                    if let Some(target) = normal(&join(&here, &p)) {
                        if files.contains_key(&target) {
                            stack.push((target, true));
                        }
                    }
                }
                Ref::Mod {
                    name,
                    path: attr,
                    inline,
                } => {
                    if let Some(p) = attr {
                        if !inline.is_empty() {
                            return Err(Problem::Unavailable(Why::UnusualLayout));
                        }
                        if let Some(target) = normal(&join(&here, &p)) {
                            if files.contains_key(&target) {
                                stack.push((target, true));
                            }
                        }
                        continue;
                    }
                    let mut base = if mod_rs {
                        here.clone()
                    } else {
                        join(&here, &stem)
                    };
                    for n in &inline {
                        base = join(&base, n);
                    }
                    let flat = join(&base, &format!("{name}.rs"));
                    let nested = join(&base, &format!("{name}/mod.rs"));
                    if files.contains_key(&flat) {
                        stack.push((flat, false));
                    }
                    if files.contains_key(&nested) {
                        stack.push((nested, true));
                    }
                }
            }
        }
    }
    if !seen.contains(entry) {
        return Err(Problem::Unavailable(Why::NotInCrate));
    }
    Ok(RustCrate {
        crate_name: manifest.crate_name,
        edition: manifest.edition,
        root,
        sample: seen.into_iter().collect(),
        dev_dependencies: manifest.dev_dependencies,
        dir,
    })
}

/// The minor version in the answer of `rustc --version` (`rustc 1.93.1 (01f6ddf75 2026-02-11)` is 93), or nothing if the answer is
/// anything else. Read strictly: it is shown to the owner.
pub fn rustc_minor(text: &str) -> Option<u32> {
    let line = text.strip_suffix('\n').unwrap_or(text);
    let rest = line.strip_prefix("rustc 1.")?;
    let version = rest.split_whitespace().next()?;
    let mut parts = version.split('.');
    let (minor, patch) = (parts.next()?, parts.next()?);
    let digits = |p: &str| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit());
    // a patch may carry a pre-release tag (`1.95.0-nightly`); the minor never does
    let patch_number = patch.split('-').next().unwrap_or("");
    (digits(minor) && digits(patch_number) && line.len() <= 120 && !line.contains('\n'))
        .then(|| minor.parse().ok())
        .flatten()
}

/// Which of the two builds a failure came from: the project's crate, or the reproducer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Crate,
    Script,
}

/// What a failed build means. The wording is fixed; the only text taken from the compiler is a plain crate name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildProblem {
    /// The script names a crate that is not available.
    Package(String),
    /// The environment, or the project, cannot be built here: not the script's fault.
    Environment(&'static str),
}

pub const LINKER_SENTENCE: &str = "the Rust compiler in the sandbox has no C linker it can use (gcc is needed under /usr/bin), so this cannot be checked here";
pub const EDITION_SENTENCE: &str = "the Rust compiler in the sandbox is too old for the edition the crate asks for, so this cannot be checked here";
pub const CRATE_SENTENCE: &str = "the project's crate does not build in this environment (it may need packages, features, a build step or a newer compiler), so this cannot be checked here";
pub const MODULE_SENTENCE: &str = "a file the crate loads is not in the sample (a module or an included file), so this cannot be checked here";

/// The first line `KK_BUILD_FAILED <stage>` the build wrapper writes, and the rest of its error stream.
pub fn build_failure(stderr: &str) -> Option<(Stage, &str)> {
    let (first, rest) = stderr.split_once('\n').unwrap_or((stderr, ""));
    match first.trim_end() {
        "KK_BUILD_FAILED crate" => Some((Stage::Crate, rest)),
        "KK_BUILD_FAILED script" => Some((Stage::Script, rest)),
        _ => None,
    }
}

/// A plain crate name from `can't find crate for `name``, or nothing.
fn missing_crate(stderr: &str) -> Option<String> {
    for line in stderr.lines() {
        if let Some(at) = line.find("can't find crate for `") {
            let rest = &line[at + "can't find crate for `".len()..];
            if let Some((name, _)) = rest.split_once('`') {
                if is_ident(name) {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

pub fn classify_build_failure(stage: Stage, stderr: &str) -> Option<BuildProblem> {
    if stderr.contains("linker `") && stderr.contains("not found") {
        return Some(BuildProblem::Environment(LINKER_SENTENCE));
    }
    if stderr.lines().any(|l| {
        l.contains("edition") && (l.contains("is unstable") || l.contains("must be one of"))
    }) {
        return Some(BuildProblem::Environment(EDITION_SENTENCE));
    }
    // a module or an included file the crate names but the sample lacks is the sample's gap; the same words in the script are the
    // script's own mistake
    if stage == Stage::Crate && (stderr.contains("E0583") || stderr.contains("couldn't read")) {
        return Some(BuildProblem::Environment(MODULE_SENTENCE));
    }
    match stage {
        Stage::Crate => Some(BuildProblem::Environment(CRATE_SENTENCE)),
        Stage::Script => missing_crate(stderr).map(BuildProblem::Package),
    }
}

/// Reads a finished run of a crate's own tests (`rustc --test`, then the test program with one thread). Anything not clearly a
/// pass is not a pass: the runner's last summary must be present, add up to the number of tests it announced, agree with the exit
/// status and with the lines that report each test; a test that is ignored is not a pass; a run where nothing passed is no
/// evidence.
pub fn parse_rust_tests(exit: &Exit, stdout: &str) -> TestRun {
    let lines: Vec<&str> = stdout.lines().collect();
    let Some(at) = lines.iter().rposition(|l| l.starts_with("test result: ")) else {
        return TestRun::Failed("no test summary".into());
    };
    let summary = lines[at];
    let (ok_word, rest) = if let Some(r) = summary.strip_prefix("test result: ok. ") {
        (true, r)
    } else if let Some(r) = summary.strip_prefix("test result: FAILED. ") {
        (false, r)
    } else {
        return TestRun::Failed("the test summary is not in a known form".into());
    };
    let mut counts: BTreeMap<&str, u32> = BTreeMap::new();
    let mut parts = rest.split("; ");
    for part in parts.by_ref().take(5) {
        let Some((n, label)) = part.split_once(' ') else {
            return TestRun::Failed("the test summary is incomplete".into());
        };
        let Ok(n) = n.parse::<u32>() else {
            return TestRun::Failed("the test summary is incomplete".into());
        };
        let key = match label {
            "passed" => "passed",
            "failed" => "failed",
            "ignored" => "ignored",
            "measured" => "measured",
            "filtered out" => "filtered",
            _ => return TestRun::Failed("the test summary is incomplete".into()),
        };
        counts.insert(key, n);
    }
    let get = |k: &str| counts.get(k).copied();
    let (Some(passed), Some(failed), Some(ignored), Some(measured), Some(_)) = (
        get("passed"),
        get("failed"),
        get("ignored"),
        get("measured"),
        get("filtered"),
    ) else {
        return TestRun::Failed("the test summary is incomplete".into());
    };
    let announced = lines[..at].iter().rev().find_map(|l| {
        l.strip_prefix("running ")
            .and_then(|r| r.strip_suffix(" tests").or_else(|| r.strip_suffix(" test")))
            .and_then(|n| n.parse::<u32>().ok())
    });
    let Some(announced) = announced else {
        return TestRun::Failed("the test run did not announce how many tests it ran".into());
    };
    if announced != passed + failed + ignored + measured {
        return TestRun::Failed("the test summary does not add up".into());
    }
    if ok_word != (failed == 0) {
        return TestRun::Failed("the test summary contradicts itself".into());
    }
    let reported_ok = lines[..at]
        .iter()
        .filter(|l| l.starts_with("test ") && l.ends_with(" ... ok"))
        .count() as u32;
    if failed == 0 && reported_ok != passed {
        return TestRun::Failed("the lines that report each test do not match the summary".into());
    }
    if failed > 0 {
        return TestRun::Failed(format!("{failed} test(s) failed"));
    }
    if *exit != Exit::Code(0) {
        return TestRun::Failed(format!("the test run ended with {exit:?}"));
    }
    if passed == 0 {
        return TestRun::NoTests;
    }
    TestRun::Passed(passed)
}

/// Whether the tokens inside `#[ ... ]` mark a test: `test`, `bench`, or a `cfg` that needs `test` (so `cfg(not(test))`, which is
/// production-only code, is not one).
fn attr_is_test(attr: &[Tok]) -> bool {
    let word = |i: usize, w: &str| attr.get(i) == Some(&Tok::Ident(w.to_string()));
    match attr.first() {
        Some(Tok::Ident(k)) if k == "test" || k == "bench" => true,
        Some(Tok::Ident(k)) if k == "cfg" || k == "cfg_attr" => (0..attr.len()).any(|i| {
            word(i, "test")
                && !(i >= 2
                    && word(i - 2, "not")
                    && attr.get(i - 1) == Some(&Tok::Punct('('))
                    && attr.get(i + 1) == Some(&Tok::Punct(')')))
        }),
        _ => false,
    }
}

/// The byte ranges of a source file that are tests: every item that carries `#[test]`, `#[bench]` or a `#[cfg(...)]` that mentions
/// `test`, from its attribute to the end of its braced body (or to its `;` for a module declared in another file). Tests in Rust
/// usually live in the file they test, so a fix is judged by whether it touches one of these.
pub fn test_regions(source: &str) -> Vec<(usize, usize)> {
    let t = spanned(source);
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < t.len() {
        if t[i].0 != Tok::Punct('#') {
            i += 1;
            continue;
        }
        let start = t[i].1;
        let mut j = i + 1;
        if t.get(j).map(|x| &x.0) == Some(&Tok::Punct('!')) {
            j += 1;
        }
        if t.get(j).map(|x| &x.0) != Some(&Tok::Punct('[')) {
            i += 1;
            continue;
        }
        let mut d = 1;
        let attr_start = j + 1;
        j += 1;
        while j < t.len() && d > 0 {
            match t[j].0 {
                Tok::Punct('[') => d += 1,
                Tok::Punct(']') => d -= 1,
                _ => {}
            }
            j += 1;
        }
        let attr: Vec<Tok> = t[attr_start..j.saturating_sub(1).max(attr_start)]
            .iter()
            .map(|x| x.0.clone())
            .collect();
        if !attr_is_test(&attr) {
            i = j;
            continue;
        }
        // an inner attribute (`#![cfg(test)]`) makes the whole file a test
        if t[i + 1].0 == Tok::Punct('!') {
            return vec![(0, source.len())];
        }
        // skip the attributes that follow, then the item up to its `;` or through its braces
        let mut k = j;
        let mut end = t.get(j.saturating_sub(1)).map_or(start, |x| x.2);
        let mut depth = 0i32;
        while k < t.len() {
            match t[k].0 {
                Tok::Punct('{') => depth += 1,
                Tok::Punct('}') => {
                    depth -= 1;
                    if depth <= 0 {
                        end = t[k].2;
                        k += 1;
                        break;
                    }
                }
                Tok::Punct(';') if depth == 0 => {
                    end = t[k].2;
                    k += 1;
                    break;
                }
                _ => {}
            }
            end = t[k].2;
            k += 1;
        }
        out.push((start, end));
        i = k;
    }
    out
}

/// Whether the part of `original` that a change replaces overlaps a test, or the change itself brings in a test attribute or a
/// module named `tests`. The change must name text that appears exactly once; if it does not, it is refused here too.
pub fn change_touches_tests(original: &str, old_text: &str, new_text: &str) -> bool {
    if touches_inline_tests(new_text) {
        return true;
    }
    let mut from = 0;
    let mut found = Vec::new();
    while let Some(i) = original[from..].find(old_text) {
        found.push(from + i);
        from += i + original[from + i..]
            .chars()
            .next()
            .map_or(1, char::len_utf8);
        if found.len() > 1 {
            break;
        }
    }
    let [at] = found.as_slice() else {
        return touches_inline_tests(old_text);
    };
    let (a, z) = (*at, *at + old_text.len());
    touches_inline_tests(old_text) || test_regions(original).iter().any(|(s, e)| a < *e && z > *s)
}

/// Whether a source file declares a unit test (`#[test]`).
pub fn has_inline_tests(text: &str) -> bool {
    tokens(text)
        .windows(3)
        .any(|w| matches!(w, [Tok::Punct('#'), Tok::Punct('['), Tok::Ident(k)] if k == "test"))
}

/// Whether the text of a change touches tests that live in the same file as the code: `#[test]`, `#[cfg(test)]` or a module named
/// `tests`. A fix that edits the tests it must pass proves nothing, and for Rust those tests are usually in the file being fixed.
pub fn touches_inline_tests(text: &str) -> bool {
    let t = tokens(text);
    let mut i = 0;
    while i < t.len() {
        if t[i] == Tok::Punct('#') {
            let mut j = i + 1;
            if t.get(j) == Some(&Tok::Punct('!')) {
                j += 1;
            }
            if t.get(j) == Some(&Tok::Punct('[')) {
                let mut d = 1;
                let start = j + 1;
                j += 1;
                while j < t.len() && d > 0 {
                    match t[j] {
                        Tok::Punct('[') => d += 1,
                        Tok::Punct(']') => d -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                if attr_is_test(&t[start..j.saturating_sub(1).max(start)]) {
                    return true;
                }
            }
        }
        if t[i] == Tok::Ident("mod".into()) {
            if let Some(Tok::Ident(n)) = t.get(i + 1) {
                if n == "tests" || n == "test" {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod inline {
    use super::*;

    #[test]
    fn tokens_skip_comments_and_literals_but_keep_strings_as_strings() {
        let t = tokens("// mod aaa;\n/* mod bbb; /* mod ccc; */ */ mod ddd; let s = \"mod eee;\"; let q = 'x'; fn f<'a>() {}");
        let idents: Vec<_> = t
            .iter()
            .filter_map(|x| {
                if let Tok::Ident(i) = x {
                    Some(i.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(idents.contains(&"ddd"));
        for hidden in ["aaa", "bbb", "ccc", "eee", "x"] {
            assert!(!idents.contains(&hidden), "{hidden}");
        }
        assert!(t.contains(&Tok::Str("mod eee;".into())));
    }
}
