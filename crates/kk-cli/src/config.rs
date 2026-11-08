//! `config.toml` in the data folder, and `kaku config [show|path|check|init]`.
//!
//! The settings are a closed set that only change how things are shown to the owner (`SETTINGS`); no setting can touch approval, the
//! model route, policy, what leaves the computer or updates. An unknown key, a wrong type or an unknown value is an error that names
//! the key, every error is listed at once, and a file with any error is not applied at all. See docs/engineering-decisions/config-file.md.

use serde_json::{json, Value as Json};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const FILE: &str = "config.toml";
/// A configuration file larger than this is refused: it is a handful of lines.
pub const MAX_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Choice(&'static [&'static str]),
    Bool,
}

pub struct Setting {
    pub name: &'static str,
    pub kind: Kind,
    pub default: &'static str,
    pub what: &'static str,
}

/// Every setting there is. Each one is bound to behaviour that exists, and each one only changes what is shown.
pub const SETTINGS: &[Setting] = &[
    Setting {
        name: "notify",
        kind: Kind::Choice(&["auto", "bell", "off"]),
        default: "auto",
        what: "how you are told something finished: a desktop notification where the terminal shows one, else the bell (auto); the bell only (bell); nothing (off)",
    },
    Setting {
        name: "color",
        kind: Kind::Choice(&["auto", "never"]),
        default: "auto",
        what: "colour in the terminal workspace; never turns it off (meaning never depends on colour)",
    },
    Setting {
        name: "quiet",
        kind: Kind::Bool,
        default: "false",
        what: "start with only the end of a review shown instead of each step, as /quiet does",
    },
    Setting {
        name: "linear",
        kind: Kind::Bool,
        default: "false",
        what: "start in the plain-line mode (no drawing over the screen) for a screen reader or a pipe",
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Text(String),
    Bool(bool),
}

impl Value {
    pub fn show(&self) -> String {
        match self {
            Value::Text(t) => t.clone(),
            Value::Bool(b) => b.to_string(),
        }
    }

    fn json(&self) -> Json {
        match self {
            Value::Text(t) => json!(t),
            Value::Bool(b) => json!(b),
        }
    }
}

/// A file that parsed and checked out: only known settings with allowed values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct File {
    pub values: BTreeMap<&'static str, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The setting it is about, or where in the file for one that is not about a setting (`line 3, column 5`).
    pub at: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loaded {
    Absent,
    Valid(File),
    Invalid(Vec<Problem>),
}

pub fn path_in(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// Text for the screen: a control character or a direction mark becomes `?`.
pub fn clean(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control()
                || matches!(
                    c,
                    '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}'
                )
            {
                '?'
            } else {
                c
            }
        })
        .collect()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb))
                .min(row[j] + 1)
                .min(row[j + 1] + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// The known setting nearest to a mistyped one, when one is close enough to be a likely typo.
pub fn nearest(key: &str) -> Option<&'static str> {
    let lower = key.to_ascii_lowercase();
    SETTINGS
        .iter()
        .map(|s| (edit_distance(&lower, s.name), s.name))
        .filter(|(d, _)| *d <= 2)
        .min()
        .map(|(_, name)| name)
}

fn describe(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => format!(
            "the text \"{}\"",
            clean(&s.chars().take(40).collect::<String>())
        ),
        toml::Value::Integer(i) => format!("the number {i}"),
        toml::Value::Float(f) => format!("the number {f}"),
        toml::Value::Boolean(b) => format!("{b}"),
        toml::Value::Datetime(_) => "a date".to_string(),
        toml::Value::Array(_) => "a list".to_string(),
        toml::Value::Table(_) => "a table".to_string(),
    }
}

