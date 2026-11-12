//! Signed method packs: a publisher's methods, checked against a key the owner pinned.
//!
//! A pack is one signed JSON file. It holds methods and, optionally, outside tool programs it suggests. No tool, route,
//! credential or permission field exists in a method, and a pack that tries to carry one is refused whole. Every method
//! that comes out is still unapproved, and a suggested program is only a suggestion: it names a program, its arguments and
//! the names of the variables it needs, never a value, and nothing is added or run until the owner adds it themselves.

pub mod catalog;

use kk_evidence::dsse;
use kk_evidence::keys::{KeyId, Signer, TrustedKeys};
use kk_methods::{Draft, Method};
use serde_json::Value;
use std::collections::BTreeSet;

pub const PACK_TYPE: &str = "application/vnd.kakurega.pack.v1+json";
pub const MAX_PACK_BYTES: usize = 256 * 1024;
pub const MAX_METHODS: usize = 20;
/// The most outside tool programs one pack may suggest, and the bounds on each.
pub const MAX_SERVERS: usize = 5;
const MAX_ARGS: usize = 32;
const MAX_ARG_CHARS: usize = 512;
const MAX_ENV: usize = 16;
const MAX_WHY_CHARS: usize = 300;
/// Variables a suggestion may not ask for: each changes what program runs or what it loads, whatever value is filled in.
const ENV_NEVER: &[&str] = &[
    "PATH",
    "PYTHONPATH",
    "PYTHONSTARTUP",
    "NODE_OPTIONS",
    "NODE_PATH",
    "BASH_ENV",
    "ENV",
    "SHELL",
    "HOME",
    "PERL5OPT",
    "RUBYOPT",
];
const ENV_NEVER_PREFIX: &[&str] = &["LD_", "DYLD_"];

/// An outside tool program a pack suggests. The owner sees exactly this and decides; nothing in it is filled in or run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestedServer {
    pub name: String,
    /// An absolute path, shown as it is; the owner's own add checks that the file is there.
    pub program: String,
    pub args: Vec<String>,
    /// The names of the variables it needs. Their values are the owner's to fill in.
    pub env: Vec<String>,
    /// The publisher's words on why it is suggested.
    pub why: String,
}
const MAX_PACK_NAME: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// `major.minor.patch`: digits only, no leading zeros, nothing else.
    pub fn parse(text: &str) -> Option<Version> {
        let mut parts = text.split('.');
        let mut n = [0u32; 3];
        for slot in &mut n {
            let p = parts.next()?;
            let ok = !p.is_empty()
                && p.len() <= 4
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p == "0" || !p.starts_with('0'));
            if !ok {
                return None;
            }
            *slot = p.parse().ok()?;
        }
        parts.next().is_none().then_some(Version(n[0], n[1], n[2]))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    TooLarge,
    /// Not signed, signed by a key that is not pinned, altered, or the wrong kind of file.
    NotSigned(String),
    Shape(&'static str),
    /// One method does not read as a valid method: its name and why.
    Method(String, String),
    Expired,
    Rollback,
}

impl Refusal {
    pub fn sentence(&self) -> String {
        match self {
            Refusal::TooLarge => "the pack is too large".into(),
            Refusal::NotSigned(_) => {
                "the pack is not signed by a publisher you trust, or was changed after signing"
                    .into()
            }
            Refusal::Shape(w) => (*w).to_string(),
            Refusal::Method(n, w) => format!("the method \"{n}\" is refused: {w}"),
            Refusal::Expired => "the pack has expired".into(),
            Refusal::Rollback => "the pack is older than one you already imported".into(),
        }
    }
}

pub struct Context<'a> {
    pub trusted: &'a TrustedKeys,
    pub now: i64,
    /// The highest version of the pack with this name that this publisher (by key id) had imported before, if any.
    pub seen: &'a dyn Fn(&str, &str) -> Option<Version>,
}

#[derive(Debug, Clone)]
pub struct Checked {
    pub key_id: KeyId,
    pub name: String,
    pub version: Version,
    pub not_after: i64,
    pub methods: Vec<Method>,
    pub servers: Vec<SuggestedServer>,
}

