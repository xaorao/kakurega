use kk_evidence::dsse;
use kk_evidence::keys::{Signer, TrustedKeys};
use kk_packs::*;
use serde_json::{json, Value};

fn signer(seed: u8) -> Signer {
    Signer::from_seed([seed; 32])
}

fn trusted(s: &Signer) -> TrustedKeys {
    let mut t = TrustedKeys::new();
    t.add(&s.public()).unwrap();
    t
}

fn method(name: &str) -> Value {
    json!({"name": name, "description": "d", "triggers": ["contract"], "avoid": [], "body": "Quote clause numbers."})
}

fn payload(name: &str, version: &str, not_after: i64, methods: Vec<Value>) -> Value {
    json!({"format": "kakurega-pack", "version": 1, "name": name, "pack_version": version, "not_after": not_after, "methods": methods})
}

fn sign_value(v: &Value, s: &Signer) -> Vec<u8> {
    let canon = kk_evidence::canon::canonicalize(v.to_string().as_bytes()).unwrap();
    dsse::encode(&dsse::sign(PACK_TYPE, canon.as_bytes(), s)).into_bytes()
}

fn good(s: &Signer) -> Vec<u8> {
    sign_value(
        &payload(
            "legal-kit",
            "1.0.0",
            2_000_000_000,
            vec![method("contract-check"), method("clause-notes")],
        ),
        s,
    )
}

fn ctx<'a>(t: &'a TrustedKeys) -> Context<'a> {
    Context {
        trusted: t,
        now: 1_700_000_000,
        seen: &|_| None,
    }
}

#[test]
fn pk01_a_pack_signed_by_a_pinned_key_yields_its_methods_and_who_signed() {
    let s = signer(1);
    let t = trusted(&s);
    let c = check_pack(&good(&s), &ctx(&t)).unwrap();
    assert_eq!(
        (c.name.as_str(), c.version.to_string().as_str()),
        ("legal-kit", "1.0.0")
    );
    assert_eq!(
        c.methods
            .iter()
            .map(|m| m.name.as_str())
            .collect::<Vec<_>>(),
        ["contract-check", "clause-notes"]
    );
    assert_eq!(c.key_id.as_str(), s.key_id().as_str());
}

#[test]
fn pk02_a_pack_from_a_key_that_was_not_pinned_is_refused() {
    let t = trusted(&signer(1));
    assert!(matches!(
        check_pack(&good(&signer(2)), &ctx(&t)),
        Err(Refusal::NotSigned(_))
    ));
    let empty = TrustedKeys::new();
    assert!(matches!(
        check_pack(&good(&signer(1)), &ctx(&empty)),
        Err(Refusal::NotSigned(_))
    ));
}

#[test]
fn pk03_a_changed_byte_in_the_payload_or_the_wrong_payload_type_is_refused() {
    let s = signer(1);
    let t = trusted(&s);
    let canon = kk_evidence::canon::canonicalize(
        payload(
            "legal-kit",
            "1.0.0",
            2_000_000_000,
            vec![method("a-method")],
        )
        .to_string()
        .as_bytes(),
    )
    .unwrap();
    let mut env = dsse::sign(PACK_TYPE, canon.as_bytes(), &s);
    env.payload = canon.replace("Quote", "Quoto").into_bytes();
    assert!(matches!(
        check_pack(dsse::encode(&env).as_bytes(), &ctx(&t)),
        Err(Refusal::NotSigned(_))
    ));
    let other = dsse::sign(
        "application/vnd.kakurega.update.v1+json",
        canon.as_bytes(),
        &s,
    );
    assert!(matches!(
        check_pack(dsse::encode(&other).as_bytes(), &ctx(&t)),
        Err(Refusal::NotSigned(_))
    ));
    assert!(matches!(
        check_pack(b"not json at all", &ctx(&t)),
        Err(Refusal::NotSigned(_))
    ));
}

