use kk_evidence::keys::{public_key_b64, Signer};
use kk_update::root::{
    follow, load_initial, rotate, sign_root, Root, RootRefusal, Seen, MAX_ROTATIONS,
};
use kk_update::{check, sign_manifest_multi, Context, Refusal, Verdict, Version};
use serde_json::{json, Value};

fn key(n: u8) -> Signer {
    Signer::from_seed([n; 32])
}

fn root_json(keys: &[&Signer], threshold: u64, version: u64, not_after: u64) -> Value {
    json!({
        "keys": keys.iter().map(|k| public_key_b64(&k.public())).collect::<Vec<_>>(),
        "not_after": not_after,
        "threshold": threshold,
        "version": version,
    })
}

const FAR: u64 = 4_000_000_000;
const NOW: u64 = 1_900_000_000;

fn signed_root(keys: &[&Signer], threshold: u64, version: u64, signers: &[&Signer]) -> Vec<u8> {
    sign_root(
        root_json(keys, threshold, version, FAR)
            .to_string()
            .as_bytes(),
        signers,
    )
    .unwrap()
}

fn manifest(version: &str) -> Value {
    json!({
        "channel": "stable", "version": version, "not_after": FAR,
        "notes": "Fixes a crash.",
        "artifacts": [{"platform": "macos-arm64", "url": "https://updates.example/k.dmg", "sha256": "a".repeat(64), "size": 1000}]
    })
}

fn announce(version: &str, signers: &[&Signer]) -> Vec<u8> {
    sign_manifest_multi(manifest(version).to_string().as_bytes(), signers).unwrap()
}

/// A root signed by hand, as a hostile publisher could: the signing tool would refuse some of these.
fn raw_root(doc: &Value, signers: &[&Signer]) -> Vec<u8> {
    let canon = kk_evidence::canon::canonicalize(doc.to_string().as_bytes()).unwrap();
    let t = kk_update::root::ROOT_TYPE;
    let mut env = kk_evidence::dsse::sign(t, canon.as_bytes(), signers[0]);
    for k in &signers[1..] {
        env.signatures.push((
            k.key_id(),
            k.sign(&kk_evidence::dsse::pae(t, canon.as_bytes())),
        ));
    }
    kk_evidence::dsse::encode(&env).into_bytes()
}

fn ctx<'a>(root: &'a Root, seen: Option<Version>) -> Context<'a> {
    Context {
        root,
        channel: "stable",
        platform: "macos-arm64",
        current: Version(1, 3, 2),
        highest_seen: seen,
        now: NOW,
    }
}

#[test]
fn rt01_a_first_root_is_taken_only_when_enough_of_its_own_keys_signed_it_and_odd_roots_are_refused()
{
    let (a, b, c) = (key(1), key(2), key(3));
    assert!(load_initial(&signed_root(&[&a, &b, &c], 2, 1, &[&a, &b])).is_ok());
    assert!(matches!(
        load_initial(&signed_root(&[&a, &b, &c], 2, 1, &[&a])),
        Err(RootRefusal::NotSigned(_))
    ));
    // signed by strangers only
    assert!(matches!(
        load_initial(&signed_root(&[&a, &b], 1, 1, &[&c])),
        Err(RootRefusal::NotSigned(_))
    ));
    let bad = |doc: Value, signers: &[&Signer]| -> Result<Root, RootRefusal> {
        load_initial(&raw_root(&doc, signers))
    };
    type Want = fn(&RootRefusal) -> bool;
    let cases: Vec<(&str, Value, Want)> = vec![
        ("version zero", root_json(&[&a], 1, 0, FAR), |e| {
            matches!(e, RootRefusal::Malformed(_))
        }),
        ("threshold zero", root_json(&[&a], 0, 1, FAR), |e| {
            matches!(e, RootRefusal::BadThreshold)
        }),
        (
            "threshold above the keys",
            root_json(&[&a, &b], 3, 1, FAR),
            |e| matches!(e, RootRefusal::BadThreshold),
        ),
        (
            "no keys",
            json!({"keys": [], "not_after": FAR, "threshold": 1, "version": 1}),
            |e| matches!(e, RootRefusal::BadKeys(_)),
        ),
        ("a key twice", root_json(&[&a, &a], 1, 1, FAR), |e| {
            matches!(e, RootRefusal::BadKeys(_))
        }),
        (
            "an extra field",
            {
                let mut d = root_json(&[&a], 1, 1, FAR);
                d["extra"] = json!(1);
                d
            },
            |e| matches!(e, RootRefusal::Malformed(_)),
        ),
        (
            "not base64",
            json!({"keys": ["***"], "not_after": FAR, "threshold": 1, "version": 1}),
            |e| matches!(e, RootRefusal::BadKeys(_)),
        ),
        (
            "short key",
            json!({"keys": [public_key_b64(&[7u8; 32])[..20].to_string()], "not_after": FAR, "threshold": 1, "version": 1}),
            |e| matches!(e, RootRefusal::BadKeys(_)),
        ),
    ];
    for (what, doc, want) in cases {
        let got = bad(doc, &[&a])
            .err()
            .unwrap_or_else(|| panic!("{what} was accepted"));
        assert!(want(&got), "{what}: refused for the wrong reason: {got:?}");
    }
    // seventeen keys is more than a root may list
    let many: Vec<Signer> = (0..17).map(|i| key(50 + i)).collect();
    let refs: Vec<&Signer> = many.iter().collect();
    assert!(matches!(
        bad(root_json(&refs, 1, 1, FAR), &[&many[0]]),
        Err(RootRefusal::BadKeys(_))
    ));
    // a weak (all zero) public key is not a usable key
    let weak = json!({"keys": [kk_evidence::b64::encode(&[0u8; 32])], "not_after": FAR, "threshold": 1, "version": 1});
    assert!(matches!(bad(weak, &[&a]), Err(RootRefusal::BadKeys(_))));
}

