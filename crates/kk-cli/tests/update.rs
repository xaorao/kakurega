use std::path::PathBuf;
use std::process::{Command, Output};

fn kaku(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kaku"))
        .args(args)
        .output()
        .unwrap()
}

fn out(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kk-updcli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("data")).unwrap();
    d
}

fn sha256(bytes: &[u8]) -> String {
    kk_evidence::capsule::sha256_hex(bytes)
}

#[test]
fn uc01_the_publisher_signs_an_installation_checks_and_a_swapped_download_is_refused() {
    let d = dir("flow");
    let p = |n: &str| d.join(n).to_string_lossy().into_owned();
    let k = kaku(&["update", "keygen", "--out", &p("pub.key")]);
    assert!(k.status.success(), "{}", out(&k));
    let public = out(&k)
        .lines()
        .find(|l| l.starts_with("public key"))
        .unwrap()
        .rsplit(": ")
        .next()
        .unwrap()
        .to_string();
    assert_eq!(
        kaku(&["update", "keygen", "--out", &p("pub.key")])
            .status
            .code(),
        Some(1),
        "an existing key is never overwritten"
    );
    let installer = b"installer bytes".to_vec();
    std::fs::write(d.join("inst.dmg"), &installer).unwrap();
    std::fs::write(d.join("bad.dmg"), b"installer bytez").unwrap();
    std::fs::write(
        d.join("m.json"),
        serde_json::json!({"channel": "stable", "version": "9.0.0", "not_after": 4_000_000_000u64, "notes": "Test", "artifacts": [{"platform": "test-x", "url": "https://updates.example/k.dmg", "sha256": sha256(&installer), "size": installer.len()}]}).to_string(),
    )
    .unwrap();
    let s = kaku(&[
        "update",
        "sign",
        "--key",
        &p("pub.key"),
        "--manifest",
        &p("m.json"),
        "--out",
        &p("m.signed"),
    ]);
    assert!(s.status.success(), "{}", out(&s));
    let common = [
        "--manifest",
        &p("m.signed"),
        "--trusted",
        &public,
        "--data-dir",
        &p("data"),
        "--platform",
        "test-x",
        "--current",
        "1.0.0",
    ];
    let c = kaku(&[&["update", "check"], &common[..]].concat());
    assert!(
        c.status.success()
            && out(&c).contains("update available: 9.0.0")
            && out(&c).contains("Nothing was downloaded"),
        "{}",
        out(&c)
    );
    assert_eq!(
        std::fs::read_to_string(d.join("data/update-seen")).unwrap(),
        "0 9.0.0",
        "the record holds the root version (0: plain publisher keys) and the version seen"
    );
    let good = kaku(
        &[
            &["update", "verify-file", "--file", &p("inst.dmg")],
            &common[..],
        ]
        .concat(),
    );
    assert!(
        good.status.success() && out(&good).contains("exactly the file"),
        "{}",
        out(&good)
    );
    let bad = kaku(
        &[
            &["update", "verify-file", "--file", &p("bad.dmg")],
            &common[..],
        ]
        .concat(),
    );
    assert_eq!(bad.status.code(), Some(1));
    assert!(out(&bad).contains("do NOT install"), "{}", out(&bad));
    // already current
    let cur = kaku(&[
        "update",
        "check",
        "--manifest",
        &p("m.signed"),
        "--trusted",
        &public,
        "--data-dir",
        &p("data"),
        "--platform",
        "test-x",
        "--current",
        "9.0.0",
    ]);
    assert!(out(&cur).contains("up to date"), "{}", out(&cur));
    // without a configured key nothing is trusted; a stranger's key is refused
    let nokey = kaku(&[
        "update",
        "check",
        "--manifest",
        &p("m.signed"),
        "--data-dir",
        &p("data"),
        "--platform",
        "test-x",
    ]);
    assert_eq!(nokey.status.code(), Some(1));
    assert!(out(&nokey).contains("no publisher key"), "{}", out(&nokey));
    let k2 = kaku(&["update", "keygen", "--out", &p("other.key")]);
    let other = out(&k2)
        .lines()
        .find(|l| l.starts_with("public key"))
        .unwrap()
        .rsplit(": ")
        .next()
        .unwrap()
        .to_string();
    let wrong = kaku(&[
        "update",
        "check",
        "--manifest",
        &p("m.signed"),
        "--trusted",
        &other,
        "--data-dir",
        &p("data"),
        "--platform",
        "test-x",
    ]);
    assert_eq!(wrong.status.code(), Some(1));
    // an announcement older than one already seen is a replay
    std::fs::write(d.join("data/update-seen"), "9.5.0").unwrap();
    let replay = kaku(&[&["update", "check"], &common[..]].concat());
    assert_eq!(replay.status.code(), Some(1));
    assert!(out(&replay).contains("Rollback"), "{}", out(&replay));
}

