//! Who may announce updates, and how that changes over time.
//!
//! A root is a signed list of publisher keys with a threshold: an announcement counts only when at least that many
//! different listed keys signed it. A root can be replaced by a newer one (version exactly one higher) only when it is
//! signed by enough OLD keys and by enough NEW keys, so neither a thief of the old keys nor an unrelated key can take over.
//! Replacing the root is also the recovery from a stolen key: the highest version seen is remembered together with the
//! root it was seen under, and a new root forgets it, so an announcement forged with a stolen key cannot lock out the real
//! releases for ever.

use kk_evidence::dsse::{self, ThresholdError, MAX_SIGNATURES};
use kk_evidence::keys::{KeyId, Signer, TrustedKeys};
use serde::Deserialize;
use std::collections::BTreeSet;

pub const ROOT_TYPE: &str = "application/vnd.kakurega.update-root.v1+json";
pub const MAX_ROOT_BYTES: usize = 16 * 1024;
/// Most root replacements one update check will follow, so a hostile server cannot keep a client busy for ever.
pub const MAX_ROTATIONS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootDoc {
    keys: Vec<String>,
    not_after: u64,
    threshold: u64,
    version: u64,
}

#[derive(Debug, Clone)]
pub struct Root {
    pub version: u64,
    pub threshold: usize,
    /// Unix time after which no announcement is accepted under this root, so a frozen old root cannot live for ever.
    pub not_after: u64,
    keys: TrustedKeys,
    ids: BTreeSet<KeyId>,
}

impl Root {
    pub fn keys(&self) -> &TrustedKeys {
        &self.keys
    }

    pub fn key_ids(&self) -> Vec<KeyId> {
        self.ids.iter().cloned().collect()
    }

    pub fn is_expired(&self, now: u64) -> bool {
        self.not_after <= now
    }

    /// Publisher keys from a plain file, any one of which may announce: root version zero, threshold one, no expiry. This is
    /// how installations that have no signed root yet are treated; a signed root replaces it.
    pub fn legacy(publics: &[[u8; 32]]) -> Option<Root> {
        if publics.is_empty() || publics.len() > MAX_SIGNATURES {
            return None;
        }
        let mut keys = TrustedKeys::new();
        let mut ids = BTreeSet::new();
        for p in publics {
            ids.insert(keys.add(p)?);
        }
        Some(Root {
            version: 0,
            threshold: 1,
            not_after: u64::MAX,
            keys,
            ids,
        })
    }

