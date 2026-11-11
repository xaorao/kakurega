//! `kaku chain`: what NEAR holds about a capsule, a model's signing key or an order, and the transactions an owner asks for.
//! Reading needs no account. Transactions are sent on testnet only, paid by an account whose key file the owner names.

use kk_chain::{Chain, ChainError, Network, Payer};
use kk_evidence::b64;
use kk_evidence::capsule::{sha256_hex, verify, VerifyOptions};
use serde_json::{json, Value};

const USAGE: &str = "usage:
  kaku chain hardware KEY                 what NEAR recorded about a model's signing key (64 hex digits)
  kaku chain attest --model NAME --key-file FILE [--account NAME]
                                          have NEAR's verifier check that model's hardware proof and record its key (testnet)
  kaku chain capsule FILE                 who registered this capsule on NEAR, and what the chain checked
  kaku chain order ID                     what an order holds
options: [--network testnet|mainnet] [--registry ACCOUNT] [--json]";

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Nanoseconds since 1970, as the chain gives them in a string, in words.
fn when(v: &Value) -> String {
    let secs = v.as_str().and_then(|s| s.parse::<u128>().ok()).unwrap_or(0) / 1_000_000_000;
    let (days, rest) = ((secs / 86_400) as i64, secs % 86_400);
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// An amount as the chain holds it, in words: NEAR to five places, or the token's smallest units.
fn amount(order: &Value) -> String {
    let raw = order["amount"]
        .as_str()
        .and_then(|s| s.parse::<u128>().ok());
    match (raw, order["token"].as_str()) {
        (Some(y), None) => {
            let whole = y / 10u128.pow(24);
            let part = y % 10u128.pow(24) / 10u128.pow(19);
            format!("{whole}.{part:05} NEAR")
        }
        (Some(u), Some(t)) => format!("{u} smallest units of token {}", clean(t)),
        (None, _) => "an unreadable amount".to_string(),
    }
}

fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("?")
}

/// Text from the chain is shown without control characters, so a record cannot rewrite the terminal.
fn clean(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(300).collect()
}

fn hardware_lines(h: &Value) -> Vec<String> {
    let mut l = vec![
        format!("Model signing key: {}", clean(text(&h["signer"]))),
        format!(
            "NEAR's verifier checked its Intel TDX quote against Intel's collateral on {}",
            when(&h["attested_at"])
        ),
        format!("Intel patch level then: {}", clean(text(&h["tcb_status"]))),
        format!("Asked for by: {}", clean(text(&h["attested_by"]))),
    ];
    if let Some(c) = h["compose_sha256"].as_str() {
        l.push(format!("Deployment description (SHA-256): {}", clean(c)));
    }
    l.push(format!(
        "Machine measurement (MRTD): {}",
        clean(text(&h["mr_td"]))
    ));
    l
}

const HARDWARE_LIMITS: [&str; 3] = [
    "that the quote is recent, beyond Intel's collateral accepting it at that time",
    "which software ran (the measurements are recorded, none is approved by the contract)",
    "anything about the GPU (NVIDIA evidence is checked off chain only)",
];

fn fail(e: &ChainError) -> u8 {
    eprintln!("{e}");
    match e {
        ChainError::Input(_) | ChainError::ReadOnlyNetwork => 2,
        _ => 1,
    }
}

