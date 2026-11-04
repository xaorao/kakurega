use kk_evidence::approval::{self, ApproverKey, KeyFileError};
use serde_json::json;
use std::path::PathBuf;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn key_path(args: &[String]) -> PathBuf {
    crate::data_dir(args).join("approver.key")
}

pub fn run(args: &[String]) -> u8 {
    let admin = match crate::approve::connect(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    match args.first().map(String::as_str) {
        Some("enroll") => {
            let dir = crate::data_dir(args);
            let existing = match approval::load_approver(&dir) {
                Ok(k) => k,
                Err(e) => {
                    eprintln!("the existing approver key is not usable: {e:?}");
                    return 1;
                }
            };
            let hardware = args.iter().any(|a| a == "--enclave");
            let key = if hardware {
                let Some(helper) = approval::find_helper() else {
                    eprintln!("the Secure Enclave helper kaku-enclave was not found; build it with spikes/enclave/build.sh or set KAKU_ENCLAVE_BIN");
                    return 1;
                };
                let blob = dir.join("approver.enclave");
                let mut cmd = std::process::Command::new(&helper);
                cmd.args(["enroll", "--blob"]).arg(&blob);
                if args.iter().any(|a| a == "--presence") {
                    cmd.arg("--presence");
                }
                match cmd.output() {
                    Ok(o) if o.status.success() => {
                        println!("Created a Secure Enclave key at {}. The private key never leaves this Mac's Secure Enclave.", blob.display());
                        ApproverKey::Enclave(approval::EnclaveKey {
                            helper,
                            blob,
                            data_dir: dir.clone(),
                        })
                    }
                    Ok(o) => {
                        eprintln!("{}", String::from_utf8_lossy(&o.stderr).trim());
                        return 1;
                    }
                    Err(e) => {
                        eprintln!("could not run the helper: {e}");
                        return 1;
                    }
                }
            } else {
                let path = key_path(args);
                match approval::load_key(&path) {
                    Ok(Some(s)) => ApproverKey::Software(s),
                    Ok(None) => match approval::create_key(&path) {
                        Ok(s) => {
                            println!(
                                "Created an approver key at {} (a plain file, private to you, mode 0600). A Secure Enclave key is stronger: use --enclave.",
                                path.display()
                            );
                            ApproverKey::Software(s)
                        }
                        Err(e) => {
                            eprintln!("could not create the key: {e:?}");
                            return 1;
                        }
                    },
                    Err(KeyFileError::Permissions) => {
                        eprintln!(
                            "{} is readable by others; fix it with chmod 600 and try again",
                            path.display()
                        );
                        return 1;
                    }
                    Err(e) => {
                        eprintln!("the key file is not usable: {e:?}");
                        return 1;
                    }
                }
            };
            let label = crate::flag(args, "--label").unwrap_or_else(|| "this terminal".into());
            let (public, id) = match (key.public_bytes(), key.key_id()) {
                (Ok(p), Ok(i)) => (p, i),
                (Err(e), _) | (_, Err(e)) => {
                    eprintln!("could not read the key: {e}");
                    return 1;
                }
            };
            let mut body = json!({"public_key": kk_evidence::b64::encode(&public), "label": label});
            if let Some(existing) = existing {
                match existing.sign_change(&approval::Action {
                    action: "enroll".into(),
                    subject: id.clone(),
                    at: now(),
                }) {
                    Ok(p) => body["proof"] = json!(p),
                    Err(e) => {
                        eprintln!("signing was refused: {e}");
                        return 1;
                    }
                }
            }
            match admin.post("/admin/approvers", body) {
                Ok((200, v)) => {
                    println!("Enrolled {}. From now on a release is approved only by a signature from an enrolled key over the exact release, recipient, policy and a one-time nonce.", v["key_id"].as_str().unwrap_or("?"));
                    0
                }
                other => {
                    eprintln!("could not enroll: {other:?}");
                    1
                }
            }
        }
        Some("list") => match admin.get("/admin/approvers") {
            Ok((200, v)) => {
                for a in v["approvers"].as_array().into_iter().flatten() {
                    println!(
                        "{}  {}",
                        a["key_id"].as_str().unwrap_or("?"),
                        a["label"].as_str().unwrap_or("")
                    );
                }
                0
            }
            other => {
                eprintln!("could not list: {other:?}");
                1
            }
        },
        Some("revoke") => {
            let Some(id) = args.get(1).filter(|a| !a.starts_with("--")) else {
                eprintln!("usage: kaku approver revoke KEY_ID");
                return 2;
            };
            let mut body = json!({"key_id": id});
            if let Ok(Some(k)) = approval::load_approver(&crate::data_dir(args)) {
                match k.sign_change(&approval::Action {
                    action: "revoke".into(),
                    subject: id.to_string(),
                    at: now(),
                }) {
                    Ok(p) => body["proof"] = json!(p),
                    Err(e) => {
                        eprintln!("signing was refused: {e}");
                        return 1;
                    }
                }
            }
            match admin.post("/admin/approvers/revoke", body) {
                Ok((200, v)) => {
                    println!(
                        "{}",
                        if v["revoked"] == true {
                            "Revoked."
                        } else {
                            "No such approver."
                        }
                    );
                    0
                }
                other => {
                    eprintln!("could not revoke: {other:?}");
                    1
                }
            }
        }
        _ => {
            eprintln!("usage: kaku approver enroll [--label NAME] | list | revoke KEY_ID");
            2
        }
    }
}