    /// One trusted key: `legacy` with a single key.
    pub fn single(public: &[u8; 32]) -> Option<Root> {
        Root::legacy(&[*public])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootRefusal {
    /// Not enough different trusted keys signed it, or it was altered.
    NotSigned(ThresholdError),
    Malformed(&'static str),
    BadKeys(&'static str),
    BadThreshold,
    WrongVersion {
        expected: u64,
        got: u64,
    },
    /// A replacement must be signed by enough of the keys already trusted.
    NotSignedByCurrentKeys(ThresholdError),
    /// A replacement must also be signed by enough of its own new keys.
    NotSignedByNewKeys(ThresholdError),
    TooManyRotations,
    Expired,
}

fn parse(envelope: &[u8]) -> Result<(dsse::Envelope, Root), RootRefusal> {
    if envelope.len() > MAX_ROOT_BYTES {
        return Err(RootRefusal::Malformed("the root is too large"));
    }
    let env =
        dsse::decode(envelope).map_err(|_| RootRefusal::Malformed("not a signed envelope"))?;
    if env.payload_type != ROOT_TYPE {
        return Err(RootRefusal::Malformed("not a root"));
    }
    let doc: RootDoc = serde_json::from_slice(&env.payload)
        .map_err(|_| RootRefusal::Malformed("the root has the wrong shape"))?;
    if doc.version == 0 {
        return Err(RootRefusal::Malformed("a root version starts at 1"));
    }
    if doc.keys.is_empty() || doc.keys.len() > MAX_SIGNATURES {
        return Err(RootRefusal::BadKeys("a root lists 1 to 16 keys"));
    }
    let mut keys = TrustedKeys::new();
    let mut ids = BTreeSet::new();
    for k in &doc.keys {
        let raw =
            kk_evidence::b64::decode(k).map_err(|_| RootRefusal::BadKeys("a key is not base64"))?;
        let raw: [u8; 32] = raw
            .try_into()
            .map_err(|_| RootRefusal::BadKeys("a key is not 32 bytes"))?;
        let id = keys
            .add(&raw)
            .ok_or(RootRefusal::BadKeys("a key is not a usable public key"))?;
        if !ids.insert(id) {
            return Err(RootRefusal::BadKeys("the same key is listed twice"));
        }
    }
    if doc.threshold < 1 || doc.threshold > ids.len() as u64 {
        return Err(RootRefusal::BadThreshold);
    }
    let root = Root {
        version: doc.version,
        threshold: doc.threshold as usize,
        not_after: doc.not_after,
        keys,
        ids,
    };
    Ok((env, root))
}

/// The first root an installation trusts, taken from outside (shipped with the app, or handed over by the owner). It must be
/// signed by enough of its own keys, which proves the keys listed are held by whoever made it, not that they are the right
/// ones: that trust is the owner's.
pub fn load_initial(envelope: &[u8]) -> Result<Root, RootRefusal> {
    let (env, root) = parse(envelope)?;
    dsse::verify_threshold(&env, root.keys(), root.threshold, &[ROOT_TYPE])
        .map_err(RootRefusal::NotSigned)?;
    Ok(root)
}

/// Replaces `current` by the root in `envelope`: its version must be exactly one higher, and enough of the CURRENT keys and
/// enough of its own NEW keys must have signed it. An old root may already have expired; that is how a lapsed installation
/// catches up.
pub fn rotate(current: &Root, envelope: &[u8]) -> Result<Root, RootRefusal> {
    let (env, next) = parse(envelope)?;
    let expected = current.version + 1;
    if next.version != expected {
        return Err(RootRefusal::WrongVersion {
            expected,
            got: next.version,
        });
    }
    dsse::verify_threshold(&env, current.keys(), current.threshold, &[ROOT_TYPE])
        .map_err(RootRefusal::NotSignedByCurrentKeys)?;
    dsse::verify_threshold(&env, next.keys(), next.threshold, &[ROOT_TYPE])
        .map_err(RootRefusal::NotSignedByNewKeys)?;
    Ok(next)
}

/// Applies replacements in order, then insists the root it ends on has not expired.
pub fn follow(current: Root, chain: &[Vec<u8>], now: u64) -> Result<Root, RootRefusal> {
    if chain.len() > MAX_ROTATIONS {
        return Err(RootRefusal::TooManyRotations);
    }
    let mut root = current;
    for envelope in chain {
        root = rotate(&root, envelope)?;
    }
    if root.is_expired(now) {
        return Err(RootRefusal::Expired);
    }
    Ok(root)
}

/// Publisher side: turns plain JSON into a canonical root signed by every given key.
pub fn sign_root(json: &[u8], signers: &[&Signer]) -> Result<Vec<u8>, String> {
    if signers.is_empty() || signers.len() > MAX_SIGNATURES {
        return Err("sign a root with 1 to 16 keys".into());
    }
    let canon =
        kk_evidence::canon::canonicalize(json).map_err(|e| format!("not canonical JSON: {e:?}"))?;
    // the same rules every client applies
    let mut env = dsse::sign(ROOT_TYPE, canon.as_bytes(), signers[0]);
    for s in &signers[1..] {
        env.signatures
            .push((s.key_id(), s.sign(&dsse::pae(ROOT_TYPE, canon.as_bytes()))));
    }
    let encoded = dsse::encode(&env).into_bytes();
    parse(&encoded).map_err(|e| format!("{e:?}"))?;
    Ok(encoded)
}

/// The highest announced version seen, with the root it was seen under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub root_version: u64,
    pub version: crate::Version,
}

impl Seen {
    /// What counts as "seen" under `root`: a record made under an older root is forgotten, which is the recovery after a
    /// key has been replaced.
    pub fn effective(seen: Option<Seen>, root: &Root) -> Option<crate::Version> {
        seen.filter(|s| s.root_version == root.version)
            .map(|s| s.version)
    }
}
