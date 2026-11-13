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
  kaku chain register JOB --key-file FILE [--account NAME] [--order ID] [--yes]
                                          publish a delivered review's capsule digest and model receipts on NEAR (testnet);
                                          with --order, for that order, which the contract then pays out
  kaku chain order ID                     what an order holds
  kaku chain order open --reviewer ACCOUNT --reviewer-key KEY --scope DIGEST --near AMOUNT --key-file FILE [--account NAME]
                   [--hours N] [--require-hardware] [--yes]
                                          set NEAR aside for a review (testnet): paid to the reviewer only for a capsule signed by
                                          KEY and registered for this order before the deadline, with the registry's fee on top
  kaku chain storage ACCOUNT [--deposit NEAR | --withdraw] [--key-file FILE [--account NAME]]
                                          NEAR an account set aside at the registry for its token orders' storage;
                                          --deposit adds to the paying account's, --withdraw takes it all back (testnet)
  kaku chain fees [--withdraw --key-file FILE [--account NAME] [--token]]
                                          what the registry takes on each paid order and what it owes its fee account;
                                          with --withdraw, send what it owes there (anyone may; testnet)
  kaku chain quote --pay-with SYMBOL@chain --usdc AMOUNT --refund-to ADDRESS --registry ACCOUNT --client ACCOUNT
                   --reviewer ACCOUNT --reviewer-key KEY --scope DIGEST [--hours N] [--require-hardware]
                                          a price from NEAR Intents for paying an order from another chain; nothing is sent
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

/// Seconds since 1970 as the date and time 1Click reads (`2030-01-01T00:00:00.000Z`).
fn rfc3339(secs: u64) -> String {
    let w = when(&json!((u128::from(secs) * 1_000_000_000).to_string()));
    format!("{}T{}.000Z", &w[..10], &w[11..19])
}

