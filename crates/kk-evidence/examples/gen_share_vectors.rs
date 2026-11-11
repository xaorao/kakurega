use kk_evidence::b64;
use kk_evidence::canon::{self, Value};
use kk_evidence::dsse;
use kk_evidence::keys::Signer;
use kk_evidence::share::{self, ShareOptions, DISCLOSURE_TYPE};
use std::collections::BTreeMap;

const SEED: [u8; 32] = [9u8; 32];

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

fn made(signer: &Signer, n: usize, names: &[&str]) -> Vec<u8> {
    let f = files(n);
    let d = share::build_disclosure(
        b"the capsule bytes",
        b"the release payload",
        &f,
        &SEED,
        signer,
    )
    .expect("disclosure");
    share::build_share(&d, &signer.public(), &f, &SEED, names).expect("share")
}

fn changed(bytes: &[u8], change: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut v = canon::parse_strict(bytes).expect("parse");
    change(&mut v);
    canon::serialize(&v).into_bytes()
}

/// A share whose disclosure statement was rewritten and signed again by `signer`.
fn restated(
    bytes: &[u8],
    signer: &Signer,
    payload_type: &str,
    change: impl FnOnce(&mut Value),
) -> Vec<u8> {
    changed(bytes, |v| {
        let env = dsse::from_value(get_mut(v, "envelope")).expect("envelope");
        let mut st = canon::parse_strict(&env.payload).expect("statement");
        change(&mut st);
        let payload = canon::serialize(&st).into_bytes();
        *get_mut(v, "envelope") = dsse::to_value(&dsse::sign(payload_type, &payload, signer));
    })
}