fn public_of(o: &Output) -> String {
    out(o)
        .lines()
        .find(|l| l.starts_with("public key"))
        .unwrap()
        .rsplit(": ")
        .next()
        .unwrap()
        .to_string()
}

#[test]
fn uc02_a_stolen_key_is_replaced_by_a_signed_root_and_its_forged_future_version_stops_blocking_real_releases(
) {
    let d = dir("root");
    let p = |n: &str| d.join(n).to_string_lossy().into_owned();
    let data = p("data");
    let mut pubs = std::collections::BTreeMap::new();
    for k in ["old", "new", "other"] {
        let r = kaku(&["update", "keygen", "--out", &p(&format!("{k}.key"))]);
        assert!(r.status.success(), "{}", out(&r));
        pubs.insert(k, public_of(&r));
    }
    let root_doc = |keys: &[&str], threshold: u64, version: u64| {
        serde_json::json!({"keys": keys, "not_after": 4_000_000_000u64, "threshold": threshold, "version": version}).to_string()
    };
    let w = |name: &str, text: &str| std::fs::write(d.join(name), text).unwrap();
    // root 1: only the old key; root 2: only the new key
    w("r1.json", &root_doc(&[&pubs["old"]], 1, 1));
    w("r2.json", &root_doc(&[&pubs["new"]], 1, 2));
    let sign = |doc: &str, keys: &[&str], out_name: &str| {
        let (doc_path, out_path) = (p(doc), p(out_name));
        let mut a = vec![
            "update",
            "root-sign",
            "--root",
            &doc_path,
            "--out",
            &out_path,
        ];
        let files: Vec<String> = keys.iter().map(|k| p(k)).collect();
        for f in &files {
            a.push("--key");
            a.push(f);
        }
        kaku(&a)
    };
    let s1 = sign("r1.json", &["old.key"], "r1.signed");
    assert!(s1.status.success(), "{}", out(&s1));
    // a root nobody signed with its own keys is refused by the signing tool and, handed over anyway, by the installation
    assert!(!sign("r1.json", &[], "nokey.signed").status.success());
    // installing the first root only describes it until it is accepted, and shows the key ids
    let dry = kaku(&[
        "update",
        "root-init",
        "--root",
        &p("r1.signed"),
        "--data-dir",
        &data,
    ]);
    assert_eq!(dry.status.code(), Some(1));
    assert!(
        out(&dry).contains("root version 1")
            && out(&dry).contains("ed25519:")
            && out(&dry).contains("--accept"),
        "{}",
        out(&dry)
    );
    assert!(
        !d.join("data/update-root.json").exists(),
        "nothing was installed without --accept"
    );
    let init = kaku(&[
        "update",
        "root-init",
        "--root",
        &p("r1.signed"),
        "--data-dir",
        &data,
        "--accept",
    ]);
    assert!(init.status.success(), "{}", out(&init));
    // a second root-init never replaces it
    let again = kaku(&[
        "update",
        "root-init",
        "--root",
        &p("r1.signed"),
        "--data-dir",
        &data,
        "--accept",
    ]);
    assert_eq!(again.status.code(), Some(1));
    assert!(out(&again).contains("root-rotate"), "{}", out(&again));
    // announcements: a forged far-future version signed with the (stolen) old key, and the real release
    let manifest = |version: &str| {
        serde_json::json!({"channel": "stable", "version": version, "not_after": 4_000_000_000u64, "notes": "n",
            "artifacts": [{"platform": "test-x", "url": "https://updates.example/k.dmg", "sha256": "a".repeat(64), "size": 10}]}).to_string()
    };
    w("forged.json", &manifest("99.0.0"));
    w("real.json", &manifest("2.0.0"));
    let ann = |m: &str, key: &str, out_name: &str| {
        kaku(&[
            "update",
            "sign",
            "--key",
            &p(key),
            "--manifest",
            &p(m),
            "--out",
            &p(out_name),
        ])
    };
    assert!(ann("forged.json", "old.key", "forged.signed")
        .status
        .success());
    let check = |f: &str| {
        kaku(&[
            "update",
            "check",
            "--manifest",
            &p(f),
            "--data-dir",
            &data,
            "--platform",
            "test-x",
            "--current",
            "1.0.0",
        ])
    };
    let c = check("forged.signed");
    assert!(
        c.status.success() && out(&c).contains("update available: 99.0.0"),
        "{}",
        out(&c)
    );
    // the lock: the genuine release, signed by the same key, is now refused as a replay
    assert!(ann("real.json", "old.key", "real.old").status.success());
    let c = check("real.old");
    assert_eq!(c.status.code(), Some(1), "{}", out(&c));
    assert!(out(&c).contains("Rollback"), "{}", out(&c));
    // the publisher replaces the key: root 2 is signed by the old and the new key
    let s2 = sign("r2.json", &["old.key", "new.key"], "r2.signed");
    assert!(s2.status.success(), "{}", out(&s2));
    // signed by the new key alone, someone taking over: refused, and nothing changes
    let hostile = sign("r2.json", &["new.key"], "r2.hostile");
    assert!(hostile.status.success());
    let rot = kaku(&[
        "update",
        "root-rotate",
        &p("r2.hostile"),
        "--data-dir",
        &data,
    ]);
    assert_eq!(rot.status.code(), Some(1));
    assert!(
        out(&rot).contains("NotSignedByCurrentKeys"),
        "{}",
        out(&rot)
    );
    let rot = kaku(&[
        "update",
        "root-rotate",
        &p("r2.signed"),
        "--data-dir",
        &data,
    ]);
    assert!(rot.status.success(), "{}", out(&rot));
    assert!(out(&rot).contains("version 1 to 2"), "{}", out(&rot));
    let show = kaku(&["update", "root-show", "--data-dir", &data]);
    assert!(out(&show).contains("root version 2"), "{}", out(&show));
    // after the replacement the real release (signed by the new key) is offered, and the forged one is no longer accepted
    assert!(ann("real.json", "new.key", "real.new").status.success());
    let c = check("real.new");
    assert!(
        c.status.success() && out(&c).contains("update available: 2.0.0"),
        "{}",
        out(&c)
    );
    let c = check("forged.signed");
    assert_eq!(c.status.code(), Some(1));
    assert!(out(&c).contains("NotSigned"), "{}", out(&c));
}

