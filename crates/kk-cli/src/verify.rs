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
        println!("recipient {}", report.recipient.as_deref().unwrap_or("?"));
        println!(
            "assurance {} via {}",
            report.binding_level.as_deref().unwrap_or("?"),
            report.route.as_deref().unwrap_or("?")
        );
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
            "established": report.established,
            "not_established": report.not_established,
        }))
    } else {
        Ok(serde_json::json!({"valid": false, "reason": report.reason}))
    }
}
