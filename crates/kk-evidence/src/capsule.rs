use crate::b64;
use crate::canon::{self, Value};
use crate::dsse::{self, Envelope};
use crate::keys::{KeyId, Signer, TrustedKeys};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const RELEASE_TYPE: &str = "application/vnd.kakurega.release.v1+json";
pub const CLAIM_TYPE: &str = "application/vnd.kakurega.claim.v1+json";
pub const MAX_FILES: usize = 256;
pub const MAX_TOTAL_BYTES: usize = 64 << 20;

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[derive(Debug, Clone)]
pub struct ReleaseSpec {
    pub capsule_id: String,
    pub created: i64,
    pub expires: i64,
    pub recipient: String,
    pub policy_version: String,
    pub binding_level: String,
    pub route: String,
    pub not_established: Vec<String>,
    pub source_commitment_scheme: String,
    pub source_commitment_digest: String,
    pub source_manifest_files: i64,
}

#[derive(Debug, Clone)]
pub struct ClaimSpec {
    pub statement: String,
    pub evidence: Vec<String>,
    pub how_to_check: String,
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Object(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn s(text: &str) -> Value {
    Value::Str(text.to_string())
}

pub fn valid_file_name(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 128
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
        && !name.contains("..")
}

pub fn build(
    release: &ReleaseSpec,
    claims: &[ClaimSpec],
    files: &BTreeMap<String, Vec<u8>>,
    signer: &Signer,
) -> Vec<u8> {
    let release_files: Vec<Value> = files
        .iter()
        .map(|(name, bytes)| {
            obj(vec![
                ("name", s(name)),
                ("sha256", s(&sha256_hex(bytes))),
                ("bytes", Value::Int(bytes.len() as i64)),
            ])
        })
        .collect();
    let release_value = obj(vec![
        ("type", s("release")),
        ("version", Value::Int(1)),
        ("capsule_id", s(&release.capsule_id)),
        ("created", Value::Int(release.created)),
        ("expires", Value::Int(release.expires)),
        ("recipient", s(&release.recipient)),
        ("policy_version", s(&release.policy_version)),
        ("files", Value::Array(release_files)),
        (
            "assurance",
            obj(vec![
                ("binding_level", s(&release.binding_level)),
                ("route", s(&release.route)),
                (
                    "not_established",
                    Value::Array(release.not_established.iter().map(|x| s(x)).collect()),
                ),
            ]),
        ),
        (
            "source_commitment",
            obj(vec![
                ("scheme", s(&release.source_commitment_scheme)),
                ("digest", s(&release.source_commitment_digest)),
                ("manifest_files", Value::Int(release.source_manifest_files)),
            ]),
        ),
    ]);
    let release_payload = canon::serialize(&release_value).into_bytes();
    let release_digest = sha256_hex(&release_payload);
    let mut envelopes = vec![dsse::to_value(&dsse::sign(
        RELEASE_TYPE,
        &release_payload,
        signer,
    ))];
    for c in claims {
        let claim_value = obj(vec![
            ("type", s("claim")),
            ("version", Value::Int(1)),
            ("release_payload_sha256", s(&release_digest)),
            ("statement", s(&c.statement)),
            (
                "evidence",
                Value::Array(c.evidence.iter().map(|x| s(x)).collect()),
            ),
            ("how_to_check", s(&c.how_to_check)),
        ]);
        let payload = canon::serialize(&claim_value).into_bytes();
        envelopes.push(dsse::to_value(&dsse::sign(CLAIM_TYPE, &payload, signer)));
    }
    let files_value = Value::Object(
        files
            .iter()
            .map(|(k, v)| (k.clone(), Value::Str(b64::encode(v))))
            .collect(),
    );
    let capsule = obj(vec![
        ("format", s("kakurega-capsule")),
        ("version", Value::Int(1)),
        (
            "keys",
            Value::Array(vec![Value::Str(b64::encode(&signer.public()))]),
        ),
        ("envelopes", Value::Array(envelopes)),
        ("files", files_value),
    ]);
    canon::serialize(&capsule).into_bytes()
}

#[derive(Debug, Clone, Default)]
pub struct VerifyOptions {
    pub pinned_keys: Option<Vec<[u8; 32]>>,
    pub now: i64,
    pub expected_recipient: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub valid: bool,
    pub reason: Option<&'static str>,
    pub signer: Option<KeyId>,
    pub signer_pinned: bool,
    pub recipient: Option<String>,
    pub binding_level: Option<String>,
    pub route: Option<String>,
    pub established: Vec<String>,
    pub not_established: Vec<String>,
}

fn fail(reason: &'static str) -> Report {
    Report {
        valid: false,
        reason: Some(reason),
        signer: None,
        signer_pinned: false,
        recipient: None,
        binding_level: None,
        route: None,
        established: Vec::new(),
        not_established: Vec::new(),
    }
}

fn member<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => m.iter().find(|(k, _)| k == name).map(|(_, v)| v),
        _ => None,
    }
}

