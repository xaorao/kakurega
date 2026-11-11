//! Sharing part of a release with someone who was not its recipient, with a proof and nothing else.
//!
//! When a capsule is made, the signer also signs one short disclosure statement: the digests of the capsule and of its
//! release statement, and the root of a hash tree over the capsule's files. Each file is one leaf, hashed together with a
//! secret salt of its own. A share is that signed statement, the files being shared with their salts, and for each the short
//! list of hashes that leads from its leaf to the root.
//!
//! A reader of a share learns: the shared files, that the signer bound them into the release with those digests, and how
//! many files the release has. They do not learn the name, size, content or hash of any other file, the recipient, or the
//! route: the other leaves appear only as salted hashes, which cannot be matched against a guess.
//!
//! The tree is the one of RFC 6962 (Certificate Transparency): a leaf is `SHA-256(0x00 || leaf)`, a node is
//! `SHA-256(0x01 || left || right)`, and a tree of n leaves splits at the largest power of two below n.

use crate::b64;
use crate::canon::{self, Value};
use crate::capsule::{
    exact_keys, int_of, is_hex64, member, obj, s, sha256_hex, str_of, valid_file_name,
};
use crate::dsse;
use crate::keys::{KeyId, Signer, TrustedKeys};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const DISCLOSURE_TYPE: &str = "application/vnd.kakurega.disclosure.v1+json";
pub const SCHEME: &str = "salted-merkle-sha256-v1";
/// The most files one share may carry.
pub const MAX_SHARED: usize = 16;
pub const MAX_SHARE_BYTES: usize = 16 << 20;
const MAX_LEAVES: usize = crate::capsule::MAX_FILES;
const SALT_LABEL: &[u8] = b"kakurega-share-salt-v1\0";

