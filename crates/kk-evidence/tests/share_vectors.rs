use kk_evidence::b64;
use kk_evidence::canon::{self, Value};
use kk_evidence::share::{self, ShareOptions};

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

const PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/share/vectors.json"
);

#[test]
fn every_shared_share_vector_gets_the_same_verdict_and_reason_in_rust() {
    let doc = canon::parse_strict(&std::fs::read(PATH).expect("vectors")).expect("parse");
    let Value::Array(vectors) = get(&doc, "vectors") else {
        panic!("vectors")
    };
    assert!(vectors.len() >= 55);
    for v in vectors {
        let name = text(get(v, "name"));
        let pinned = match get(v, "pinned") {
            Value::Null => None,
            Value::Array(keys) => Some(
                keys.iter()
                    .map(|k| {
                        <[u8; 32]>::try_from(b64::decode(text(k)).expect("key")).expect("32 bytes")
                    })
                    .collect(),
            ),
            _ => panic!("pinned"),
        };
        let bytes = b64::decode(text(get(v, "share"))).expect("share");
        let report = share::verify_share(
            &bytes,
            &ShareOptions {
                pinned_keys: pinned,
            },
        );
        assert_eq!(Value::Bool(report.valid), *get(v, "valid"), "{name}");
        let reason = match get(v, "reason") {
            Value::Null => None,
            Value::Str(s) => Some(s.as_str()),
            _ => panic!("reason"),
        };
        assert_eq!(report.reason, reason, "{name}");
        let Value::Array(files) = get(v, "files") else {
            panic!("files")
        };
        assert_eq!(
            report
                .files
                .iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>(),
            files.iter().map(text).collect::<Vec<_>>(),
            "{name}"
        );
        if report.valid {
            assert_eq!(
                report.signer.as_ref().map(|k| k.as_str()),
                Some(text(get(v, "signer"))),
                "{name}"
            );
        }
    }
}

#[test]
fn the_share_vector_generator_is_deterministic() {
    let out = std::env::temp_dir().join("kk-share-vectors-check.json");
    let status = std::process::Command::new(env!("CARGO"))
        .args([
            "run",
            "-q",
            "-p",
            "kk-evidence",
            "--example",
            "gen_share_vectors",
            "--",
        ])
        .arg(&out)
        .status()
        .expect("run generator");
    assert!(status.success());
    assert_eq!(
        std::fs::read(&out).expect("fresh"),
        std::fs::read(PATH).expect("committed"),
        "fixtures/share/vectors.json is out of date; regenerate it"
    );
}