fn main() {
    let signer = Signer::from_seed([1u8; 32]);
    let other = Signer::from_seed([2u8; 32]);
    let one = made(&signer, 7, &["finding-03.txt"]);
    let two = made(&signer, 5, &["finding-01.txt", "finding-04.txt"]);
    let mut out: Vec<Value> = Vec::new();
    let mut add = |name: &str,
                   bytes: Vec<u8>,
                   pinned: Option<&[[u8; 32]]>,
                   valid: bool,
                   reason: Option<&str>| {
        let report = share::verify_share(
            &bytes,
            &ShareOptions {
                pinned_keys: pinned.map(|k| k.to_vec()),
            },
        );
        assert_eq!((report.valid, report.reason), (valid, reason), "{name}");
        out.push(obj(vec![
            ("name", s(name)),
            ("share", s(&b64::encode(&bytes))),
            (
                "pinned",
                match pinned {
                    None => Value::Null,
                    Some(keys) => Value::Array(keys.iter().map(|k| s(&b64::encode(k))).collect()),
                },
            ),
            ("valid", Value::Bool(valid)),
            ("reason", reason.map(s).unwrap_or(Value::Null)),
            (
                "signer",
                report
                    .signer
                    .as_ref()
                    .map(|k| s(k.as_str()))
                    .unwrap_or(Value::Null),
            ),
            ("total_files", Value::Int(report.total_files as i64)),
            (
                "files",
                Value::Array(report.files.iter().map(|(n, _)| s(n)).collect()),
            ),
        ]));
    };
    let leaf = |v: &mut Value, i: usize, field: &str, to: Value| {
        *get_mut(&mut arr_mut(get_mut(v, "leaves"))[i], field) = to;
    };

    add("one file of seven", one.clone(), None, true, None);
    add("two files of five", two.clone(), None, true, None);
    add(
        "three files of seven, given out of order",
        made(
            &signer,
            7,
            &["finding-06.txt", "finding-00.txt", "finding-03.txt"],
        ),
        None,
        true,
        None,
    );
    add(
        "the only file of a release of one",
        made(&signer, 1, &["finding-00.txt"]),
        None,
        true,
        None,
    );
    for n in [2usize, 3, 6, 8, 9, 13] {
        let last = format!("finding-{:02}.txt", n - 1);
        add(
            &format!("the last file of {n}"),
            made(&signer, n, &[&last]),
            None,
            true,
            None,
        );
        add(
            &format!("the first file of {n}"),
            made(&signer, n, &["finding-00.txt"]),
            None,
            true,
            None,
        );
    }
    {
        // a shared file is shown as text by any reader; this one tries to be markup
        let mut f = files(3);
        f.insert(
            "finding-01.txt".into(),
            b"<img src=x onerror=\"window.pwned=1\"><script>window.pwned=1</script> The token stays valid after logout.".to_vec(),
        );
        let d = share::build_disclosure(
            b"the capsule bytes",
            b"the release payload",
            &f,
            &SEED,
            &signer,
        )
        .expect("disclosure");
        let hostile = share::build_share(&d, &signer.public(), &f, &SEED, &["finding-01.txt"])
            .expect("share");
        add(
            "a file whose text looks like markup",
            hostile,
            None,
            true,
            None,
        );
    }
    add(
        "pinned to its signer",
        one.clone(),
        Some(&[signer.public()]),
        true,
        None,
    );
    add(
        "pinned to another key",
        one.clone(),
        Some(&[other.public()]),
        false,
        Some("untrusted_signer"),
    );
    add(
        "an empty pin list",
        one.clone(),
        Some(&[]),
        false,
        Some("no_keys"),
    );

    add(
        "content changed",
        changed(&two, |v| {
            leaf(
                v,
                0,
                "content",
                s(&b64::encode(b"a finding that was never made")),
            )
        }),
        None,
        false,
        Some("not_in_release"),
    );
    add(
        "name changed to another file's",
        changed(&two, |v| leaf(v, 0, "name", s("finding-00.txt"))),
        None,
        false,
        Some("not_in_release"),
    );
    add(
        "salt changed",
        changed(&two, |v| leaf(v, 0, "salt", s(&b64::encode(&[0u8; 32])))),
        None,
        false,
        Some("not_in_release"),
    );
    add(
        "place changed",
        changed(&two, |v| leaf(v, 0, "index", Value::Int(2))),
        None,
        false,
        Some("not_in_release"),
    );
    add(
        "a step of the path changed",
        changed(&two, |v| {
            arr_mut(get_mut(&mut arr_mut(get_mut(v, "leaves"))[0], "path"))[0] = s(&"0".repeat(64))
        }),
        None,
        false,
        Some("not_in_release"),
    );
    add(
        "a step of the path removed",
        changed(&two, |v| {
            arr_mut(get_mut(&mut arr_mut(get_mut(v, "leaves"))[0], "path")).pop();
        }),
        None,
        false,
        Some("not_in_release"),
    );
    add(
        "a step added to the path",
        changed(&two, |v| {
            arr_mut(get_mut(&mut arr_mut(get_mut(v, "leaves"))[0], "path")).push(s(&"0".repeat(64)))
        }),
        None,
        false,
        Some("not_in_release"),
    );
    add(
        "nine steps in a path",
        changed(&two, |v| {
            let p = arr_mut(get_mut(&mut arr_mut(get_mut(v, "leaves"))[0], "path"));
            while p.len() < 9 {
                p.push(s(&"0".repeat(64)));
            }
        }),
        None,
        false,
        Some("leaf_shape"),
    );
    add(
        "place beyond the release",
        changed(&two, |v| leaf(v, 0, "index", Value::Int(5))),
        None,
        false,
        Some("leaf_place"),
    );
    add(
        "negative place",
        changed(&two, |v| leaf(v, 0, "index", Value::Int(-1))),
        None,
        false,
        Some("leaf_place"),
    );
    add(
        "two leaves at one place",
        changed(&two, |v| leaf(v, 1, "index", Value::Int(1))),
        None,
        false,
        Some("leaf_place"),
    );
    add(
        "a name that leaves the folder",
        changed(&two, |v| leaf(v, 0, "name", s("../etc/passwd"))),
        None,
        false,
        Some("bad_file_name"),
    );
    add(
        "a salt of 31 bytes",
        changed(&two, |v| leaf(v, 0, "salt", s(&b64::encode(&[0u8; 31])))),
        None,
        false,
        Some("leaf_shape"),
    );
    add(
        "content that is not base64url",
        changed(&two, |v| leaf(v, 0, "content", s("***"))),
        None,
        false,
        Some("leaf_shape"),
    );
    add(
        "a path step that is not a hash",
        changed(&two, |v| {
            arr_mut(get_mut(&mut arr_mut(get_mut(v, "leaves"))[0], "path"))[0] = s("abc")
        }),
        None,
        false,
        Some("leaf_shape"),
    );
    add(
        "a leaf with a field too many",
        changed(&two, |v| {
            if let Value::Object(m) = &mut arr_mut(get_mut(v, "leaves"))[0] {
                m.push(("note".into(), s("x")));
            }
        }),
        None,
        false,
        Some("leaf_shape"),
    );
    add(
        "the files in another order",
        changed(&two, |v| arr_mut(get_mut(v, "leaves")).swap(0, 1)),
        None,
        false,
        Some("leaf_order"),
    );
    add(
        "the same file twice",
        changed(&two, |v| {
            let l = arr_mut(get_mut(v, "leaves"));
            l[1] = l[0].clone();
        }),
        None,
        false,
        Some("leaf_order"),
    );
    add(
        "no file",
        changed(&two, |v| arr_mut(get_mut(v, "leaves")).clear()),
        None,
        false,
        Some("leaf_count"),
    );
    add(
        "a top field too many",
        changed(&two, |v| {
            if let Value::Object(m) = v {
                m.push(("note".into(), s("x")));
            }
        }),
        None,
        false,
        Some("share_shape"),
    );
    add(
        "another format name",
        changed(&two, |v| *get_mut(v, "format") = s("kakurega-capsule")),
        None,
        false,
        Some("share_shape"),
    );
    add(
        "version 2",
        changed(&two, |v| *get_mut(v, "version") = Value::Int(2)),
        None,
        false,
        Some("unsupported_version"),
    );
    add(
        "a key added to the list",
        changed(&two, |v| {
            arr_mut(get_mut(v, "keys")).push(s(&b64::encode(&other.public())))
        }),
        None,
        false,
        Some("key_list"),
    );
    add(
        "the key swapped for another",
        changed(&two, |v| {
            arr_mut(get_mut(v, "keys"))[0] = s(&b64::encode(&other.public()))
        }),
        None,
        false,
        Some("unknown_signer"),
    );
    add(
        "no key",
        changed(&two, |v| arr_mut(get_mut(v, "keys")).clear()),
        None,
        false,
        Some("no_keys"),
    );
    add(
        "a key that is not 32 bytes",
        changed(&two, |v| arr_mut(get_mut(v, "keys"))[0] = s("AAAA")),
        None,
        false,
        Some("bad_key"),
    );
    add(
        "an envelope that is not one",
        changed(&two, |v| *get_mut(v, "envelope") = s("x")),
        None,
        false,
        Some("bad_envelope"),
    );
    add(
        "the signature of another statement",
        changed(&two, |v| {
            let sig = get_mut(
                &mut arr_mut(get_mut(get_mut(v, "envelope"), "signatures"))[0],
                "sig",
            )
            .clone();
            let mut o = canon::parse_strict(&one).unwrap();
            let other_sig = get_mut(
                &mut arr_mut(get_mut(get_mut(&mut o, "envelope"), "signatures"))[0],
                "sig",
            )
            .clone();
            assert_ne!(sig, other_sig);
            *get_mut(
                &mut arr_mut(get_mut(get_mut(v, "envelope"), "signatures"))[0],
                "sig",
            ) = other_sig;
        }),
        None,
        false,
        Some("bad_signature"),
    );

    add(
        "a statement of another kind, signed",
        restated(
            &two,
            &signer,
            "application/vnd.kakurega.release.v1+json",
            |_| {},
        ),
        None,
        false,
        Some("unknown_payload_type"),
    );
    add(
        "a statement with a field too many, signed",
        restated(&two, &signer, DISCLOSURE_TYPE, |st| {
            if let Value::Object(m) = st {
                m.push(("note".into(), s("x")));
            }
        }),
        None,
        false,
        Some("disclosure_shape"),
    );
    add(
        "a statement of version 2, signed",
        restated(&two, &signer, DISCLOSURE_TYPE, |st| {
            *get_mut(st, "version") = Value::Int(2)
        }),
        None,
        false,
        Some("unsupported_version"),
    );
    add(
        "a statement of another scheme, signed",
        restated(&two, &signer, DISCLOSURE_TYPE, |st| {
            *get_mut(st, "scheme") = s("plain-sha256")
        }),
        None,
        false,
        Some("unsupported_version"),
    );
    add(
        "a statement that says no files, signed",
        restated(&two, &signer, DISCLOSURE_TYPE, |st| {
            *get_mut(st, "leaves") = Value::Int(0)
        }),
        None,
        false,
        Some("disclosure_shape"),
    );
    add(
        "a statement that says more files, signed",
        restated(&two, &signer, DISCLOSURE_TYPE, |st| {
            *get_mut(st, "leaves") = Value::Int(6)
        }),
        None,
        false,
        Some("not_in_release"),
    );
    add(
        "a statement with a root that is not a hash, signed",
        restated(&two, &signer, DISCLOSURE_TYPE, |st| {
            *get_mut(st, "root") = s("abc")
        }),
        None,
        false,
        Some("disclosure_shape"),
    );
    add(
        "a statement with a capsule digest that is not a hash, signed",
        restated(&two, &signer, DISCLOSURE_TYPE, |st| {
            *get_mut(st, "capsule_sha256") = s("abc")
        }),
        None,
        false,
        Some("disclosure_shape"),
    );
    add(
        "the statement signed by someone else, with the owner's key listed",
        restated(&two, &other, DISCLOSURE_TYPE, |_| {}),
        None,
        false,
        Some("unknown_signer"),
    );

    let mut spaced = two.clone();
    spaced.insert(1, b' ');
    add(
        "a space added",
        spaced,
        None,
        false,
        Some("share_not_canonical"),
    );
    add(
        "not JSON",
        b"not json".to_vec(),
        None,
        false,
        Some("share_not_json"),
    );

    let doc = obj(vec![
        ("vectors", Value::Array(out)),
        ("note", s("generated by crates/kk-evidence/examples/gen_share_vectors.rs; do not edit by hand")),
    ]);
    let path = std::env::args().nth(1).expect("output path");
    std::fs::write(path, canon::serialize(&doc) + "\n").expect("write");
}