fn pack_name_ok(n: &str) -> bool {
    let mut c = n.chars();
    c.next().is_some_and(|f| f.is_ascii_alphanumeric())
        && n.len() <= MAX_PACK_NAME
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

struct Parsed {
    name: String,
    version: Version,
    not_after: i64,
    methods: Vec<Method>,
    servers: Vec<SuggestedServer>,
}

fn plain_text(s: &str, max: usize) -> bool {
    s.chars().count() <= max && !s.chars().any(char::is_control)
}

fn env_name_ok(n: &str) -> bool {
    let mut c = n.chars();
    c.next().is_some_and(|f| f.is_ascii_uppercase() || f == '_')
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !ENV_NEVER.contains(&n)
        && !ENV_NEVER_PREFIX.iter().any(|p| n.starts_with(p))
}

fn parse_servers(v: Option<&Value>) -> Result<Vec<SuggestedServer>, Refusal> {
    let Some(v) = v else { return Ok(Vec::new()) };
    let items = v
        .as_array()
        .ok_or(Refusal::Shape("the suggested tool programs are not a list"))?;
    if items.len() > MAX_SERVERS {
        return Err(Refusal::Shape(
            "a pack suggests at most 5 outside tool programs",
        ));
    }
    let mut out: Vec<SuggestedServer> = Vec::new();
    for item in items {
        let obj = item.as_object().ok_or(Refusal::Shape(
            "a suggested tool program is not in the expected form",
        ))?;
        let allowed = ["name", "program", "args", "env", "why"];
        if !obj.keys().all(|k| allowed.contains(&k.as_str()))
            || !obj.contains_key("name")
            || !obj.contains_key("program")
        {
            return Err(Refusal::Shape("a suggested tool program has fields it should not have, or lacks its name or program"));
        }
        let name = obj["name"]
            .as_str()
            .filter(|n| pack_name_ok(n))
            .ok_or(Refusal::Shape(
                "a suggested tool program's name is not a plain name",
            ))?;
        let program = obj["program"]
            .as_str()
            .filter(|p| {
                p.starts_with('/')
                    && plain_text(p, 512)
                    && !p.split('/').any(|part| part == "..")
                    && !p.ends_with('/')
            })
            .ok_or(Refusal::Shape(
                "a suggested tool program must be named by its full path, with no .. in it",
            ))?;
        let args: Vec<String> = match obj.get("args") {
            None => Vec::new(),
            Some(a) => a
                .as_array()
                .filter(|a| a.len() <= MAX_ARGS)
                .and_then(|a| {
                    a.iter()
                        .map(|x| {
                            x.as_str()
                                .filter(|s| plain_text(s, MAX_ARG_CHARS))
                                .map(String::from)
                        })
                        .collect()
                })
                .ok_or(Refusal::Shape(
                    "a suggested tool program's arguments are not up to 32 lines of plain text",
                ))?,
        };
        let env: Vec<String> = match obj.get("env") {
            None => Vec::new(),
            Some(a) => a
                .as_array()
                .filter(|a| a.len() <= MAX_ENV)
                .and_then(|a| a.iter().map(|x| x.as_str().filter(|s| env_name_ok(s)).map(String::from)).collect())
                .ok_or(Refusal::Shape("a suggested tool program may ask for up to 16 variables by plain upper-case names, never one that changes what runs (PATH, LD_ and the like)"))?,
        };
        if env.iter().collect::<BTreeSet<_>>().len() != env.len() {
            return Err(Refusal::Shape(
                "a suggested tool program names a variable twice",
            ));
        }
        let why = match obj.get("why") {
            None => String::new(),
            Some(w) => w
                .as_str()
                .filter(|w| plain_text(w, MAX_WHY_CHARS))
                .map(String::from)
                .ok_or(Refusal::Shape("the reason given for a suggested tool program is not up to 300 characters of plain text"))?,
        };
        if out.iter().any(|s| s.name.eq_ignore_ascii_case(name)) {
            return Err(Refusal::Shape(
                "two suggested tool programs have the same name",
            ));
        }
        out.push(SuggestedServer {
            name: name.to_string(),
            program: program.to_string(),
            args,
            env,
            why,
        });
    }
    Ok(out)
}

fn parse(v: &Value) -> Result<Parsed, Refusal> {
    let obj = v
        .as_object()
        .ok_or(Refusal::Shape("the pack is not in the expected form"))?;
    let known = [
        "format",
        "version",
        "name",
        "pack_version",
        "not_after",
        "methods",
    ];
    // the outside tool programs a pack suggests are the one optional field
    let optional = ["servers"];
    if !known.iter().all(|k| obj.contains_key(*k))
        || !obj
            .keys()
            .all(|k| known.contains(&k.as_str()) || optional.contains(&k.as_str()))
    {
        return Err(Refusal::Shape(
            "the pack has fields it should not have, or lacks some",
        ));
    }
    if obj["format"] != "kakurega-pack" || obj["version"] != 1 {
        return Err(Refusal::Shape(
            "this is not a pack of a version this app reads",
        ));
    }
    let name = obj["name"]
        .as_str()
        .filter(|n| pack_name_ok(n))
        .ok_or(Refusal::Shape("the pack name is not a plain name"))?;
    let version = obj["pack_version"]
        .as_str()
        .and_then(Version::parse)
        .ok_or(Refusal::Shape("the pack version is not major.minor.patch"))?;
    let not_after = obj["not_after"]
        .as_i64()
        .ok_or(Refusal::Shape("the pack has no expiry"))?;
    let items = obj["methods"]
        .as_array()
        .ok_or(Refusal::Shape("the pack has no list of methods"))?;
    if items.is_empty() || items.len() > MAX_METHODS {
        return Err(Refusal::Shape("a pack holds 1 to 20 methods"));
    }
    let mut methods = Vec::new();
    let mut names = BTreeSet::new();
    for item in items {
        let shown = item["name"]
            .as_str()
            .unwrap_or("?")
            .chars()
            .take(64)
            .collect::<String>();
        let draft = Draft::from_json(item)
            .map_err(|e| Refusal::Method(shown.clone(), e.sentence().into()))?;
        let m =
            Method::new(draft).map_err(|e| Refusal::Method(shown.clone(), e.sentence().into()))?;
        if !names.insert(m.name.clone()) {
            return Err(Refusal::Shape("two methods in the pack have the same name"));
        }
        methods.push(m);
    }
    let servers = parse_servers(obj.get("servers"))?;
    Ok(Parsed {
        name: name.to_string(),
        version,
        not_after,
        methods,
        servers,
    })
}

/// Checks a pack file: size, signature by a pinned key, shape, every method, expiry, and rollback.
pub fn check_pack(bytes: &[u8], cx: &Context<'_>) -> Result<Checked, Refusal> {
    if bytes.len() > MAX_PACK_BYTES {
        return Err(Refusal::TooLarge);
    }
    let env = dsse::decode(bytes).map_err(|e| Refusal::NotSigned(format!("{e:?}")))?;
    let verified = dsse::verify(&env, cx.trusted, &[PACK_TYPE])
        .map_err(|e| Refusal::NotSigned(format!("{e:?}")))?;
    let v: Value = serde_json::from_slice(&env.payload)
        .map_err(|_| Refusal::Shape("the pack is not in the expected form"))?;
    let p = parse(&v)?;
    if p.not_after <= cx.now {
        return Err(Refusal::Expired);
    }
    if (cx.seen)(&p.name, verified.key_id.as_str()).is_some_and(|seen| p.version < seen) {
        return Err(Refusal::Rollback);
    }
    Ok(Checked {
        key_id: verified.key_id,
        name: p.name,
        version: p.version,
        not_after: p.not_after,
        methods: p.methods,
        servers: p.servers,
    })
}

/// Signs a pack for publishing. It refuses what a recipient would refuse, so a publisher learns of a problem before shipping it.
pub fn sign_pack(draft: &Value, signer: &Signer, now: i64) -> Result<String, Refusal> {
    let p = parse(draft)?;
    if p.not_after <= now {
        return Err(Refusal::Expired);
    }
    let canon = kk_evidence::canon::canonicalize(draft.to_string().as_bytes())
        .map_err(|_| Refusal::Shape("the pack could not be written in canonical form"))?;
    Ok(dsse::encode(&dsse::sign(
        PACK_TYPE,
        canon.as_bytes(),
        signer,
    )))
}
