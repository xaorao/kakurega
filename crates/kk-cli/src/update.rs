use kk_evidence::{b64, keys};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn platform() -> String {
    let os = std::env::consts::OS;
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    format!("{os}-{arch}")
}

fn load_signer(path: &str) -> Result<keys::Signer, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read the signing key {path}: {e}"))?;
    let hex = text.trim();
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("the signing key file must hold 64 hex digits".into());
    }
    let mut seed = [0u8; 32];
    for (i, b) in seed.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).map_err(|_| "bad hex")?;
    }
    Ok(keys::Signer::from_seed(seed))
}

fn trusted(args: &[String]) -> Result<keys::TrustedKeys, String> {
    let mut t = keys::TrustedKeys::new();
    let mut sources = crate::flags(args, "--trusted");
    if let Ok(text) = std::fs::read_to_string(crate::data_dir(args).join("update-publisher.pub")) {
        sources.extend(
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from),
        );
    }
    if sources.is_empty() {
        return Err("no publisher key is configured, so no update announcement can be trusted. Give --trusted KEY or put the key in update-publisher.pub in the data folder".into());
    }
    for s in sources {
        let raw = b64::decode(&s).map_err(|_| "a publisher key is not base64url")?;
        let raw: [u8; 32] = raw
            .try_into()
            .map_err(|_| "a publisher key must be 32 bytes")?;
        t.add(&raw)
            .ok_or("a publisher key is not a valid public key")?;
    }
    Ok(t)
}

fn seen_path(args: &[String]) -> std::path::PathBuf {
    crate::data_dir(args).join("update-seen")
}

pub fn run(args: &[String]) -> u8 {
    let result = match args.first().map(String::as_str) {
        Some("keygen") => keygen(&args[1..]),
        Some("sign") => sign(&args[1..]),
        Some("check") => check(&args[1..]),
        Some("verify-file") => verify_file(&args[1..]),
        _ => Err("usage: kaku update keygen --out FILE | sign --key FILE --manifest JSON --out FILE | check --manifest FILE [--current X.Y.Z] [--channel C] [--platform P] [--trusted KEY] | verify-file --manifest FILE --file F".into()),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn keygen(args: &[String]) -> Result<(), String> {
    let out = crate::flag(args, "--out").ok_or("give --out FILE for the private signing key")?;
    let mut seed = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut seed))
        .map_err(|e| format!("no randomness available: {e}"))?;
    let hex: String = seed.iter().map(|b| format!("{b:02x}")).collect();
    let signer = keys::Signer::from_seed(seed);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&out)
        .and_then(|mut f| f.write_all(hex.as_bytes()))
        .map_err(|e| format!("cannot write {out}: {e} (an existing file is never overwritten)"))?;
    println!(
        "private key written to {out} (keep it offline; anyone holding it can announce updates)"
    );
    println!(
        "public key (give this to installations): {}",
        keys::public_key_b64(&signer.public())
    );
    println!("key id: {}", signer.key_id());
    Ok(())
}

fn sign(args: &[String]) -> Result<(), String> {
    let key = crate::flag(args, "--key").ok_or("give --key FILE")?;
    let manifest = crate::flag(args, "--manifest").ok_or("give --manifest JSON")?;
    let out = crate::flag(args, "--out").ok_or("give --out FILE")?;
    let json = std::fs::read(&manifest).map_err(|e| format!("cannot read {manifest}: {e}"))?;
    let signed = kk_update::sign_manifest(&json, &load_signer(&key)?)?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&out)
        .and_then(|mut f| f.write_all(&signed))
        .map_err(|e| format!("cannot write {out}: {e} (an existing file is never overwritten)"))?;
    println!("signed announcement written to {out}");
    Ok(())
}

fn offer_of(args: &[String], remember: bool) -> Result<(kk_update::Offer, Vec<u8>), String> {
    let manifest =
        crate::flag(args, "--manifest").ok_or("give --manifest FILE (the signed announcement)")?;
    let bytes = std::fs::read(&manifest).map_err(|e| format!("cannot read {manifest}: {e}"))?;
    let trusted = trusted(args)?;
    let current =
        crate::flag(args, "--current").unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let current =
        kk_update::Version::parse(&current).ok_or("--current must be major.minor.patch")?;
    let seen = std::fs::read_to_string(seen_path(args))
        .ok()
        .and_then(|t| kk_update::Version::parse(t.trim()));
    let channel = crate::flag(args, "--channel").unwrap_or_else(|| "stable".into());
    let platform = crate::flag(args, "--platform").unwrap_or_else(platform);
    let cx = kk_update::Context {
        trusted: &trusted,
        channel: &channel,
        platform: &platform,
        current,
        highest_seen: seen,
        now: now(),
    };
    let verdict = kk_update::check(&bytes, &cx).map_err(|r| format!("refused: {r:?}"))?;
    if remember {
        if let Some(v) = kk_update::announced_version(&bytes) {
            if seen.is_none_or(|s| v > s) {
                let _ = std::fs::create_dir_all(crate::data_dir(args));
                let _ = std::fs::write(seen_path(args), v.to_string());
            }
        }
    }
    match verdict {
        kk_update::Verdict::UpToDate => Err(format!("up to date ({current})")),
        kk_update::Verdict::Available(o) => Ok((o, bytes)),
    }
}

fn check(args: &[String]) -> Result<(), String> {
    match offer_of(args, true) {
        Ok((o, _)) => {
            println!("update available: {}", o.version);
            println!("download: {}", o.artifact.url);
            println!("sha256: {}  size: {}", o.artifact.sha256, o.artifact.size);
            println!("notes:\n{}", o.notes);
            println!("Nothing was downloaded or installed. Check a download with: kaku update verify-file --manifest FILE --file F");
            Ok(())
        }
        Err(e) if e.starts_with("up to date") => {
            println!("{e}");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn verify_file(args: &[String]) -> Result<(), String> {
    let file = crate::flag(args, "--file").ok_or("give --file F (the downloaded installer)")?;
    let (offer, _) = offer_of(args, false)?;
    let bytes = std::fs::read(&file).map_err(|e| format!("cannot read {file}: {e}"))?;
    kk_update::verify_download(&offer, &bytes)
        .map_err(|r| format!("do NOT install this file: {r:?}"))?;
    println!(
        "ok: {file} is exactly the file the publisher signed for version {}",
        offer.version
    );
    Ok(())
}
