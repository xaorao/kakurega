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
        .with_init_call("new", json!({}))?
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
        .await
        .map_err(|x| x)?;
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
        .await
        .map_err(|x| x)?;
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
