//! The contract on a real NEAR runtime (a local sandbox node): the Ed25519 host function, storage billing, refunds and
//! the "first registration wins" rule, with real signatures made in the test.

use ed25519_dalek::{Signer as _, SigningKey};
use near_api::{AccountId, NearGas, NearToken};
use near_sdk::serde_json::{Value, json};

const NEAR: u128 = 10u128.pow(24);
const PREFIX: &str = "kakurega-register-v2";

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}
/// The registry's digest of the receipts as presented, computed here independently of the contract.
fn receipts_digest(receipts: &[Value]) -> String {
    use sha2::Digest;
    let mut text = String::new();
    for r in receipts {
        text.push_str(&format!(
            "{}:{}:{}:{}:{}\n",
            r["signer"].as_str().unwrap(),
            r["model"].as_str().unwrap(),
            r["request_sha256"].as_str().unwrap(),
            r["response_sha256"].as_str().unwrap(),
            r["signature"].as_str().unwrap()
        ));
    }
    hex(&sha2::Sha256::digest(text.as_bytes()))
}

fn bind(k: &SigningKey, digest: &str, receipts: &[Value], who: &str) -> String {
    hex(&k
        .sign(format!("{PREFIX}:{digest}:{}:{who}", receipts_digest(receipts)).as_bytes())
        .to_bytes())
}
fn receipt(k: &SigningKey, model: &str, req: &str, resp: &str) -> Value {
    json!({
        "signer": hex(k.verifying_key().as_bytes()),
        "model": model,
        "request_sha256": req,
        "response_sha256": resp,
        "signature": hex(&k.sign(format!("{model}:{req}:{resp}").as_bytes()).to_bytes()),
    })
}

struct Env {
    sandbox: near_sandbox::Sandbox,
    net: near_api::NetworkConfig,
    key: std::sync::Arc<near_api::Signer>,
}

impl Env {
    async fn account(&self, name: &str, near: u128) -> AccountId {
        let id: AccountId = name.parse().unwrap();
        self.sandbox
            .create_account(name.parse().unwrap())
            .initial_balance(NearToken::from_near(near))
            .send()
            .await
            .unwrap();
        id
    }
    async fn call(
        &self,
        c: &AccountId,
        who: &AccountId,
        method: &str,
        args: Value,
        deposit: u128,
    ) -> Result<(), String> {
        let out = near_api::Contract(c.clone())
            .call_function(method, args)
            .transaction()
            .gas(NearGas::from_tgas(100))
            .deposit(NearToken::from_yoctonear(deposit))
            .with_signer(who.clone(), self.key.clone())
            .send_to(&self.net)
            .await
            .unwrap()
            .into_full()
            .unwrap()
            .into_result();
        out.map(|_| ()).map_err(|e| format!("{e:?}"))
    }
    async fn view(&self, c: &AccountId, method: &str, args: Value) -> Value {
        near_api::Contract(c.clone())
            .call_function(method, args)
            .read_only()
            .fetch_from(&self.net)
            .await
            .unwrap()
            .data
    }
    async fn balance(&self, who: &AccountId) -> u128 {
        near_api::Tokens::account(who.clone())
            .near_balance()
            .fetch_from(&self.net)
            .await
            .unwrap()
            .total
            .as_yoctonear()
    }
}

