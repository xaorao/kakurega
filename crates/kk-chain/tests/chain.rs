use kk_chain::collateral::{encode, quote_platform, raw_object, CollateralError, Parts};
use kk_chain::registry::{read_model_quote, reason_of};
use kk_chain::{Chain, ChainError, Network, Payer};
use serde_json::json;

/// A real NEAR AI Cloud model quote (public data), saved from the live service.
const QUOTE: &[u8] = include_bytes!("fixtures/model_quote.bin");

#[test]
fn ch01_the_platform_is_read_from_a_real_quote() {
    assert_eq!(
        quote_platform(QUOTE).unwrap(),
        ("00A06D080000".to_string(), "platform")
    );
}

#[test]
fn ch02_a_quote_without_a_readable_certificate_is_refused_in_words() {
    assert_eq!(
        quote_platform(b"no certificate here"),
        Err(CollateralError::Quote("no certificate"))
    );
    let mut cut = QUOTE[..QUOTE.len() - 2000].to_vec();
    // a quote cut inside its first certificate
    let begin = cut
        .windows(27)
        .position(|w| w == b"-----BEGIN CERTIFICATE-----")
        .unwrap();
    cut.truncate(begin + 200);
    assert_eq!(
        quote_platform(&cut),
        Err(CollateralError::Quote("unfinished certificate"))
    );
    let mut junk = b"-----BEGIN CERTIFICATE-----\n!!!!\n-----END CERTIFICATE-----".to_vec();
    assert_eq!(
        quote_platform(&junk),
        Err(CollateralError::Quote("certificate is not base64"))
    );
    junk = b"-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----".to_vec();
    assert_eq!(
        quote_platform(&junk),
        Err(CollateralError::Quote("no platform id"))
    );
    // a certificate whose platform id is not the six-byte string Intel uses, and one cut short
    use base64::Engine as _;
    let pem = |der: &[u8]| {
        format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----",
            base64::engine::general_purpose::STANDARD.encode(der)
        )
    };
    let mut der = vec![
        0x30, 0x10, 0x06, 0x0a, 0x2a, 0x86, 0x48, 0x86, 0xf8, 0x4d, 0x01, 0x0d, 0x01, 0x04,
    ];
    der.extend([0x05, 0x06, 1, 2, 3, 4, 5, 6]);
    assert_eq!(
        quote_platform(pem(&der).as_bytes()),
        Err(CollateralError::Quote("platform id in an unknown form"))
    );
    der.truncate(der.len() - 4);
    assert_eq!(
        quote_platform(pem(&der).as_bytes()),
        Err(CollateralError::Quote("short platform id"))
    );
}