fn known_list() -> String {
    SETTINGS
        .iter()
        .map(|s| s.name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Reads and checks the text of a configuration file. Every problem is returned, not just the first.
pub fn validate(text: &str) -> Result<File, Vec<Problem>> {
    let table: toml::Table = match text.parse() {
        Ok(t) => t,
        Err(e) => {
            let at = match e.span() {
                Some(span) => {
                    let before = &text[..span.start.min(text.len())];
                    let line = before.matches('\n').count() + 1;
                    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
                    format!("line {line}, column {col}")
                }
                None => "the file".to_string(),
            };
            return Err(vec![Problem {
                at,
                message: clean(e.message()),
            }]);
        }
    };
    let mut problems = Vec::new();
    let mut file = File::default();
    for (key, value) in &table {
        let Some(setting) = SETTINGS.iter().find(|s| s.name == key) else {
            let hint = nearest(key).map_or(String::new(), |n| format!(" (did you mean `{n}`?)"));
            problems.push(Problem {
                at: clean(key),
                message: format!("unknown setting{hint}. The settings are: {}", known_list()),
            });
            continue;
        };
        match (setting.kind, value) {
            (Kind::Bool, toml::Value::Boolean(b)) => {
                file.values.insert(setting.name, Value::Bool(*b));
            }
            (Kind::Choice(allowed), toml::Value::String(s)) if allowed.contains(&s.as_str()) => {
                file.values.insert(setting.name, Value::Text(s.clone()));
            }
            (Kind::Bool, other) => problems.push(Problem {
                at: setting.name.into(),
                message: format!("must be true or false, not {}", describe(other)),
            }),
            (Kind::Choice(allowed), other) => problems.push(Problem {
                at: setting.name.into(),
                message: format!(
                    "must be one of {}, not {}",
                    allowed
                        .iter()
                        .map(|a| format!("\"{a}\""))
                        .collect::<Vec<_>>()
                        .join(", "),
                    describe(other)
                ),
            }),
        }
    }
    if problems.is_empty() {
        Ok(file)
    } else {
        Err(problems)
    }
}

/// Reads the file in the data folder: absent, valid, or invalid with the reasons.
pub fn load(dir: &Path) -> Loaded {
    let path = path_in(dir);
    let Ok(meta) = std::fs::metadata(&path) else {
        return Loaded::Absent;
    };
    let problem = |m: &str| {
        Loaded::Invalid(vec![Problem {
            at: "the file".into(),
            message: m.into(),
        }])
    };
    if !meta.is_file() {
        return problem("is not a regular file");
    }
    if meta.len() > MAX_BYTES {
        return problem(&format!(
            "is larger than {} KiB, which is far more than any setting needs",
            MAX_BYTES / 1024
        ));
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => match validate(&text) {
            Ok(f) => Loaded::Valid(f),
            Err(p) => Loaded::Invalid(p),
        },
        Err(e) => problem(&format!("could not be read as text: {}", e.kind())),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Default,
    File,
    Env(&'static str),
    Flag(&'static str),
}

impl Source {
    pub fn show(&self) -> String {
        match self {
            Source::Default => "the default".into(),
            Source::File => FILE.to_string(),
            Source::Env(n) => format!("the environment ({n})"),
            Source::Flag(n) => format!("the option {n}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    pub name: &'static str,
    pub value: Value,
    pub source: Source,
}

fn default_of(s: &Setting) -> Value {
    match s.kind {
        Kind::Bool => Value::Bool(s.default == "true"),
        Kind::Choice(_) => Value::Text(s.default.to_string()),
    }
}

fn off_word(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "off" | "false" | "no"
    )
}

/// The value of every setting and where it came from: a command-line option, then an environment variable, then the file, then the
/// default. The environment variables keep the meaning they always had.
pub fn resolve(
    file: Option<&File>,
    env: &dyn Fn(&str) -> Option<OsString>,
    args: &[String],
) -> Vec<Effective> {
    SETTINGS
        .iter()
        .map(|s| {
            let from_file = file.and_then(|f| f.values.get(s.name)).cloned();
            let (value, source) = match s.name {
                "notify" => match env("KAKU_NOTIFY") {
                    Some(v) => {
                        let v = v.to_string_lossy().to_string();
                        let word = v.trim().to_ascii_lowercase();
                        let value = if off_word(&word) && !word.is_empty() {
                            "off"
                        } else if word == "bell" {
                            "bell"
                        } else {
                            "auto"
                        };
                        (Value::Text(value.into()), Source::Env("KAKU_NOTIFY"))
                    }
                    None => file_or_default(s, from_file),
                },
                "color" => {
                    if env("NO_COLOR").is_some() {
                        (Value::Text("never".into()), Source::Env("NO_COLOR"))
                    } else {
                        file_or_default(s, from_file)
                    }
                }
                "linear" => {
                    if args.iter().any(|a| a == "--linear") {
                        (Value::Bool(true), Source::Flag("--linear"))
                    } else if let Some(v) = env("KAKU_LINEAR") {
                        (
                            Value::Bool(!off_word(&v.to_string_lossy())),
                            Source::Env("KAKU_LINEAR"),
                        )
                    } else {
                        file_or_default(s, from_file)
                    }
                }
                _ => file_or_default(s, from_file),
            };
            Effective {
                name: s.name,
                value,
                source,
            }
        })
        .collect()
}

fn file_or_default(s: &Setting, from_file: Option<Value>) -> (Value, Source) {
    match from_file {
        Some(v) => (v, Source::File),
        None => (default_of(s), Source::Default),
    }
}

pub fn value_of<'a>(eff: &'a [Effective], name: &str) -> &'a Value {
    &eff.iter()
        .find(|e| e.name == name)
        .expect("a setting that exists")
        .value
}

/// The one line said at the top of the workspace when the file was not applied.
pub fn notice_for(problems: &[Problem]) -> String {
    let first = &problems[0];
    let more = if problems.len() > 1 {
        format!(" and {} more", problems.len() - 1)
    } else {
        String::new()
    };
    format!(
        "{FILE} was not applied, so the defaults are in use: {}: {}{more}. Run `kaku config check` to see each problem.",
        clean(&first.at),
        clean(&first.message)
    )
}

/// What the terminal workspace needs, from the resolved settings and a notice if the file was not applied.
pub fn tui_settings(eff: &[Effective], notice: Option<String>) -> kk_tui::settings::Settings {
    kk_tui::settings::Settings {
        notify: match value_of(eff, "notify") {
            Value::Text(t) if t == "bell" || t == "off" => Some(t.clone()),
            _ => None,
        },
        color: !matches!(value_of(eff, "color"), Value::Text(t) if t == "never"),
        quiet: matches!(value_of(eff, "quiet"), Value::Bool(true)),
        notice,
    }
}

pub fn template() -> String {
    let mut out = String::from(
        "# Kakurega settings. Every line below is commented out, so this file changes nothing until you remove a `#`.\n\
         # These only change how things are shown to you. None of them can change approval, the model, policy or what leaves this computer.\n\
         # A command-line option beats an environment variable, which beats this file, which beats the default.\n\
         # A mistake (an unknown name, a wrong value) is reported by `kaku config check`, and then none of the file is applied.\n\n",
    );
    for s in SETTINGS {
        out.push_str(&format!("# {}\n", s.what));
        let shown = match s.kind {
            Kind::Bool => s.default.to_string(),
            Kind::Choice(allowed) => {
                format!("\"{}\"   # one of: {}", s.default, allowed.join(", "))
            }
        };
        out.push_str(&format!("# {} = {shown}\n\n", s.name));
    }
    out
}

fn show(dir: &Path, args: &[String]) -> u8 {
    let loaded = load(dir);
    let (file, status) = match &loaded {
        Loaded::Absent => (None, "not present, so the defaults are in use".to_string()),
        Loaded::Valid(f) => (
            Some(f),
            format!("applied, {} setting(s) set", f.values.len()),
        ),
        Loaded::Invalid(p) => (
            None,
            format!(
                "NOT APPLIED, {} problem(s): run `kaku config check`",
                p.len()
            ),
        ),
    };
    let eff = resolve(file, &|k| std::env::var_os(k), args);
    if args.iter().any(|a| a == "--json") {
        let v = json!({
            "config": 1,
            "file": path_in(dir).display().to_string(),
            "status": match &loaded { Loaded::Absent => "absent", Loaded::Valid(_) => "applied", Loaded::Invalid(_) => "not_applied" },
            "settings": eff.iter().map(|e| {
                let s = SETTINGS.iter().find(|s| s.name == e.name).expect("a setting that exists");
                json!({"name": e.name, "value": e.value.json(), "source": e.source.show(), "default": s.default})
            }).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return 0;
    }
    println!(
        "Kakurega settings\nfile: {}: {status}\n",
        clean(&path_in(dir).display().to_string())
    );
    for e in &eff {
        let s = SETTINGS
            .iter()
            .find(|s| s.name == e.name)
            .expect("a setting that exists");
        println!(
            "  {:<7} {:<6} from {}",
            e.name,
            e.value.show(),
            e.source.show()
        );
        println!("          {}", s.what);
    }
    0
}

fn check(dir: &Path, args: &[String]) -> u8 {
    let loaded = load(dir);
    if args.iter().any(|a| a == "--json") {
        let v = match &loaded {
            Loaded::Absent => json!({"config": 1, "status": "absent", "problems": []}),
            Loaded::Valid(f) => {
                json!({"config": 1, "status": "ok", "set": f.values.keys().collect::<Vec<_>>(), "problems": []})
            }
            Loaded::Invalid(p) => {
                json!({"config": 1, "status": "invalid", "problems": p.iter().map(|x| json!({"at": clean(&x.at), "message": clean(&x.message)})).collect::<Vec<_>>()})
            }
        };
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return u8::from(matches!(loaded, Loaded::Invalid(_)));
    }
    match loaded {
        Loaded::Absent => {
            println!("No {FILE} in {}: the defaults are in use. `kaku config init` writes a commented template.", clean(&dir.display().to_string()));
            0
        }
        Loaded::Valid(f) => {
            println!(
                "{FILE} is correct. Set: {}.",
                if f.values.is_empty() {
                    "nothing (every line is commented out)".to_string()
                } else {
                    f.values.keys().copied().collect::<Vec<_>>().join(", ")
                }
            );
            0
        }
        Loaded::Invalid(problems) => {
            println!(
                "{FILE} has {} problem(s), so none of it is applied:",
                problems.len()
            );
            for p in &problems {
                println!("  {}: {}", clean(&p.at), clean(&p.message));
            }
            1
        }
    }
}

fn init(dir: &Path) -> u8 {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = path_in(dir);
    if let Err(e) = kk_service::keys::ensure_private_dir(dir) {
        eprintln!("kaku config: cannot make {}: {e}", dir.display());
        return 1;
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        Ok(mut f) => {
            if f.write_all(template().as_bytes()).is_err() {
                eprintln!("kaku config: could not write {}", path.display());
                return 1;
            }
            println!(
                "Wrote {}. Every line is commented out; remove a `#` to use a setting.",
                path.display()
            );
            0
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            eprintln!(
                "kaku config: {} already exists and was not changed.",
                path.display()
            );
            1
        }
        Err(e) => {
            eprintln!(
                "kaku config: could not create {}: {}",
                path.display(),
                e.kind()
            );
            1
        }
    }
}

/// The first word that is not an option or an option's value: options may come before it or after it.
pub fn subcommand(args: &[String]) -> Option<String> {
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if a == "--data-dir" {
            skip = true;
        } else if !a.starts_with('-') {
            return Some(a.clone());
        }
    }
    None
}

pub fn run(args: &[String]) -> u8 {
    let dir = crate::data_dir(args);
    match subcommand(args).as_deref() {
        None | Some("show") => show(&dir, args),
        Some("path") => {
            println!("{}", path_in(&dir).display());
            0
        }
        Some("check") => check(&dir, args),
        Some("init") => init(&dir),
        Some(other) => {
            eprintln!(
                "kaku config: unknown command `{}`. Use show, path, check or init.",
                clean(other)
            );
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fresh(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kk-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k: &str| {
            owned
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| OsString::from(v))
        }
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn get<'a>(eff: &'a [Effective], name: &str) -> &'a Effective {
        eff.iter().find(|e| e.name == name).unwrap()
    }

    #[test]
    fn cf01_the_settings_are_a_closed_set_that_only_changes_how_things_are_shown() {
        let names: Vec<&str> = SETTINGS.iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["notify", "color", "quiet", "linear"]);
        // nothing that names approval, the model route, policy, what leaves the computer, updates, trust or secrets can be a setting
        for forbidden in [
            "approv",
            "route",
            "polic",
            "model",
            "send",
            "release",
            "deliver",
            "update",
            "token",
            "key",
            "secret",
            "permission",
            "trust",
            "label",
            "sandbox",
            "disclos",
            "upload",
            "share",
            "telemetry",
            "plugin",
            "provider",
            "mcp",
            "agent",
            "tool",
            "network",
            "proxy",
        ] {
            for s in SETTINGS {
                assert!(
                    !s.name.contains(forbidden),
                    "`{}` names `{forbidden}`",
                    s.name
                );
                assert!(
                    !s.what.to_lowercase().contains("approve") || s.name == "quiet",
                    "{}",
                    s.name
                );
            }
        }
        for s in SETTINGS {
            assert!(s.what.len() > 20, "{} is described", s.name);
            match s.kind {
                Kind::Choice(allowed) => assert!(allowed.contains(&s.default), "{}", s.name),
                Kind::Bool => assert!(matches!(s.default, "true" | "false"), "{}", s.name),
            }
        }
        // the type the terminal receives has only display fields
        let t = tui_settings(&resolve(None, &env_of(&[]), &[]), None);
        assert_eq!(
            t,
            kk_tui::settings::Settings {
                notify: None,
                color: true,
                quiet: false,
                notice: None
            }
        );
    }

    #[test]
    fn cf02_a_correct_file_is_read_and_a_comment_or_blank_file_sets_nothing() {
        let f = validate(
            "# comment\nnotify = \"bell\"\ncolor = \"never\"\nquiet = true\nlinear = false\n",
        )
        .unwrap();
        assert_eq!(f.values.get("notify"), Some(&Value::Text("bell".into())));
        assert_eq!(f.values.get("color"), Some(&Value::Text("never".into())));
        assert_eq!(f.values.get("quiet"), Some(&Value::Bool(true)));
        assert_eq!(f.values.get("linear"), Some(&Value::Bool(false)));
        assert!(validate("").unwrap().values.is_empty());
        assert!(validate("\n\n# only comments\n").unwrap().values.is_empty());
    }

    #[test]
    fn cf03_every_wrong_type_or_value_is_an_error_that_names_the_setting_and_what_was_found() {
        let cases: &[(&str, &str, &str)] = &[
            ("quiet = \"yes\"", "quiet", "the text \"yes\""),
            ("quiet = 1", "quiet", "the number 1"),
            ("quiet = 1.5", "quiet", "the number 1.5"),
            ("quiet = [true]", "quiet", "a list"),
            ("quiet = { a = 1 }", "quiet", "a table"),
            ("quiet = 1979-05-27", "quiet", "a date"),
            ("linear = \"true\"", "linear", "the text \"true\""),
            ("notify = true", "notify", "true"),
            ("notify = 5", "notify", "the number 5"),
            ("notify = \"loud\"", "notify", "the text \"loud\""),
            ("notify = \"Bell\"", "notify", "the text \"Bell\""),
            ("notify = \"\"", "notify", "the text \"\""),
            ("notify = \"auto \"", "notify", "the text \"auto \""),
            ("color = \"always\"", "color", "the text \"always\""),
            ("color = false", "color", "false"),
        ];
        for (text, at, found) in cases {
            let p = validate(text).unwrap_err();
            assert_eq!(p.len(), 1, "{text}: {p:?}");
            assert_eq!(p[0].at, *at, "{text}");
            assert!(p[0].message.contains(found), "{text}: {}", p[0].message);
            assert!(
                p[0].message.starts_with("must be"),
                "{text}: {}",
                p[0].message
            );
        }
        // a choice error lists the allowed values
        let p = validate("notify = \"x\"").unwrap_err();
        assert!(
            p[0].message.contains("\"auto\", \"bell\", \"off\""),
            "{}",
            p[0].message
        );
        // a long or control-laden value is shortened and cleaned
        let p = validate(&format!("notify = \"{}\"", "a".repeat(200))).unwrap_err();
        assert!(p[0].message.len() < 160, "{}", p[0].message);
        let p = validate("notify = \"x\\u001b[2Jy\"").unwrap_err();
        assert!(!p[0].message.contains('\u{1b}'));
    }

    #[test]
    fn cf04_every_problem_is_listed_at_once_and_one_mistake_means_none_of_the_file_is_applied() {
        let text =
            "notify = \"bell\"\nquiet = \"yes\"\ncolour = \"never\"\nlinear = 3\n[ui]\nx = 1\n";
        let p = validate(text).unwrap_err();
        let at: Vec<&str> = p.iter().map(|x| x.at.as_str()).collect();
        assert_eq!(at.len(), 4, "{p:?}");
        for expected in ["quiet", "colour", "linear", "ui"] {
            assert!(at.contains(&expected), "{at:?}");
        }
        assert!(!at.contains(&"notify"), "the correct line is not a problem");
        // all or nothing: the correct `notify` line is not applied either, because validate gives no File at all
        assert!(validate(text).is_err());
    }

    #[test]
    fn cf05_an_unknown_setting_is_named_with_the_nearest_known_one_when_it_is_a_likely_typo() {
        for (typo, near) in [
            ("quite", Some("quiet")),
            ("Notify", Some("notify")),
            ("colour", Some("color")),
            ("linearr", Some("linear")),
            ("QUIET", Some("quiet")),
            ("notfy", Some("notify")),
            ("zzzzzz", None),
            ("approve", None),
            ("update", None),
            ("model", None),
        ] {
            assert_eq!(nearest(typo), near, "{typo}");
            let p = validate(&format!("{typo} = true")).unwrap_err();
            assert_eq!(p[0].at, typo);
            assert!(
                p[0].message.starts_with("unknown setting"),
                "{}",
                p[0].message
            );
            match near {
                Some(n) => assert!(
                    p[0].message.contains(&format!("did you mean `{n}`?")),
                    "{}",
                    p[0].message
                ),
                None => assert!(!p[0].message.contains("did you mean"), "{}", p[0].message),
            }
            assert!(p[0].message.contains("notify, color, quiet, linear"));
        }
        // a table or dotted key for something that is not a setting
        assert_eq!(validate("[ui]\ncolor = \"never\"").unwrap_err()[0].at, "ui");
        assert_eq!(validate("ui.color = \"never\"").unwrap_err()[0].at, "ui");
        // a setting spelled as a table is a type error, not an unknown name
        let p = validate("[notify]\nx = 1").unwrap_err();
        assert!(p[0].message.contains("a table"), "{}", p[0].message);
    }

    #[test]
    fn cf06_a_syntax_error_is_one_problem_at_the_right_line_and_column_and_never_a_panic() {
        let cases: &[(&str, usize)] = &[
            ("quiet = \n", 1),
            ("notify = \"unclosed\n", 1),
            ("\n\nquiet true\n", 3),
            ("quiet = true\nquiet = false\n", 2),
            ("= true\n", 1),
            ("quiet = true\n[[[\n", 2),
            ("\"é\" = 1\nnotify = \n", 2),
            ("quiet = true # ééé\nlinear = \n", 2),
        ];
        for (text, line) in cases {
            let p = validate(text).unwrap_err();
            assert_eq!(p.len(), 1, "{text:?}: {p:?}");
            assert!(
                p[0].at.starts_with(&format!("line {line}, column ")),
                "{text:?}: {}",
                p[0].at
            );
            assert!(!p[0].message.is_empty());
        }
        // the column counts characters, not bytes: the stray `b` is the 15th character of its line (the 17th byte)
        let p = validate("notify = \"éé\" b\n").unwrap_err();
        assert_eq!(p[0].at, "line 1, column 15");
        // odd bytes in a string that is still valid text
        assert!(validate("quiet = true\u{0}\n").is_err());
        let _ = validate("\u{feff}quiet = true\n");
        let _ = validate(&"[".repeat(5000));
        let _ = validate(&"a = ".repeat(10_000));
    }

    #[test]
    fn cf07_loading_the_file_handles_absent_directories_big_files_bad_bytes_and_unreadable_files() {
        let d = fresh("load");
        assert_eq!(load(&d), Loaded::Absent);
        let path = path_in(&d);
        std::fs::write(&path, "quiet = true\n").unwrap();
        assert!(matches!(load(&d), Loaded::Valid(f) if f.values.len() == 1));
        // a link to a correct file is read
        let real = d.join("real.toml");
        std::fs::write(&real, "linear = true\n").unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&real, &path).unwrap();
        assert!(
            matches!(load(&d), Loaded::Valid(f) if f.values.get("linear") == Some(&Value::Bool(true)))
        );
        std::fs::remove_file(&path).unwrap();
        // a folder where the file should be
        std::fs::create_dir(&path).unwrap();
        assert!(
            matches!(load(&d), Loaded::Invalid(p) if p[0].message.contains("not a regular file"))
        );
        std::fs::remove_dir(&path).unwrap();
        // too large
        std::fs::write(&path, vec![b'#'; (MAX_BYTES + 1) as usize]).unwrap();
        assert!(matches!(load(&d), Loaded::Invalid(p) if p[0].message.contains("64 KiB")));
        std::fs::write(&path, vec![b'#'; MAX_BYTES as usize]).unwrap();
        assert!(
            matches!(load(&d), Loaded::Valid(_)),
            "exactly the limit is fine"
        );
        // not text
        std::fs::write(&path, [0xff, 0xfe, 0x00, 0x80]).unwrap();
        assert!(
            matches!(load(&d), Loaded::Invalid(p) if p[0].message.contains("could not be read as text"))
        );
        // unreadable (a process allowed to read anyway, root, cannot show this)
        std::fs::write(&path, "quiet = true\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read_to_string(&path).is_err() {
            assert!(
                matches!(load(&d), Loaded::Invalid(p) if p[0].message.contains("could not be read"))
            );
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cf08_a_flag_beats_the_environment_which_beats_the_file_which_beats_the_default_for_every_setting(
    ) {
        let file = validate("notify = \"bell\"\ncolor = \"never\"\nquiet = true\nlinear = true\n")
            .unwrap();
        let none = env_of(&[]);
        // the file over the defaults
        let eff = resolve(Some(&file), &none, &[]);
        for (n, v) in [
            ("notify", Value::Text("bell".into())),
            ("color", Value::Text("never".into())),
            ("quiet", Value::Bool(true)),
            ("linear", Value::Bool(true)),
        ] {
            assert_eq!(
                (&get(&eff, n).value, &get(&eff, n).source),
                (&v, &Source::File),
                "{n}"
            );
        }
        // the defaults when there is no file
        let eff = resolve(None, &none, &[]);
        for s in SETTINGS {
            assert_eq!(get(&eff, s.name).source, Source::Default);
            assert_eq!(get(&eff, s.name).value.show(), s.default);
        }
        // KAKU_NOTIFY keeps its old meaning
        for (v, want) in [
            ("off", "off"),
            ("0", "off"),
            ("false", "off"),
            ("no", "off"),
            ("OFF", "off"),
            (" No ", "off"),
            ("bell", "bell"),
            ("BELL", "bell"),
            ("auto", "auto"),
            ("1", "auto"),
            ("", "auto"),
            ("desktop", "auto"),
        ] {
            let eff = resolve(Some(&file), &env_of(&[("KAKU_NOTIFY", v)]), &[]);
            assert_eq!(
                get(&eff, "notify").value,
                Value::Text(want.into()),
                "KAKU_NOTIFY={v:?}"
            );
            assert_eq!(get(&eff, "notify").source, Source::Env("KAKU_NOTIFY"));
        }
        // NO_COLOR present (even empty) wins over the file, as it always did; absent leaves the file
        for v in ["1", "", "anything"] {
            let eff = resolve(None, &env_of(&[("NO_COLOR", v)]), &[]);
            assert_eq!(
                (&get(&eff, "color").value, &get(&eff, "color").source),
                (&Value::Text("never".into()), &Source::Env("NO_COLOR")),
                "{v:?}"
            );
        }
        let only_auto = validate("color = \"auto\"").unwrap();
        let eff = resolve(Some(&only_auto), &env_of(&[("NO_COLOR", "1")]), &[]);
        assert_eq!(
            get(&eff, "color").value,
            Value::Text("never".into()),
            "the environment beats a file that says auto"
        );
        // linear: the option beats the environment beats the file, and the environment can say no
        let off_file = validate("linear = false").unwrap();
        let eff = resolve(Some(&off_file), &none, &args(&["--linear"]));
        assert_eq!(
            (&get(&eff, "linear").value, &get(&eff, "linear").source),
            (&Value::Bool(true), &Source::Flag("--linear"))
        );
        let eff = resolve(Some(&off_file), &env_of(&[("KAKU_LINEAR", "1")]), &[]);
        assert_eq!(
            (&get(&eff, "linear").value, &get(&eff, "linear").source),
            (&Value::Bool(true), &Source::Env("KAKU_LINEAR"))
        );
        for v in ["0", "off", "false", "no", "", "OFF"] {
            let eff = resolve(Some(&file), &env_of(&[("KAKU_LINEAR", v)]), &[]);
            assert_eq!(
                get(&eff, "linear").value,
                Value::Bool(false),
                "KAKU_LINEAR={v:?} says no even though the file says yes"
            );
        }
        let eff = resolve(
            Some(&file),
            &env_of(&[("KAKU_LINEAR", "0")]),
            &args(&["--linear"]),
        );
        assert_eq!(
            get(&eff, "linear").value,
            Value::Bool(true),
            "the option beats the environment"
        );
        // quiet has no environment variable and no option: only the file and the default
        let eff = resolve(
            Some(&file),
            &env_of(&[("KAKU_QUIET", "0")]),
            &args(&["--quiet"]),
        );
        assert_eq!(get(&eff, "quiet").source, Source::File);
    }

    #[test]
    fn cf09_the_terminal_gets_the_notify_color_and_quiet_values_and_the_notice() {
        let eff =
            |f: &str, e: &[(&str, &str)]| resolve(Some(&validate(f).unwrap()), &env_of(e), &[]);
        let t = tui_settings(
            &eff("notify = \"bell\"\nquiet = true\ncolor = \"never\"", &[]),
            Some("n".into()),
        );
        assert_eq!(
            (t.notify.as_deref(), t.color, t.quiet, t.notice.as_deref()),
            (Some("bell"), false, true, Some("n"))
        );
        let t = tui_settings(&eff("notify = \"off\"", &[]), None);
        assert_eq!(
            (t.notify.as_deref(), t.color, t.quiet, t.notice),
            (Some("off"), true, false, None)
        );
        let t = tui_settings(&eff("", &[]), None);
        assert_eq!(t.notify, None, "auto is no setting at all");
        // the environment variable's value reaches the terminal as it always did
        let t = tui_settings(&eff("", &[("KAKU_NOTIFY", "OFF")]), None);
        assert_eq!(t.notify.as_deref(), Some("off"));
    }

    #[test]
    fn cf10_the_notice_is_one_clean_line_that_names_the_first_problem_and_counts_the_rest() {
        let one = notice_for(&[Problem {
            at: "quite".into(),
            message: "unknown setting".into(),
        }]);
        assert!(
            one.starts_with(
                "config.toml was not applied, so the defaults are in use: quite: unknown setting."
            ),
            "{one}"
        );
        assert!(!one.contains("more") && one.contains("kaku config check"));
        let two = notice_for(&[
            Problem {
                at: "a".into(),
                message: "x".into(),
            },
            Problem {
                at: "b".into(),
                message: "y".into(),
            },
            Problem {
                at: "c".into(),
                message: "z".into(),
            },
        ]);
        assert!(two.contains("a: x and 2 more."), "{two}");
        let evil = notice_for(&[Problem {
            at: "k\u{1b}[2J".into(),
            message: "m\u{202e}\u{7}".into(),
        }]);
        for bad in ['\u{1b}', '\u{202e}', '\u{7}', '\n'] {
            assert!(!evil.contains(bad), "{evil:?}");
        }
    }

    #[test]
    fn cf11_the_template_is_correct_changes_nothing_and_its_default_lines_are_the_real_defaults() {
        let t = template();
        let f = validate(&t).unwrap();
        assert!(f.values.is_empty(), "every line is commented out");
        for s in SETTINGS {
            assert!(
                t.contains(&format!("# {} = ", s.name)) && t.contains(s.what),
                "{}",
                s.name
            );
            if let Kind::Choice(allowed) = s.kind {
                for a in allowed {
                    assert!(t.contains(a), "{} lists {a}", s.name);
                }
            }
        }
        // remove the comment mark from every setting line: it is a correct file that says exactly the defaults
        let live: String = t
            .lines()
            .map(|l| match l.strip_prefix("# ") {
                Some(rest)
                    if SETTINGS
                        .iter()
                        .any(|s| rest.starts_with(&format!("{} = ", s.name))) =>
                {
                    rest.to_string()
                }
                _ => l.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        let f = validate(&live).unwrap();
        assert_eq!(f.values.len(), SETTINGS.len());
        let eff = resolve(Some(&f), &env_of(&[]), &[]);
        for s in SETTINGS {
            assert_eq!(get(&eff, s.name).value.show(), s.default, "{}", s.name);
        }
    }

    #[test]
    fn cf12_init_writes_an_owner_only_template_makes_the_folder_owner_only_and_never_overwrites() {
        let base = fresh("init");
        let dir = base.join("data");
        assert_eq!(init(&dir), 0);
        let p = path_in(&dir);
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700,
            "a new folder is owner only"
        );
        assert_eq!(std::fs::read_to_string(&p).unwrap(), template());
        // it is not overwritten, and what someone wrote there stays
        std::fs::write(&p, "quiet = true\n").unwrap();
        assert_eq!(init(&dir), 1);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "quiet = true\n");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn cf13_the_subcommand_is_the_first_word_that_is_not_an_option_or_an_options_value() {
        assert_eq!(
            subcommand(&args(&["--data-dir", "/x", "show"])).as_deref(),
            Some("show")
        );
        assert_eq!(
            subcommand(&args(&["check", "--data-dir", "/x"])).as_deref(),
            Some("check")
        );
        assert_eq!(
            subcommand(&args(&["--json", "--data-dir", "/show", "path"])).as_deref(),
            Some("path"),
            "a folder named show is a value, not a command"
        );
        assert_eq!(subcommand(&args(&["--linear"])), None);
        assert_eq!(subcommand(&args(&[])), None);
        assert_eq!(subcommand(&args(&["--data-dir"])), None);
    }
}