#[tokio::test]
async fn the_registry_works_on_a_real_runtime() -> testresult::TestResult {
    let wasm = std::fs::read(cargo_near_build::build_with_cli(Default::default())?)?;
    let sandbox = near_sandbox::Sandbox::start_sandbox().await?;
    let net = near_api::NetworkConfig::from_rpc_url("sandbox", sandbox.rpc_addr.parse()?);
    let k = near_api::Signer::from_secret_key(
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY
            .parse()
            .unwrap(),
    )?;
    let e = Env {
        sandbox,
        net,
        key: k,
    };

    let contract = e.account("registry.sandbox", 20).await;
    near_api::Contract::deploy(contract.clone())
        .use_code(wasm)
        .with_init_call(
            "new",
            json!({"verifier": "verifier.sandbox", "token": null, "min_token_order": null}),
        )?
        .with_signer(e.key.clone())
        .send_to(&e.net)
        .await?
        .assert_success();
    let alice = e.account("alice.sandbox", 10).await;
    let mallory = e.account("mallory.sandbox", 10).await;

    let digest = "ab".repeat(32);
    let (cap, model) = (key(1), key(2));
    let rc = receipt(
        &model,
        "Qwen/Qwen3.8-27B",
        &"11".repeat(32),
        &"22".repeat(32),
    );
    let args = |who: &str| {
        json!({
            "capsule_sha256": digest,
            "capsule_signer": hex(cap.verifying_key().as_bytes()),
            "binding_signature": bind(&cap, &digest, std::slice::from_ref(&rc), who),
            "receipts": [rc.clone()],
        })
    };

    // Another account cannot use Alice's binding.
    let stolen = e
        .call(
            &contract,
            &mallory,
            "register",
            args("alice.sandbox"),
            NEAR / 10,
        )
        .await;
    assert!(
        stolen
            .unwrap_err()
            .contains("binding signature does not verify")
    );

    // A small-order key can sign anything on this runtime, so it is never accepted as the capsule signer.
    let zero = json!({
        "capsule_sha256": digest,
        "capsule_signer": "00".repeat(32),
        "binding_signature": "00".repeat(64),
        "receipts": [],
    });
    let weak = e.call(&contract, &alice, "register", zero, NEAR / 10).await;
    assert!(weak.unwrap_err().contains("not a usable key"));

    // A forged receipt is refused on the real runtime.
    let mut forged = args("alice.sandbox");
    forged["receipts"][0]["response_sha256"] = json!("33".repeat(32));
    let forged_list = forged["receipts"].as_array().unwrap().clone();
    forged["binding_signature"] = json!(bind(&cap, &digest, &forged_list, "alice.sandbox"));
    let bad = e
        .call(&contract, &alice, "register", forged, NEAR / 10)
        .await;
    assert!(
        bad.unwrap_err()
            .contains("receipt is malformed or its signature does not verify")
    );
    assert_eq!(e.view(&contract, "total", json!({})).await, json!("0"));

    // The honest registration succeeds, is billed for its storage only, and the rest comes back.
    let before = e.balance(&alice).await;
    e.call(&contract, &alice, "register", args("alice.sandbox"), NEAR)
        .await?;
    let spent = before - e.balance(&alice).await;
    assert!(
        spent < NEAR / 5,
        "storage should cost far less than the 1 NEAR attached, spent {spent}"
    );
    let got = e
        .view(
            &contract,
            "get",
            json!({ "capsule_sha256": digest, "capsule_signer": hex(cap.verifying_key().as_bytes()) }),
        )
        .await;
    assert_eq!(got["registered_by"], "alice.sandbox");
    assert_eq!(got["receipts"].as_array().unwrap().len(), 1);
    assert_eq!(e.view(&contract, "total", json!({})).await, json!("1"));

    // First registration wins.
    let again = e
        .call(
            &contract,
            &mallory,
            "register",
            args("mallory.sandbox"),
            NEAR / 10,
        )
        .await;
    assert!(
        again
            .unwrap_err()
            .contains("already registered under this signing key")
    );

    // A different key registering the same digest does not take the capsule's place: it is its own entry.
    let squatter_key = key(7);
    let squatter = e.account("squatter.sandbox", 10).await;
    let sq_args = json!({
        "capsule_sha256": digest,
        "capsule_signer": hex(squatter_key.verifying_key().as_bytes()),
        "binding_signature": bind(&squatter_key, &digest, &[], "squatter.sandbox"),
        "receipts": [],
    });
    e.call(&contract, &squatter, "register", sq_args, NEAR / 10)
        .await?;
    assert_eq!(e.view(&contract, "total", json!({})).await, json!("2"));
    let original = e
        .view(&contract, "get", json!({ "capsule_sha256": digest, "capsule_signer": hex(cap.verifying_key().as_bytes()) }))
        .await;
    assert_eq!(
        original["registered_by"], "alice.sandbox",
        "the owner's entry is untouched"
    );

    // Swapped receipts do not match what the capsule key signed.
    let other_digest = "cd".repeat(32);
    let signed_for = vec![rc.clone()];
    let swapped = vec![receipt(
        &model,
        "Qwen/Qwen3.8-27B",
        &"11".repeat(32),
        &"33".repeat(32),
    )];
    let swap_args = json!({
        "capsule_sha256": other_digest,
        "capsule_signer": hex(cap.verifying_key().as_bytes()),
        "binding_signature": bind(&cap, &other_digest, &signed_for, "alice.sandbox"),
        "receipts": swapped,
    });
    let swap = e
        .call(&contract, &alice, "register", swap_args, NEAR / 10)
        .await;
    assert!(
        swap.unwrap_err()
            .contains("binding signature does not verify")
    );

    // The free check on the real runtime.
    let ok = e.view(&contract, "verify_receipts", json!({ "receipts": [rc, { "signer": "00".repeat(32), "model": "m", "request_sha256": "11".repeat(32), "response_sha256": "22".repeat(32), "signature": "00".repeat(64) }] })).await;
    assert_eq!(ok, json!([true, false]));
    Ok(())
}

// ---- the hardware check on a real runtime, with NEAR's real verifier code and a live NEAR AI Cloud quote ----

const VERIFIER_ON_TESTNET: &str = "tee-verifier-2026-07-22.testnet";
const TESTNET_RPC: &str = "https://rpc.testnet.fastnear.com";
const NEAR_AI: &str = "https://cloud-api.near.ai/v1";
const INTEL_PCS: &str = "https://api.trustedservices.intel.com";
const INTEL_ROOT_CRL: &str = "https://certificates.trustedservices.intel.com/IntelSGXRootCA.der";

