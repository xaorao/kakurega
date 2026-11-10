use kk_evidence::b64;
use kk_evidence::canon::{self, Value};
use kk_evidence::capsule::{self, VerifyOptions};

fn get<'a>(v: &'a Value, k: &str) -> &'a Value {
    match v {
        Value::Object(m) => {
            &m.iter()
                .find(|(key, _)| key == k)
                .unwrap_or_else(|| panic!("missing {k}"))
                .1
        }
        _ => panic!("not an object"),
    }
}

fn text(v: &Value) -> &str {
    match v {
        Value::Str(s) => s,
        _ => panic!("not a string"),
    }
}

#[test]
fn every_shared_vector_gets_the_same_verdict_and_reason_in_rust() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/capsule/vectors.json"
    );
    let doc = canon::parse_strict(&std::fs::read(path).expect("vectors")).expect("parse");
    let Value::Array(vectors) = get(&doc, "vectors") else {
        panic!("vectors")
    };
    assert!(vectors.len() >= 45);
    for v in vectors {
        let name = text(get(v, "name"));
        let opts = get(v, "options");
        let pinned = match get(opts, "pinned") {
            Value::Null => None,
            Value::Array(keys) => Some(
                keys.iter()
                    .map(|k| {
                        let raw = b64::decode(text(k)).expect("key");
                        <[u8; 32]>::try_from(raw).expect("32 bytes")
                    })
                    .collect(),
            ),
            _ => panic!("pinned"),
        };
        let Value::Int(now) = get(opts, "now") else {
            panic!("now")
        };
        let recipient = match get(opts, "recipient") {
            Value::Null => None,
            Value::Str(s) => Some(s.clone()),
            _ => panic!("recipient"),
        };
        let bytes = b64::decode(text(get(v, "capsule"))).expect("capsule");
        let report = capsule::verify(
            &bytes,
            &VerifyOptions {
                pinned_keys: pinned,
                now: *now,
                expected_recipient: recipient,
            },
        );
        let expect_valid = matches!(get(v, "valid"), Value::Bool(true));
        assert_eq!(report.valid, expect_valid, "{name}: {report:?}");
        let expect_reason = match get(v, "reason") {
            Value::Null => None,
            Value::Str(s) => Some(s.as_str()),
            _ => panic!("reason"),
        };
        assert_eq!(report.reason, expect_reason, "{name}");
        let Value::Int(expected_receipts) = get(v, "receipts") else {
            panic!("receipts")
        };
        assert_eq!(
            report.receipts.len() as i64,
            *expected_receipts,
            "{name}: receipts"
        );
        if expect_valid {
            assert_eq!(
                report.signer.as_ref().map(|k| k.as_str()),
                Some(text(get(v, "signer"))),
                "{name}"
            );
        }
    }
}

#[test]
fn the_generator_is_deterministic() {
    let out = std::env::temp_dir().join("kk-capsule-vectors-check.json");
    let status = std::process::Command::new(env!("CARGO"))
        .args([
            "run",
            "-q",
            "-p",
            "kk-evidence",
            "--example",
            "gen_capsule_vectors",
            "--",
        ])
        .arg(&out)
        .status()
        .expect("run generator");
    assert!(status.success());
    let fresh = std::fs::read(&out).expect("fresh");
    let committed = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/capsule/vectors.json"
    ))
    .expect("committed");
    assert_eq!(
        fresh, committed,
        "fixtures/capsule/vectors.json is out of date; regenerate it"
    );
}
