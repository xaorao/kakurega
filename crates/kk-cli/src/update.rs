use kk_evidence::{b64, keys};
use kk_update::root::{self, Root, Seen};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;

pub(crate) fn now() -> u64 {
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

pub(crate) fn load_signer(path: &str) -> Result<keys::Signer, String> {
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

fn root_path(args: &[String]) -> std::path::PathBuf {
    crate::data_dir(args).join("update-root.json")
}

/// Who may announce updates here: the signed root kept in the data folder when there is one, otherwise the plain publisher
/// keys from `--trusted` and `update-publisher.pub`.
fn load_root(args: &[String]) -> Result<Root, String> {
    if let Ok(bytes) = std::fs::read(root_path(args)) {
        return root::load_initial(&bytes)
            .map_err(|e| format!("the saved root in update-root.json is not valid: {e:?}"));
    }
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
        return Err("no publisher key or root is configured, so no update announcement can be trusted. Install a root with `kaku update root-init --root FILE --accept`, or give --trusted KEY, or put the key in update-publisher.pub in the data folder".into());
    }
    let mut publics = Vec::new();
    for s in sources {
        let raw = b64::decode(&s).map_err(|_| "a publisher key is not base64url")?;
        let raw: [u8; 32] = raw
            .try_into()
            .map_err(|_| "a publisher key must be 32 bytes")?;
        publics.push(raw);
    }
    Root::legacy(&publics).ok_or_else(|| "a publisher key is not a valid public key".to_string())
}

fn describe(r: &Root) -> String {
    let ids: Vec<String> = r.key_ids().iter().map(|k| format!("  {k}")).collect();
    format!(
        "root version {}, {} of {} keys must sign, valid until {}\n{}",
        r.version,
        r.threshold,
        ids.len(),
        if r.not_after == u64::MAX {
            "no limit".to_string()
        } else {
            format!("unix time {}", r.not_after)
        },
        ids.join("\n")
    )
}

fn read_seen(args: &[String]) -> Option<Seen> {
    let text = std::fs::read_to_string(seen_path(args)).ok()?;
    let mut parts = text.split_whitespace();
    let first = parts.next()?;
    match parts.next() {
        // "ROOT_VERSION VERSION": the version seen and the root it was seen under
        Some(v) => Some(Seen {
            root_version: first.parse().ok()?,
            version: kk_update::Version::parse(v)?,
        }),
        // an older record that held only a version: seen before any signed root
        None => Some(Seen {
            root_version: 0,
            version: kk_update::Version::parse(first)?,
        }),
    }
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
        Some("download") => download(&args[1..]),
        Some("root-sign") => root_sign(&args[1..]),
        Some("root-init") => root_init(&args[1..]),
        Some("root-rotate") => root_rotate(&args[1..]),
        Some("root-show") => root_show(&args[1..]),
        _ => Err("usage: kaku update keygen --out FILE | sign --key FILE --manifest JSON --out FILE | check --manifest FILE [--current X.Y.Z] [--channel C] [--platform P] [--trusted KEY] | verify-file --manifest FILE --file F | download --manifest FILE [--out DIR] [--current X.Y.Z] [--channel C] [--platform P] [--trusted KEY] | root-sign --root JSON --key FILE [--key FILE ...] --out FILE | root-init --root FILE [--accept] | root-rotate FILE... | root-show".into()),
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

pub(crate) fn keygen(args: &[String]) -> Result<(), String> {
    keygen_for(args, "announce updates")
}

pub(crate) fn keygen_for(args: &[String], power: &str) -> Result<(), String> {
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
    println!("private key written to {out} (keep it offline; anyone holding it can {power})");
    println!(
        "public key (give this to installations): {}",
        keys::public_key_b64(&signer.public())
    );
    println!("key id: {}", signer.key_id());
    Ok(())
}

fn sign(args: &[String]) -> Result<(), String> {
    let keys_given = crate::flags(args, "--key");
    if keys_given.is_empty() {
        return Err("give --key FILE (one per publisher that must sign)".into());
    }
    let manifest = crate::flag(args, "--manifest").ok_or("give --manifest JSON")?;
    let out = crate::flag(args, "--out").ok_or("give --out FILE")?;
    let json = std::fs::read(&manifest).map_err(|e| format!("cannot read {manifest}: {e}"))?;
    let signers = keys_given
        .iter()
        .map(|k| load_signer(k))
        .collect::<Result<Vec<_>, _>>()?;
    let refs: Vec<&keys::Signer> = signers.iter().collect();
    let signed = kk_update::sign_manifest_multi(&json, &refs)?;
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
    let root = load_root(args)?;
    let current =
        crate::flag(args, "--current").unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    let current =
        kk_update::Version::parse(&current).ok_or("--current must be major.minor.patch")?;
    let seen = Seen::effective(read_seen(args), &root);
    let channel = crate::flag(args, "--channel").unwrap_or_else(|| "stable".into());
    let platform = crate::flag(args, "--platform").unwrap_or_else(platform);
    let cx = kk_update::Context {
        root: &root,
        channel: &channel,
        platform: &platform,
        current,
        highest_seen: seen,
        now: now(),
    };
    let verdict = kk_update::check(&bytes, &cx)
        .map_err(|r| format!("refused: {}", refusal_words(&format!("{r:?}"))))?;
    if remember {
        if let Some(v) = kk_update::announced_version(&bytes) {
            if seen.is_none_or(|s| v > s) {
                let _ = std::fs::create_dir_all(crate::data_dir(args));
                let _ = kk_vault::durable::write_atomic(
                    &seen_path(args),
                    format!("{} {v}", root.version).as_bytes(),
                    0o600,
                );
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
    kk_update::verify_download(&offer, &bytes).map_err(|r| {
        format!(
            "do NOT install this file: {}",
            refusal_words(&format!("{r:?}"))
        )
    })?;
    println!(
        "ok: {file} is exactly the file the publisher signed for version {}",
        offer.version
    );
    Ok(())
}

fn write_new(out: &str, bytes: &[u8]) -> Result<(), String> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(|e| format!("cannot write {out}: {e} (an existing file is never overwritten)"))
}

/// Publisher side: signs a root description with every given key.
fn root_sign(args: &[String]) -> Result<(), String> {
    let json_path = crate::flag(args, "--root").ok_or("give --root JSON (the root description)")?;
    let out = crate::flag(args, "--out").ok_or("give --out FILE")?;
    let key_files = crate::flags(args, "--key");
    let json = std::fs::read(&json_path).map_err(|e| format!("cannot read {json_path}: {e}"))?;
    let signers = key_files
        .iter()
        .map(|k| load_signer(k))
        .collect::<Result<Vec<_>, _>>()?;
    let refs: Vec<&keys::Signer> = signers.iter().collect();
    let signed = root::sign_root(&json, &refs)?;
    write_new(&out, &signed)?;
    println!("signed root written to {out}");
    Ok(())
}

/// Installs the first root. This decides who may ever announce updates here, so it only describes the root until it is run
/// again with --accept, and it never replaces an installed root (that is what root-rotate checks).
fn root_init(args: &[String]) -> Result<(), String> {
    let file = crate::flag(args, "--root").ok_or("give --root FILE (a signed root)")?;
    let bytes = std::fs::read(&file).map_err(|e| format!("cannot read {file}: {e}"))?;
    if root_path(args).exists() {
        return Err("a root is already installed. Replace it with `kaku update root-rotate FILE...`, which checks that the old keys agree".into());
    }
    let new = root::load_initial(&bytes).map_err(|e| format!("refused: {e:?}"))?;
    println!("{}", describe(&new));
    if !args.iter().any(|a| a == "--accept") {
        println!("Only these keys may announce updates if you accept. Check the key ids with the publisher, then run again with --accept.");
        return Err("not installed".into());
    }
    std::fs::create_dir_all(crate::data_dir(args)).map_err(|e| e.to_string())?;
    write_new(&root_path(args).to_string_lossy(), &bytes)?;
    println!("root installed");
    Ok(())
}

/// Applies a chain of newer roots. Nothing is saved unless the whole chain checks out.
fn root_rotate(args: &[String]) -> Result<(), String> {
    // the root files, in order: every argument that is not a flag or the value of --data-dir
    let mut files: Vec<&String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--data-dir" {
            it.next();
        } else if !a.starts_with("--") {
            files.push(a);
        }
    }
    if files.is_empty() {
        return Err("give the newer root files in order: kaku update root-rotate FILE...".into());
    }
    let current = std::fs::read(root_path(args))
        .map_err(|_| "no root is installed yet: use root-init first".to_string())
        .and_then(|b| {
            root::load_initial(&b).map_err(|e| format!("the saved root is not valid: {e:?}"))
        })?;
    let mut chain = Vec::new();
    for f in &files {
        chain.push(std::fs::read(f).map_err(|e| format!("cannot read {f}: {e}"))?);
    }
    let last = chain.last().cloned().unwrap_or_default();
    let next =
        root::follow(current.clone(), &chain, now()).map_err(|e| format!("refused: {e:?}"))?;
    let tmp = root_path(args).with_extension("json.new");
    let _ = std::fs::remove_file(&tmp);
    write_new(&tmp.to_string_lossy(), &last)?;
    std::fs::rename(&tmp, root_path(args)).map_err(|e| format!("cannot save the new root: {e}"))?;
    println!(
        "root replaced: version {} to {}",
        current.version, next.version
    );
    println!("{}", describe(&next));
    println!("The highest version seen under the old root is no longer counted, so a forged announcement made with a stolen key cannot block real releases.");
    Ok(())
}

fn root_show(args: &[String]) -> Result<(), String> {
    let root = load_root(args)?;
    println!("{}", describe(&root));
    if !root_path(args).exists() {
        println!("(no signed root is installed; these are the plain publisher keys)");
    }
    if root.is_expired(now()) {
        println!(
            "this root has EXPIRED: no announcement is accepted until a newer root is applied"
        );
    }
    Ok(())
}

/// Downloads the file a valid, signed announcement names, into a private folder, and checks it against the signed size and
/// checksum before it is given its final name. Nothing is opened, extracted or run.
fn download(args: &[String]) -> Result<(), String> {
    let (offer, _) = offer_of(args, true)?;
    let platform = crate::flag(args, "--platform").unwrap_or_else(platform);
    let dir = crate::flag(args, "--out")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::data_dir(args).join("updates"));
    let transport = kk_fetch::UreqTransport {
        timeout: std::time::Duration::from_secs(60 * 60),
    };
    let fetcher = kk_fetch::Fetcher {
        dns: &kk_fetch::SystemDns,
        transport: &transport,
    };
    println!(
        "downloading {} bytes for version {} (nothing is opened or run)",
        offer.artifact.size, offer.version
    );
    let path = kk_update::download::download(&offer, &platform, &dir, &fetcher).map_err(|r| {
        format!(
            "the download was refused and nothing was kept: {}",
            refusal_words(&format!("{r:?}"))
        )
    })?;
    println!(
        "ok: {} is exactly the file the publisher signed for version {}",
        path.display(),
        offer.version
    );
    println!("Installing it is a separate step that needs a signed and notarised build.");
    Ok(())
}

/// A refusal of the update check, in words, with the check's own name for it after, for anyone who searches for it.
fn refusal_words(reason: &str) -> String {
    let known = [
        ("NoArtifactForThisComputer", "the announcement lists no file for this kind of computer"),
        ("NotSigned", "it is not signed by enough of the keys this program trusts, or it was altered"),
        ("BadArtifact", "the file does not match what the signed announcement says it must be"),
        ("RootExpired", "the list of keys this program trusts is past its date; a newer signed list has to be applied first"),
        ("Expired", "the announcement is past its date"),
        ("Rollback", "it is older than an announcement this program has already seen, so it may be a replay"),
        ("WrongChannel", "it was made for another release channel"),
        ("Malformed", "it is not a well-formed announcement"),
    ];
    match known.iter().find(|(k, _)| reason.contains(k)) {
        Some((name, words)) => format!("{words} ({name})"),
        None => reason.to_string(),
    }
}