fn b64(b: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(b)
}
fn unb64(s: &str) -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(s).unwrap()
}
fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
        .collect()
}
fn borsh_bytes(out: &mut Vec<u8>, b: &[u8]) {
    out.extend((b.len() as u32).to_le_bytes());
    out.extend(b);
}
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
/// The platform id (FMSPC) and issuing authority, read from the certificate inside the quote.
fn quote_platform(quote: &[u8]) -> (String, &'static str) {
    let begin = find(quote, b"-----BEGIN CERTIFICATE-----").expect("a certificate in the quote");
    let end = find(&quote[begin..], b"-----END CERTIFICATE-----").unwrap() + begin;
    let body: String = String::from_utf8_lossy(&quote[begin + 27..end])
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let der = unb64(&body);
    let oid = [
        0x06, 0x0a, 0x2a, 0x86, 0x48, 0x86, 0xf8, 0x4d, 0x01, 0x0d, 0x01, 0x04,
    ];
    let at = find(&der, &oid).expect("the FMSPC field") + oid.len();
    assert_eq!((der[at], der[at + 1]), (0x04, 0x06));
    let fmspc: String = der[at + 2..at + 8]
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect();
    let ca = if find(&der, b"Processor CA").is_some() {
        "processor"
    } else {
        "platform"
    };
    (fmspc, ca)
}
/// The exact text of one object inside Intel's signed answer, as sent.
fn raw_object(body: &str, key: &str) -> String {
    let start = body.find(&format!("\"{key}\":")).unwrap() + key.len() + 3;
    let mut depth = 0usize;
    for (i, c) in body[start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return body[start..start + i + 1].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced object");
}
/// Intel's collateral for this quote, as the verifier's Borsh `Collateral`.
async fn collateral_for(http: &reqwest::Client, quote: &[u8]) -> Vec<u8> {
    let header = |r: &reqwest::Response, h: &str| {
        percent_encoding::percent_decode_str(r.headers()[h].to_str().unwrap())
            .decode_utf8()
            .unwrap()
            .to_string()
    };
    let (fmspc, ca) = quote_platform(quote);
    let r = http
        .get(format!(
            "{INTEL_PCS}/tdx/certification/v4/tcb?fmspc={fmspc}"
        ))
        .send()
        .await
        .unwrap();
    let tcb_chain = header(&r, "TCB-Info-Issuer-Chain");
    let tcb = r.text().await.unwrap();
    let r = http
        .get(format!("{INTEL_PCS}/tdx/certification/v4/qe/identity"))
        .send()
        .await
        .unwrap();
    let qe_chain = header(&r, "SGX-Enclave-Identity-Issuer-Chain");
    let qe = r.text().await.unwrap();
    let r = http
        .get(format!(
            "{INTEL_PCS}/sgx/certification/v4/pckcrl?ca={ca}&encoding=der"
        ))
        .send()
        .await
        .unwrap();
    let crl_chain = header(&r, "SGX-PCK-CRL-Issuer-Chain");
    let pck_crl = r.bytes().await.unwrap();
    let root_crl = http
        .get(INTEL_ROOT_CRL)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    let sig = |body: &str| {
        unhex(
            near_sdk::serde_json::from_str::<Value>(body).unwrap()["signature"]
                .as_str()
                .unwrap(),
        )
    };
    let mut out = Vec::new();
    borsh_bytes(&mut out, crl_chain.as_bytes());
    borsh_bytes(&mut out, &root_crl);
    borsh_bytes(&mut out, &pck_crl);
    borsh_bytes(&mut out, tcb_chain.as_bytes());
    borsh_bytes(&mut out, raw_object(&tcb, "tcbInfo").as_bytes());
    borsh_bytes(&mut out, &sig(&tcb));
    borsh_bytes(&mut out, qe_chain.as_bytes());
    borsh_bytes(&mut out, raw_object(&qe, "enclaveIdentity").as_bytes());
    borsh_bytes(&mut out, &sig(&qe));
    out.push(0);
    out
}

/// Needs the network: NEAR testnet (the verifier's code), NEAR AI Cloud (a fresh quote, no key) and Intel (collateral).
#[tokio::test]
#[ignore]
async fn live_a_real_model_quote_is_checked_by_nears_verifier_and_its_key_recorded()
-> testresult::TestResult {
    let http = reqwest::Client::new();
    let code: Value = http
        .post(TESTNET_RPC)
        .json(
            &json!({"jsonrpc": "2.0", "id": 1, "method": "query", "params": {
            "request_type": "view_code", "finality": "final", "account_id": VERIFIER_ON_TESTNET}}),
        )
        .send()
        .await?
        .json()
        .await?;
    let verifier_wasm = unb64(
        code["result"]["code_base64"]
            .as_str()
            .expect("the verifier's code"),
    );
    let wasm = std::fs::read(cargo_near_build::build_with_cli(Default::default())?)?;
    let sandbox = near_sandbox::Sandbox::start_sandbox().await?;
    let net = near_api::NetworkConfig::from_rpc_url("sandbox", sandbox.rpc_addr.parse()?);
    let k = near_api::Signer::from_secret_key(
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY
            .parse()
            .unwrap(),
    )?;
    let e = Env {
        sandbox,
        net,
        key: k,
    };
    let verifier = e.account("verifier.sandbox", 30).await;
    near_api::Contract::deploy(verifier.clone())
        .use_code(verifier_wasm)
        .without_init_call()
        .with_signer(e.key.clone())
        .send_to(&e.net)
        .await?
        .assert_success();
    let contract = e.account("registry.sandbox", 20).await;
    near_api::Contract::deploy(contract.clone())
        .use_code(wasm)
        .with_init_call(
            "new",
            json!({"verifier": "verifier.sandbox", "token": null, "min_token_order": null}),
        )?
        .with_signer(e.key.clone())
        .send_to(&e.net)
        .await?
        .assert_success();
    let alice = e.account("alice.sandbox", 10).await;
    let mallory = e.account("mallory.sandbox", 10).await;

    // Alice asks the model service for a quote that answers her challenge.
    let challenge = e
        .view(
            &contract,
            "challenge_for",
            json!({"account_id": "alice.sandbox"}),
        )
        .await;
    let challenge = challenge.as_str().unwrap().to_string();
    let att: Value = http
        .get(format!("{NEAR_AI}/attestation/report"))
        .query(&[
            ("model", "Qwen/Qwen3.8-27B"),
            ("provider", "near"),
            ("nonce", challenge.as_str()),
            ("include_tls_fingerprint", "false"),
            ("signing_algo", "ed25519"),
        ])
        .send()
        .await?
        .json()
        .await?;
    let model = &att["model_attestations"][0];
    let signer = model["signing_address"]
        .as_str()
        .expect("a signing key")
        .to_string();
    let quote = unhex(model["intel_quote"].as_str().expect("a quote"));
    let collateral = collateral_for(&http, &quote).await;
    println!(
        "quote {} bytes, collateral {} bytes, signer {signer}",
        quote.len(),
        collateral.len()
    );
    let attest = |q: Vec<u8>| json!({"quote": b64(&q), "collateral": b64(&collateral)});
    let send = |who: AccountId, args: Value| {
        let (c, net, key) = (contract.clone(), e.net.clone(), e.key.clone());
        async move {
            near_api::Contract(c)
                .call_function("attest_signer", args)
                .transaction()
                .gas(NearGas::from_tgas(300))
                .deposit(NearToken::from_yoctonear(NEAR / 10))
                .with_signer(who, key)
                .send_to(&net)
                .await
                .unwrap()
                .into_full()
                .unwrap()
        }
    };
    let hardware = |s: String| {
        let (c, e) = (contract.clone(), &e);
        async move { e.view(&c, "hardware", json!({"signer": s})).await }
    };

    // A quote with one changed byte in its signed body: the real verifier rejects it, nothing is recorded, the money returns.
    let mut bad = quote.clone();
    bad[200] ^= 1;
    let before = e.balance(&alice).await;
    let _ = send(alice.clone(), attest(bad)).await;
    assert!(
        hardware(signer.clone()).await.is_null(),
        "a tampered quote records nothing"
    );
    let spent = before - e.balance(&alice).await;
    assert!(
        spent < NEAR / 20,
        "the deposit came back after a refusal (spent {spent})"
    );

    // Mallory relays Alice's quote: it does not answer Mallory's challenge.
    let _ = send(mallory.clone(), attest(quote.clone())).await;
    assert!(
        hardware(signer.clone()).await.is_null(),
        "another account's quote records nothing"
    );

    // Alice's own quote is checked against Intel's roots on chain and her model's key is recorded.
    let before = e.balance(&alice).await;
    let done = send(alice.clone(), attest(quote.clone())).await;
    println!(
        "attest_signer outcome: {:?}",
        done.clone().into_result().map(|_| ())
    );
    let h = hardware(signer.clone()).await;
    println!("recorded: {h}");
    assert_eq!(h["signer"], signer);
    assert_eq!(h["tcb_status"], "UpToDate");
    assert_eq!(h["attested_by"], "alice.sandbox");
    assert!(h["compose_sha256"].as_str().is_some_and(|c| c.len() == 64));
    let spent = before - e.balance(&alice).await;
    println!("cost to Alice: {} yoctoNEAR", spent);
    assert!(
        spent < NEAR / 10,
        "the surplus of the deposit came back (spent {spent})"
    );
    Ok(())
}

/// On NEAR testnet, against the deployed registry: `KAKU_TESTNET_REGISTRY` names the contract and `KAKU_TESTNET_KEY_FILE` a
/// legacy key file of the account that pays (a throwaway testnet account). Spends testnet gas only.
#[tokio::test]
#[ignore]
async fn testnet_a_real_model_quote_is_recorded_by_the_deployed_registry() -> testresult::TestResult
{
    let contract: AccountId = std::env::var("KAKU_TESTNET_REGISTRY")?.parse()?;
    let key_file: Value = near_sdk::serde_json::from_str(&std::fs::read_to_string(
        std::env::var("KAKU_TESTNET_KEY_FILE")?,
    )?)?;
    let who: AccountId = match key_file["account_id"].as_str() {
        Some(a) => a.parse()?,
        None => std::env::var("KAKU_TESTNET_ACCOUNT")?.parse()?,
    };
    let signer = near_api::Signer::from_secret_key(
        key_file["private_key"]
            .as_str()
            .expect("private_key")
            .parse()?,
    )?;
    let net = near_api::NetworkConfig::testnet();
    let http = reqwest::Client::new();
    let view = |method: &'static str, args: Value| {
        let (c, net) = (contract.clone(), net.clone());
        async move {
            near_api::Contract(c)
                .call_function(method, args)
                .read_only::<Value>()
                .fetch_from(&net)
                .await
                .unwrap()
                .data
        }
    };
    let challenge = view("challenge_for", json!({"account_id": who})).await;
    let att: Value = http
        .get(format!("{NEAR_AI}/attestation/report"))
        .query(&[
            ("model", "Qwen/Qwen3.8-27B"),
            ("provider", "near"),
            ("nonce", challenge.as_str().unwrap()),
            ("include_tls_fingerprint", "false"),
            ("signing_algo", "ed25519"),
        ])
        .send()
        .await?
        .json()
        .await?;
    let model = &att["model_attestations"][0];
    let signer_key = model["signing_address"]
        .as_str()
        .expect("a signing key")
        .to_string();
    let quote = unhex(model["intel_quote"].as_str().expect("a quote"));
    let collateral = collateral_for(&http, &quote).await;
    let done = near_api::Contract(contract.clone())
        .call_function(
            "attest_signer",
            json!({"quote": b64(&quote), "collateral": b64(&collateral)}),
        )
        .transaction()
        .gas(NearGas::from_tgas(300))
        .deposit(NearToken::from_yoctonear(NEAR / 10))
        .with_signer(who.clone(), signer)
        .send_to(&net)
        .await?
        .into_full()
        .expect("a final result");
    println!("transaction: {}", done.outcome().transaction_hash);
    println!("total gas burnt: {}", done.total_gas_burnt);
    for o in done.receipt_outcomes() {
        println!("  receipt on {}: {}", o.executor_id, o.gas_burnt);
    }
    done.into_result().map_err(|e| format!("{e:?}"))?;
    let h = view("hardware", json!({"signer": signer_key})).await;
    println!("recorded: {h}");
    assert_eq!(h["signer"], signer_key);
    assert_eq!(h["attested_by"], who.as_str());
    assert_eq!(h["tcb_status"], "UpToDate");
    Ok(())
}

