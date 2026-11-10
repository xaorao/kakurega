//! `kaku pack`: a publisher makes a key, signs a pack, and anyone checks it against the publisher's public key.

use std::path::{Path, PathBuf};
use std::process::Command;

fn kaku(args: &[&str]) -> (i32, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_kaku"))
        .args(args)
        .output()
        .unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into(),
        String::from_utf8_lossy(&o.stderr).into(),
    )
}

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kk-pack-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn public_of(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|l| l.strip_prefix("public key (give this to installations): "))
        .unwrap()
        .trim()
        .to_string()
}

fn write(p: &Path, text: &str) {
    std::fs::write(p, text).unwrap();
}

const PACK: &str = r#"{"format":"kakurega-pack","version":1,"name":"legal-kit","pack_version":"1.0.0","not_after":4000000000,"methods":[{"name":"contract-check","description":"d","triggers":["contract"],"avoid":[],"body":"Quote clause numbers."}]}"#;

#[test]
fn pc01_keygen_sign_and_check_make_a_pack_that_only_the_right_key_accepts() {
    let d = dir("ok");
    let key = d.join("pub.key");
    let (rc, out, err) = kaku(&["pack", "keygen", "--out", key.to_str().unwrap()]);
    assert_eq!(rc, 0, "{err}");
    assert!(out.contains("sign packs in your name"), "{out}");
    let public = public_of(&out);
    write(&d.join("pack.json"), PACK);
    let signed = d.join("pack.kkp");
    let (rc, out, err) = kaku(&[
        "pack",
        "sign",
        "--key",
        key.to_str().unwrap(),
        "--pack",
        d.join("pack.json").to_str().unwrap(),
        "--out",
        signed.to_str().unwrap(),
    ]);
    assert_eq!(rc, 0, "{err}");
    assert!(out.contains("publisher key id"), "{out}");
    let (rc, out, err) = kaku(&[
        "pack",
        "check",
        "--pack",
        signed.to_str().unwrap(),
        "--trusted",
        &public,
    ]);
    assert_eq!(rc, 0, "{err}");
    assert!(
        out.contains("OK: pack legal-kit 1.0.0")
            && out.contains("contract-check")
            && out.contains("unapproved"),
        "{out}"
    );
    // another publisher's key does not accept it
    let other = d.join("other.key");
    let (_, out2, _) = kaku(&["pack", "keygen", "--out", other.to_str().unwrap()]);
    let (rc, _, err) = kaku(&[
        "pack",
        "check",
        "--pack",
        signed.to_str().unwrap(),
        "--trusted",
        &public_of(&out2),
    ]);
    assert_eq!(rc, 1);
    assert!(err.contains("not signed by a publisher you trust"), "{err}");
    // a changed file is refused
    let mut bytes = std::fs::read(&signed).unwrap();
    let i = bytes.len() / 2;
    bytes[i] ^= 1;
    std::fs::write(d.join("changed.kkp"), bytes).unwrap();
    assert_eq!(
        kaku(&[
            "pack",
            "check",
            "--pack",
            d.join("changed.kkp").to_str().unwrap(),
            "--trusted",
            &public
        ])
        .0,
        1
    );
}

#[test]
fn pc02_sign_refuses_what_a_recipient_would_refuse_and_never_overwrites() {
    let d = dir("refuse");
    let key = d.join("k.key");
    kaku(&["pack", "keygen", "--out", key.to_str().unwrap()]);
    for (label, text) in [
        (
            "a power field",
            PACK.replace("\"avoid\":[]", "\"avoid\":[],\"tools\":[\"shell\"]"),
        ),
        (
            "a bad method name",
            PACK.replace("contract-check", "bad name!"),
        ),
        ("expired", PACK.replace("4000000000", "1000")),
        ("not json", "nope".to_string()),
    ] {
        write(&d.join("p.json"), &text);
        let (rc, _, err) = kaku(&[
            "pack",
            "sign",
            "--key",
            key.to_str().unwrap(),
            "--pack",
            d.join("p.json").to_str().unwrap(),
            "--out",
            d.join(format!("{label}.kkp").replace(' ', "_"))
                .to_str()
                .unwrap(),
        ]);
        assert_eq!(rc, 1, "{label}: {err}");
        assert!(
            !d.join(format!("{label}.kkp").replace(' ', "_")).exists(),
            "{label}: nothing is written"
        );
    }
    write(&d.join("p.json"), PACK);
    let out = d.join("o.kkp");
    assert_eq!(
        kaku(&[
            "pack",
            "sign",
            "--key",
            key.to_str().unwrap(),
            "--pack",
            d.join("p.json").to_str().unwrap(),
            "--out",
            out.to_str().unwrap()
        ])
        .0,
        0
    );
    assert_eq!(
        kaku(&[
            "pack",
            "sign",
            "--key",
            key.to_str().unwrap(),
            "--pack",
            d.join("p.json").to_str().unwrap(),
            "--out",
            out.to_str().unwrap()
        ])
        .0,
        1,
        "an existing file is never overwritten"
    );
    assert_eq!(
        kaku(&["pack", "keygen", "--out", key.to_str().unwrap()]).0,
        1,
        "nor an existing key"
    );
    assert_eq!(kaku(&["pack", "bogus"]).0, 1);
    assert_eq!(
        kaku(&[
            "pack",
            "check",
            "--pack",
            out.to_str().unwrap(),
            "--trusted",
            "AAAA"
        ])
        .0,
        1,
        "a short key is refused"
    );
}
