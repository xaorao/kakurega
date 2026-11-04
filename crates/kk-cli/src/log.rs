use kk_eventlog::{verify_export, Anchor};
use kk_evidence::b64;

fn export_text(args: &[String]) -> Result<String, String> {
    let admin = crate::approve::connect(args)?;
    let (status, text) = admin.get_text("/admin/log")?;
    if status != 200 {
        return Err(format!("the service refused the export ({status})"));
    }
    Ok(text)
}

pub fn run(args: &[String]) -> u8 {
    match args.first().map(String::as_str) {
        Some("export") => {
            let text = match export_text(&args[1..]) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("{e}");
                    return 1;
                }
            };
            match crate::flag(args, "--out") {
                Some(path) => match std::fs::write(&path, &text) {
                    Ok(()) => {
                        match verify_export(&text, None, None) {
                            Ok(r) => println!(
                                "Saved {path}: {} entries. Anchor to keep: {}:{}",
                                r.count, r.count, r.head
                            ),
                            Err(f) => {
                                println!("Saved {path}, but it does not verify: {}", f.as_str())
                            }
                        }
                        0
                    }
                    Err(e) => {
                        eprintln!("cannot write {path}: {e}");
                        1
                    }
                },
                None => {
                    println!("{text}");
                    0
                }
            }
        }
        Some("verify") => {
            let Some(path) = args.get(1).filter(|a| !a.starts_with("--")) else {
                eprintln!("usage: kaku log verify FILE [--pin KEY] [--anchor COUNT:HEAD]");
                return 2;
            };
            let text = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("cannot read {path}: {e}");
                    return 2;
                }
            };
            let pin = match crate::flag(args, "--pin") {
                Some(k) => match b64::decode(&k)
                    .ok()
                    .and_then(|r| <[u8; 32]>::try_from(r).ok())
                {
                    Some(p) => Some(p),
                    None => {
                        eprintln!("--pin needs a base64url Ed25519 public key (32 bytes)");
                        return 2;
                    }
                },
                None => None,
            };
            let anchor = match crate::flag(args, "--anchor") {
                Some(a) => match a.split_once(':').and_then(|(c, h)| {
                    Some(Anchor {
                        count: c.parse().ok()?,
                        head: h.to_string(),
                    })
                }) {
                    Some(a) => Some(a),
                    None => {
                        eprintln!("--anchor needs COUNT:HEAD as printed by `kaku log export`");
                        return 2;
                    }
                },
                None => None,
            };
            match verify_export(&text, pin.as_ref(), anchor.as_ref()) {
                Ok(r) => {
                    println!("VALID: {} entries, head {}", r.count, r.head);
                    println!(
                        "signed by {}{}",
                        r.signer,
                        if r.signer_pinned {
                            " (the key you pinned)"
                        } else {
                            " (key taken from the file; pin it to be sure)"
                        }
                    );
                    if anchor.is_none() {
                        println!("No anchor given: entries removed from the END of the log cannot be detected. Keep the anchor from an earlier export and pass --anchor.");
                    }
                    0
                }
                Err(f) => {
                    println!("INVALID: {}", f.as_str());
                    1
                }
            }
        }
        _ => {
            eprintln!("usage: kaku log export [--out FILE] | kaku log verify FILE [--pin KEY] [--anchor COUNT:HEAD]");
            2
        }
    }
}