// ---- orders on a real runtime ----

fn now_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}
fn bind_order(k: &SigningKey, digest: &str, receipts: &[Value], who: &str, order: u64) -> String {
    hex(&k
        .sign(
            format!(
                "{PREFIX}:{digest}:{}:{who}:order:{order}",
                receipts_digest(receipts)
            )
            .as_bytes(),
        )
        .to_bytes())
}
fn order_terms(reviewer: &str, cap: &SigningKey, seconds: u64) -> Value {
    json!({
        "reviewer": reviewer,
        "reviewer_key": hex(cap.verifying_key().as_bytes()),
        "scope_sha256": "55".repeat(32),
        "deadline": (now_nanos() + seconds * 1_000_000_000).to_string(),
        "require_hardware": false,
    })
}
fn delivery(cap: &SigningKey, digest: &str, who: &str, order: u64) -> Value {
    json!({
        "order_id": order.to_string(),
        "capsule_sha256": digest,
        "capsule_signer": hex(cap.verifying_key().as_bytes()),
        "binding_signature": bind_order(cap, digest, &[], who, order),
        "receipts": [],
    })
}

async fn start() -> testresult::TestResult<Env> {
    let sandbox = near_sandbox::Sandbox::start_sandbox().await?;
    let net = near_api::NetworkConfig::from_rpc_url("sandbox", sandbox.rpc_addr.parse()?);
    let key = near_api::Signer::from_secret_key(
        near_sandbox::config::DEFAULT_GENESIS_ACCOUNT_PRIVATE_KEY
            .parse()
            .unwrap(),
    )?;
    Ok(Env { sandbox, net, key })
}

