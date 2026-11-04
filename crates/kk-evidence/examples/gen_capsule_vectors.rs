use kk_evidence::b64;
use kk_evidence::canon::{self, Value};
use kk_evidence::capsule::{
    self, sha256_hex, ClaimSpec, ReleaseSpec, VerifyOptions, CLAIM_TYPE, RELEASE_TYPE,
};
use kk_evidence::dsse;
use kk_evidence::keys::Signer;
use std::collections::BTreeMap;

const NOW: i64 = 1_700_000_100;

fn obj(v: Vec<(&str, Value)>) -> Value {
    Value::Object(v.into_iter().map(|(k, x)| (k.to_string(), x)).collect())
}

fn s(t: &str) -> Value {
    Value::Str(t.to_string())
}

fn get_mut<'a>(v: &'a mut Value, key: &str) -> &'a mut Value {
    match v {
        Value::Object(m) => &mut m.iter_mut().find(|(k, _)| k == key).expect("key").1,
        _ => panic!("not an object"),
    }
}

fn arr_mut(v: &mut Value) -> &mut Vec<Value> {
    match v {
        Value::Array(a) => a,
        _ => panic!("not an array"),
    }
}

fn parse(bytes: &[u8]) -> Value {
    canon::parse_strict(bytes).expect("parse")
}

fn emit(v: &Value) -> Vec<u8> {
    canon::serialize(v).into_bytes()
}

fn base_files() -> BTreeMap<String, Vec<u8>> {
    let mut f = BTreeMap::new();
    f.insert(
        "finding.md".to_string(),
        b"Token reuse after logout in auth/session.rs line 88.\n".to_vec(),
    );
    f.insert(
        "repro.sh".to_string(),
        b"#!/bin/sh\necho reproduced\n".to_vec(),
    );
    f
}

fn spec() -> ReleaseSpec {
    ReleaseSpec {
        capsule_id: "00112233445566778899aabbccddeeff".into(),
        created: 1_700_000_000,
        expires: 1_700_086_400,
        recipient: "acme-audit".into(),
        policy_version: "policy-1".into(),
        binding_level: "B1".into(),
        route: "device_only".into(),
        not_established: vec![
            "That the private source repository is the one that was reviewed.".into(),
        ],
        source_commitment_scheme: "hmac-sha256-keyed".into(),
        source_commitment_digest: sha256_hex(b"manifest"),
        source_manifest_files: 38,
    }
}

fn claim() -> ClaimSpec {
    ClaimSpec {
        statement: "Session tokens remain valid after logout.".into(),
        evidence: vec!["finding.md".into(), "repro.sh".into()],
        how_to_check: "Run repro.sh and compare with finding.md".into(),
    }
}

fn opts_json(pinned: Option<&[[u8; 32]]>, now: i64, recipient: Option<&str>) -> Value {
    obj(vec![
        (
            "pinned",
            match pinned {
                None => Value::Null,
                Some(keys) => {
                    Value::Array(keys.iter().map(|k| Value::Str(b64::encode(k))).collect())
                }
            },
        ),
        ("now", Value::Int(now)),
        ("recipient", recipient.map(s).unwrap_or(Value::Null)),
    ])
}

