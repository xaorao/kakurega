//! A publisher's signed catalog: the list of packs they offer, each bound to the exact file by its SHA-256.
//!
//! The catalog is one signed JSON file, checked against a key the owner pinned, like a pack. It gives no power and holds no
//! method text: it says which packs exist, at which version, what each is for, and which bytes are the real ones. A pack
//! taken from a catalog is still checked whole as a pack, and every method in it still arrives unapproved.
//!
//! What the signature and the two dates are for: a changed list is refused; a list past its expiry is refused, so an old
//! list cannot be shown for ever to hide a newer one; a list issued before the one already seen from the same publisher
//! is refused, so a withdrawn pack cannot be brought back by replaying an earlier list.

use crate::{Refusal, Version};
use kk_evidence::dsse;
use kk_evidence::keys::{KeyId, Signer, TrustedKeys};
use serde_json::Value;
use std::collections::BTreeSet;

pub const CATALOG_TYPE: &str = "application/vnd.kakurega.catalog.v1+json";
pub const MAX_CATALOG_BYTES: usize = 256 * 1024;
pub const MAX_ENTRIES: usize = 100;
const MAX_DESCRIPTION: usize = 250;
const MAX_FILE_NAME: usize = 100;
const MAX_METHOD_NAMES: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub version: Version,
    pub description: String,
    /// The file's own name, with no folder in it.
    pub file: String,
    /// SHA-256 of the signed pack file, lowercase hex.
    pub sha256: String,
    /// The names of the methods the pack holds, as the publisher lists them. The pack itself is what counts.
    pub methods: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub key_id: KeyId,
    pub issued: i64,
    pub not_after: i64,
    pub entries: Vec<Entry>,
}

fn plain_name(n: &str, max: usize) -> bool {
    let mut c = n.chars();
    c.next().is_some_and(|f| f.is_ascii_alphanumeric())
        && n.len() <= max
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !n.contains("..")
}