#[tokio::test]
async fn orders_pay_the_reviewer_for_the_capsule_and_return_unmet_ones_on_a_real_runtime()
-> testresult::TestResult {
    let wasm = std::fs::read(cargo_near_build::build_with_cli(Default::default())?)?;
    let e = start().await?;
    let contract = e.account("registry.sandbox", 20).await;
    near_api::Contract::deploy(contract.clone())
        .use_code(wasm)
        .with_init_call(
            "new",
            json!({"verifier": "verifier.sandbox", "token": null, "min_token_order": null}),
        )?
        .with_signer(e.key.clone())
        .send_to(&e.net)
        .await?
        .assert_success();
    let carl = e.account("carl.sandbox", 10).await;
    let rita = e.account("rita.sandbox", 10).await;
    let mallory = e.account("mallory.sandbox", 10).await;
    let cap = key(1);

    // Carl sets one NEAR aside for Rita's capsule.
    e.call(
        &contract,
        &carl,
        "open_order",
        json!({"terms": order_terms("rita.sandbox", &cap, 600)}),
        NEAR,
    )
    .await
    .map_err(|x| format!("open_order: {x}"))?;
    let o = e.view(&contract, "order", json!({"order_id": "0"})).await;
    assert_eq!(o["state"], "Open");
    let amount: u128 = o["amount"].as_str().unwrap().parse()?;
    assert!(
        amount < NEAR && amount > NEAR - NEAR / 100,
        "the order is the deposit less its storage: {amount}"
    );

    // Mallory cannot take it, with her own key or with a replay of a signature for her account.
    let digest = "cd".repeat(32);
    let stolen = e
        .call(
            &contract,
            &mallory,
            "register_for_order",
            delivery(&cap, &digest, "mallory.sandbox", 0),
            NEAR / 10,
        )
        .await;
    assert!(stolen.unwrap_err().contains("only the order's reviewer"));
    let own = e
        .call(
            &contract,
            &mallory,
            "register_for_order",
            delivery(&key(9), &digest, "mallory.sandbox", 0),
            NEAR / 10,
        )
        .await;
    assert!(own.is_err());
    assert_eq!(
        e.view(&contract, "order", json!({"order_id": "0"})).await["state"],
        "Open"
    );

    // Rita registers her capsule for the order and is paid in the same transaction.
    let before = e.balance(&rita).await;
    e.call(
        &contract,
        &rita,
        "register_for_order",
        delivery(&cap, &digest, "rita.sandbox", 0),
        NEAR / 10,
    )
    .await
    .map_err(|x| format!("register_for_order: {x}"))?;
    let o = e.view(&contract, "order", json!({"order_id": "0"})).await;
    assert_eq!(
        (o["state"].as_str(), o["capsule_sha256"].as_str()),
        (Some("Paid"), Some(digest.as_str()))
    );
    let gained = e.balance(&rita).await as i128 - before as i128;
    assert!(
        gained > (amount - NEAR / 50) as i128 && gained < amount as i128,
        "Rita received the order's amount, less her own fees and storage: {gained} of {amount}"
    );
    let entry = e
        .view(&contract, "get", json!({"capsule_sha256": digest, "capsule_signer": hex(cap.verifying_key().as_bytes())}))
        .await;
    assert_eq!(entry["order"], "0");
    // It cannot be earned or refunded again.
    assert!(
        e.call(&contract, &rita, "claim_order", json!({"order_id": "0"}), 0)
            .await
            .is_err()
    );
    assert!(
        e.call(
            &contract,
            &carl,
            "refund_order",
            json!({"order_id": "0"}),
            0
        )
        .await
        .is_err()
    );

    // A second order nobody delivers goes back to Carl, and only after its deadline.
    let before = e.balance(&carl).await;
    e.call(
        &contract,
        &carl,
        "open_order",
        json!({"terms": order_terms("rita.sandbox", &cap, 62)}),
        NEAR,
    )
    .await
    .map_err(|x| format!("open_order 2: {x}"))?;
    let early = e
        .call(
            &contract,
            &mallory,
            "refund_order",
            json!({"order_id": "1"}),
            0,
        )
        .await;
    assert!(early.unwrap_err().contains("deadline has not passed"));
    tokio::time::sleep(std::time::Duration::from_secs(66)).await;
    let late = e
        .call(
            &contract,
            &rita,
            "register_for_order",
            delivery(&cap, &"ef".repeat(32), "rita.sandbox", 1),
            NEAR / 10,
        )
        .await;
    assert!(late.unwrap_err().contains("deadline has passed"));
    e.call(
        &contract,
        &mallory,
        "refund_order",
        json!({"order_id": "1"}),
        0,
    )
    .await
    .map_err(|x| format!("refund_order: {x}"))?;
    assert_eq!(
        e.view(&contract, "order", json!({"order_id": "1"})).await["state"],
        "Refunded"
    );
    let lost = before as i128 - e.balance(&carl).await as i128;
    assert!(
        lost < (NEAR / 50) as i128,
        "Carl got the order back, less fees and the order's storage: {lost}"
    );
    Ok(())
}