fn main() {
    let signer = Signer::from_seed([1u8; 32]);
    let other = Signer::from_seed([2u8; 32]);
    let good = capsule::build(&spec(), &[claim()], &base_files(), &signer);
    let mut out: Vec<Value> = Vec::new();
    let mut add = |name: &str,
                   bytes: Vec<u8>,
                   pinned: Option<&[[u8; 32]]>,
                   now: i64,
                   recipient: Option<&str>,
                   expect_valid: bool,
                   reason: Option<&str>| {
        let report = capsule::verify(
            &bytes,
            &VerifyOptions {
                pinned_keys: pinned.map(|p| p.to_vec()),
                now,
                expected_recipient: recipient.map(String::from),
            },
        );
        assert_eq!(report.valid, expect_valid, "{name}: {report:?}");
        assert_eq!(
            report.reason,
            reason.map(|r| &*Box::leak(r.to_string().into_boxed_str())),
            "{name}: {report:?}"
        );
        out.push(obj(vec![
            ("name", s(name)),
            ("capsule", Value::Str(b64::encode(&bytes))),
            ("options", opts_json(pinned, now, recipient)),
            ("valid", Value::Bool(expect_valid)),
            ("reason", reason.map(s).unwrap_or(Value::Null)),
            (
                "signer",
                report
                    .signer
                    .as_ref()
                    .map(|k| s(k.as_str()))
                    .unwrap_or(Value::Null),
            ),
        ]));
    };
    let pin_good = [signer.public()];
    let pin_other = [other.public()];

    add("valid", good.clone(), None, NOW, None, true, None);
    add(
        "valid pinned",
        good.clone(),
        Some(&pin_good),
        NOW,
        Some("acme-audit"),
        true,
        None,
    );
    add(
        "pinned to a different key",
        good.clone(),
        Some(&pin_other),
        NOW,
        None,
        false,
        Some("untrusted_signer"),
    );
    add(
        "expired",
        good.clone(),
        None,
        1_700_086_401,
        None,
        false,
        Some("expired"),
    );
    add(
        "wrong recipient",
        good.clone(),
        None,
        NOW,
        Some("someone-else"),
        false,
        Some("wrong_recipient"),
    );

    let mut v = parse(&good);
    {
        let envs = arr_mut(get_mut(&mut v, "envelopes"));
        let sigs = arr_mut(get_mut(&mut envs[0], "signatures"));
        let mut raw = b64::decode(match get_mut(&mut sigs[0], "sig") {
            Value::Str(t) => t,
            _ => unreachable!(),
        })
        .unwrap();
        raw[3] ^= 0x01;
        *get_mut(&mut sigs[0], "sig") = Value::Str(b64::encode(&raw));
    }
    add(
        "signature bit flipped",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("bad_signature"),
    );

    let mut v = parse(&good);
    {
        let envs = arr_mut(get_mut(&mut v, "envelopes"));
        let payload = match get_mut(&mut envs[0], "payload") {
            Value::Str(t) => b64::decode(t).unwrap(),
            _ => unreachable!(),
        };
        let text = String::from_utf8(payload)
            .unwrap()
            .replace("acme-audit", "acme-auditX");
        *get_mut(&mut envs[0], "payload") = Value::Str(b64::encode(text.as_bytes()));
    }
    add(
        "release payload edited after signing",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("bad_signature"),
    );

    let mut v = parse(&good);
    *get_mut(get_mut(&mut v, "files"), "repro.sh") =
        Value::Str(b64::encode(b"#!/bin/sh\necho changed!!\n"));
    add(
        "attachment content changed",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("digest_mismatch"),
    );

    let mut v = parse(&good);
    if let Value::Object(m) = get_mut(&mut v, "files") {
        m.retain(|(k, _)| k != "repro.sh");
    }
    add(
        "attachment removed",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("attachment_missing"),
    );

    let mut v = parse(&good);
    if let Value::Object(m) = get_mut(&mut v, "files") {
        m.push(("extra.txt".to_string(), Value::Str(b64::encode(b"hidden"))));
    }
    add(
        "attachment added",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("attachment_extra"),
    );

    let mut v = parse(&good);
    {
        let envs = arr_mut(get_mut(&mut v, "envelopes"));
        *get_mut(&mut envs[0], "payloadType") = Value::Str(CLAIM_TYPE.to_string());
    }
    add(
        "payload type swapped (type confusion)",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("bad_signature"),
    );

    let mut v = parse(&good);
    {
        let envs = arr_mut(get_mut(&mut v, "envelopes"));
        let payload = match get_mut(&mut envs[0], "payload") {
            Value::Str(t) => b64::decode(t).unwrap(),
            _ => unreachable!(),
        };
        let e = dsse::sign("application/x-unknown+json", &payload, &signer);
        envs[0] = dsse::to_value(&e);
    }
    add(
        "unknown payload type, validly signed",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("unknown_payload_type"),
    );

    let resigned = capsule::build(&spec(), &[claim()], &base_files(), &other);
    add(
        "whole capsule re-signed by another key, not pinned",
        resigned.clone(),
        None,
        NOW,
        None,
        true,
        None,
    );
    add(
        "whole capsule re-signed by another key, original pinned",
        resigned,
        Some(&pin_good),
        NOW,
        None,
        false,
        Some("untrusted_signer"),
    );

    let mut v = parse(&good);
    {
        let envs = arr_mut(get_mut(&mut v, "envelopes"));
        let payload = match get_mut(&mut envs[1], "payload") {
            Value::Str(t) => b64::decode(t).unwrap(),
            _ => unreachable!(),
        };
        envs[1] = dsse::to_value(&dsse::sign(CLAIM_TYPE, &payload, &other));
        arr_mut(get_mut(&mut v, "keys")).push(Value::Str(b64::encode(&other.public())));
    }
    add(
        "claim signed by a second key",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("mixed_signers"),
    );

    let mut v = parse(&good);
    {
        let envs = arr_mut(get_mut(&mut v, "envelopes"));
        let sigs = arr_mut(get_mut(&mut envs[0], "signatures"));
        let dup = sigs[0].clone();
        sigs.push(dup);
    }
    add(
        "two signatures on one envelope",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("signature_count"),
    );

    let release_value = |mutate: &dyn Fn(&mut Value)| -> Vec<u8> {
        let base = capsule::build(&spec(), &[claim()], &base_files(), &signer);
        let mut c = parse(&base);
        let envs = arr_mut(get_mut(&mut c, "envelopes"));
        let payload = match get_mut(&mut envs[0], "payload") {
            Value::Str(t) => b64::decode(t).unwrap(),
            _ => unreachable!(),
        };
        let mut r = parse(&payload);
        mutate(&mut r);
        envs[0] = dsse::to_value(&dsse::sign(RELEASE_TYPE, &emit(&r), &signer));
        let new_digest = sha256_hex(&emit(&r));
        let claim_payload = match get_mut(&mut envs[1], "payload") {
            Value::Str(t) => b64::decode(t).unwrap(),
            _ => unreachable!(),
        };
        let mut cl = parse(&claim_payload);
        *get_mut(&mut cl, "release_payload_sha256") = Value::Str(new_digest);
        envs[1] = dsse::to_value(&dsse::sign(CLAIM_TYPE, &emit(&cl), &signer));
        emit(&c)
    };
    add(
        "expires not after created (signed)",
        release_value(&|r| *get_mut(r, "expires") = Value::Int(1_700_000_000)),
        None,
        NOW,
        None,
        false,
        Some("release_shape"),
    );
    add(
        "unknown binding level (signed)",
        release_value(&|r| *get_mut(get_mut(r, "assurance"), "binding_level") = s("B9")),
        None,
        NOW,
        None,
        false,
        Some("release_shape"),
    );
    add(
        "file name with path traversal (signed)",
        {
            let mut files = base_files();
            let b = files.remove("repro.sh").unwrap();
            files.insert("..x".to_string(), b);
            let mut bytes = capsule::build(&spec(), &[], &files, &signer);
            let _ = &mut bytes;
            bytes
        },
        None,
        NOW,
        None,
        false,
        Some("bad_file_name"),
    );
    add(
        "duplicate file name in release (signed)",
        release_value(&|r| {
            let files = arr_mut(get_mut(r, "files"));
            let dup = files[0].clone();
            files.push(dup);
        }),
        None,
        NOW,
        None,
        false,
        Some("duplicate_file_name"),
    );

    let mut v = parse(&good);
    {
        let envs = arr_mut(get_mut(&mut v, "envelopes"));
        let payload = match get_mut(&mut envs[0], "payload") {
            Value::Str(t) => b64::decode(t).unwrap(),
            _ => unreachable!(),
        };
        let spaced = String::from_utf8(payload).unwrap().replacen("{", "{ ", 1);
        envs[0] = dsse::to_value(&dsse::sign(RELEASE_TYPE, spaced.as_bytes(), &signer));
    }
    add(
        "release payload not canonical (signed)",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("payload_not_canonical"),
    );

    let mut v = parse(&good);
    {
        let envs = arr_mut(get_mut(&mut v, "envelopes"));
        let claim_payload = match get_mut(&mut envs[1], "payload") {
            Value::Str(t) => b64::decode(t).unwrap(),
            _ => unreachable!(),
        };
        let mut cl = parse(&claim_payload);
        *get_mut(&mut cl, "release_payload_sha256") = Value::Str(sha256_hex(b"other release"));
        envs[1] = dsse::to_value(&dsse::sign(CLAIM_TYPE, &emit(&cl), &signer));
    }
    add(
        "claim bound to a different release (signed)",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("claim_not_bound"),
    );

    let mut v = parse(&good);
    {
        let envs = arr_mut(get_mut(&mut v, "envelopes"));
        let claim_payload = match get_mut(&mut envs[1], "payload") {
            Value::Str(t) => b64::decode(t).unwrap(),
            _ => unreachable!(),
        };
        let mut cl = parse(&claim_payload);
        arr_mut(get_mut(&mut cl, "evidence")).push(s("nonexistent.txt"));
        envs[1] = dsse::to_value(&dsse::sign(CLAIM_TYPE, &emit(&cl), &signer));
    }
    add(
        "claim cites a file that is not in the release (signed)",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("claim_evidence_missing"),
    );

    let mut v = parse(&good);
    {
        arr_mut(get_mut(&mut v, "envelopes")).remove(0);
    }
    add(
        "release statement removed",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("release_missing"),
    );

    let mut v = parse(&good);
    {
        let envs = arr_mut(get_mut(&mut v, "envelopes"));
        let first = envs[0].clone();
        envs.push(first);
    }
    add(
        "two release statements",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("duplicate_release"),
    );

    let mut v = parse(&good);
    *get_mut(&mut v, "version") = Value::Int(2);
    add(
        "capsule version 2 (downgrade/upgrade)",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("unsupported_version"),
    );

    let mut v = parse(&good);
    if let Value::Object(m) = &mut v {
        m.push(("note".to_string(), s("hello")));
    }
    add(
        "unknown top-level field",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("capsule_shape"),
    );

    let mut v = parse(&good);
    *get_mut(&mut v, "keys") = Value::Array(vec![]);
    add(
        "no keys listed",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("no_keys"),
    );

    let mut v = parse(&good);
    *get_mut(&mut v, "keys") = Value::Array(vec![Value::Str(b64::encode(&[0u8; 32]))]);
    add(
        "all-zero public key",
        emit(&v),
        None,
        NOW,
        None,
        false,
        Some("bad_key"),
    );

    let mut tail = good.clone();
    tail.push(b'x');
    add(
        "trailing garbage",
        tail,
        None,
        NOW,
        None,
        false,
        Some("capsule_not_json"),
    );
    let mut nl = good.clone();
    nl.push(b'\n');
    add(
        "trailing newline",
        nl,
        None,
        NOW,
        None,
        false,
        Some("capsule_not_canonical"),
    );

    let doc = obj(vec![("vectors", Value::Array(out)), ("note", s("generated by crates/kk-evidence/examples/gen_capsule_vectors.rs; seed keys 0x01.. and 0x02.."))]);
    let path = std::env::args().nth(1).expect("output path");
    std::fs::write(path, canon::serialize(&doc) + "\n").expect("write");
}