pub fn run(args: &[String]) -> u8 {
    let network = match crate::flag(args, "--network").as_deref() {
        None => Network::Testnet,
        Some(n) => match Network::parse(n) {
            Some(n) => n,
            None => {
                eprintln!("--network is testnet or mainnet");
                return 2;
            }
        },
    };
    let registry = match (crate::flag(args, "--registry"), &network) {
        (Some(r), _) => r,
        (None, Network::Testnet) => kk_chain::TESTNET_REGISTRY.to_string(),
        (None, Network::Mainnet) => {
            eprintln!("no registry is known on mainnet; name one with --registry");
            return 2;
        }
    };
    let as_json = args.iter().any(|a| a == "--json");
    let chain = match Chain::new(network, &registry) {
        Ok(c) => c,
        Err(e) => return fail(&e),
    };
    let rt = match kk_chain::runtime() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("could not start: {e}");
            return 1;
        }
    };
    let value_flags = [
        "--network",
        "--registry",
        "--model",
        "--key-file",
        "--account",
    ];
    let mut words: Vec<&String> = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
        } else if value_flags.contains(&a.as_str()) {
            skip = true;
        } else if !a.starts_with("--") {
            words.push(a);
        } else if a != "--json" {
            eprintln!("unknown option `{a}`\n{USAGE}");
            return 2;
        }
    }
    let place = format!("{} on NEAR {}", chain.registry(), chain.network().name());
    match (words.first().map(|s| s.as_str()), words.get(1)) {
        (Some("hardware"), Some(key)) => match rt.block_on(chain.hardware(key)) {
            Err(e) => fail(&e),
            Ok(None) => {
                if as_json {
                    println!(
                        "{}",
                        json!({"recorded": false, "registry": chain.registry()})
                    );
                } else {
                    println!("No hardware record for this key at {place}.");
                    println!("That says only that nobody has had it checked there yet.");
                }
                3
            }
            Ok(Some(h)) => {
                if as_json {
                    println!(
                        "{}",
                        json!({"recorded": true, "registry": chain.registry(), "hardware": h})
                    );
                } else {
                    println!("Recorded at {place}:");
                    for l in hardware_lines(&h) {
                        println!("  {l}");
                    }
                    println!("\nThis does not establish:");
                    for l in HARDWARE_LIMITS {
                        println!("  · {l}");
                    }
                }
                0
            }
        },
        (Some("attest"), None) => {
            let (Some(model), Some(file)) = (
                crate::flag(args, "--model"),
                crate::flag(args, "--key-file"),
            ) else {
                eprintln!("{USAGE}");
                return 2;
            };
            let payer = match Payer::from_key_file(
                std::path::Path::new(&file),
                crate::flag(args, "--account").as_deref(),
            ) {
                Ok(p) => p,
                Err(e) => return fail(&e),
            };
            if !as_json {
                eprintln!(
                    "Asking NEAR AI Cloud for {}'s hardware quote, Intel for its collateral, then {place}. Paid by {}.",
                    clean(&model),
                    payer.account
                );
            }
            match rt.block_on(chain.attest_model(&payer, &model)) {
                Err(e) => fail(&e),
                Ok((sent, h)) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({"transaction": sent.transaction, "gas_tgas": sent.gas_tgas, "registry": chain.registry(), "hardware": h})
                        );
                    } else {
                        println!(
                            "Recorded. Transaction {} ({:.1} Tgas).",
                            sent.transaction, sent.gas_tgas
                        );
                        for l in hardware_lines(&h) {
                            println!("  {l}");
                        }
                    }
                    0
                }
            }
        }
        (Some("order"), Some(id)) => {
            let Ok(id) = id.parse::<u64>() else {
                eprintln!("an order is named by its number");
                return 2;
            };
            match rt.block_on(chain.order(id)) {
                Err(e) => fail(&e),
                Ok(None) => {
                    println!("No order {id} at {place}.");
                    3
                }
                Ok(Some(o)) => {
                    if as_json {
                        println!("{o}");
                    } else {
                        println!("Order {id} at {place}: {}", clean(text(&o["state"])));
                        println!("  Client: {}", clean(text(&o["client"])));
                        println!(
                            "  Reviewer: {} with capsule key {}",
                            clean(text(&o["reviewer"])),
                            clean(text(&o["reviewer_key"]))
                        );
                        println!("  Amount: {}", amount(&o));
                        println!(
                            "  Opened {}, deadline {}",
                            when(&o["opened_at"]),
                            when(&o["deadline"])
                        );
                        println!(
                            "  Hardware required: {}",
                            if o["require_hardware"] == true {
                                "yes"
                            } else {
                                "no"
                            }
                        );
                        if let Some(c) = o["capsule_sha256"].as_str() {
                            println!("  Capsule: {}", clean(c));
                        }
                    }
                    0
                }
            }
        }
        (Some("capsule"), Some(path)) => {
            let bytes = match std::fs::read(path.as_str()) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("cannot read {path}: {e}");
                    return 2;
                }
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let report = verify(
                &bytes,
                &VerifyOptions {
                    pinned_keys: None,
                    now,
                    expected_recipient: None,
                },
            );
            // the report names the signer by its key id; the registry is keyed by the key itself, which the capsule carries
            let signer = report.signer.as_ref().and_then(|id| {
                let file: Value = serde_json::from_slice(&bytes).ok()?;
                file["keys"].as_array()?.iter().find_map(|k| {
                    let raw = <[u8; 32]>::try_from(b64::decode(k.as_str()?).ok()?).ok()?;
                    (kk_evidence::keys::KeyId::of(&raw) == *id).then_some(raw)
                })
            });
            let (true, Some(signer)) = (report.valid, signer) else {
                eprintln!(
                    "this file is not a valid capsule ({}); nothing was looked up",
                    report.reason.unwrap_or("unknown")
                );
                return 4;
            };
            let (digest, signer) = (sha256_hex(&bytes), hex(&signer));
            let found = rt.block_on(async {
                let entry = chain.entry(&digest, &signer).await?;
                let mut keys: Vec<String> = Vec::new();
                for r in &report.receipts {
                    if !keys.contains(&r.signer) {
                        keys.push(r.signer.clone());
                    }
                }
                let mut hardware = Vec::new();
                for k in keys {
                    hardware.push((k.clone(), chain.hardware(&k).await?));
                }
                let order = match entry
                    .as_ref()
                    .and_then(|e| e["order"].as_str())
                    .and_then(|s| s.parse::<u64>().ok())
                {
                    Some(id) => chain.order(id).await?.map(|o| (id, o)),
                    None => None,
                };
                Ok::<_, ChainError>((entry, hardware, order))
            });
            let (entry, hardware, order) = match found {
                Ok(f) => f,
                Err(e) => return fail(&e),
            };
            if as_json {
                println!(
                    "{}",
                    json!({
                        "capsule_sha256": digest, "capsule_signer": signer, "registry": chain.registry(),
                        "entry": entry,
                        "hardware": hardware.iter().map(|(k, h)| json!({"signer": k, "record": h})).collect::<Vec<_>>(),
                        "order": order.as_ref().map(|(id, o)| json!({"id": id, "order": o})),
                    })
                );
                return if entry.is_some() { 0 } else { 3 };
            }
            println!("Capsule {digest}\nsigned by {signer}");
            let Some(e) = entry else {
                println!("\nNot registered at {place} under this signing key.");
                return 3;
            };
            println!(
                "\nRegistered at {place} by {} on {}.",
                clean(text(&e["registered_by"])),
                when(&e["registered_at"])
            );
            println!("That account proved it controls the capsule's signing key.");
            let on_chain = e["receipts"].as_array().cloned().unwrap_or_default();
            println!("Model signatures the contract verified: {}", on_chain.len());
            let missing = report
                .receipts
                .iter()
                .filter(|r| {
                    !on_chain.iter().any(|c| {
                        c["signer"] == json!(r.signer)
                            && c["request_sha256"] == json!(r.request_sha256)
                            && c["response_sha256"] == json!(r.response_sha256)
                    })
                })
                .count();
            if missing > 0 {
                println!(
                    "  {missing} of this capsule's model signatures were not registered with it."
                );
            }
            for (key, record) in &hardware {
                match record {
                    Some(h) => {
                        println!("\nHardware record for model key {key}:");
                        for l in hardware_lines(h).into_iter().skip(1) {
                            println!("  {l}");
                        }
                    }
                    None => println!("\nNo hardware record at this registry for model key {key}."),
                }
            }
            if let Some((id, o)) = order {
                println!(
                    "\nRegistered for order {id} ({}): {} from {}.",
                    clean(text(&o["state"])),
                    amount(&o),
                    clean(text(&o["client"]))
                );
            }
            println!("\nThe chain does not establish that any claim in the capsule is true.");
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cc01_chain_times_read_as_dates() {
        assert_eq!(when(&json!("0")), "1970-01-01 00:00:00 UTC");
        assert_eq!(
            when(&json!("1700000000000000000")),
            "2023-11-14 22:13:20 UTC"
        );
        assert_eq!(
            when(&json!("951782400000000000")),
            "2000-02-29 00:00:00 UTC"
        );
        assert_eq!(when(&json!(null)), "1970-01-01 00:00:00 UTC");
    }

    #[test]
    fn cc05_amounts_read_in_near_or_in_the_tokens_units() {
        assert_eq!(
            amount(&json!({"amount": "497090000000000000000000", "token": null})),
            "0.49709 NEAR"
        );
        assert_eq!(
            amount(&json!({"amount": "12000000000000000000000000", "token": null})),
            "12.00000 NEAR"
        );
        assert_eq!(
            amount(&json!({"amount": "5000000", "token": "usdc.testnet"})),
            "5000000 smallest units of token usdc.testnet"
        );
        assert_eq!(amount(&json!({"amount": "lots"})), "an unreadable amount");
    }

    #[test]
    fn cc02_text_from_the_chain_cannot_carry_control_characters() {
        assert_eq!(clean("ok\x1b[2J\nname"), "ok[2Jname");
        assert_eq!(clean(&"x".repeat(400)).len(), 300);
    }

    #[test]
    fn cc03_a_hardware_record_is_shown_with_who_asked_and_when() {
        let h = json!({"signer": "ab", "attested_at": "1700000000000000000", "tcb_status": "UpToDate",
            "attested_by": "alice.testnet", "compose_sha256": "cd", "mr_td": "ef"});
        let l = hardware_lines(&h).join("\n");
        assert!(
            l.contains("2023-11-14")
                && l.contains("UpToDate")
                && l.contains("alice.testnet")
                && l.contains("cd"),
            "{l}"
        );
        let mut none = h.clone();
        none["compose_sha256"] = json!(null);
        assert!(!hardware_lines(&none)
            .join("\n")
            .contains("Deployment description"));
    }

    #[test]
    fn cc04_bad_commands_and_options_are_refused_before_any_network_use() {
        let run = |a: &[&str]| super::run(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(run(&[]), 2);
        assert_eq!(run(&["hardware"]), 2);
        assert_eq!(run(&["hardware", "XYZ"]), 2);
        assert_eq!(run(&["order", "seven"]), 2);
        assert_eq!(run(&["attest"]), 2);
        assert_eq!(run(&["attest", "--model", "m"]), 2);
        assert_eq!(run(&["hardware", &"ab".repeat(32), "--network", "moon"]), 2);
        assert_eq!(
            run(&["hardware", &"ab".repeat(32), "--network", "mainnet"]),
            2,
            "no mainnet registry is known"
        );
        assert_eq!(run(&["hardware", &"ab".repeat(32), "--verbose"]), 2);
        assert_eq!(run(&["capsule", "/nonexistent/file.kkg"]), 2);
        assert_eq!(
            run(&[
                "attest",
                "--model",
                "m",
                "--key-file",
                "/nonexistent/key.json"
            ]),
            2
        );
    }
}