#[tokio::test]
async fn a_fee_on_top_is_paid_to_the_fee_account_only_for_a_delivered_review_on_a_real_runtime()
-> testresult::TestResult {
    let wasm = std::fs::read(cargo_near_build::build_with_cli(Default::default())?)?;
    let e = start().await?;
    let contract = e.account("registry.sandbox", 20).await;
    let fees = e.account("fees.sandbox", 1).await;
    near_api::Contract::deploy(contract.clone())
        .use_code(wasm)
        .with_init_call(
            "new",
            json!({"verifier": "verifier.sandbox", "token": null, "min_token_order": null, "fee_bps": 200, "fee_account": "fees.sandbox"}),
        )?
        .with_signer(e.key.clone())
        .send_to(&e.net)
        .await?
        .assert_success();
    assert_eq!(
        e.view(&contract, "fee_terms", json!({})).await,
        json!({"bps": 200, "account": "fees.sandbox"})
    );
    let carl = e.account("carl.sandbox", 10).await;
    let rita = e.account("rita.sandbox", 10).await;
    let cap = key(1);

    // Carl asks for one NEAR to reach Rita: the price is one NEAR and 2% on top; what he attaches beyond it and the
    // storage comes back.
    let price = e
        .view(&contract, "price", json!({"amount": NEAR.to_string()}))
        .await;
    assert_eq!(price["fee"], (NEAR / 50).to_string());
    let mut terms = order_terms("rita.sandbox", &cap, 62);
    terms["amount"] = json!(NEAR.to_string());
    let carl_before = e.balance(&carl).await;
    e.call(
        &contract,
        &carl,
        "open_order",
        json!({"terms": terms}),
        2 * NEAR,
    )
    .await
    .map_err(|x| format!("open_order: {x}"))?;
    let spent = carl_before - e.balance(&carl).await;
    assert!(
        spent > NEAR + NEAR / 50 && spent < NEAR + NEAR / 50 + NEAR / 50,
        "Carl paid the price, the order's storage and gas, and got the rest back: {spent}"
    );
    let o = e.view(&contract, "order", json!({"order_id": "0"})).await;
    assert_eq!(
        (o["amount"].as_str(), o["fee"].as_str()),
        (
            Some(NEAR.to_string().as_str()),
            Some((NEAR / 50).to_string().as_str())
        )
    );

    // Rita delivers: she receives the whole amount; the fee is owed to the fee account and sent there by anyone's call.
    let rita_before = e.balance(&rita).await;
    let digest = "cd".repeat(32);
    e.call(
        &contract,
        &rita,
        "register_for_order",
        delivery(&cap, &digest, "rita.sandbox", 0),
        NEAR / 10,
    )
    .await
    .map_err(|x| format!("register_for_order: {x}"))?;
    let gained = e.balance(&rita).await as i128 - rita_before as i128;
    assert!(
        gained > (NEAR - NEAR / 50) as i128 && gained < NEAR as i128,
        "Rita received the amount, less her own gas and storage: {gained}"
    );
    assert_eq!(
        e.view(&contract, "fees_owed", json!({})).await["near"],
        (NEAR / 50).to_string()
    );
    let fees_before = e.balance(&fees).await;
    e.call(
        &contract,
        &carl,
        "withdraw_fees",
        json!({"in_token": false}),
        0,
    )
    .await
    .map_err(|x| format!("withdraw_fees: {x}"))?;
    assert_eq!(
        e.balance(&fees).await - fees_before,
        NEAR / 50,
        "the fee reached the fee account, whoever asked"
    );
    assert_eq!(e.view(&contract, "fees_owed", json!({})).await["near"], "0");
    assert!(
        e.call(
            &contract,
            &carl,
            "withdraw_fees",
            json!({"in_token": false}),
            0
        )
        .await
        .unwrap_err()
        .contains("no fees are waiting")
    );

    // An order nobody delivers goes back whole, fee included.
    let mut terms = order_terms("rita.sandbox", &cap, 62);
    terms["amount"] = json!(NEAR.to_string());
    e.call(
        &contract,
        &carl,
        "open_order",
        json!({"terms": terms}),
        2 * NEAR,
    )
    .await
    .map_err(|x| format!("open_order 2: {x}"))?;
    tokio::time::sleep(std::time::Duration::from_secs(66)).await;
    let carl_before = e.balance(&carl).await;
    e.call(
        &contract,
        &rita,
        "refund_order",
        json!({"order_id": "1"}),
        0,
    )
    .await
    .map_err(|x| format!("refund_order: {x}"))?;
    assert_eq!(
        e.balance(&carl).await - carl_before,
        NEAR + NEAR / 50,
        "the amount and the fee came back"
    );
    assert_eq!(
        e.view(&contract, "fees_owed", json!({})).await["near"],
        "0",
        "a refund earns nothing"
    );
    Ok(())
}

