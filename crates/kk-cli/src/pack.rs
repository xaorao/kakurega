//! `kaku pack`: make a publisher key, sign a pack of methods, and check a pack against a key you trust. Importing is done by the
//! owner in the web page; nothing here touches the owner's saved methods.

use kk_evidence::keys::{self, TrustedKeys};
use std::io::Write;

const USAGE: &str = "usage: kaku pack keygen --out FILE | sign --key FILE --pack JSON --out FILE | check --pack FILE --trusted PUBLIC_KEY\n       kaku pack catalog --key FILE --dir FOLDER --out FILE [--days N] | check-catalog --catalog FILE --trusted PUBLIC_KEY [--dir FOLDER]";

pub fn run(args: &[String]) -> u8 {
    let result = match args.first().map(String::as_str) {
        Some("keygen") => crate::update::keygen_for(&args[1..], "sign packs in your name"),
        Some("sign") => sign(&args[1..]),
        Some("check") => check(&args[1..]),
        Some("catalog") => catalog(&args[1..]),
        Some("check-catalog") => check_catalog(&args[1..]),
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
        seen: &|_, _| None,
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

fn trusted_from(key: &str) -> Result<(TrustedKeys, [u8; 32]), String> {
    let raw = kk_evidence::b64::decode(key).map_err(|_| "the key is not base64".to_string())?;
    let public: [u8; 32] = raw
        .as_slice()
        .try_into()
        .map_err(|_| "the key must be 32 bytes".to_string())?;
    let mut trusted = TrustedKeys::new();
    trusted
        .add(&public)
        .ok_or("that key cannot be used to sign packs")?;
    Ok((trusted, public))
}

/// Writes a signed catalog of every pack file in a folder that this key signed. A file that is not a pack signed by this
/// key stops the command: a publisher's catalog lists the publisher's own packs and nothing it has not checked.
fn catalog(args: &[String]) -> Result<(), String> {
    let key = crate::flag(args, "--key").ok_or("give --key FILE (the publisher's private key)")?;
    let dir = crate::flag(args, "--dir")
        .ok_or("give --dir FOLDER (the folder with the signed pack files)")?;
    let out = crate::flag(args, "--out").ok_or("give --out FILE")?;
    let days: i64 = match crate::flag(args, "--days") {
        Some(d) => d
            .parse()
            .ok()
            .filter(|d| (1..=730).contains(d))
            .ok_or("--days is a number from 1 to 730")?,
        None => 90,
    };
    let signer = crate::update::load_signer(&key)?;
    let mut trusted = TrustedKeys::new();
    trusted
        .add(&signer.public())
        .ok_or("that key cannot be used to sign packs")?;
    let now = crate::update::now() as i64;
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| format!("cannot read {dir}: {e}"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "kkp") && p.is_file())
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!(
            "{dir} holds no pack files (signed packs end in .kkp)"
        ));
    }
    let mut packs = Vec::new();
    for path in &files {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("a pack file's name is not plain text")?;
        let bytes =
            std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let cx = kk_packs::Context {
            trusted: &trusted,
            now,
            seen: &|_, _| None,
        };
        let c = kk_packs::check_pack(&bytes, &cx)
            .map_err(|r| format!("{name} is not a pack this key signed: {}", r.sentence()))?;
        let described = c
            .methods
            .iter()
            .map(|m| m.description.as_str())
            .find(|d| !d.is_empty())
            .unwrap_or("")
            .to_string();
        packs.push(serde_json::json!({
            "name": c.name,
            "pack_version": c.version.to_string(),
            "description": described,
            "file": name,
            "sha256": kk_evidence::capsule::sha256_hex(&bytes),
            "methods": c.methods.iter().map(|m| m.name.clone()).collect::<Vec<_>>(),
        }));
    }
    let draft = serde_json::json!({"format": "kakurega-catalog", "version": 1, "issued": now, "not_after": now + days * 86_400, "packs": packs});
    let signed = kk_packs::catalog::sign_catalog(&draft, &signer, now)
        .map_err(|r| format!("not signed: {}", r.sentence()))?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&out)
        .and_then(|mut f| f.write_all(signed.as_bytes()))
        .map_err(|e| format!("cannot write {out}: {e} (an existing file is never overwritten)"))?;
    println!(
        "{}",
        kk_types::words::counted(&format!(
            "signed catalog of {} pack(s) written to {out}; it expires in {days} days",
            packs.len()
        ))
    );
    println!("publisher key id: {}", signer.key_id());
    Ok(())
}

fn check_catalog(args: &[String]) -> Result<(), String> {
    let file = crate::flag(args, "--catalog").ok_or("give --catalog FILE")?;
    let key = crate::flag(args, "--trusted")
        .ok_or("give --trusted PUBLIC_KEY (the publisher's public key)")?;
    let bytes = std::fs::read(&file).map_err(|e| format!("cannot read {file}: {e}"))?;
    let (trusted, public) = trusted_from(&key)?;
    let c =
        kk_packs::catalog::check_catalog(&bytes, &trusted, crate::update::now() as i64, &|_| None)
            .map_err(|r| {
                format!(
                    "refused: {}",
                    r.sentence().replace("the pack", "the catalog")
                )
            })?;
    println!(
        "{}",
        kk_types::words::counted(&format!(
            "OK: catalog of {} pack(s) signed by {}",
            c.entries.len(),
            keys::KeyId::of(&public).as_str()
        ))
    );
    let dir = crate::flag(args, "--dir");
    let mut wrong = 0;
    for e in &c.entries {
        let state = match &dir {
            None => String::new(),
            Some(d) => match std::fs::read(std::path::Path::new(d).join(&e.file)) {
                Ok(b) if kk_evidence::capsule::sha256_hex(&b) == e.sha256 => {
                    "  file matches".into()
                }
                Ok(_) => {
                    wrong += 1;
                    "  FILE DOES NOT MATCH".into()
                }
                Err(_) => {
                    wrong += 1;
                    "  file missing".into()
                }
            },
        };
        println!("  {} {}  {}{}", e.name, e.version, e.file, state);
    }
    if wrong > 0 {
        return Err(kk_types::words::counted(&format!(
            "{wrong} file(s) are missing or are not the ones the catalog lists"
        )));
    }
    Ok(())
}
