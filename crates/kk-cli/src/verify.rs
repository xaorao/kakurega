use kk_evidence::b64;
use kk_evidence::capsule::{verify, VerifyOptions};

pub fn run(args: &[String]) -> u8 {
    let Some(path) = args.first() else {
        eprintln!("usage: kaku verify CAPSULE [--pin KEY]");
        return 2;
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return 2;
        }
    };
    let pinned = crate::flags(args, "--pin")
        .iter()
        .map(|k| {
            b64::decode(k)
                .ok()
                .and_then(|raw| <[u8; 32]>::try_from(raw).ok())
        })
        .collect::<Option<Vec<_>>>();
    let has_pin = args.iter().any(|a| a == "--pin");
    if has_pin && pinned.is_none() {
        eprintln!("--pin needs a base64url Ed25519 public key (32 bytes)");
        return 2;
    }
    // a share (some files of a release, with a proof) is checked by its own rules
    if is_share(&bytes) {
        return share(
            &bytes,
            if has_pin { pinned } else { None },
            crate::flag(args, "--out").as_deref(),
        );
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let report = verify(
        &bytes,
        &VerifyOptions {
            pinned_keys: if has_pin { pinned } else { None },
            now,
            expected_recipient: None,
        },
    );
    if report.valid {
        println!("VALID");
        println!(
            "signed by {}{}",
            report.signer.as_ref().map(|k| k.as_str()).unwrap_or("?"),
            if report.signer_pinned {
                " (the key you pinned)"
            } else {
                " (key taken from the capsule; pin it to be sure who signed)"
            }
        );
        if let Some(k) = report.signer_public {
            println!(
                "signing key {} (the value an order on NEAR names as the reviewer key)",
                k.iter().map(|b| format!("{b:02x}")).collect::<String>()
            );
        }
        println!("recipient {}", report.recipient.as_deref().unwrap_or("?"));
        println!(
            "assurance {} via {}",
            report.binding_level.as_deref().unwrap_or("?"),
            report.route.as_deref().unwrap_or("?")
        );
        if !report.receipts.is_empty() {
            println!(
                "\nModel answers signed by their model ({}):",
                report.receipts.len()
            );
            for r in &report.receipts {
                println!(
                    "  {} answer {}: request {}, answer {}, signer {}",
                    r.model,
                    r.completion_id,
                    &r.request_sha256[..12],
                    &r.response_sha256[..12],
                    &r.signer[..12]
                );
            }
        }
        println!("\nEstablished:");
        for e in &report.established {
            println!("  + {e}");
        }
        println!("\nNot established:");
        for n in &report.not_established {
            println!("  - {n}");
        }
        0
    } else {
        println!("INVALID: {}", report.reason.unwrap_or("unknown"));
        1
    }
}

pub fn check_bytes(bytes: &[u8]) -> Result<serde_json::Value, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let report = verify(
        bytes,
        &VerifyOptions {
            pinned_keys: None,
            now,
            expected_recipient: None,
        },
    );
    if report.valid {
        Ok(serde_json::json!({
            "valid": true,
            "signer": report.signer.as_ref().map(|k| k.as_str()),
            "signer_pinned": report.signer_pinned,
            "recipient": report.recipient,
            "assurance": report.binding_level,
            "route": report.route,
            "receipts": report.receipts.iter().map(|r| serde_json::json!({
                "completion_id": r.completion_id, "model": r.model, "signer": r.signer,
                "request_sha256": r.request_sha256, "response_sha256": r.response_sha256,
            })).collect::<Vec<_>>(),
            "established": report.established,
            "not_established": report.not_established,
        }))
    } else {
        Ok(serde_json::json!({"valid": false, "reason": report.reason}))
    }
}

/// Whether a file says it is a share. Only its own word for it: the check that follows decides whether it is a valid one.
fn is_share(bytes: &[u8]) -> bool {
    bytes.len() < 32 << 20
        && bytes
            .windows(25)
            .any(|w| w == b"\"format\":\"kakurega-share\"")
}

/// Checks a share and prints what it shows. With `--out DIR` the shared files are also written into a new folder.
fn share(bytes: &[u8], pinned: Option<Vec<[u8; 32]>>, out: Option<&str>) -> u8 {
    use kk_evidence::share::{verify_share, ShareOptions};
    let report = verify_share(
        bytes,
        &ShareOptions {
            pinned_keys: pinned,
        },
    );
    if !report.valid {
        println!("INVALID: {}", report.reason.unwrap_or("unknown"));
        return 1;
    }
    println!("VALID SHARE");
    println!(
        "signed by {}{}",
        report.signer.as_ref().map(|k| k.as_str()).unwrap_or("?"),
        if report.signer_pinned {
            " (the key you pinned)"
        } else {
            " (key taken from the share; pin it to be sure who signed)"
        }
    );
    println!(
        "{} of the release's {} files are shared:",
        report.files.len(),
        report.total_files
    );
    for (name, content) in &report.files {
        println!("  {name}  ({} bytes)", content.len());
    }
    println!(
        "capsule {}",
        report.capsule_sha256.as_deref().unwrap_or("?")
    );
    println!("\nEstablished:");
    for e in &report.established {
        println!("  + {e}");
    }
    println!("\nNot established:");
    for n in &report.not_established {
        println!("  - {n}");
    }
    if let Some(dir) = out {
        let dir = std::path::Path::new(dir);
        // a new folder only: nothing that exists is written over, and a name was already checked to be a plain file name
        if let Err(e) = std::fs::create_dir(dir) {
            eprintln!("cannot make the folder {}: {e}", dir.display());
            return 2;
        }
        for (name, content) in &report.files {
            if let Err(e) = std::fs::write(dir.join(name), content) {
                eprintln!("cannot write {name}: {e}");
                return 2;
            }
        }
        // the check script reads a list of findings; one is written here for the shared finding(s) only. It is made on this
        // computer from the shared entries and is not part of what was signed.
        let entries: Vec<serde_json::Value> = report
            .files
            .iter()
            .filter(|(name, _)| name.starts_with("meta-") && name.ends_with(".json"))
            .filter_map(|(_, content)| serde_json::from_slice(content).ok())
            .collect();
        let has_index = report.files.iter().any(|(name, _)| name == "index.json");
        if !entries.is_empty() && !has_index {
            let index = serde_json::json!({ "findings": entries });
            if let Err(e) = std::fs::write(dir.join("index.json"), index.to_string()) {
                eprintln!("cannot write index.json: {e}");
                return 2;
            }
            println!(
                "\nThe shared files are in {}. To re-run the finding: python3 {}/run-check.py",
                dir.display(),
                dir.display()
            );
        } else {
            println!("\nThe shared files are in {}", dir.display());
        }
    }
    0
}