async fn testnet_code(http: &reqwest::Client, account: &str) -> Vec<u8> {
    let code: Value = http
        .post(TESTNET_RPC)
        .json(
            &json!({"jsonrpc": "2.0", "id": 1, "method": "query", "params": {
            "request_type": "view_code", "finality": "final", "account_id": account}}),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    unb64(
        code["result"]["code_base64"]
            .as_str()
            .expect("contract code"),
    )
}

/// The token path on a real runtime, with a real NEP-141 contract (wrapped NEAR's code, fetched from testnet).
#[tokio::test]
#[ignore]
async fn live_a_token_order_is_opened_by_a_transfer_paid_in_the_token_and_survives_a_failed_payout()
-> testresult::TestResult {
    let http = reqwest::Client::new();
    let token_wasm = testnet_code(&http, "wrap.testnet").await;
    let wasm = std::fs::read(cargo_near_build::build_with_cli(Default::default())?)?;
    let e = start().await?;
    let token = e.account("token.sandbox", 30).await;
    near_api::Contract::deploy(token.clone())
        .use_code(token_wasm)
        .with_init_call("new", json!({}))?
        .with_signer(e.key.clone())
        .send_to(&e.net)
        .await?
        .assert_success();
    let contract = e.account("registry.sandbox", 20).await;
    near_api::Contract::deploy(contract.clone())
        .use_code(wasm.clone())
        .with_init_call(
            "new",
            json!({"verifier": "verifier.sandbox", "token": "token.sandbox", "min_token_order": (NEAR / 10).to_string()}),
        )?
        .with_signer(e.key.clone())
        .send_to(&e.net)
        .await?
        .assert_success();
    let carl = e.account("carl.sandbox", 20).await;
    let rita = e.account("rita.sandbox", 10).await;
    let cap = key(1);
    let balance = |who: &'static str| {
        let (t, e) = (token.clone(), &e);
        async move {
            e.view(&t, "ft_balance_of", json!({"account_id": who}))
                .await
                .as_str()
                .unwrap()
                .parse::<u128>()
                .unwrap()
        }
    };
    let register = |who: &'static str| json!({"account_id": who});
    // Carl and the registry can hold the token; Rita cannot yet. Carl wraps five NEAR.
    e.call(
        &token,
        &carl,
        "storage_deposit",
        register("carl.sandbox"),
        NEAR / 100,
    )
    .await?;
    e.call(
        &token,
        &carl,
        "storage_deposit",
        register("registry.sandbox"),
        NEAR / 100,
    )
    .await?;
    e.call(&token, &carl, "near_deposit", json!({}), 5 * NEAR)
        .await?;
    let transfer = |amount: u128, msg: String| json!({"receiver_id": "registry.sandbox", "amount": amount.to_string(), "msg": msg});

    // A transfer with no terms, and one below the smallest order, come straight back.
    e.call(
        &token,
        &carl,
        "ft_transfer_call",
        transfer(NEAR, "pay rita".into()),
        1,
    )
    .await?;
    e.call(
        &token,
        &carl,
        "ft_transfer_call",
        transfer(
            NEAR / 100,
            order_terms("rita.sandbox", &cap, 600).to_string(),
        ),
        1,
    )
    .await?;
    assert_eq!(
        balance("carl.sandbox").await,
        5 * NEAR,
        "both were refunded in full"
    );
    assert_eq!(balance("registry.sandbox").await, 0);
    assert_eq!(e.view(&contract, "orders_total", json!({})).await, "0");

    // A transfer with terms opens an order in the token.
    e.call(
        &token,
        &carl,
        "ft_transfer_call",
        transfer(NEAR, order_terms("rita.sandbox", &cap, 600).to_string()),
        1,
    )
    .await?;
    let o = e.view(&contract, "order", json!({"order_id": "0"})).await;
    assert_eq!(
        (
            o["state"].as_str(),
            o["token"].as_str(),
            o["client"].as_str()
        ),
        (Some("Open"), Some("token.sandbox"), Some("carl.sandbox"))
    );
    assert_eq!(o["amount"], NEAR.to_string());
    assert_eq!(
        (
            balance("carl.sandbox").await,
            balance("registry.sandbox").await
        ),
        (4 * NEAR, NEAR)
    );

    // Rita registers her capsule, but she cannot hold the token yet: the payout fails, the order stays earned, the money stays.
    let digest = "cd".repeat(32);
    e.call(
        &contract,
        &rita,
        "register_for_order",
        delivery(&cap, &digest, "rita.sandbox", 0),
        NEAR / 10,
    )
    .await?;
    assert_eq!(
        e.view(&contract, "order", json!({"order_id": "0"})).await["state"],
        "Earned"
    );
    assert_eq!(balance("registry.sandbox").await, NEAR);

    // Once she can hold it, anyone may retry and the money goes to her, once.
    e.call(
        &token,
        &rita,
        "storage_deposit",
        register("rita.sandbox"),
        NEAR / 100,
    )
    .await?;
    e.call(&contract, &carl, "claim_order", json!({"order_id": "0"}), 0)
        .await?;
    assert_eq!(
        e.view(&contract, "order", json!({"order_id": "0"})).await["state"],
        "Paid"
    );
    assert_eq!(
        (
            balance("rita.sandbox").await,
            balance("registry.sandbox").await
        ),
        (NEAR, 0)
    );
    assert!(
        e.call(&contract, &rita, "claim_order", json!({"order_id": "0"}), 0)
            .await
            .is_err()
    );
    assert_eq!(balance("rita.sandbox").await, NEAR);

    // A second registry takes 2% on top, in the token too. Carl names Rita's amount and sends more than its price: the
    // price is kept and the rest comes back through the token's own refund.
    let fees_registry = e.account("fee-registry.sandbox", 20).await;
    e.account("fees.sandbox", 1).await;
    near_api::Contract::deploy(fees_registry.clone())
        .use_code(wasm.clone())
        .with_init_call(
            "new",
            json!({"verifier": "verifier.sandbox", "token": "token.sandbox", "min_token_order": (NEAR / 10).to_string(),
                "fee_bps": 200, "fee_account": "fees.sandbox"}),
        )?
        .with_signer(e.key.clone())
        .send_to(&e.net)
        .await?
        .assert_success();
    e.call(
        &token,
        &carl,
        "storage_deposit",
        register("fee-registry.sandbox"),
        NEAR / 100,
    )
    .await?;
    let carl_before = balance("carl.sandbox").await;
    let mut terms = order_terms("rita.sandbox", &cap, 600);
    terms["amount"] = json!(NEAR.to_string());
    e.call(
        &token,
        &carl,
        "ft_transfer_call",
        json!({"receiver_id": "fee-registry.sandbox", "amount": (2 * NEAR).to_string(), "msg": terms.to_string()}),
        1,
    )
    .await?;
    assert_eq!(
        carl_before - balance("carl.sandbox").await,
        NEAR + NEAR / 50,
        "the price was kept, the rest came back"
    );
    let o = e
        .view(&fees_registry, "order", json!({"order_id": "0"}))
        .await;
    assert_eq!(
        (o["amount"].as_str(), o["fee"].as_str()),
        (
            Some(NEAR.to_string().as_str()),
            Some((NEAR / 50).to_string().as_str())
        )
    );
    let rita_before = balance("rita.sandbox").await;
    e.call(
        &fees_registry,
        &rita,
        "register_for_order",
        delivery(&cap, &"ab".repeat(32), "rita.sandbox", 0),
        NEAR / 10,
    )
    .await?;
    assert_eq!(
        balance("rita.sandbox").await - rita_before,
        NEAR,
        "Rita received the whole amount in the token"
    );
    assert_eq!(
        e.view(&fees_registry, "fees_owed", json!({})).await["token"],
        (NEAR / 50).to_string()
    );
    // The fee account cannot hold the token yet: the transfer fails and the fee stays owed, not lost.
    e.call(
        &fees_registry,
        &carl,
        "withdraw_fees",
        json!({"in_token": true}),
        0,
    )
    .await?;
    assert_eq!(
        e.view(&fees_registry, "fees_owed", json!({})).await["token"],
        (NEAR / 50).to_string()
    );
    assert_eq!(
        balance("fee-registry.sandbox").await,
        NEAR / 50,
        "the fee is still held by the registry"
    );
    e.call(
        &token,
        &carl,
        "storage_deposit",
        register("fees.sandbox"),
        NEAR / 100,
    )
    .await?;
    e.call(
        &fees_registry,
        &rita,
        "withdraw_fees",
        json!({"in_token": true}),
        0,
    )
    .await?;
    assert_eq!(
        balance("fees.sandbox").await,
        NEAR / 50,
        "the fee reached the fee account"
    );
    assert_eq!(
        e.view(&fees_registry, "fees_owed", json!({})).await["token"],
        "0"
    );
    assert_eq!(balance("fee-registry.sandbox").await, 0);
    Ok(())
}