fn one_line(t: &str, max: usize) -> bool {
    t.chars().count() <= max && !t.chars().any(|c| c.is_control() || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'))
}

fn exact_fields(obj: &serde_json::Map<String, Value>, known: &[&str]) -> bool {
    obj.len() == known.len() && known.iter().all(|k| obj.contains_key(*k))
}

fn parse(v: &Value) -> Result<(i64, i64, Vec<Entry>), Refusal> {
    let obj = v
        .as_object()
        .ok_or(Refusal::Shape("the catalog is not in the expected form"))?;
    if !exact_fields(obj, &["format", "version", "issued", "not_after", "packs"]) {
        return Err(Refusal::Shape(
            "the catalog has fields it should not have, or lacks some",
        ));
    }
    if obj["format"] != "kakurega-catalog" || obj["version"] != 1 {
        return Err(Refusal::Shape(
            "this is not a catalog of a version this app reads",
        ));
    }
    let issued = obj["issued"]
        .as_i64()
        .filter(|i| *i > 0)
        .ok_or(Refusal::Shape(
            "the catalog does not say when it was issued",
        ))?;
    let not_after = obj["not_after"]
        .as_i64()
        .filter(|n| *n > issued)
        .ok_or(Refusal::Shape("the catalog has no expiry after its issue"))?;
    let items = obj["packs"]
        .as_array()
        .ok_or(Refusal::Shape("the catalog has no list of packs"))?;
    if items.is_empty() || items.len() > MAX_ENTRIES {
        return Err(Refusal::Shape("a catalog lists 1 to 100 packs"));
    }
    let mut names = BTreeSet::new();
    let mut files = BTreeSet::new();
    let mut entries = Vec::new();
    for item in items {
        let o = item.as_object().ok_or(Refusal::Shape(
            "a catalog entry is not in the expected form",
        ))?;
        if !exact_fields(
            o,
            &[
                "name",
                "pack_version",
                "description",
                "file",
                "sha256",
                "methods",
            ],
        ) {
            return Err(Refusal::Shape(
                "a catalog entry has fields it should not have, or lacks some",
            ));
        }
        let name = o["name"]
            .as_str()
            .filter(|n| plain_name(n, 64))
            .ok_or(Refusal::Shape(
                "a pack name in the catalog is not a plain name",
            ))?;
        let version = o["pack_version"]
            .as_str()
            .and_then(Version::parse)
            .ok_or(Refusal::Shape(
                "a pack version in the catalog is not major.minor.patch",
            ))?;
        let description = o["description"]
            .as_str()
            .filter(|d| one_line(d, MAX_DESCRIPTION))
            .ok_or(Refusal::Shape(
                "a description in the catalog is not one line of up to 250 characters",
            ))?;
        let file = o["file"]
            .as_str()
            .filter(|f| plain_name(f, MAX_FILE_NAME))
            .ok_or(Refusal::Shape(
                "a file name in the catalog is not a plain file name",
            ))?;
        let sha256 = o["sha256"]
            .as_str()
            .filter(|h| {
                h.len() == 64
                    && h.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
            .ok_or(Refusal::Shape(
                "a digest in the catalog is not 64 lowercase hex characters",
            ))?;
        let methods: Vec<String> = o["methods"]
            .as_array()
            .filter(|m| !m.is_empty() && m.len() <= MAX_METHOD_NAMES)
            .and_then(|m| {
                m.iter()
                    .map(|x| x.as_str().filter(|n| plain_name(n, 64)).map(str::to_string))
                    .collect()
            })
            .ok_or(Refusal::Shape(
                "a catalog entry does not list its methods by plain names",
            ))?;
        if !names.insert(name.to_string()) || !files.insert(file.to_string()) {
            return Err(Refusal::Shape(
                "two catalog entries have the same pack name or file",
            ));
        }
        entries.push(Entry {
            name: name.to_string(),
            version,
            description: description.to_string(),
            file: file.to_string(),
            sha256: sha256.to_string(),
            methods,
        });
    }
    Ok((issued, not_after, entries))
}

/// Checks a catalog file: size, signature by a pinned key, shape, expiry, and that it is not older than the one already
/// seen from the same publisher (`seen_issued`, by key id).
pub fn check_catalog(
    bytes: &[u8],
    trusted: &TrustedKeys,
    now: i64,
    seen_issued: &dyn Fn(&str) -> Option<i64>,
) -> Result<Catalog, Refusal> {
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err(Refusal::TooLarge);
    }
    let env = dsse::decode(bytes).map_err(|e| Refusal::NotSigned(format!("{e:?}")))?;
    let verified = dsse::verify(&env, trusted, &[CATALOG_TYPE])
        .map_err(|e| Refusal::NotSigned(format!("{e:?}")))?;
    let v: Value = serde_json::from_slice(&env.payload)
        .map_err(|_| Refusal::Shape("the catalog is not in the expected form"))?;
    let (issued, not_after, entries) = parse(&v)?;
    if not_after <= now {
        return Err(Refusal::Expired);
    }
    // a list dated ahead of now would shut out every later list from this publisher once it was opened
    if issued > now + 600 {
        return Err(Refusal::Shape("the catalog is dated in the future"));
    }
    if seen_issued(verified.key_id.as_str()).is_some_and(|seen| issued < seen) {
        return Err(Refusal::Rollback);
    }
    Ok(Catalog {
        key_id: verified.key_id,
        issued,
        not_after,
        entries,
    })
}

/// Signs a catalog for publishing, refusing what a reader would refuse.
pub fn sign_catalog(draft: &Value, signer: &Signer, now: i64) -> Result<String, Refusal> {
    let (_, not_after, _) = parse(draft)?;
    if not_after <= now {
        return Err(Refusal::Expired);
    }
    let canon = kk_evidence::canon::canonicalize(draft.to_string().as_bytes())
        .map_err(|_| Refusal::Shape("the catalog could not be written in canonical form"))?;
    Ok(dsse::encode(&dsse::sign(
        CATALOG_TYPE,
        canon.as_bytes(),
        signer,
    )))
}

/// The entry these pack bytes are, if the catalog lists exactly these bytes.
pub fn entry_for<'a>(catalog: &'a Catalog, pack_bytes: &[u8]) -> Option<&'a Entry> {
    let digest = kk_evidence::capsule::sha256_hex(pack_bytes);
    catalog.entries.iter().find(|e| e.sha256 == digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn signer(seed: u8) -> Signer {
        Signer::from_seed([seed; 32])
    }
    fn trusted(s: &Signer) -> TrustedKeys {
        let mut t = TrustedKeys::new();
        t.add(&s.public());
        t
    }
    fn draft() -> Value {
        json!({"format": "kakurega-catalog", "version": 1, "issued": 1000, "not_after": 5000, "packs": [
            {"name": "writing", "pack_version": "1.2.0", "description": "Plain-language writing methods.", "file": "writing-1.2.0.kkp", "sha256": "a".repeat(64), "methods": ["plain-summary"]},
            {"name": "review", "pack_version": "0.3.1", "description": "Review checklists.", "file": "review-0.3.1.kkp", "sha256": "b".repeat(64), "methods": ["pr-checklist", "risk-list"]},
        ]})
    }
    fn none(_: &str) -> Option<i64> {
        None
    }

    #[test]
    fn ct01_a_signed_catalog_reads_back_and_a_changed_or_foreign_one_is_refused() {
        let s = signer(7);
        let signed = sign_catalog(&draft(), &s, 2000).unwrap();
        let c = check_catalog(signed.as_bytes(), &trusted(&s), 2000, &none).unwrap();
        assert_eq!(c.entries.len(), 2);
        assert_eq!(
            (c.entries[0].name.as_str(), c.entries[0].version, c.issued),
            ("writing", Version(1, 2, 0), 1000)
        );
        // signed by someone the owner did not pin
        assert!(matches!(
            check_catalog(signed.as_bytes(), &trusted(&signer(8)), 2000, &none),
            Err(Refusal::NotSigned(_))
        ));
        // one character of the signed text changed
        let mut env = dsse::decode(signed.as_bytes()).unwrap();
        let at = env
            .payload
            .windows(7)
            .position(|w| w == b"writing")
            .unwrap();
        env.payload[at] = b'W';
        let changed = dsse::encode(&env);
        assert_ne!(changed, signed);
        assert!(check_catalog(changed.as_bytes(), &trusted(&s), 2000, &none).is_err());
        // a signed pack is not a catalog, whoever signed it
        let other = dsse::encode(&dsse::sign(
            crate::PACK_TYPE,
            draft().to_string().as_bytes(),
            &s,
        ));
        assert!(matches!(
            check_catalog(other.as_bytes(), &trusted(&s), 2000, &none),
            Err(Refusal::NotSigned(_))
        ));
        assert!(matches!(
            check_catalog(
                &vec![b'x'; MAX_CATALOG_BYTES + 1],
                &trusted(&s),
                2000,
                &none
            ),
            Err(Refusal::TooLarge)
        ));
    }

    #[test]
    fn ct02_an_expired_list_and_a_list_older_than_the_one_seen_are_refused() {
        let s = signer(7);
        let signed = sign_catalog(&draft(), &s, 2000).unwrap();
        assert_eq!(
            check_catalog(signed.as_bytes(), &trusted(&s), 5000, &none).unwrap_err(),
            Refusal::Expired
        );
        assert_eq!(
            check_catalog(signed.as_bytes(), &trusted(&s), 2000, &|_| Some(1001)).unwrap_err(),
            Refusal::Rollback
        );
        assert!(
            check_catalog(signed.as_bytes(), &trusted(&s), 2000, &|_| Some(1000)).is_ok(),
            "the same list again is fine"
        );
        assert_eq!(
            sign_catalog(&draft(), &s, 5000).unwrap_err(),
            Refusal::Expired
        );
    }

    #[test]
    fn ct03_a_catalog_that_does_not_read_exactly_as_one_is_not_signed() {
        let s = signer(7);
        let change = |f: &dyn Fn(&mut Value)| {
            let mut d = draft();
            f(&mut d);
            sign_catalog(&d, &s, 2000)
        };
        for (what, f) in [
            (
                "an extra field",
                Box::new(|d: &mut Value| d["tools"] = json!(["shell"])) as Box<dyn Fn(&mut Value)>,
            ),
            (
                "an extra field in an entry",
                Box::new(|d: &mut Value| d["packs"][0]["url"] = json!("https://x.example")),
            ),
            (
                "a file name with a folder",
                Box::new(|d: &mut Value| d["packs"][0]["file"] = json!("../writing.kkp")),
            ),
            (
                "a file name with a slash",
                Box::new(|d: &mut Value| d["packs"][0]["file"] = json!("a/b.kkp")),
            ),
            (
                "an uppercase digest",
                Box::new(|d: &mut Value| d["packs"][0]["sha256"] = json!("A".repeat(64))),
            ),
            (
                "a short digest",
                Box::new(|d: &mut Value| d["packs"][0]["sha256"] = json!("abc")),
            ),
            (
                "a description of two lines",
                Box::new(|d: &mut Value| d["packs"][0]["description"] = json!("one\ntwo")),
            ),
            (
                "a description with a hidden mark",
                Box::new(|d: &mut Value| d["packs"][0]["description"] = json!("safe\u{202E}evil")),
            ),
            (
                "a bad version",
                Box::new(|d: &mut Value| d["packs"][0]["pack_version"] = json!("1.2")),
            ),
            (
                "two entries with one name",
                Box::new(|d: &mut Value| d["packs"][1]["name"] = json!("writing")),
            ),
            (
                "two entries with one file",
                Box::new(|d: &mut Value| d["packs"][1]["file"] = json!("writing-1.2.0.kkp")),
            ),
            ("no packs", Box::new(|d: &mut Value| d["packs"] = json!([]))),
            (
                "no methods listed",
                Box::new(|d: &mut Value| d["packs"][0]["methods"] = json!([])),
            ),
            (
                "an expiry before the issue",
                Box::new(|d: &mut Value| d["not_after"] = json!(999)),
            ),
            (
                "another format",
                Box::new(|d: &mut Value| d["format"] = json!("kakurega-pack")),
            ),
        ] {
            assert!(matches!(change(&*f), Err(Refusal::Shape(_))), "{what}");
        }
    }

    #[test]
    fn ct04_only_the_exact_bytes_a_catalog_lists_are_its_pack() {
        let s = signer(7);
        let pack = b"the signed pack file";
        let mut d = draft();
        d["packs"][0]["sha256"] = json!(kk_evidence::capsule::sha256_hex(pack));
        let c = check_catalog(
            sign_catalog(&d, &s, 2000).unwrap().as_bytes(),
            &trusted(&s),
            2000,
            &none,
        )
        .unwrap();
        assert_eq!(
            entry_for(&c, pack).map(|e| e.name.as_str()),
            Some("writing")
        );
        assert!(entry_for(&c, b"the signed pack file ").is_none());
        assert!(entry_for(&c, b"").is_none());
    }
}