#[test]
fn pk04_size_method_count_and_name_uniqueness_are_bounded() {
    let s = signer(1);
    let t = trusted(&s);
    let many: Vec<Value> = (0..=MAX_METHODS)
        .map(|i| method(&format!("m{i}")))
        .collect();
    assert!(matches!(
        check_pack(
            &sign_value(&payload("kit", "1.0.0", 2_000_000_000, many), &s),
            &ctx(&t)
        ),
        Err(Refusal::Shape(_))
    ));
    let dup = vec![method("same-name"), method("same-name")];
    assert!(matches!(
        check_pack(
            &sign_value(&payload("kit", "1.0.0", 2_000_000_000, dup), &s),
            &ctx(&t)
        ),
        Err(Refusal::Shape(_))
    ));
    let mut big = method("big-one");
    big["body"] = json!("x".repeat(8000));
    let huge: Vec<Value> = (0..20)
        .map(|i| {
            let mut b = big.clone();
            b["name"] = json!(format!("big-{i}"));
            b
        })
        .collect();
    let bytes = sign_value(&payload("kit", "1.0.0", 2_000_000_000, huge), &s);
    assert!(
        bytes.len() < MAX_PACK_BYTES,
        "the largest legal pack fits: {}",
        bytes.len()
    );
    assert!(
        check_pack(&bytes, &ctx(&t)).is_ok(),
        "twenty methods of the largest size are accepted"
    );
    let junk = vec![b'x'; MAX_PACK_BYTES + 1];
    assert!(matches!(
        check_pack(&junk, &ctx(&t)),
        Err(Refusal::TooLarge)
    ));
    assert!(
        matches!(
            check_pack(
                &sign_value(&payload("kit", "1.0.0", 2_000_000_000, vec![]), &s),
                &ctx(&t)
            ),
            Err(Refusal::Shape(_))
        ),
        "an empty pack is not a pack"
    );
}