fn testnet_signer(var: &str) -> testresult::TestResult<std::sync::Arc<near_api::Signer>> {
    let file: Value =
        near_sdk::serde_json::from_str(&std::fs::read_to_string(std::env::var(var)?)?)?;
    Ok(near_api::Signer::from_secret_key(
        file["private_key"].as_str().expect("private_key").parse()?,
    )?)
}

/// On NEAR testnet: a client account opens a NEAR order and the reviewer account is paid for its capsule.
/// `KAKU_TESTNET_REGISTRY`, `KAKU_TESTNET_CLIENT` with `KAKU_TESTNET_CLIENT_KEY_FILE`, `KAKU_TESTNET_REVIEWER` with
/// `KAKU_TESTNET_REVIEWER_KEY_FILE`. Throwaway accounts; testnet NEAR only.
#[tokio::test]
#[ignore]
async fn testnet_an_order_is_opened_and_paid_for_the_reviewers_capsule() -> testresult::TestResult {
    let contract: AccountId = std::env::var("KAKU_TESTNET_REGISTRY")?.parse()?;
    let client: AccountId = std::env::var("KAKU_TESTNET_CLIENT")?.parse()?;
    let reviewer: AccountId = std::env::var("KAKU_TESTNET_REVIEWER")?.parse()?;
    let (client_key, reviewer_key) = (
        testnet_signer("KAKU_TESTNET_CLIENT_KEY_FILE")?,
        testnet_signer("KAKU_TESTNET_REVIEWER_KEY_FILE")?,
    );
    let net = near_api::NetworkConfig::testnet();
    let view = |method: &'static str, args: Value| {
        let (c, net) = (contract.clone(), net.clone());
        async move {
            near_api::Contract(c)
                .call_function(method, args)
                .read_only::<Value>()
                .fetch_from(&net)
                .await
                .unwrap()
                .data
        }
    };
    let send = |who: AccountId,
                key: std::sync::Arc<near_api::Signer>,
                method: &'static str,
                args: Value,
                deposit: u128| {
        let (c, net) = (contract.clone(), net.clone());
        async move {
            let done = near_api::Contract(c)
                .call_function(method, args)
                .transaction()
                .gas(NearGas::from_tgas(100))
                .deposit(NearToken::from_yoctonear(deposit))
                .with_signer(who, key)
                .send_to(&net)
                .await
                .unwrap()
                .into_full()
                .unwrap();
            println!(
                "{method}: {} ({})",
                done.outcome().transaction_hash,
                done.total_gas_burnt
            );
            done.into_result().map(|_| ()).map_err(|e| format!("{e:?}"))
        }
    };
    let id: u64 = view("orders_total", json!({}))
        .await
        .as_str()
        .unwrap()
        .parse()?;
    // a capsule key and digest made for this run only
    let seed: [u8; 32] = <sha2::Sha256 as sha2::Digest>::digest(now_nanos().to_le_bytes()).into();
    let cap = SigningKey::from_bytes(&seed);
    let digest = hex(&<sha2::Sha256 as sha2::Digest>::digest(seed));
    // the client asks for a quarter NEAR to reach the reviewer; the registry's fee goes on top and the rest comes back
    let fee_terms = view("fee_terms", json!({})).await;
    println!("fee terms: {fee_terms}");
    let mut terms = order_terms(reviewer.as_str(), &cap, 900);
    terms["amount"] = json!((NEAR / 4).to_string());
    send(
        client.clone(),
        client_key.clone(),
        "open_order",
        json!({"terms": terms}),
        NEAR / 2,
    )
    .await?;
    let o = view("order", json!({"order_id": id.to_string()})).await;
    assert_eq!(
        (
            o["state"].as_str(),
            o["client"].as_str(),
            o["amount"].as_str()
        ),
        (
            Some("Open"),
            Some(client.as_str()),
            Some((NEAR / 4).to_string().as_str())
        )
    );
    let fee: u128 = o["fee"].as_str().unwrap().parse()?;
    let bps = u128::from(fee_terms["bps"].as_u64().unwrap_or(0) as u16);
    assert_eq!(
        fee,
        (NEAR / 4 * bps).div_ceil(10_000),
        "the fee on top is the registry's share of the amount"
    );
    let owed_before: u128 = view("fees_owed", json!({})).await["near"]
        .as_str()
        .unwrap()
        .parse()?;
    let before = near_api::Tokens::account(reviewer.clone())
        .near_balance()
        .fetch_from(&net)
        .await?
        .total
        .as_yoctonear();
    send(
        reviewer.clone(),
        reviewer_key,
        "register_for_order",
        delivery(&cap, &digest, reviewer.as_str(), id),
        NEAR / 10,
    )
    .await?;
    let o = view("order", json!({"order_id": id.to_string()})).await;
    println!("order {id}: {o}");
    assert_eq!(o["state"], "Paid");
    let after = near_api::Tokens::account(reviewer.clone())
        .near_balance()
        .fetch_from(&net)
        .await?
        .total
        .as_yoctonear();
    let amount: u128 = o["amount"].as_str().unwrap().parse()?;
    println!(
        "reviewer gained {} yoctoNEAR of an order of {amount}",
        after as i128 - before as i128
    );
    assert!(after > before + amount - NEAR / 20);
    let owed: u128 = view("fees_owed", json!({})).await["near"]
        .as_str()
        .unwrap()
        .parse()?;
    assert_eq!(
        owed,
        owed_before + fee,
        "the fee was earned with the capsule"
    );
    if fee > 0 {
        let fees_account: AccountId = fee_terms["account"].as_str().unwrap().parse()?;
        let balance = |who: AccountId| {
            let net = net.clone();
            async move {
                near_api::Tokens::account(who)
                    .near_balance()
                    .fetch_from(&net)
                    .await
                    .unwrap()
                    .total
                    .as_yoctonear()
            }
        };
        let before = balance(fees_account.clone()).await;
        send(
            client.clone(),
            client_key,
            "withdraw_fees",
            json!({"in_token": false}),
            0,
        )
        .await?;
        let gained = balance(fees_account.clone()).await - before;
        println!("fee account {fees_account} gained {gained} yoctoNEAR");
        assert_eq!(
            gained, owed,
            "everything owed reached the fee account, sent by the client"
        );
    }
    Ok(())
}