#[test]
fn rt02_a_replacement_needs_enough_old_keys_and_enough_new_keys_and_the_next_version() {
    let (a, b, c, d) = (key(1), key(2), key(3), key(4));
    let v1 = load_initial(&signed_root(&[&a, &b], 2, 1, &[&a, &b])).unwrap();
    // the proper replacement: both old keys and the new key
    let v2 = rotate(&v1, &signed_root(&[&c], 1, 2, &[&a, &b, &c])).unwrap();
    assert_eq!((v2.version, v2.threshold), (2, 1));
    // announcements now follow the new key, not the old ones
    assert!(matches!(
        check(&announce("1.4.0", &[&c]), &ctx(&v2, None)),
        Ok(Verdict::Available(_))
    ));
    assert!(matches!(
        check(&announce("1.4.0", &[&a, &b]), &ctx(&v2, None)),
        Err(Refusal::NotSigned(_))
    ));
    // signed by the old keys only: the new keys never agreed
    assert!(matches!(
        rotate(&v1, &signed_root(&[&c], 1, 2, &[&a, &b])),
        Err(RootRefusal::NotSignedByNewKeys(_))
    ));
    // signed by the new keys only: someone is taking over
    assert!(matches!(
        rotate(&v1, &signed_root(&[&c], 1, 2, &[&c])),
        Err(RootRefusal::NotSignedByCurrentKeys(_))
    ));
    // too few old keys for the old threshold of two
    assert!(matches!(
        rotate(&v1, &signed_root(&[&c], 1, 2, &[&a, &c])),
        Err(RootRefusal::NotSignedByCurrentKeys(_))
    ));
    // the version must be exactly one higher: skipped, repeated, or older
    for (v, what) in [(3u64, "skipped"), (1, "repeated"), (0, "older")] {
        let bytes = raw_root(&root_json(&[&c], 1, v, FAR), &[&a, &b, &c]);
        let got = rotate(&v1, &bytes)
            .err()
            .unwrap_or_else(|| panic!("{what} was accepted"));
        if v == 0 {
            assert!(matches!(got, RootRefusal::Malformed(_)), "{what}: {got:?}");
        } else {
            assert!(
                matches!(got, RootRefusal::WrongVersion { expected: 2, got: g } if g == v),
                "{what}: {got:?}"
            );
        }
    }
    // an old root can never be put back: v2 is now current, so v2 again or v1 is refused
    assert!(matches!(
        rotate(&v2, &signed_root(&[&d], 1, 2, &[&c, &d])),
        Err(RootRefusal::WrongVersion {
            expected: 3,
            got: 2
        })
    ));
}

#[test]
fn rt03_a_chain_is_followed_in_order_up_to_a_limit_and_must_end_on_an_unexpired_root() {
    let ks: Vec<Signer> = (1..=40).map(key).collect();
    let v1 = load_initial(&signed_root(&[&ks[0]], 1, 1, &[&ks[0]])).unwrap();
    let step = |i: usize| signed_root(&[&ks[i]], 1, i as u64 + 1, &[&ks[i - 1], &ks[i]]);
    let chain: Vec<Vec<u8>> = (1..=3).map(step).collect();
    let end = follow(v1.clone(), &chain, NOW).unwrap();
    assert_eq!(end.version, 4);
    // out of order is refused
    assert!(follow(v1.clone(), &[chain[1].clone(), chain[0].clone()], NOW).is_err());
    // more rotations than the limit are refused before any work
    let long: Vec<Vec<u8>> = (1..=MAX_ROTATIONS + 1).map(step).collect();
    assert!(matches!(
        follow(v1.clone(), &long, NOW),
        Err(RootRefusal::TooManyRotations)
    ));
    // exactly the limit is fine
    let at_limit: Vec<Vec<u8>> = (1..=MAX_ROTATIONS).map(step).collect();
    assert!(follow(v1.clone(), &at_limit, NOW).is_ok());
    // an expired MIDDLE root does not stop a lapsed installation catching up, but the final root must not be expired
    let old = |v: u64, ks: &[&Signer], by: &[&Signer], exp: u64| {
        sign_root(root_json(ks, 1, v, exp).to_string().as_bytes(), by).unwrap()
    };
    let r2 = old(2, &[&ks[1]], &[&ks[0], &ks[1]], NOW - 10);
    let r3 = old(3, &[&ks[2]], &[&ks[1], &ks[2]], FAR);
    assert_eq!(
        follow(v1.clone(), &[r2.clone(), r3], NOW).unwrap().version,
        3
    );
    assert!(matches!(follow(v1, &[r2], NOW), Err(RootRefusal::Expired)));
}