/// A decimal amount such as `25` or `25.5` in a token's smallest units; `None` when it is not a positive amount that fits.
fn to_units(amount: &str, decimals: u32) -> Option<u128> {
    let (whole, part) = amount.split_once('.').unwrap_or((amount, ""));
    if whole.is_empty()
        || whole.len() > 12
        || part.len() > decimals as usize
        || !whole
            .bytes()
            .chain(part.bytes())
            .all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let padded = format!("{part:0<width$}", width = decimals as usize);
    let units = whole
        .parse::<u128>()
        .ok()?
        .checked_mul(10u128.checked_pow(decimals)?)?
        + if padded.is_empty() {
            0
        } else {
            padded.parse::<u128>().ok()?
        };
    (units > 0).then_some(units)
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

/// A raw amount in the order's unit, in words.
fn units(order: &Value, key: &str) -> String {
    let mut o = order.clone();
    o["amount"] = order[key].clone();
    amount(&o)
}

/// A share in hundredths of a percent, as a percentage.
fn percent(bps: u64) -> String {
    if bps.is_multiple_of(100) {
        format!("{}%", bps / 100)
    } else {
        format!("{}.{:02}%", bps / 100, bps % 100)
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
        "--order",
        "--data-dir",
        "--pay-with",
        "--usdc",
        "--refund-to",
        "--reviewer",
        "--reviewer-key",
        "--scope",
        "--hours",
        "--client",
        "--deposit",
        "--near",
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
        } else if ![
            "--json",
            "--yes",
            "--require-hardware",
            "--withdraw",
            "--token",
        ]
        .contains(&a.as_str())
        {
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
        (Some("register"), Some(job)) => {
            let Some(file) = crate::flag(args, "--key-file") else {
                eprintln!("{USAGE}");
                return 2;
            };
            let order = match crate::flag(args, "--order").map(|o| o.parse::<u64>()) {
                None => None,
                Some(Ok(n)) => Some(n),
                Some(Err(_)) => {
                    eprintln!("an order is named by its number");
                    return 2;
                }
            };
            if !job.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') || job.is_empty() {
                eprintln!("that is not a review id");
                return 2;
            }
            let payer = match Payer::from_key_file(
                std::path::Path::new(&file),
                crate::flag(args, "--account").as_deref(),
            ) {
                Ok(p) => p,
                Err(e) => return fail(&e),
            };
            let admin = match crate::approve::connect(args) {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("{e}");
                    return 1;
                }
            };
            let ticket = match admin.post(
                &format!("/admin/jobs/{job}/chain-ticket"),
                json!({"account": payer.account.as_str(), "order": order}),
            ) {
                Ok((200, t)) => t,
                Ok((_, e)) => {
                    eprintln!(
                        "the service did not prepare the registration: {}",
                        clean(e["error"]["message"].as_str().unwrap_or("no reason given"))
                    );
                    return 1;
                }
                Err(e) => {
                    eprintln!("{e}");
                    return 1;
                }
            };
            let count = ticket["receipts"].as_array().map_or(0, Vec::len);
            println!(
                "This will be published at {place}, for everyone to read, and cannot be removed:"
            );
            println!(
                "  the capsule's digest {}",
                clean(text(&ticket["capsule_sha256"]))
            );
            println!(
                "  the key that signed it {}",
                clean(text(&ticket["capsule_signer"]))
            );
            println!("{}", kk_types::words::counted(&format!("  {count} model receipt(s): each model's name and signing key, and the digests of its request and answer")));
            println!("  the account {} and the time", payer.account);
            if let Some(id) = order {
                println!("  that it is delivered for order {id}");
            }
            if ticket["receipts_left_out"].as_u64().unwrap_or(0) > 0 {
                println!("{}", kk_types::words::counted(&format!(
                    "  ({} further receipt(s) in the capsule are not published: a registration carries at most 128)",
                    ticket["receipts_left_out"]
                )));
            }
            println!(
                "Not published: the capsule itself, or any file, finding, question or answer."
            );
            if !args.iter().any(|a| a == "--yes") {
                use std::io::{BufRead, IsTerminal, Write};
                if !std::io::stdin().is_terminal() {
                    eprintln!("nothing was published: confirm at a terminal, or pass --yes");
                    return 2;
                }
                print!("Type yes to publish: ");
                let _ = std::io::stdout().flush();
                let mut line = String::new();
                if std::io::stdin().lock().read_line(&mut line).is_err() || line.trim() != "yes" {
                    println!("Nothing was published.");
                    return 3;
                }
            }
            match rt.block_on(chain.register(&payer, &ticket)) {
                Err(e) => fail(&e),
                Ok((sent, e)) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({"transaction": sent.transaction, "gas_tgas": sent.gas_tgas, "registry": chain.registry(), "entry": e})
                        );
                    } else {
                        println!(
                            "Registered. Transaction {} ({:.1} Tgas).",
                            sent.transaction, sent.gas_tgas
                        );
                        println!(
                            "  by {} on {}; model signatures the contract verified: {}",
                            clean(text(&e["registered_by"])),
                            when(&e["registered_at"]),
                            registered_count(&e)
                        );
                    }
                    0
                }
            }
        }
        (Some("quote"), None) => {
            use kk_chain::intents::{self, Ask};
            let need = |name: &str| crate::flag(args, name);
            let (
                Some(pay_with),
                Some(usdc),
                Some(refund_to),
                Some(reviewer),
                Some(reviewer_key),
                Some(scope),
                Some(client),
            ) = (
                need("--pay-with"),
                need("--usdc"),
                need("--refund-to"),
                need("--reviewer"),
                need("--reviewer-key"),
                need("--scope"),
                need("--client"),
            )
            else {
                eprintln!("{USAGE}");
                return 2;
            };
            if crate::flag(args, "--registry").is_none() {
                eprintln!("name the registry's account on NEAR mainnet with --registry (NEAR Intents has no test network)");
                return 2;
            }
            let hex64 = |s: &str| {
                s.len() == 64 && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
            };
            let hours = match crate::flag(args, "--hours").map(|h| h.parse::<u64>()) {
                None => 72,
                Some(Ok(h)) if (1..=8760).contains(&h) => h,
                _ => {
                    eprintln!("--hours is a number from 1 to 8760");
                    return 2;
                }
            };
            if !kk_evidence::registry::is_account_id(&client) {
                eprintln!("--client is the NEAR account the order belongs to: a refund after the deadline goes there");
                return 2;
            }
            if !kk_evidence::registry::is_account_id(&reviewer)
                || !hex64(&reviewer_key)
                || !hex64(&scope)
                || refund_to.is_empty()
                || refund_to.len() > 128
                || !refund_to
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_' | b':'))
            {
                eprintln!("the reviewer is a NEAR account, the reviewer key and the scope are 64 lowercase hex digits each, and the refund address is a plain address on the paying chain");
                return 2;
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let terms = json!({
                "reviewer": reviewer,
                "reviewer_key": reviewer_key,
                "scope_sha256": scope,
                "deadline": (u128::from(now + hours * 3600) * 1_000_000_000).to_string(),
                "require_hardware": args.iter().any(|a| a == "--require-hardware"),
                "client": client,
            });
            let quoted = rt.block_on(async {
                let tokens = intents::tokens(chain.http()).await?;
                let pay = intents::find(&tokens, &pay_with)?;
                let arrive = intents::find(&tokens, "USDC@near")?;
                let units = to_units(&usdc, arrive.decimals).ok_or_else(|| {
                    ChainError::Input("--usdc is a positive amount such as 25 or 25.50".into())
                })?;
                // the payer sends the price of the reviewer's amount, which the terms name: the registry's own price on
                // mainnet when it can be read, else at Kakurega's fee. A wrong guess cannot cost the payer: the contract
                // keeps exactly the price or hands the whole transfer back.
                let read = match Chain::new(Network::Mainnet, chain.registry()) {
                    Ok(main) => main.price(units).await.ok(),
                    Err(_) => None,
                };
                let (fee, read_from_registry) = match read {
                    Some((a, f, _)) if a == units => (f, true),
                    _ => (
                        kk_chain::price_with(units, kk_chain::DEFAULT_FEE_BPS)
                            .map(|(_, f)| f)
                            .ok_or_else(|| ChainError::Input("--usdc is too large".into()))?,
                        false,
                    ),
                };
                let mut sent_terms = terms.clone();
                sent_terms["amount"] = json!(units.to_string());
                let arrive_units = units + fee;
                let ask = Ask {
                    pay,
                    arrive,
                    arrive_units,
                    registry: chain.registry(),
                    refund_to: &refund_to,
                    terms: &sent_terms,
                    deadline_rfc3339: &rfc3339(now + 600),
                };
                intents::dry_quote(chain.http(), &ask)
                    .await
                    .map(|q| (q, (fee, read_from_registry), sent_terms))
            });
            match quoted {
                Err(e) => fail(&e),
                Ok((q, fee, terms)) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({"dry": true, "pay": q.pay, "pay_symbol": q.pay_symbol, "pay_chain": q.pay_chain,
                                "arrive": q.arrive, "arrive_units": q.arrive_units, "seconds": q.seconds,
                                "registry": chain.registry(), "terms": terms, "fee_units": fee.0.to_string(),
                                "fee_read_from_registry": fee.1})
                        );
                    } else {
                        println!(
                            "DRY QUOTE. Nothing is sent and no deposit address was asked for."
                        );
                        println!(
                            "To set {} USDC aside at {} for {}, a payer would send about {} {} on {}.",
                            clean(&q.arrive),
                            chain.registry(),
                            clean(&reviewer),
                            clean(&q.pay),
                            clean(&q.pay_symbol),
                            clean(&q.pay_chain)
                        );
                        if fee.1 {
                            println!(
                                "That is {} USDC for the reviewer and the registry's fee of {} smallest units on top.",
                                clean(&usdc),
                                fee.0
                            );
                        } else {
                            println!(
                                "That is {} USDC for the reviewer and Kakurega's fee of {} smallest units on top: the registry could not be read on NEAR mainnet, so its own fee was not checked. The terms name the reviewer's amount, so the registry keeps exactly its price and hands back any surplus, or returns the whole transfer if this is too little.",
                                clean(&usdc),
                                fee.0
                            );
                        }
                        println!(
                            "The order belongs to {}: a refund after the deadline goes there. Its record is paid from that account's storage balance at the registry, so set one first (kaku chain storage {} --deposit 0.01).",
                            clean(&client),
                            clean(&client)
                        );
                        println!(
                            "It would arrive in about {} seconds, carrying the order's terms as the transfer's message; the order would wait {hours} hours for the reviewer's capsule.",
                            q.seconds
                        );
                        println!("A real payment needs the registry on NEAR mainnet and the owner's word. This build does not send one.");
                    }
                    0
                }
            }
        }
        (Some("storage"), Some(account)) => {
            if !kk_evidence::registry::is_account_id(account) {
                eprintln!("name the NEAR account whose storage balance to read");
                return 2;
            }
            let deposit = crate::flag(args, "--deposit");
            let withdraw = args.iter().any(|a| a == "--withdraw");
            if deposit.is_some() || withdraw {
                let Some(file) = crate::flag(args, "--key-file") else {
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
                if payer.account.as_str() != account.as_str() {
                    eprintln!("the key file pays for {}, not {}: a storage balance is set and taken back by its own account", payer.account, clean(account));
                    return 2;
                }
                let sent = match (deposit, withdraw) {
                    (Some(_), true) => {
                        eprintln!("--deposit or --withdraw, not both");
                        return 2;
                    }
                    (Some(near), false) => match to_units(&near, 24) {
                        Some(y) if y > 0 => rt.block_on(
                            chain.storage_deposit(&payer, kk_chain::NearToken::from_yoctonear(y)),
                        ),
                        _ => {
                            eprintln!("--deposit is an amount of NEAR such as 0.01");
                            return 2;
                        }
                    },
                    (None, _) => rt.block_on(chain.storage_withdraw(&payer)),
                };
                return match sent {
                    Err(e) => fail(&e),
                    Ok(s) => {
                        if as_json {
                            println!(
                                "{}",
                                json!({"transaction": s.transaction, "gas_tgas": s.gas_tgas})
                            );
                        } else {
                            println!("Done at {place}, in transaction {}.", s.transaction);
                        }
                        0
                    }
                };
            }
            match rt.block_on(chain.storage_balance(account)) {
                Err(e) => fail(&e),
                Ok(y) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({"account": account, "storage_balance": y.to_string()})
                        );
                    } else {
                        println!(
                            "{} holds {} at {place} for the storage of its token orders.",
                            clean(account),
                            units(&json!({"near": y.to_string()}), "near")
                        );
                    }
                    0
                }
            }
        }
        (Some("fees"), None) => {
            if args.iter().any(|a| a == "--withdraw") {
                let Some(file) = crate::flag(args, "--key-file") else {
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
                let in_token = args.iter().any(|a| a == "--token");
                return match rt.block_on(chain.withdraw_fees(&payer, in_token)) {
                    Err(e) => fail(&e),
                    Ok(sent) => {
                        if as_json {
                            println!(
                                "{}",
                                json!({"transaction": sent.transaction, "gas_tgas": sent.gas_tgas, "in_token": in_token})
                            );
                        } else {
                            println!(
                                "Sent what {} owed its fee account, in transaction {}.",
                                place, sent.transaction
                            );
                        }
                        0
                    }
                };
            }
            match rt.block_on(async {
                Ok::<_, ChainError>((chain.fee_terms().await?, chain.fees_owed().await?))
            }) {
                Err(e) => fail(&e),
                Ok((terms, owed)) => {
                    if as_json {
                        println!("{}", json!({"terms": terms, "owed": owed}));
                    } else {
                        let bps = terms["bps"].as_u64().unwrap_or(0);
                        if bps == 0 {
                            println!("{place} takes no fee: the reviewer receives everything set aside for an order.");
                        } else {
                            println!(
                                "{place} takes {} on top of the reviewer's amount for each paid order, sent only to {}.",
                                percent(bps),
                                clean(text(&terms["account"]))
                            );
                            println!("  The fee is earned when the reviewer registers the capsule, and returned to the client with the amount on a refund.");
                            println!(
                                "  Owed now: {} and {} smallest units of the accepted token.",
                                units(&json!({"near": owed["near"]}), "near"),
                                clean(text(&owed["token"]))
                            );
                        }
                    }
                    0
                }
            }
        }
        (Some("order"), Some(w)) if w.as_str() == "open" => {
            let need = |name: &str| crate::flag(args, name);
            let (Some(reviewer), Some(key), Some(scope), Some(near), Some(file)) = (
                need("--reviewer"),
                need("--reviewer-key"),
                need("--scope"),
                need("--near"),
                need("--key-file"),
            ) else {
                eprintln!("{USAGE}");
                return 2;
            };
            if key.starts_with("ed25519:") {
                eprintln!("that is the capsule key's ed25519: fingerprint, not the key: give the \"signing key\" line kaku verify prints for one of the reviewer's capsules");
                return 2;
            }
            let hex64 = |s: &str| {
                s.len() == 64 && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
            };
            if !kk_evidence::registry::is_account_id(&reviewer) || !hex64(&key) || !hex64(&scope) {
                eprintln!("the reviewer is a NEAR account; the reviewer key (the \"signing key\" line of kaku verify on one of the reviewer's capsules, not its ed25519: fingerprint) and the scope digest are 64 lowercase hex digits each");
                return 2;
            }
            let Some(amount) = to_units(&near, 24).filter(|y| *y > 0) else {
                eprintln!("--near is the amount the reviewer receives, such as 0.25");
                return 2;
            };
            let hours = match crate::flag(args, "--hours").map(|h| h.parse::<u64>()) {
                None => 72,
                Some(Ok(h)) if (1..=8760).contains(&h) => h,
                _ => {
                    eprintln!("--hours is a number from 1 to 8760");
                    return 2;
                }
            };
            let payer = match Payer::from_key_file(
                std::path::Path::new(&file),
                crate::flag(args, "--account").as_deref(),
            ) {
                Ok(p) => p,
                Err(e) => return fail(&e),
            };
            let (reviews, fee, total) = match rt.block_on(chain.price(amount)) {
                Ok(p) => p,
                Err(e) => return fail(&e),
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let require_hardware = args.iter().any(|a| a == "--require-hardware");
            let terms = json!({
                "reviewer": reviewer,
                "reviewer_key": key,
                "scope_sha256": scope,
                "deadline": (u128::from(now + hours * 3600) * 1_000_000_000).to_string(),
                "require_hardware": require_hardware,
            });
            let near_words = |y: u128| units(&json!({"y": y.to_string()}), "y");
            if !as_json {
                println!("An order at {place}, paid by {}:", payer.account);
                println!("  {} goes to {} for a capsule signed by key {}…, registered for this order within {hours} hours", near_words(reviews), clean(&reviewer), &key[..16]);
                println!("  Kakurega's fee on top: {}; you attach {} and room for the order's record, and what is not used comes back", near_words(fee), near_words(total));
                if require_hardware {
                    println!("  paid only if every model answer in the capsule is signed by a key whose hardware quote this registry saw verified after the order opened");
                }
                println!("  if nothing is registered in time, anyone can send the amount and the fee back to {}", payer.account);
            }
            if !args.iter().any(|a| a == "--yes") {
                use std::io::{BufRead, IsTerminal, Write};
                if !std::io::stdin().is_terminal() {
                    eprintln!("no order was opened: confirm at a terminal, or pass --yes");
                    return 2;
                }
                print!("Type yes to open the order: ");
                let _ = std::io::stdout().flush();
                let mut line = String::new();
                if std::io::stdin().lock().read_line(&mut line).is_err() || line.trim() != "yes" {
                    println!("No order was opened.");
                    return 3;
                }
            }
            match rt.block_on(chain.open_order(
                &payer,
                terms,
                kk_chain::NearToken::from_yoctonear(amount),
            )) {
                Err(e) => fail(&e),
                Ok((sent, id)) => {
                    if as_json {
                        println!(
                            "{}",
                            json!({"order": id, "transaction": sent.transaction, "gas_tgas": sent.gas_tgas, "registry": chain.registry()})
                        );
                    } else {
                        println!("Order {id} opened. Transaction {} ({:.1} Tgas). Register the capsule for it with: kaku chain register JOB --order {id} --key-file ...", sent.transaction, sent.gas_tgas);
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
                        println!("  Amount: {} to the reviewer", amount(&o));
                        if o["fee"].as_str().is_some_and(|f| f != "0") {
                            println!(
                                "  Fee on top: {} (earned when the capsule is registered, returned with the amount on a refund)",
                                units(&o, "fee")
                            );
                        }
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
            println!(
                "Model signatures the contract verified: {}",
                registered_count(&e)
            );
            match registered_against(&e, &report.receipts) {
                Registered::Same { missing: 0 } => {}
                Registered::Same { missing } => println!(
                    "  {missing} of this capsule's model signatures were not registered with it."
                ),
                Registered::Different => println!(
                    "  The model signatures registered with it are not this capsule's own: their digest differs."
                ),
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

/// How many model signatures a registry entry says the contract verified: the count of a registry that keeps their digest, or
/// the list of an older one that kept each.
fn registered_count(e: &Value) -> u64 {
    e["receipt_count"]
        .as_u64()
        .or_else(|| e["receipts"].as_array().map(|a| a.len() as u64))
        .unwrap_or(0)
}

#[derive(Debug, PartialEq, Eq)]
enum Registered {
    /// The registered signatures are this capsule's own, the first ones in its order; `missing` were left out.
    Same { missing: usize },
    /// They are not: their digest is not the digest of this capsule's.
    Different,
}

/// Whether the signatures registered with an entry are this capsule's own. A registry that keeps their digest is compared with
/// the digest of the capsule's first `receipt_count` receipts (the ones a registration carries); an older one is compared
/// receipt by receipt.
fn registered_against(e: &Value, receipts: &[kk_evidence::capsule::ReceiptReport]) -> Registered {
    if let Some(digest) = e["receipts_sha256"].as_str() {
        let n = e["receipt_count"].as_u64().unwrap_or(0) as usize;
        if n > receipts.len() || kk_evidence::registry::receipts_digest(&receipts[..n]) != digest {
            return Registered::Different;
        }
        return Registered::Same {
            missing: receipts.len() - n,
        };
    }
    let on_chain = e["receipts"].as_array().cloned().unwrap_or_default();
    Registered::Same {
        missing: receipts
            .iter()
            .filter(|r| {
                !on_chain.iter().any(|c| {
                    c["signer"] == json!(r.signer)
                        && c["request_sha256"] == json!(r.request_sha256)
                        && c["response_sha256"] == json!(r.response_sha256)
                })
            })
            .count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cc08_the_registered_signatures_are_matched_to_the_capsules_by_digest_and_an_older_registry_by_each(
    ) {
        let r = |n: u8| kk_evidence::capsule::ReceiptReport {
            completion_id: format!("c{n}"),
            signer: format!("{n:02x}").repeat(32),
            model: "Qwen/Qwen3.8-27B".into(),
            request_sha256: format!("{:02x}", n + 10).repeat(32),
            response_sha256: format!("{:02x}", n + 20).repeat(32),
            signature: format!("{:02x}", n + 30).repeat(64),
        };
        let mine: Vec<_> = (1..=20).map(r).collect();
        let digest =
            |rs: &[kk_evidence::capsule::ReceiptReport]| kk_evidence::registry::receipts_digest(rs);
        // all twenty registered
        let all = json!({"receipts_sha256": digest(&mine), "receipt_count": 20});
        assert_eq!(registered_count(&all), 20);
        assert_eq!(
            registered_against(&all, &mine),
            Registered::Same { missing: 0 }
        );
        // the first sixteen registered, four left out
        let first = json!({"receipts_sha256": digest(&mine[..16]), "receipt_count": 16});
        assert_eq!(
            registered_against(&first, &mine),
            Registered::Same { missing: 4 }
        );
        // another capsule's signatures, more than this capsule has, or one changed: not this capsule's
        let mut changed = mine.clone();
        changed[19].signature = "ff".repeat(64);
        assert_eq!(registered_against(&all, &changed), Registered::Different);
        let more = json!({"receipts_sha256": digest(&mine), "receipt_count": 21});
        assert_eq!(registered_against(&more, &mine), Registered::Different);
        // an older registry kept each receipt
        let older = json!({"receipts": [
            {"signer": mine[0].signer, "request_sha256": mine[0].request_sha256, "response_sha256": mine[0].response_sha256}
        ]});
        assert_eq!(registered_count(&older), 1);
        assert_eq!(
            registered_against(&older, &mine[..3]),
            Registered::Same { missing: 2 }
        );
    }

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
        let order = json!({"amount": "250000000000000000000000", "fee": "5000000000000000000000", "token": null});
        assert_eq!(units(&order, "fee"), "0.00500 NEAR");
        assert_eq!(
            units(
                &json!({"amount": "1", "fee": "100000", "token": "usdc.testnet"}),
                "fee"
            ),
            "100000 smallest units of token usdc.testnet"
        );
        assert_eq!(
            (percent(200), percent(250), percent(1000), percent(5)),
            ("2%".into(), "2.50%".into(), "10%".into(), "0.05%".into())
        );
    }

    #[test]
    fn cc06_amounts_and_times_are_written_the_way_the_quote_service_reads_them() {
        assert_eq!(to_units("25", 6), Some(25_000_000));
        assert_eq!(to_units("25.5", 6), Some(25_500_000));
        assert_eq!(to_units("0.000001", 6), Some(1));
        for bad in [
            "",
            "0",
            "0.0",
            "-1",
            "1e3",
            "25.1234567",
            ".5",
            "1,000",
            "9999999999999",
            "25 ",
        ] {
            assert_eq!(to_units(bad, 6), None, "{bad:?}");
        }
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(rfc3339(1_700_000_000), "2023-11-14T22:13:20.000Z");
    }

    #[test]
    fn cc07_a_quote_with_missing_or_malformed_terms_never_reaches_the_network() {
        let run = |a: &[&str]| super::run(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let key = "ab".repeat(32);
        let full = [
            "quote",
            "--pay-with",
            "USDC@sol",
            "--usdc",
            "25",
            "--refund-to",
            "ADDR1",
            "--reviewer",
            "rita.near",
            "--reviewer-key",
            &key,
            "--scope",
            &key,
        ];
        assert_eq!(run(&full[..11]), 2, "no scope");
        assert_eq!(run(&full), 2, "no mainnet registry named");
        let with = |extra: &[&'static str]| {
            let mut v: Vec<&str> = full.to_vec();
            v.extend(["--network", "mainnet", "--registry", "registry.near"]);
            v.extend(extra);
            v
        };
        assert_eq!(run(&with(&["--hours", "0"])), 2);
        assert_eq!(run(&with(&["--hours", "soon"])), 2);
        let mut bad_key = with(&[]);
        bad_key[10] = "XYZ";
        assert_eq!(run(&bad_key), 2);
        let mut bad_refund = with(&[]);
        bad_refund[6] = "addr with spaces";
        assert_eq!(run(&bad_refund), 2);
        let mut bad_reviewer = with(&[]);
        bad_reviewer[8] = "Rita";
        assert_eq!(run(&bad_reviewer), 2);
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
        assert_eq!(
            run(&["fees", "--withdraw"]),
            2,
            "a withdrawal names the key file that pays for it"
        );
        assert_eq!(run(&["fees", "extra"]), 2);
        assert_eq!(
            run(&["order", "open"]),
            2,
            "an order names its reviewer, key, scope, amount and payer"
        );
        let fp = format!("ed25519:{}", "cd".repeat(32));
        assert_eq!(
            run(&[
                "order",
                "open",
                "--reviewer",
                "rita.testnet",
                "--reviewer-key",
                &fp,
                "--scope",
                &"ab".repeat(32),
                "--near",
                "0.1",
                "--key-file",
                "/nonexistent"
            ]),
            2,
            "a fingerprint is not taken for the key"
        );
        let key = "ab".repeat(32);
        assert_eq!(
            run(&[
                "order",
                "open",
                "--reviewer",
                "Not An Account",
                "--reviewer-key",
                &key,
                "--scope",
                &key,
                "--near",
                "0.1",
                "--key-file",
                "/nonexistent"
            ]),
            2
        );
        assert_eq!(
            run(&[
                "order",
                "open",
                "--reviewer",
                "rita.testnet",
                "--reviewer-key",
                &key,
                "--scope",
                &key,
                "--near",
                "nothing",
                "--key-file",
                "/nonexistent"
            ]),
            2
        );
        assert_eq!(
            run(&[
                "order",
                "open",
                "--reviewer",
                "rita.testnet",
                "--reviewer-key",
                &key,
                "--scope",
                &key,
                "--near",
                "0.1",
                "--key-file",
                "/nonexistent",
                "--hours",
                "0"
            ]),
            2
        );
        assert_eq!(run(&["storage", "Not An Account"]), 2);
        assert_eq!(
            run(&["storage", "carl.testnet", "--deposit", "0.01"]),
            2,
            "a deposit names the key file that pays"
        );
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
        assert_eq!(run(&["register"]), 2);
        assert_eq!(run(&["register", "job-1"]), 2, "no key file");
        assert_eq!(
            run(&[
                "register",
                "job-1",
                "--key-file",
                "/nonexistent/key.json",
                "--order",
                "x"
            ]),
            2
        );
        assert_eq!(
            run(&["register", "../etc", "--key-file", "/nonexistent/key.json"]),
            2,
            "not a review id"
        );
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
