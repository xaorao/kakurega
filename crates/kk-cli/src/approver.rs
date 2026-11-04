use kk_evidence::approval::{create_key, load_key, KeyFileError};
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
            let path = key_path(args);
            let signer = match load_key(&path) {
                Ok(Some(s)) => s,
                Ok(None) => match create_key(&path) {
                    Ok(s) => {
                        println!(
                            "Created an approver key at {} (private to you, mode 0600).",
                            path.display()
                        );
                        s
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
            };
            let label = crate::flag(args, "--label").unwrap_or_else(|| "this terminal".into());
            let id = kk_evidence::keys::KeyId::of(&signer.public());
            let mut body =
                json!({"public_key": kk_evidence::b64::encode(&signer.public()), "label": label});
            if let Some(existing) = crate::flag(args, "--sign-with")
                .and_then(|p| load_key(std::path::Path::new(&p)).ok().flatten())
            {
                body["proof"] = json!(kk_evidence::approval::sign_action(
                    &existing,
                    &kk_evidence::approval::Action {
                        action: "enroll".into(),
                        subject: id.as_str().to_string(),
                        at: now()
                    }
                ));
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
            if let Ok(Some(k)) = load_key(&key_path(args)) {
                body["proof"] = json!(kk_evidence::approval::sign_action(
                    &k,
                    &kk_evidence::approval::Action {
                        action: "revoke".into(),
                        subject: id.to_string(),
                        at: now()
                    }
                ));
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