fn sha(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex32(h: &str) -> Option<[u8; 32]> {
    if !is_hex64(h) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&h[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// HMAC-SHA-256, written out: the salts come from one secret seed, and a keyed hash is the right tool for that.
fn hmac(key: &[u8; 32], message: &[&[u8]]) -> [u8; 32] {
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..32 {
        ipad[i] ^= key[i];
        opad[i] ^= key[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    for m in message {
        inner.update(m);
    }
    let inner: [u8; 32] = inner.finalize().into();
    sha(&[&opad, &inner])
}

/// The salt of one file: derived from the capsule's secret seed and the file's name, so it can be made again whenever
/// the owner shares, and cannot be guessed without the seed.
pub fn salt(seed: &[u8; 32], name: &str) -> [u8; 32] {
    hmac(seed, &[SALT_LABEL, name.as_bytes()])
}

/// The secret seed of one capsule's salts, made from a secret the owner's service holds and the capsule's digest. The same
/// capsule always gets the same seed, so a share can be made at any time; another capsule gets another.
pub fn seed_for(secret: &[u8; 32], capsule_sha256: &str) -> [u8; 32] {
    hmac(
        secret,
        &[b"kakurega-share-seed-v1\0", capsule_sha256.as_bytes()],
    )
}

/// The bytes of a capsule's release statement, and its files by name.
pub type CapsuleParts = (Vec<u8>, BTreeMap<String, Vec<u8>>);

/// The files of a capsule and the bytes of its release statement, for a capsule that verifies. `None` otherwise.
pub fn capsule_parts(capsule: &[u8], now: i64) -> Option<CapsuleParts> {
    let report = crate::capsule::verify(
        capsule,
        &crate::capsule::VerifyOptions {
            pinned_keys: None,
            now,
            expected_recipient: None,
        },
    );
    // an expired capsule is still the owner's own to share from
    if !report.valid && report.reason != Some("expired") {
        return None;
    }
    let top = canon::parse_strict(capsule).ok()?;
    let Value::Array(envs) = member(&top, "envelopes")? else {
        return None;
    };
    let release = envs
        .iter()
        .filter_map(|e| dsse::from_value(e).ok())
        .find(|e| e.payload_type == crate::capsule::RELEASE_TYPE)?;
    let Value::Object(listed) = member(&top, "files")? else {
        return None;
    };
    let mut files = BTreeMap::new();
    for (name, content) in listed {
        let Value::Str(text) = content else {
            return None;
        };
        files.insert(name.clone(), b64::decode(text).ok()?);
    }
    Some((release.payload, files))
}

/// The hash of one leaf. The name's length is written before the name, so a name cannot run into the content.
pub fn leaf_hash(salt: &[u8; 32], name: &str, content: &[u8]) -> [u8; 32] {
    let len = (name.len() as u16).to_be_bytes();
    sha(&[&[0u8], salt, &len, name.as_bytes(), content])
}

fn node(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    sha(&[&[1u8], left, right])
}

fn split(n: usize) -> usize {
    // the largest power of two strictly below n (n is at least 2)
    let mut k = 1;
    while k * 2 < n {
        k *= 2;
    }
    k
}

/// The root of the tree over these leaf hashes, in this order. `None` for no leaves.
pub fn root(leaves: &[[u8; 32]]) -> Option<[u8; 32]> {
    match leaves.len() {
        0 => None,
        1 => Some(leaves[0]),
        n => {
            let k = split(n);
            Some(node(&root(&leaves[..k])?, &root(&leaves[k..])?))
        }
    }
}

/// The hashes that lead from leaf `m` to the root, nearest first.
pub fn path(leaves: &[[u8; 32]], m: usize) -> Option<Vec<[u8; 32]>> {
    if m >= leaves.len() {
        return None;
    }
    if leaves.len() == 1 {
        return Some(Vec::new());
    }
    let k = split(leaves.len());
    let mut out;
    if m < k {
        out = path(&leaves[..k], m)?;
        out.push(root(&leaves[k..])?);
    } else {
        out = path(&leaves[k..], m - k)?;
        out.push(root(&leaves[..k])?);
    }
    Some(out)
}

/// Whether `leaf` at position `m` of a tree of `n` leaves, with this path, gives `want`. The path must be exactly as long
/// as that position needs: one that is too long or too short is refused, so a proof has one form only.
pub fn path_leads_to(
    leaf: &[u8; 32],
    m: usize,
    n: usize,
    path: &[[u8; 32]],
    want: &[u8; 32],
) -> bool {
    if m >= n {
        return false;
    }
    let (mut f, mut last) = (m, n - 1);
    let mut r = *leaf;
    for p in path {
        if last == 0 {
            return false;
        }
        if f % 2 == 1 || f == last {
            r = node(p, &r);
            if f % 2 == 0 {
                while f % 2 == 0 && f != 0 {
                    f >>= 1;
                    last >>= 1;
                }
            }
        } else {
            r = node(&r, p);
        }
        f >>= 1;
        last >>= 1;
    }
    last == 0 && r == *want
}

fn leaf_hashes(files: &BTreeMap<String, Vec<u8>>, seed: &[u8; 32]) -> Vec<[u8; 32]> {
    files
        .iter()
        .map(|(name, content)| leaf_hash(&salt(seed, name), name, content))
        .collect()
}

/// The signed disclosure statement for a capsule, made when the capsule is made. `capsule` is the capsule's exact bytes and
/// `release_payload` the bytes of its release statement. `None` when there are no files, or too many.
pub fn build_disclosure(
    capsule: &[u8],
    release_payload: &[u8],
    files: &BTreeMap<String, Vec<u8>>,
    seed: &[u8; 32],
    signer: &Signer,
) -> Option<Vec<u8>> {
    if files.is_empty() || files.len() > MAX_LEAVES || !files.keys().all(|n| valid_file_name(n)) {
        return None;
    }
    let tree_root = root(&leaf_hashes(files, seed))?;
    let statement = obj(vec![
        ("type", s("disclosure")),
        ("version", Value::Int(1)),
        ("capsule_sha256", s(&sha256_hex(capsule))),
        ("release_payload_sha256", s(&sha256_hex(release_payload))),
        ("scheme", s(SCHEME)),
        ("root", s(&hex(&tree_root))),
        ("leaves", Value::Int(files.len() as i64)),
    ]);
    let payload = canon::serialize(&statement).into_bytes();
    Some(dsse::encode(&dsse::sign(DISCLOSURE_TYPE, &payload, signer)).into_bytes())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareError {
    /// The disclosure statement does not read as one.
    Disclosure,
    /// A name asked for is not a file of the capsule, or is asked for twice, or none or too many were asked for.
    Names,
    /// The files and seed given do not produce the root the statement was signed over.
    RootMismatch,
}

/// A share of the named files. `disclosure` is what `build_disclosure` returned; `files` and `seed` are the capsule's own.
pub fn build_share(
    disclosure: &[u8],
    public: &[u8; 32],
    files: &BTreeMap<String, Vec<u8>>,
    seed: &[u8; 32],
    names: &[&str],
) -> Result<Vec<u8>, ShareError> {
    let env = dsse::decode(disclosure).map_err(|_| ShareError::Disclosure)?;
    let statement = canon::parse_strict(&env.payload).map_err(|_| ShareError::Disclosure)?;
    let signed_root = str_of(member(&statement, "root")).ok_or(ShareError::Disclosure)?;
    let unique: BTreeSet<&str> = names.iter().copied().collect();
    if names.is_empty() || names.len() > MAX_SHARED || unique.len() != names.len() {
        return Err(ShareError::Names);
    }
    let hashes = leaf_hashes(files, seed);
    if root(&hashes).map(|r| hex(&r)).as_deref() != Some(signed_root) {
        return Err(ShareError::RootMismatch);
    }
    let order: Vec<&String> = files.keys().collect();
    let mut leaves = Vec::new();
    for name in unique {
        let index = order
            .iter()
            .position(|n| n.as_str() == name)
            .ok_or(ShareError::Names)?;
        let steps = path(&hashes, index).ok_or(ShareError::Names)?;
        leaves.push(obj(vec![
            ("name", s(name)),
            ("salt", s(&b64::encode(&salt(seed, name)))),
            ("content", s(&b64::encode(&files[name]))),
            ("index", Value::Int(index as i64)),
            (
                "path",
                Value::Array(steps.iter().map(|h| s(&hex(h))).collect()),
            ),
        ]));
    }
    let share = obj(vec![
        ("format", s("kakurega-share")),
        ("version", Value::Int(1)),
        ("keys", Value::Array(vec![s(&b64::encode(public))])),
        ("envelope", dsse::to_value(&env)),
        ("leaves", Value::Array(leaves)),
    ]);
    Ok(canon::serialize(&share).into_bytes())
}

#[derive(Debug, Clone, Default)]
pub struct ShareOptions {
    pub pinned_keys: Option<Vec<[u8; 32]>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareReport {
    pub valid: bool,
    pub reason: Option<&'static str>,
    pub signer: Option<KeyId>,
    pub signer_pinned: bool,
    /// The digest of the whole capsule this share comes from: what a registry on NEAR records.
    pub capsule_sha256: Option<String>,
    pub release_payload_sha256: Option<String>,
    /// How many files the release has in all.
    pub total_files: usize,
    /// The shared files, by name.
    pub files: Vec<(String, Vec<u8>)>,
    pub established: Vec<String>,
    pub not_established: Vec<String>,
}

fn fail(reason: &'static str) -> ShareReport {
    ShareReport {
        valid: false,
        reason: Some(reason),
        signer: None,
        signer_pinned: false,
        capsule_sha256: None,
        release_payload_sha256: None,
        total_files: 0,
        files: Vec::new(),
        established: Vec::new(),
        not_established: Vec::new(),
    }
}

/// Checks a share: its form, the signature on the disclosure statement, and for every shared file that it is a leaf of the
/// signed tree at the place it says.
pub fn verify_share(bytes: &[u8], opts: &ShareOptions) -> ShareReport {
    if bytes.len() > MAX_SHARE_BYTES {
        return fail("share_too_large");
    }
    let Ok(top) = canon::parse_strict(bytes) else {
        return fail("share_not_json");
    };
    if canon::serialize(&top).as_bytes() != bytes {
        return fail("share_not_canonical");
    }
    if !exact_keys(&top, &["format", "version", "keys", "envelope", "leaves"])
        || str_of(member(&top, "format")) != Some("kakurega-share")
    {
        return fail("share_shape");
    }
    if int_of(member(&top, "version")) != Some(1) {
        return fail("unsupported_version");
    }
    let (Some(Value::Array(keys)), Some(envelope), Some(Value::Array(leaves))) = (
        member(&top, "keys"),
        member(&top, "envelope"),
        member(&top, "leaves"),
    ) else {
        return fail("share_shape");
    };
    let mut claimed: Vec<[u8; 32]> = Vec::new();
    for k in keys {
        let Some(raw) = (match k {
            Value::Str(t) => b64::decode(t)
                .ok()
                .and_then(|r| <[u8; 32]>::try_from(r).ok()),
            _ => None,
        }) else {
            return fail("bad_key");
        };
        claimed.push(raw);
    }
    let pinned = opts.pinned_keys.is_some();
    let mut trusted = TrustedKeys::new();
    for k in opts.pinned_keys.as_ref().unwrap_or(&claimed) {
        if trusted.add(k).is_none() {
            return fail("bad_key");
        }
    }
    if trusted.is_empty() {
        return fail("no_keys");
    }
    let Ok(env) = dsse::from_value(envelope) else {
        return fail("bad_envelope");
    };
    let verified = match dsse::verify(&env, &trusted, &[DISCLOSURE_TYPE]) {
        Ok(v) => v,
        Err(dsse::VerifyError::UnknownPayloadType) => return fail("unknown_payload_type"),
        Err(dsse::VerifyError::WrongSignatureCount) => return fail("signature_count"),
        Err(dsse::VerifyError::UnknownKey) => {
            return fail(if pinned {
                "untrusted_signer"
            } else {
                "unknown_signer"
            })
        }
        Err(dsse::VerifyError::BadSignature) => return fail("bad_signature"),
        Err(dsse::VerifyError::PayloadNotCanonical) => return fail("payload_not_canonical"),
    };
    // as for a capsule: the key list is not signed, so it must be exactly the key that signed
    if claimed.len() != 1 || KeyId::of(&claimed[0]) != verified.key_id {
        return fail("key_list");
    }
    let Ok(st) = canon::parse_strict(&env.payload) else {
        return fail("payload_not_canonical");
    };
    if !exact_keys(
        &st,
        &[
            "type",
            "version",
            "capsule_sha256",
            "release_payload_sha256",
            "scheme",
            "root",
            "leaves",
        ],
    ) || str_of(member(&st, "type")) != Some("disclosure")
    {
        return fail("disclosure_shape");
    }
    if int_of(member(&st, "version")) != Some(1) || str_of(member(&st, "scheme")) != Some(SCHEME) {
        return fail("unsupported_version");
    }
    let (Some(capsule_sha), Some(release_sha), Some(root_hex), Some(total)) = (
        str_of(member(&st, "capsule_sha256")).filter(|x| is_hex64(x)),
        str_of(member(&st, "release_payload_sha256")).filter(|x| is_hex64(x)),
        str_of(member(&st, "root")),
        int_of(member(&st, "leaves")),
    ) else {
        return fail("disclosure_shape");
    };
    let Some(want) = unhex32(root_hex) else {
        return fail("disclosure_shape");
    };
    if total < 1 || total as usize > MAX_LEAVES {
        return fail("disclosure_shape");
    }
    let total = total as usize;
    if leaves.is_empty() || leaves.len() > MAX_SHARED {
        return fail("leaf_count");
    }
    let mut names = BTreeSet::new();
    let mut places = BTreeSet::new();
    let mut files = Vec::new();
    let mut last_name: Option<String> = None;
    for leaf in leaves {
        if !exact_keys(leaf, &["name", "salt", "content", "index", "path"]) {
            return fail("leaf_shape");
        }
        let (
            Some(name),
            Some(salt_text),
            Some(content_text),
            Some(index),
            Some(Value::Array(steps)),
        ) = (
            str_of(member(leaf, "name")),
            str_of(member(leaf, "salt")),
            str_of(member(leaf, "content")),
            int_of(member(leaf, "index")),
            member(leaf, "path"),
        )
        else {
            return fail("leaf_shape");
        };
        if !valid_file_name(name) {
            return fail("bad_file_name");
        }
        // one order only, so the same share cannot be written as two different files
        if last_name.as_deref().is_some_and(|prev| prev >= name) {
            return fail("leaf_order");
        }
        last_name = Some(name.to_string());
        if index < 0
            || index as usize >= total
            || !places.insert(index)
            || !names.insert(name.to_string())
        {
            return fail("leaf_place");
        }
        let (Some(salt), Ok(content)) = (
            b64::decode(salt_text)
                .ok()
                .and_then(|r| <[u8; 32]>::try_from(r).ok()),
            b64::decode(content_text),
        ) else {
            return fail("leaf_shape");
        };
        let mut hashes = Vec::new();
        for step in steps {
            let Some(h) = (match step {
                Value::Str(t) => unhex32(t),
                _ => None,
            }) else {
                return fail("leaf_shape");
            };
            hashes.push(h);
        }
        if hashes.len() > 8 {
            return fail("leaf_shape");
        }
        if !path_leads_to(
            &leaf_hash(&salt, name, &content),
            index as usize,
            total,
            &hashes,
            &want,
        ) {
            return fail("not_in_release");
        }
        files.push((name.to_string(), content));
    }
    let mut not_established = vec![
        "Anything about the release's other files: this share shows none of their names, sizes or contents.".to_string(),
        "That what a shared file says is true; only that the signer put exactly this file in the release.".to_string(),
        "Who the release was made for, or which model route produced it: a share does not carry that.".to_string(),
    ];
    if !pinned {
        not_established.push("Who holds the signing key: no key was pinned, so the key shown comes from the share itself.".to_string());
    }
    ShareReport {
        valid: true,
        reason: None,
        signer: Some(verified.key_id.clone()),
        signer_pinned: pinned,
        capsule_sha256: Some(capsule_sha.to_string()),
        release_payload_sha256: Some(release_sha.to_string()),
        total_files: total,
        established: vec![
            format!("The disclosure statement carries a valid Ed25519 signature from {}.", verified.key_id),
            format!(
                "Each of the {} shared file(s) is exactly one of the {} files the signer bound into the release with that statement.",
                files.len(),
                total
            ),
            format!("The capsule it comes from has the SHA-256 digest {capsule_sha}; a registry record for that digest is about the same release."),
        ],
        not_established,
        files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(n: usize) -> BTreeMap<String, Vec<u8>> {
        (0..n)
            .map(|i| {
                (
                    format!("finding-{i:02}.txt"),
                    format!("finding number {i}: the token stays valid after logout").into_bytes(),
                )
            })
            .collect()
    }

    fn signer() -> Signer {
        Signer::from_seed([7u8; 32])
    }

    const SEED: [u8; 32] = [9u8; 32];

    fn made(n: usize, names: &[&str]) -> (Vec<u8>, Vec<u8>, BTreeMap<String, Vec<u8>>) {
        let f = files(n);
        let d = build_disclosure(
            b"the capsule bytes",
            b"the release payload",
            &f,
            &SEED,
            &signer(),
        )
        .unwrap();
        let share = build_share(&d, &signer().public(), &f, &SEED, names).unwrap();
        (d, share, f)
    }

    /// Rewrites one member of the share's top object and serialises it again, canonically.
    fn rewrite(share: &[u8], change: impl FnOnce(&mut Vec<(String, Value)>)) -> Vec<u8> {
        let Value::Object(m) = canon::parse_strict(share).unwrap() else {
            panic!()
        };
        let mut m: Vec<(String, Value)> = m.into_iter().collect();
        change(&mut m);
        canon::serialize(&Value::Object(m.into_iter().collect())).into_bytes()
    }

    fn leaf_mut<'a>(m: &'a mut [(String, Value)], i: usize, field: &str) -> &'a mut Value {
        let Value::Array(leaves) = &mut m.iter_mut().find(|(k, _)| k == "leaves").unwrap().1 else {
            panic!()
        };
        let Value::Object(leaf) = &mut leaves[i] else {
            panic!()
        };
        leaf.iter_mut()
            .find(|(k, _)| k == field)
            .map(|(_, v)| v)
            .unwrap()
    }

    #[test]
    fn sh01_every_leaf_of_every_tree_size_has_a_path_that_leads_to_the_root_and_only_that_one() {
        for n in 1..=33usize {
            let leaves: Vec<[u8; 32]> = (0..n).map(|i| sha(&[&[i as u8], b"leaf"])).collect();
            let r = root(&leaves).unwrap();
            for m in 0..n {
                let p = path(&leaves, m).unwrap();
                assert!(path_leads_to(&leaves[m], m, n, &p, &r), "n={n} m={m}");
                // the same path at another place, for another size, or with a step added, removed or changed, does not
                if n > 1 {
                    assert!(
                        !path_leads_to(&leaves[m], (m + 1) % n, n, &p, &r),
                        "n={n} m={m} wrong place"
                    );
                    assert!(
                        !path_leads_to(&leaves[(m + 1) % n], m, n, &p, &r),
                        "n={n} m={m} wrong leaf"
                    );
                    let mut short = p.clone();
                    short.pop();
                    assert!(
                        !path_leads_to(&leaves[m], m, n, &short, &r),
                        "n={n} m={m} short"
                    );
                    let mut changed = p.clone();
                    changed[0][0] ^= 1;
                    assert!(
                        !path_leads_to(&leaves[m], m, n, &changed, &r),
                        "n={n} m={m} changed"
                    );
                }
                // a leaf has one place: its path leads to the root from there and from nowhere else
                for other in (0..n).filter(|o| *o != m) {
                    assert!(
                        !path_leads_to(&leaves[m], other, n, &p, &r),
                        "n={n}: leaf {m} passed as leaf {other}"
                    );
                }
                let mut long = p.clone();
                long.push([0u8; 32]);
                assert!(
                    !path_leads_to(&leaves[m], m, n, &long, &r),
                    "n={n} m={m} long"
                );
            }
            assert!(path(&leaves, n).is_none());
        }
        assert!(root(&[]).is_none());
        // known answer: two leaves are one node over them, in order
        let (a, b) = ([1u8; 32], [2u8; 32]);
        assert_eq!(root(&[a, b]).unwrap(), sha(&[&[1u8], &a, &b]));
        assert_ne!(root(&[a, b]), root(&[b, a]));
    }

    #[test]
    fn sh02_a_leaf_binds_its_salt_its_name_and_its_content_and_a_name_cannot_run_into_the_content()
    {
        let s1 = salt(&SEED, "a.txt");
        assert_eq!(s1, salt(&SEED, "a.txt"));
        assert_ne!(s1, salt(&SEED, "b.txt"));
        assert_ne!(
            s1,
            salt(&[8u8; 32], "a.txt"),
            "another capsule's seed gives another salt"
        );
        let h = leaf_hash(&s1, "a.txt", b"hello");
        assert_ne!(h, leaf_hash(&s1, "a.txt", b"hellp"));
        assert_ne!(h, leaf_hash(&s1, "b.txt", b"hello"));
        assert_ne!(h, leaf_hash(&[0u8; 32], "a.txt", b"hello"));
        assert_ne!(leaf_hash(&s1, "ab", b"c"), leaf_hash(&s1, "a", b"bc"));
        // a leaf is never a node: the first byte hashed differs
        assert_ne!(sha(&[&[0u8], &[3u8; 64]]), sha(&[&[1u8], &[3u8; 64]]));
        // HMAC-SHA-256 known answer (RFC 4231, test case 2 uses a short key; here the key is 32 bytes of 0x0b and "Hi There")
        assert_eq!(
            hex(&hmac(&[0x0b; 32], &[b"Hi There"])),
            "198a607eb44bfbc69903a0f1cf2bbdc5ba0aa3f3d9ae3c1c7a3b1696a0b68cf7"
        );
    }

    #[test]
    fn sh03_a_share_of_one_file_verifies_and_shows_nothing_of_the_others() {
        let (_, share, f) = made(7, &["finding-03.txt"]);
        let r = verify_share(&share, &ShareOptions::default());
        assert!(r.valid, "{:?}", r.reason);
        assert_eq!(
            r.files,
            vec![("finding-03.txt".to_string(), f["finding-03.txt"].clone())]
        );
        assert_eq!(r.total_files, 7);
        assert_eq!(
            r.capsule_sha256.as_deref(),
            Some(sha256_hex(b"the capsule bytes").as_str())
        );
        assert_eq!(
            r.release_payload_sha256.as_deref(),
            Some(sha256_hex(b"the release payload").as_str())
        );
        assert_eq!(r.signer, Some(signer().key_id()));
        assert!(
            !r.signer_pinned
                && r.not_established
                    .iter()
                    .any(|n| n.contains("no key was pinned"))
        );
        // nothing of any other file is in the share: not a name, not its text, not its plain hash, not its salt
        let text = String::from_utf8(share.clone()).unwrap();
        for (name, content) in f.iter().filter(|(n, _)| n.as_str() != "finding-03.txt") {
            assert!(!text.contains(name.as_str()), "{name}");
            assert!(!text.contains(&b64::encode(content)), "{name} content");
            assert!(!text.contains(&sha256_hex(content)), "{name} plain hash");
            assert!(
                !text.contains(&b64::encode(&salt(&SEED, name))),
                "{name} salt"
            );
        }
        // pinned to the signer it is the same; pinned to someone else it is refused
        let pinned = verify_share(
            &share,
            &ShareOptions {
                pinned_keys: Some(vec![signer().public()]),
            },
        );
        assert!(pinned.valid && pinned.signer_pinned);
        let other = verify_share(
            &share,
            &ShareOptions {
                pinned_keys: Some(vec![Signer::from_seed([1u8; 32]).public()]),
            },
        );
        assert_eq!(other.reason, Some("untrusted_signer"));
        // several files in one share, each with its own path
        let (_, three, f) = made(7, &["finding-06.txt", "finding-00.txt", "finding-03.txt"]);
        let r = verify_share(&three, &ShareOptions::default());
        assert!(r.valid, "{:?}", r.reason);
        assert_eq!(
            r.files.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["finding-00.txt", "finding-03.txt", "finding-06.txt"]
        );
        assert_eq!(r.files[2].1, f["finding-06.txt"]);
        // a release of one file: the path is empty and it still verifies
        let (_, one, _) = made(1, &["finding-00.txt"]);
        assert!(verify_share(&one, &ShareOptions::default()).valid);
    }

    #[test]
    fn sh04_a_share_that_was_changed_in_any_part_is_refused_with_the_reason() {
        let (_, share, _) = made(5, &["finding-01.txt", "finding-04.txt"]);
        let check = |bytes: Vec<u8>, want: &str| {
            let r = verify_share(&bytes, &ShareOptions::default());
            assert_eq!((r.valid, r.reason), (false, Some(want)), "{want}");
            assert!(r.files.is_empty(), "a refused share shows no file");
        };
        // the content, the name, the salt, the place or a step of the path of a shared file
        check(
            rewrite(&share, |m| {
                *leaf_mut(m, 0, "content") = s(&b64::encode(b"a finding that was never made"))
            }),
            "not_in_release",
        );
        check(
            rewrite(&share, |m| *leaf_mut(m, 0, "name") = s("finding-00.txt")),
            "not_in_release",
        );
        check(
            rewrite(&share, |m| {
                *leaf_mut(m, 0, "salt") = s(&b64::encode(&[0u8; 32]))
            }),
            "not_in_release",
        );
        check(
            rewrite(&share, |m| *leaf_mut(m, 0, "index") = Value::Int(2)),
            "not_in_release",
        );
        check(
            rewrite(&share, |m| {
                let Value::Array(p) = leaf_mut(m, 0, "path") else {
                    panic!()
                };
                p[0] = s(&"0".repeat(64));
            }),
            "not_in_release",
        );
        check(
            rewrite(&share, |m| *leaf_mut(m, 0, "index") = Value::Int(5)),
            "leaf_place",
        );
        check(
            rewrite(&share, |m| *leaf_mut(m, 0, "index") = Value::Int(-1)),
            "leaf_place",
        );
        check(
            rewrite(&share, |m| *leaf_mut(m, 0, "name") = s("../etc/passwd")),
            "bad_file_name",
        );
        check(
            rewrite(&share, |m| {
                *leaf_mut(m, 0, "salt") = s(&b64::encode(&[0u8; 31]))
            }),
            "leaf_shape",
        );
        // the same file twice, the files in another order, no file, a field too many
        check(
            rewrite(&share, |m| {
                let Value::Array(l) = &mut m.iter_mut().find(|(k, _)| k == "leaves").unwrap().1
                else {
                    panic!()
                };
                l.swap(0, 1);
            }),
            "leaf_order",
        );
        check(
            rewrite(&share, |m| {
                let Value::Array(l) = &mut m.iter_mut().find(|(k, _)| k == "leaves").unwrap().1
                else {
                    panic!()
                };
                l[1] = l[0].clone();
            }),
            "leaf_order",
        );
        check(
            rewrite(&share, |m| {
                let Value::Array(l) = &mut m.iter_mut().find(|(k, _)| k == "leaves").unwrap().1
                else {
                    panic!()
                };
                l.clear();
            }),
            "leaf_count",
        );
        check(
            rewrite(&share, |m| m.push(("note".into(), s("x")))),
            "share_shape",
        );
        check(
            rewrite(&share, |m| {
                m.iter_mut().find(|(k, _)| k == "version").unwrap().1 = Value::Int(2)
            }),
            "unsupported_version",
        );
        check(
            rewrite(&share, |m| {
                m.iter_mut().find(|(k, _)| k == "format").unwrap().1 = s("kakurega-capsule")
            }),
            "share_shape",
        );
        // a key added to the list, or the list emptied
        let extra = b64::encode(&Signer::from_seed([1u8; 32]).public());
        check(
            rewrite(&share, |m| {
                let Value::Array(k) = &mut m.iter_mut().find(|(k, _)| k == "keys").unwrap().1
                else {
                    panic!()
                };
                k.push(s(&extra));
            }),
            "key_list",
        );
        check(
            rewrite(&share, |m| {
                let Value::Array(k) = &mut m.iter_mut().find(|(k, _)| k == "keys").unwrap().1
                else {
                    panic!()
                };
                k.clear();
            }),
            "no_keys",
        );
        // not canonical (a space added), not JSON, too large
        let mut spaced = share.clone();
        spaced.insert(1, b' ');
        check(spaced, "share_not_canonical");
        check(b"not json".to_vec(), "share_not_json");
        check(vec![b' '; MAX_SHARE_BYTES + 1], "share_too_large");
    }

    #[test]
    fn sh05_a_disclosure_signed_by_someone_else_or_for_another_tree_cannot_carry_a_file() {
        let f = files(4);
        let mine = build_disclosure(b"c", b"r", &f, &SEED, &signer()).unwrap();
        // a forger signs their own statement over a tree that holds a file of their choosing, and presents the owner's key
        let forger = Signer::from_seed([1u8; 32]);
        let mut forged_files = f.clone();
        forged_files.insert(
            "finding-01.txt".into(),
            b"a finding the owner never made".to_vec(),
        );
        let theirs = build_disclosure(b"c", b"r", &forged_files, &SEED, &forger).unwrap();
        let share = build_share(
            &theirs,
            &signer().public(),
            &forged_files,
            &SEED,
            &["finding-01.txt"],
        )
        .unwrap();
        assert_eq!(
            verify_share(&share, &ShareOptions::default()).reason,
            Some("unknown_signer")
        );
        // signed by the forger's own key it verifies as theirs, and a reader who pinned the owner refuses it
        let own = build_share(
            &theirs,
            &forger.public(),
            &forged_files,
            &SEED,
            &["finding-01.txt"],
        )
        .unwrap();
        let r = verify_share(&own, &ShareOptions::default());
        assert_eq!(r.signer, Some(forger.key_id()));
        assert_eq!(
            verify_share(
                &own,
                &ShareOptions {
                    pinned_keys: Some(vec![signer().public()])
                }
            )
            .reason,
            Some("untrusted_signer")
        );
        // the owner's real statement cannot be made to carry the forged file
        assert_eq!(
            build_share(
                &mine,
                &signer().public(),
                &forged_files,
                &SEED,
                &["finding-01.txt"]
            ),
            Err(ShareError::RootMismatch)
        );
        // nor a file with another seed, a name that is not there, a name twice, none, or too many
        assert_eq!(
            build_share(
                &mine,
                &signer().public(),
                &f,
                &[1u8; 32],
                &["finding-01.txt"]
            ),
            Err(ShareError::RootMismatch)
        );
        assert_eq!(
            build_share(&mine, &signer().public(), &f, &SEED, &["finding-09.txt"]),
            Err(ShareError::Names)
        );
        assert_eq!(
            build_share(
                &mine,
                &signer().public(),
                &f,
                &SEED,
                &["finding-01.txt", "finding-01.txt"]
            ),
            Err(ShareError::Names)
        );
        assert_eq!(
            build_share(&mine, &signer().public(), &f, &SEED, &[]),
            Err(ShareError::Names)
        );
        assert_eq!(
            build_share(
                b"not an envelope",
                &signer().public(),
                &f,
                &SEED,
                &["finding-01.txt"]
            ),
            Err(ShareError::Disclosure)
        );
        // a capsule's release envelope is not a disclosure statement
        let release =
            dsse::encode(&dsse::sign(crate::capsule::RELEASE_TYPE, b"{}", &signer())).into_bytes();
        let Err(e) = build_share(&release, &signer().public(), &f, &SEED, &["finding-01.txt"])
        else {
            panic!()
        };
        assert!(matches!(
            e,
            ShareError::Disclosure | ShareError::RootMismatch
        ));
        // no files, too many files, or a file name that is not plain: no statement is made
        assert!(build_disclosure(b"c", b"r", &BTreeMap::new(), &SEED, &signer()).is_none());
        let mut bad = f.clone();
        bad.insert("../x".into(), vec![]);
        assert!(build_disclosure(b"c", b"r", &bad, &SEED, &signer()).is_none());
    }

    #[test]
    fn sh06_the_statement_is_the_same_bytes_every_time_and_two_capsules_with_the_same_files_have_different_trees(
    ) {
        let f = files(3);
        let a = build_disclosure(b"c", b"r", &f, &SEED, &signer()).unwrap();
        assert_eq!(
            a,
            build_disclosure(b"c", b"r", &f, &SEED, &signer()).unwrap()
        );
        let b = build_disclosure(b"c", b"r", &f, &[2u8; 32], &signer()).unwrap();
        assert_ne!(
            a, b,
            "another seed, another root: a salted hash of one capsule says nothing about another"
        );
        let s1 = build_share(&a, &signer().public(), &f, &SEED, &["finding-00.txt"]).unwrap();
        assert_eq!(
            s1,
            build_share(&a, &signer().public(), &f, &SEED, &["finding-00.txt"]).unwrap()
        );
    }
}
