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
        "9.0.0"
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