#[test]
fn rt04_an_expired_root_accepts_nothing_even_when_the_signatures_are_good() {
    let a = key(1);
    let doc = root_json(&[&a], 1, 1, NOW - 1);
    let root = load_initial(&sign_root(doc.to_string().as_bytes(), &[&a]).unwrap()).unwrap();
    assert_eq!(
        check(&announce("1.4.0", &[&a]), &ctx(&root, None)),
        Err(Refusal::RootExpired),
        "a frozen old root must not keep the installation trusting for ever"
    );
}

#[test]
fn rt04b_a_root_expires_at_the_second_it_names_not_a_second_later() {
    let a = key(1);
    let at = |not_after: u64| {
        load_initial(
            &sign_root(
                root_json(&[&a], 1, 1, not_after).to_string().as_bytes(),
                &[&a],
            )
            .unwrap(),
        )
        .unwrap()
    };
    assert_eq!(
        check(&announce("1.4.0", &[&a]), &ctx(&at(NOW), None)),
        Err(Refusal::RootExpired)
    );
    assert!(matches!(
        check(&announce("1.4.0", &[&a]), &ctx(&at(NOW + 1), None)),
        Ok(Verdict::Available(_))
    ));
}

#[test]
fn rt05_announcements_can_ask_for_more_than_one_publisher() {
    let (a, b, c) = (key(1), key(2), key(3));
    let root = load_initial(&signed_root(&[&a, &b, &c], 2, 1, &[&a, &b, &c])).unwrap();
    assert!(matches!(
        check(&announce("1.4.0", &[&a]), &ctx(&root, None)),
        Err(Refusal::NotSigned(_))
    ));
    assert!(matches!(
        check(&announce("1.4.0", &[&a, &c]), &ctx(&root, None)),
        Ok(Verdict::Available(_))
    ));
}

#[test]
fn rt06_replacing_the_root_ends_the_lock_a_forged_far_future_version_would_cause() {
    let (old_key, new_key) = (key(1), key(2));
    let v1 = load_initial(&signed_root(&[&old_key], 1, 1, &[&old_key])).unwrap();
    // someone with the stolen old key announces version 99: it is accepted once and remembered under root 1
    let forged = announce("99.0.0", &[&old_key]);
    assert!(matches!(
        check(&forged, &ctx(&v1, None)),
        Ok(Verdict::Available(_))
    ));
    let seen = Some(Seen {
        root_version: 1,
        version: Version(99, 0, 0),
    });
    // from then on every real release below it is refused as a replay
    assert!(matches!(
        check(
            &announce("1.4.0", &[&old_key]),
            &ctx(&v1, Seen::effective(seen, &v1))
        ),
        Err(Refusal::Rollback { .. })
    ));
    // the publisher replaces the key
    let v2 = rotate(&v1, &signed_root(&[&new_key], 1, 2, &[&old_key, &new_key])).unwrap();
    // the record made under the old root no longer counts, so the real release is offered again
    assert_eq!(Seen::effective(seen, &v2), None);
    assert!(matches!(
        check(
            &announce("1.4.0", &[&new_key]),
            &ctx(&v2, Seen::effective(seen, &v2))
        ),
        Ok(Verdict::Available(_))
    ));
    // and the stolen key can no longer announce anything
    assert!(matches!(
        check(&forged, &ctx(&v2, None)),
        Err(Refusal::NotSigned(_))
    ));
    // a record made under the current root still protects against replays
    let again = Some(Seen {
        root_version: 2,
        version: Version(1, 4, 0),
    });
    assert!(matches!(
        check(
            &announce("1.3.9", &[&new_key]),
            &ctx(&v2, Seen::effective(again, &v2))
        ),
        Err(Refusal::Rollback { .. })
    ));
}

#[test]
fn rt07_the_signing_tool_refuses_a_root_no_client_would_accept() {
    let a = key(1);
    for (what, doc) in [
        ("threshold zero", root_json(&[&a], 0, 1, FAR)),
        ("version zero", root_json(&[&a], 1, 0, FAR)),
        ("a key twice", root_json(&[&a, &a], 1, 1, FAR)),
        ("threshold above the keys", root_json(&[&a], 2, 1, FAR)),
    ] {
        assert!(
            sign_root(doc.to_string().as_bytes(), &[&a]).is_err(),
            "{what}"
        );
    }
    assert!(sign_root(root_json(&[&a], 1, 1, FAR).to_string().as_bytes(), &[]).is_err());
}