fn exact_keys(v: &Value, names: &[&str]) -> bool {
    match v {
        Value::Object(m) => {
            m.len() == names.len() && names.iter().all(|n| m.iter().any(|(k, _)| k == n))
        }
        _ => false,
    }
}

fn str_of(v: Option<&Value>) -> Option<&str> {
    match v {
        Some(Value::Str(x)) => Some(x),
        _ => None,
    }
}

fn int_of(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Int(x)) => Some(*x),
        _ => None,
    }
}

fn is_hex64(x: &str) -> bool {
    x.len() == 64
        && x.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn verify(bytes: &[u8], opts: &VerifyOptions) -> Report {
    if bytes.len() > MAX_TOTAL_BYTES * 2 {
        return fail("capsule_too_large");
    }
    let Ok(root) = canon::parse_strict(bytes) else {
        return fail("capsule_not_json");
    };
    if canon::serialize(&root).as_bytes() != bytes {
        return fail("capsule_not_canonical");
    }
    if !exact_keys(&root, &["envelopes", "files", "format", "keys", "version"]) {
        return fail("capsule_shape");
    }
    if str_of(member(&root, "format")) != Some("kakurega-capsule") {
        return fail("capsule_shape");
    }
    if int_of(member(&root, "version")) != Some(1) {
        return fail("unsupported_version");
    }
    let (Some(Value::Array(keys)), Some(Value::Array(envs)), Some(Value::Object(files))) = (
        member(&root, "keys"),
        member(&root, "envelopes"),
        member(&root, "files"),
    ) else {
        return fail("capsule_shape");
    };

    let mut claimed: Vec<[u8; 32]> = Vec::new();
    for k in keys {
        let Value::Str(t) = k else {
            return fail("bad_key");
        };
        let Ok(raw) = b64::decode(t) else {
            return fail("bad_key");
        };
        let Ok(raw): Result<[u8; 32], _> = raw.try_into() else {
            return fail("bad_key");
        };
        claimed.push(raw);
    }
    let pinned = opts.pinned_keys.is_some();
    let source = opts.pinned_keys.as_ref().unwrap_or(&claimed);
    let mut trusted = TrustedKeys::new();
    for k in source {
        if trusted.add(k).is_none() {
            return fail("bad_key");
        }
    }
    if trusted.is_empty() {
        return fail("no_keys");
    }

    if envs.is_empty() || envs.len() > 64 {
        return fail("envelope_count");
    }
    let mut release: Option<(Envelope, Value)> = None;
    let mut claims: Vec<Value> = Vec::new();
    let mut signer: Option<KeyId> = None;
    for e in envs {
        let Ok(env) = dsse::from_value(e) else {
            return fail("bad_envelope");
        };
        let verified = match dsse::verify(&env, &trusted, &[RELEASE_TYPE, CLAIM_TYPE]) {
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
        match &signer {
            None => signer = Some(verified.key_id.clone()),
            Some(s0) if *s0 != verified.key_id => return fail("mixed_signers"),
            _ => {}
        }
        let Ok(value) = canon::parse_strict(&env.payload) else {
            return fail("payload_not_canonical");
        };
        if verified.payload_type == RELEASE_TYPE {
            if release.is_some() {
                return fail("duplicate_release");
            }
            release = Some((env, value));
        } else {
            claims.push(value);
        }
    }
    let Some((release_env, rel)) = release else {
        return fail("release_missing");
    };

    if !exact_keys(
        &rel,
        &[
            "type",
            "version",
            "capsule_id",
            "created",
            "expires",
            "recipient",
            "policy_version",
            "files",
            "assurance",
            "source_commitment",
        ],
    ) || str_of(member(&rel, "type")) != Some("release")
    {
        return fail("release_shape");
    }
    if int_of(member(&rel, "version")) != Some(1) {
        return fail("unsupported_version");
    }
    let (Some(created), Some(expires)) = (
        int_of(member(&rel, "created")),
        int_of(member(&rel, "expires")),
    ) else {
        return fail("release_shape");
    };
    let (Some(capsule_id), Some(recipient), Some(policy_version)) = (
        str_of(member(&rel, "capsule_id")),
        str_of(member(&rel, "recipient")),
        str_of(member(&rel, "policy_version")),
    ) else {
        return fail("release_shape");
    };
    if capsule_id.is_empty()
        || recipient.is_empty()
        || policy_version.is_empty()
        || expires <= created
    {
        return fail("release_shape");
    }
    let Some(assurance) = member(&rel, "assurance") else {
        return fail("release_shape");
    };
    let Some(commit) = member(&rel, "source_commitment") else {
        return fail("release_shape");
    };
    if !exact_keys(assurance, &["binding_level", "route", "not_established"])
        || !exact_keys(commit, &["scheme", "digest", "manifest_files"])
    {
        return fail("release_shape");
    }
    let (Some(level), Some(route), Some(Value::Array(not_est))) = (
        str_of(member(assurance, "binding_level")),
        str_of(member(assurance, "route")),
        member(assurance, "not_established"),
    ) else {
        return fail("release_shape");
    };
    if !matches!(level, "B0" | "B1" | "B2" | "B3") {
        return fail("release_shape");
    }
    let mut not_established: Vec<String> = Vec::new();
    for n in not_est {
        let Value::Str(t) = n else {
            return fail("release_shape");
        };
        not_established.push(t.clone());
    }
    if !str_of(member(commit, "digest")).is_some_and(is_hex64)
        || str_of(member(commit, "scheme")).is_none()
        || int_of(member(commit, "manifest_files")).is_none()
    {
        return fail("release_shape");
    }

    if opts.now > expires {
        return fail("expired");
    }
    if let Some(expected) = &opts.expected_recipient {
        if expected != recipient {
            return fail("wrong_recipient");
        }
    }

    let Some(Value::Array(listed)) = member(&rel, "files") else {
        return fail("release_shape");
    };
    if listed.len() > MAX_FILES {
        return fail("too_many_files");
    }
    let mut names = BTreeSet::new();
    let mut total = 0usize;
    for f in listed {
        if !exact_keys(f, &["name", "sha256", "bytes"]) {
            return fail("release_shape");
        }
        let (Some(name), Some(digest), Some(len)) = (
            str_of(member(f, "name")),
            str_of(member(f, "sha256")),
            int_of(member(f, "bytes")),
        ) else {
            return fail("release_shape");
        };
        if !valid_file_name(name) {
            return fail("bad_file_name");
        }
        if !names.insert(name.to_string()) {
            return fail("duplicate_file_name");
        }
        if !is_hex64(digest) || len < 0 {
            return fail("release_shape");
        }
        let Some((_, Value::Str(content))) = files.iter().find(|(k, _)| k == name) else {
            return fail("attachment_missing");
        };
        let Ok(raw) = b64::decode(content) else {
            return fail("bad_attachment");
        };
        total += raw.len();
        if total > MAX_TOTAL_BYTES {
            return fail("capsule_too_large");
        }
        if raw.len() as i64 != len || sha256_hex(&raw) != digest {
            return fail("digest_mismatch");
        }
    }
    if files.iter().any(|(k, _)| !names.contains(k)) {
        return fail("attachment_extra");
    }

    let release_digest = sha256_hex(&release_env.payload);
    for c in &claims {
        if !exact_keys(
            c,
            &[
                "type",
                "version",
                "release_payload_sha256",
                "statement",
                "evidence",
                "how_to_check",
            ],
        ) || str_of(member(c, "type")) != Some("claim")
            || int_of(member(c, "version")) != Some(1)
        {
            return fail("claim_shape");
        }
        if str_of(member(c, "release_payload_sha256")) != Some(release_digest.as_str()) {
            return fail("claim_not_bound");
        }
        let Some(Value::Array(ev)) = member(c, "evidence") else {
            return fail("claim_shape");
        };
        for item in ev {
            let Value::Str(n) = item else {
                return fail("claim_shape");
            };
            if !names.contains(n) {
                return fail("claim_evidence_missing");
            }
        }
    }

    let signer = signer.expect("at least one envelope verified");
    let mut established = vec![
        format!("The release statement and {} claim statement(s) carry a valid Ed25519 signature from {}.", claims.len(), signer),
        format!("All {} released file(s) match the signed digests and sizes exactly; nothing is missing or extra.", names.len()),
        "Every claim is bound to this exact release statement.".to_string(),
    ];
    if pinned {
        established.push("The signing key is one you pinned.".to_string());
    }
    let mut not_established_all = not_established;
    not_established_all.push("That any claim is true; only that it was signed and that its evidence files are as signed.".to_string());
    if !pinned {
        not_established_all.push("Who holds the signing key: no key was pinned, so the key shown comes from the capsule itself.".to_string());
    }
    Report {
        valid: true,
        reason: None,
        signer: Some(signer),
        signer_pinned: pinned,
        recipient: Some(recipient.to_string()),
        binding_level: Some(level.to_string()),
        route: Some(route.to_string()),
        established,
        not_established: not_established_all,
    }
}
