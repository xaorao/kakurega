//! `kaku pack`: make a publisher key, sign a pack of methods, and check a pack against a key you trust. Importing is done by the
//! owner in the web page; nothing here touches the owner's saved methods.

use kk_evidence::keys::{self, TrustedKeys};
use std::io::Write;

const USAGE: &str = "usage: kaku pack keygen --out FILE | sign --key FILE --pack JSON --out FILE | check --pack FILE --trusted PUBLIC_KEY";

pub fn run(args: &[String]) -> u8 {
    let result = match args.first().map(String::as_str) {
        Some("keygen") => crate::update::keygen_for(&args[1..], "sign packs in your name"),
        Some("sign") => sign(&args[1..]),
        Some("check") => check(&args[1..]),
        _ => Err(USAGE.into()),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn sign(args: &[String]) -> Result<(), String> {
    let key = crate::flag(args, "--key").ok_or("give --key FILE (the publisher's private key)")?;
    let pack = crate::flag(args, "--pack").ok_or("give --pack JSON (the pack to sign)")?;
    let out = crate::flag(args, "--out").ok_or("give --out FILE")?;
    let text = std::fs::read_to_string(&pack).map_err(|e| format!("cannot read {pack}: {e}"))?;
    let draft: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "the pack file is not JSON".to_string())?;
    let signer = crate::update::load_signer(&key)?;
    let signed = kk_packs::sign_pack(&draft, &signer, crate::update::now() as i64)
        .map_err(|r| format!("not signed: {}", r.sentence()))?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&out)
        .and_then(|mut f| f.write_all(signed.as_bytes()))
        .map_err(|e| format!("cannot write {out}: {e} (an existing file is never overwritten)"))?;
    println!("signed pack written to {out}");
    println!("publisher key id: {}", signer.key_id());
    Ok(())
}

fn check(args: &[String]) -> Result<(), String> {
    let pack = crate::flag(args, "--pack").ok_or("give --pack FILE")?;
    let key = crate::flag(args, "--trusted")
        .ok_or("give --trusted PUBLIC_KEY (the publisher's public key)")?;
    let bytes = std::fs::read(&pack).map_err(|e| format!("cannot read {pack}: {e}"))?;
    let raw = kk_evidence::b64::decode(&key).map_err(|_| "the key is not base64".to_string())?;
    let public: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| "the key must be 32 bytes".to_string())?;
    let mut trusted = TrustedKeys::new();
    trusted
        .add(&public)
        .ok_or("that key cannot be used to sign packs")?;
    let cx = kk_packs::Context {
        trusted: &trusted,
        now: crate::update::now() as i64,
        seen: &|_| None,
    };
    let c = kk_packs::check_pack(&bytes, &cx).map_err(|r| format!("refused: {}", r.sentence()))?;
    println!(
        "OK: pack {} {} signed by {}",
        c.name,
        c.version,
        keys::KeyId::of(&public).as_str()
    );
    for m in &c.methods {
        println!(
            "  {}  triggers: {}",
            m.name,
            if m.triggers.is_empty() {
                "(only /name)".to_string()
            } else {
                m.triggers.join(", ")
            }
        );
    }
    println!(
        "Importing is done in the web page, under Instructions. Every method arrives unapproved."
    );
    Ok(())
}