#[test]
fn ch03_a_signed_object_is_cut_out_exactly_as_sent() {
    let body = r#"{"tcbInfo":{"id":"TDX","note":"a } brace and a \" quote in text","levels":[{"a":1},{"b":{"c":2}}]},"signature":"ab12"}"#;
    let cut = raw_object(body, "tcbInfo").unwrap();
    assert_eq!(
        cut,
        r#"{"id":"TDX","note":"a } brace and a \" quote in text","levels":[{"a":1},{"b":{"c":2}}]}"#
    );
    assert!(raw_object(body, "enclaveIdentity").is_err());
    assert!(raw_object(r#"{"tcbInfo":"text"}"#, "tcbInfo").is_err());
    assert!(raw_object(r#"{"tcbInfo":{"open":1"#, "tcbInfo").is_err());
}

fn parts() -> Parts {
    Parts {
        pck_crl_issuer_chain: "CHAIN-A".into(),
        root_ca_crl: vec![1, 2],
        pck_crl: vec![3],
        tcb_info_issuer_chain: "CHAIN-B".into(),
        tcb_answer: r#"{"tcbInfo":{"x":1},"signature":"0aff"}"#.into(),
        qe_identity_issuer_chain: "CHAIN-C".into(),
        qe_answer: r#"{"enclaveIdentity":{"y":2},"signature":"10"}"#.into(),
    }
}

#[test]
fn ch04_the_collateral_is_laid_out_as_the_verifier_reads_it() {
    let out = encode(&parts()).unwrap();
    let mut want = Vec::new();
    let mut put = |b: &[u8]| {
        want.extend((b.len() as u32).to_le_bytes());
        want.extend(b);
    };
    put(b"CHAIN-A");
    put(&[1, 2]);
    put(&[3]);
    put(b"CHAIN-B");
    put(br#"{"x":1}"#);
    put(&[0x0a, 0xff]);
    put(b"CHAIN-C");
    put(br#"{"y":2}"#);
    put(&[0x10]);
    want.push(0);
    assert_eq!(out, want);
}

#[test]
fn ch05_an_answer_without_its_object_or_with_a_bad_signature_is_refused() {
    let mut p = parts();
    p.tcb_answer = r#"{"signature":"0a"}"#.into();
    assert!(encode(&p).is_err());
    let mut p = parts();
    p.qe_answer = r#"{"enclaveIdentity":{"y":2},"signature":"xyz"}"#.into();
    assert!(encode(&p).is_err());
    let mut p = parts();
    p.qe_answer = r#"{"enclaveIdentity":{"y":2}}"#.into();
    assert!(encode(&p).is_err());
    let mut p = parts();
    p.tcb_answer = "not json".into();
    assert!(encode(&p).is_err());
}

#[test]
fn ch06_a_report_is_read_only_when_it_carries_a_key_and_a_plausible_quote() {
    let hex: String = QUOTE.iter().map(|b| format!("{b:02x}")).collect();
    let key = "89154a43a47684742c3d1753252bb6f49f61da9d8c84fc9b1def2df130cb2d93";
    let good = json!({"model_attestations": [{"signing_address": key, "intel_quote": hex}]});
    let q = read_model_quote(&good).unwrap();
    assert_eq!((q.signer.as_str(), q.quote.len()), (key, QUOTE.len()));
    for bad in [
        json!({}),
        json!({"model_attestations": []}),
        json!({"model_attestations": [{"signing_address": "0xabc", "intel_quote": hex}]}),
        json!({"model_attestations": [{"signing_address": key.to_uppercase(), "intel_quote": hex}]}),
        json!({"model_attestations": [{"signing_address": key, "intel_quote": "zz"}]}),
        json!({"model_attestations": [{"signing_address": key, "intel_quote": "00"}]}),
        json!({"model_attestations": [{"signing_address": key, "intel_quote": "00".repeat(17 * 1024)}]}),
    ] {
        assert!(
            matches!(read_model_quote(&bad), Err(ChainError::Answer(_))),
            "{bad}"
        );
    }
}

/// A key made for this run only: no key text lives in the repository.
fn test_key() -> String {
    near_api::signer::generate_secret_key().unwrap().to_string()
}

#[test]
fn ch07_a_key_file_names_its_account_or_is_given_one_and_never_shows_the_key() {
    let key = test_key();
    let secret = key.trim_start_matches("ed25519:").to_string();
    assert!(secret.len() > 40);
    let with = format!(r#"{{"account_id":"alice.testnet","private_key":"{key}"}}"#);
    let p = Payer::from_key_json(&with, None).unwrap();
    assert_eq!(p.account.as_str(), "alice.testnet");
    assert_eq!(format!("{p:?}"), "Payer(alice.testnet)");
    assert!(!format!("{p:?}").contains(&secret[..12]));
    let without = format!(r#"{{"private_key":"{key}"}}"#);
    assert_eq!(
        Payer::from_key_json(&without, Some("bob.testnet"))
            .unwrap()
            .account
            .as_str(),
        "bob.testnet"
    );
    for (text, account) in [
        (without.as_str(), None),
        (with.as_str(), Some("mallory.testnet")),
        (r#"{"account_id":"alice.testnet"}"#, None),
        (
            r#"{"account_id":"alice.testnet","private_key":"nope"}"#,
            None,
        ),
        (r#"{"account_id":"Not An Account","private_key":"x"}"#, None),
        ("not json", None),
    ] {
        let e = Payer::from_key_json(text, account).unwrap_err();
        let shown = format!("{e} {e:?}");
        assert!(matches!(e, ChainError::Input(_)), "{shown}");
        assert!(
            !shown.contains(&secret[..12]),
            "an error never carries the key"
        );
    }
}

#[cfg(unix)]
#[test]
fn ch08_a_key_file_other_users_can_read_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("kk-chain-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("key.json");
    std::fs::write(
        &path,
        format!(
            r#"{{"account_id":"alice.testnet","private_key":"{}"}}"#,
            test_key()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let e = Payer::from_key_file(&path, None).unwrap_err();
    assert!(e.to_string().contains("chmod 600"), "{e}");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        Payer::from_key_file(&path, None).unwrap().account.as_str(),
        "alice.testnet"
    );
    assert!(Payer::from_key_file(&dir.join("missing.json"), None).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn ch09_no_transaction_is_sent_on_mainnet_and_bad_inputs_never_reach_the_network() {
    let chain = Chain::new(Network::Mainnet, "registry.near").unwrap();
    let payer = Payer::from_key_json(
        &format!(
            r#"{{"account_id":"alice.near","private_key":"{}"}}"#,
            test_key()
        ),
        None,
    )
    .unwrap();
    assert!(matches!(
        chain.attest_model(&payer, "some/model").await,
        Err(ChainError::ReadOnlyNetwork)
    ));
    assert!(matches!(
        chain.refund_order(&payer, 0).await,
        Err(ChainError::ReadOnlyNetwork)
    ));
    assert!(matches!(
        chain.claim_order(&payer, 0).await,
        Err(ChainError::ReadOnlyNetwork)
    ));
    assert!(matches!(
        chain
            .open_order(&payer, json!({}), near_api::NearToken::from_near(1))
            .await,
        Err(ChainError::ReadOnlyNetwork)
    ));
    assert!(matches!(
        chain.hardware("XYZ").await,
        Err(ChainError::Input(_))
    ));
    assert!(matches!(
        chain.entry("ab", "cd").await,
        Err(ChainError::Input(_))
    ));
    assert!(matches!(
        chain.model_quote("", &"00".repeat(32)).await,
        Err(ChainError::Input(_))
    ));
    assert!(matches!(
        chain.model_quote("m\nx", &"00".repeat(32)).await,
        Err(ChainError::Input(_))
    ));
    assert!(Chain::new(Network::Testnet, "Not An Account").is_err());
    assert_eq!(
        (
            Network::parse("testnet"),
            Network::parse("mainnet"),
            Network::parse("x")
        ),
        (Some(Network::Testnet), Some(Network::Mainnet), None)
    );
}

#[test]
fn ch10_a_refusal_is_reported_in_the_contracts_own_words() {
    let d = r#"ExecutionFailure { ... ActionError { kind: FunctionCallError(ExecutionError("Smart contract panicked: the order is not open")) } }"#;
    assert_eq!(reason_of(d), "the order is not open");
    assert_eq!(reason_of("something else broke"), "the transaction failed");
}

/// LIVE on NEAR testnet. `KAKU_TESTNET_KEY_FILE` and `KAKU_TESTNET_ACCOUNT` name a throwaway account that pays.
#[tokio::test]
#[ignore]
async fn live_a_models_hardware_is_recorded_through_the_client_and_read_back() {
    let chain = Chain::new(Network::Testnet, kk_chain::TESTNET_REGISTRY).unwrap();
    let payer = Payer::from_key_file(
        std::path::Path::new(&std::env::var("KAKU_TESTNET_KEY_FILE").unwrap()),
        std::env::var("KAKU_TESTNET_ACCOUNT").ok().as_deref(),
    )
    .unwrap();
    let (sent, record) = chain
        .attest_model(&payer, "Qwen/Qwen3.8-27B")
        .await
        .unwrap();
    println!(
        "transaction {} ({:.1} Tgas)\n{record}",
        sent.transaction, sent.gas_tgas
    );
    assert_eq!(record["tcb_status"], "UpToDate");
    assert_eq!(record["attested_by"], payer.account.as_str());
    let again = chain
        .hardware(record["signer"].as_str().unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(again, record);
    assert!(chain.hardware(&"ab".repeat(32)).await.unwrap().is_none());
    assert!(chain.order(999_999).await.unwrap().is_none());
    assert_eq!(chain.order(0).await.unwrap().unwrap()["state"], "Paid");
}