#[test]
fn pk05_one_bad_method_refuses_the_whole_pack_and_names_it() {
    let s = signer(1);
    let t = trusted(&s);
    let mut bad = method("bad method!");
    bad["name"] = json!("bad method!");
    let r = check_pack(
        &sign_value(
            &payload("kit", "1.0.0", 2_000_000_000, vec![method("good-one"), bad]),
            &s,
        ),
        &ctx(&t),
    );
    match r {
        Err(Refusal::Method(n, _)) => assert_eq!(n, "bad method!"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn pk06_a_method_that_tries_to_carry_a_power_is_refused() {
    let s = signer(1);
    let t = trusted(&s);
    for key in ["tools", "route", "credentials", "allow", "hooks", "colour"] {
        let mut m = method("sneaky");
        m[key] = json!(["x"]);
        let r = check_pack(
            &sign_value(&payload("kit", "1.0.0", 2_000_000_000, vec![m]), &s),
            &ctx(&t),
        );
        assert!(matches!(r, Err(Refusal::Method(_, _))), "{key}: {r:?}");
    }
}

#[test]
fn pk07_an_unknown_top_level_field_a_bad_pack_name_or_version_and_a_wrong_format_are_refused() {
    let s = signer(1);
    let t = trusted(&s);
    let refuse = |v: Value| {
        matches!(
            check_pack(&sign_value(&v, &s), &ctx(&t)),
            Err(Refusal::Shape(_))
        )
    };
    let mut v = payload("kit", "1.0.0", 2_000_000_000, vec![method("a-method")]);
    v["extra"] = json!(1);
    assert!(refuse(v));
    for name in ["", "a b", "a/b", &"x".repeat(65)] {
        assert!(
            refuse(payload(
                name,
                "1.0.0",
                2_000_000_000,
                vec![method("a-method")]
            )),
            "{name:?}"
        );
    }
    for version in ["1", "1.0", "1.0.0-beta", "01.0.0", "a.b.c", "1.0.0.0"] {
        assert!(
            refuse(payload(
                "kit",
                version,
                2_000_000_000,
                vec![method("a-method")]
            )),
            "{version}"
        );
    }
    let mut v = payload("kit", "1.0.0", 2_000_000_000, vec![method("a-method")]);
    v["format"] = json!("something-else");
    assert!(refuse(v));
    let mut v = payload("kit", "1.0.0", 2_000_000_000, vec![method("a-method")]);
    v["version"] = json!(2);
    assert!(refuse(v));
}

#[test]
fn pk08_an_expired_pack_is_refused_and_an_older_version_than_one_already_seen_is_a_rollback() {
    let s = signer(1);
    let t = trusted(&s);
    let expired = sign_value(
        &payload("kit", "1.0.0", 1_600_000_000, vec![method("a-method")]),
        &s,
    );
    assert!(matches!(
        check_pack(&expired, &ctx(&t)),
        Err(Refusal::Expired)
    ));
    let v2 = sign_value(
        &payload("kit", "2.0.0", 2_000_000_000, vec![method("a-method")]),
        &s,
    );
    let v1 = sign_value(
        &payload("kit", "1.9.9", 2_000_000_000, vec![method("a-method")]),
        &s,
    );
    let seen = |name: &str| (name == "kit").then(|| Version::parse("2.0.0")).flatten();
    let c = Context {
        trusted: &t,
        now: 1_700_000_000,
        seen: &seen,
    };
    assert!(
        check_pack(&v2, &c).is_ok(),
        "the same version again is fine"
    );
    assert!(matches!(check_pack(&v1, &c), Err(Refusal::Rollback)));
    let other = sign_value(
        &payload(
            "other-kit",
            "1.0.0",
            2_000_000_000,
            vec![method("a-method")],
        ),
        &s,
    );
    assert!(
        check_pack(&other, &c).is_ok(),
        "the highest seen belongs to one pack name only"
    );
    let other = sign_value(
        &payload(
            "other-kit",
            "1.0.0",
            2_000_000_000,
            vec![method("a-method")],
        ),
        &s,
    );
    assert!(
        check_pack(&other, &c).is_ok(),
        "the highest seen belongs to one pack name only"
    );
}

#[test]
fn pk09_two_signatures_or_none_are_refused() {
    let s = signer(1);
    let t = trusted(&s);
    let canon = kk_evidence::canon::canonicalize(
        payload("kit", "1.0.0", 2_000_000_000, vec![method("a-method")])
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    let mut env = dsse::sign(PACK_TYPE, canon.as_bytes(), &s);
    env.signatures.push(env.signatures[0].clone());
    assert!(matches!(
        check_pack(dsse::encode(&env).as_bytes(), &ctx(&t)),
        Err(Refusal::NotSigned(_))
    ));
    env.signatures.clear();
    assert!(matches!(
        check_pack(dsse::encode(&env).as_bytes(), &ctx(&t)),
        Err(Refusal::NotSigned(_))
    ));
}

#[test]
fn pk10_signing_a_draft_makes_a_pack_that_checks_and_refuses_an_invalid_draft() {
    let s = signer(3);
    let t = trusted(&s);
    let draft = payload("kit", "1.2.3", 2_000_000_000, vec![method("a-method")]);
    let bytes = sign_pack(&draft, &s, 1_700_000_000).unwrap();
    assert_eq!(
        check_pack(bytes.as_bytes(), &ctx(&t))
            .unwrap()
            .version
            .to_string(),
        "1.2.3"
    );
    let bad = payload("kit", "1.2.3", 2_000_000_000, vec![method("bad name!")]);
    assert!(
        sign_pack(&bad, &s, 1_700_000_000).is_err(),
        "a publisher cannot sign what a recipient would refuse"
    );
    let old = payload("kit", "1.2.3", 1_600_000_000, vec![method("a-method")]);
    assert!(
        sign_pack(&old, &s, 1_700_000_000).is_err(),
        "nor one that is already expired"
    );
}
