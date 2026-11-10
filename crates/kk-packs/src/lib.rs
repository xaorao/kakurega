//! Signed method packs: a publisher's methods, checked against a key the owner pinned.
//!
//! A pack is one signed JSON file. It holds methods and nothing else: no tool, route, credential or permission field exists in a
//! method, and a pack that tries to carry one is refused whole. Every method that comes out is still unapproved.

use kk_evidence::dsse;
use kk_evidence::keys::{KeyId, Signer, TrustedKeys};
use kk_methods::{Draft, Method};
use serde_json::Value;
use std::collections::BTreeSet;

pub const PACK_TYPE: &str = "application/vnd.kakurega.pack.v1+json";
pub const MAX_PACK_BYTES: usize = 256 * 1024;
pub const MAX_METHODS: usize = 20;
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
    /// The highest version of the pack with this name that was imported before, if any.
    pub seen: &'a dyn Fn(&str) -> Option<Version>,
}

#[derive(Debug, Clone)]
pub struct Checked {
    pub key_id: KeyId,
    pub name: String,
    pub version: Version,
    pub not_after: i64,
    pub methods: Vec<Method>,
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
    if obj.len() != known.len() || !known.iter().all(|k| obj.contains_key(*k)) {
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
    Ok(Parsed {
        name: name.to_string(),
        version,
        not_after,
        methods,
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
    if (cx.seen)(&p.name).is_some_and(|seen| p.version < seen) {
        return Err(Refusal::Rollback);
    }
    Ok(Checked {
        key_id: verified.key_id,
        name: p.name,
        version: p.version,
        not_after: p.not_after,
        methods: p.methods,
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