#[test]
fn uc03_download_refuses_without_trust_on_a_tampered_announcement_when_up_to_date_and_when_the_host_is_unreachable_and_keeps_nothing(
) {
    let d = dir("download");
    let p = |n: &str| d.join(n).to_string_lossy().into_owned();
    let data = p("data");
    let k = kaku(&["update", "keygen", "--out", &p("pub.key")]);
    assert!(k.status.success(), "{}", out(&k));
    let public = public_of(&k);
    let manifest = |version: &str| {
        serde_json::json!({"channel": "stable", "version": version, "not_after": 4_000_000_000u64, "notes": "n",
            "artifacts": [{"platform": "test-x", "url": "https://updates.invalid/k.dmg", "sha256": "a".repeat(64), "size": 10}]}).to_string()
    };
    std::fs::write(d.join("m.json"), manifest("9.0.0")).unwrap();
    std::fs::write(d.join("old.json"), manifest("0.0.1")).unwrap();
    for (m, o) in [("m.json", "m.signed"), ("old.json", "old.signed")] {
        let s = kaku(&[
            "update",
            "sign",
            "--key",
            &p("pub.key"),
            "--manifest",
            &p(m),
            "--out",
            &p(o),
        ]);
        assert!(s.status.success(), "{}", out(&s));
    }
    let outdir = p("downloads");
    let dl = |manifest_file: &str, trust: bool| {
        let manifest_path = p(manifest_file);
        let mut a = vec![
            "update",
            "download",
            "--manifest",
            &manifest_path,
            "--data-dir",
            &data,
            "--out",
            &outdir,
            "--platform",
            "test-x",
            "--current",
            "1.0.0",
        ];
        if trust {
            a.push("--trusted");
            a.push(&public);
        }
        kaku(&a)
    };
    let nothing_kept = || {
        !std::path::Path::new(&outdir).exists()
            || std::fs::read_dir(&outdir).unwrap().next().is_none()
    };
    // nothing is trusted: no network is touched
    let r = dl("m.signed", false);
    assert_eq!(r.status.code(), Some(1));
    assert!(
        out(&r).contains("no publisher key or root is configured"),
        "{}",
        out(&r)
    );
    // an announcement altered after signing
    let mut text = std::fs::read(d.join("m.signed")).unwrap();
    let at = text.iter().position(|b| *b == b'"').unwrap() + 40;
    text[at] = if text[at] == b'A' { b'B' } else { b'A' };
    std::fs::write(d.join("tampered.signed"), &text).unwrap();
    let r = dl("tampered.signed", true);
    assert_eq!(r.status.code(), Some(1));
    assert!(out(&r).contains("refused"), "{}", out(&r));
    // already up to date: nothing to download
    let r = dl("old.signed", true);
    assert_eq!(r.status.code(), Some(1));
    assert!(out(&r).contains("up to date"), "{}", out(&r));
    // a valid announcement whose host cannot be reached: refused, nothing kept
    let r = dl("m.signed", true);
    assert_eq!(r.status.code(), Some(1), "{}", out(&r));
    assert!(out(&r).contains("nothing was kept"), "{}", out(&r));
    assert!(nothing_kept(), "no file or temporary file is left");
}
