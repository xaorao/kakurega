//! A fake NEAR AI Cloud gateway for tests that run without an account: it serves the attestation reports, decrypts the end to end
//! encrypted request the way the model's enclave would, encrypts the answer to the reply key, and signs what it sent. The report
//! layout, the field encryption and the signed text follow the fixtures of the official inference SDK (MIT, NEAR AI,
//! `rs/tests/inference_client.rs` and `rs/tests/support`); the quote verifier is a fixture that reads the quote's own report data, so
//! nothing here says anything about real hardware. Everything this module produces is SIMULATED.

use async_trait::async_trait;
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305,
};
use ed25519_dalek::Signer;
use nearai_inference_sdk::{
    TcbStatus, TdxQuoteVerificationResult, TdxQuoteVerifier, VerificationError,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256, Sha384, Sha512};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

pub const MODEL: &str = "vendor/model";
pub const APP_COMPOSE: &str = "{\"services\":{\"model\":\"example@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}}";
pub const MODEL_SEED: u8 = 7;
pub const GATEWAY_SEED: u8 = 9;
pub const OTHER_SEED: u8 = 8;

pub fn key(seed: u8) -> ed25519_dalek::SigningKey {
    ed25519_dalek::SigningKey::from_bytes(&[seed; 32])
}

pub fn address(seed: u8) -> String {
    hex::encode(key(seed).verifying_key().as_bytes())
}

pub fn sha256_hex(v: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(v.as_ref()))
}

/// Reads the report data out of the quote string itself, so a report whose data does not match what it claims fails.
pub struct FixtureQuote;

#[async_trait]
impl TdxQuoteVerifier for FixtureQuote {
    async fn verify(&self, quote: &str) -> Result<TdxQuoteVerificationResult, VerificationError> {
        let mut mr_config_id = vec![0x01];
        mr_config_id.extend(Sha256::digest(APP_COMPOSE.as_bytes()));
        mr_config_id.extend([0u8; 15]);
        let mut rtmr_input = vec![0u8; 48];
        rtmr_input.extend(vec![0u8; 48]);
        Ok(TdxQuoteVerificationResult {
            tcb_status: TcbStatus::UpToDate,
            advisory_ids: vec![],
            debug_enabled: false,
            report_data: hex::decode(quote).unwrap_or_default(),
            mr_config_id,
            rt_mr3: Sha384::digest(&rtmr_input).to_vec(),
        })
    }
}

fn report(nonce: &str, seed: u8) -> Value {
    let public = address(seed);
    let data = format!("{public}{nonce}");
    json!({"request_nonce": nonce, "signing_algo": "ed25519", "signing_address": public, "signing_public_key": public,
        "intel_quote": data, "report_data": data, "event_log": [{"digest": "00".repeat(48), "imr": 3}],
        "info": {"tcb_info": {"app_compose": APP_COMPOSE}}, "model_name": MODEL})
}

fn derived(shared: &[u8]) -> [u8; 32] {
    let mut k = [0; 32];
    hkdf::Hkdf::<Sha256>::new(None, shared)
        .expand(b"ed25519_encryption", &mut k)
        .unwrap();
    k
}

/// What the enclave does with one encrypted field of the request: decrypt it with the model's key, binding it to the reply key
/// and the field's place (E2EE v3). `None` when the field does not authenticate.
pub fn decrypt_field(cipher_hex: &str, reply_public: &str, field: &str) -> Option<String> {
    let mut aad = b"nearai-e2ee-request-v3".to_vec();
    aad.push(2);
    aad.extend(hex::decode(reply_public).ok()?);
    aad.push(0);
    aad.extend_from_slice(field.as_bytes());
    let c = hex::decode(cipher_hex).ok()?;
    if c.len() < 72 {
        return None;
    }
    let scalar: [u8; 32] = Sha512::digest([MODEL_SEED; 32])[..32].try_into().unwrap();
    let secret = x25519_dalek::StaticSecret::from(scalar);
    let public: [u8; 32] = c[..32].try_into().ok()?;
    let shared = secret.diffie_hellman(&public.into());
    let plain = XChaCha20Poly1305::new((&derived(shared.as_bytes())).into())
        .decrypt(
            c[32..56].into(),
            Payload {
                msg: &c[56..],
                aad: &aad,
            },
        )
        .ok()?;
    String::from_utf8(plain).ok()
}

/// What the enclave does with the answer: encrypt it to the client's reply key.
pub fn encrypt_reply(reply_public: &str, text: &str) -> String {
    let public = curve25519_dalek::edwards::CompressedEdwardsY(
        hex::decode(reply_public).unwrap().try_into().unwrap(),
    )
    .decompress()
    .unwrap()
    .to_montgomery();
    let secret = x25519_dalek::StaticSecret::from([3; 32]);
    let shared = secret.diffie_hellman(&public.to_bytes().into());
    let nonce = [2u8; 24];
    let cipher = XChaCha20Poly1305::new((&derived(shared.as_bytes())).into())
        .encrypt((&nonce).into(), text.as_bytes())
        .unwrap();
    let mut out = x25519_dalek::PublicKey::from(&secret).as_bytes().to_vec();
    out.extend(nonce);
    out.extend(cipher);
    hex::encode(out)
}

/// How the fake gateway misbehaves, one at a time.
#[derive(Default, Clone)]
pub struct Faults {
    /// Sign the answer with the gateway's key and say so, instead of the model's.
    pub gateway_signs: bool,
    /// Change a byte of the answer after it was signed.
    pub tamper: bool,
    /// Sign with a key that is not the attested model's.
    pub wrong_signer: bool,
    /// The model's report carries data that does not match the quote's.
    pub bad_model_report: bool,
    /// Answer every chat request with this status.
    pub chat_status: Option<u16>,
    /// The text the enclave answers with (a tool call when `Some`).
    pub reply: Option<String>,
    /// Answer with a tool call instead of text.
    pub tool_call: bool,
}

pub struct Gateway {
    pub server: MockServer,
    pub chats: Arc<AtomicUsize>,
    /// Every plaintext the enclave decrypted, in order.
    pub seen: Arc<Mutex<Vec<String>>>,
    /// The raw request bodies as they crossed the wire.
    pub wire: Arc<Mutex<Vec<String>>>,
}

/// A request body and the answer's bytes, as they crossed the wire.
type Wire = (Vec<u8>, Vec<u8>);

struct Fixture {
    faults: Faults,
    chats: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<String>>>,
    wire: Arc<Mutex<Vec<String>>>,
    receipts: Mutex<HashMap<String, Wire>>,
}

impl Respond for Fixture {
    fn respond(&self, r: &Request) -> ResponseTemplate {
        let path = r.url.path();
        if path == "/v1/attestation/report" {
            let q: HashMap<_, _> = r.url.query_pairs().collect();
            let nonce = q["nonce"].to_string();
            return if q.contains_key("model") {
                let mut rep = report(&nonce, MODEL_SEED);
                if self.faults.bad_model_report {
                    rep["report_data"] = json!("ff".repeat(64));
                    rep["intel_quote"] = json!("ff".repeat(64));
                }
                ResponseTemplate::new(200).set_body_json(json!({"model_attestations": [rep]}))
            } else {
                ResponseTemplate::new(200)
                    .set_body_json(json!({"gateway_attestation": report(&nonce, GATEWAY_SEED)}))
            };
        }
        if path.starts_with("/v1/model/") {
            return ResponseTemplate::new(200).set_body_json(
                json!({"metadata": {"providerType": "vllm", "attestationSupported": true}}),
            );
        }
        if path == "/v1/chat/completions" {
            let n = self.chats.fetch_add(1, Ordering::SeqCst);
            if let Some(s) = self.faults.chat_status {
                return ResponseTemplate::new(s);
            }
            let body: Value = serde_json::from_slice(&r.body).unwrap();
            self.wire
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&r.body).into_owned());
            let reply_public = r
                .headers
                .get("x-client-pub-key")
                .map(|v| v.to_str().unwrap().to_string());
            let Some(reply_public) = reply_public else {
                return ResponseTemplate::new(400).set_body_string("the request was not encrypted");
            };
            assert_eq!(r.headers.get("x-encryption-version").unwrap(), "3");
            assert_eq!(
                r.headers.get("authorization").unwrap(),
                "Bearer fixture-key"
            );
            for m in body["messages"].as_array().unwrap() {
                let plain = decrypt_field(
                    m["content"].as_str().unwrap(),
                    &reply_public,
                    "messages[].content",
                );
                match plain {
                    Some(p) => self.seen.lock().unwrap().push(p),
                    None => {
                        return ResponseTemplate::new(400)
                            .set_body_string("a field did not authenticate")
                    }
                }
            }
            let text = self
                .faults
                .reply
                .clone()
                .unwrap_or_else(|| "A private answer.".into());
            let id = format!("chat-{n}");
            let message = if self.faults.tool_call {
                // a tool call's arguments and name are encrypted fields too
                json!({"content": null, "tool_calls": [{"id": "call_a", "type": "function", "function": {
                    "name": encrypt_reply(&reply_public, "read_file"),
                    "arguments": encrypt_reply(&reply_public, "{\"path\": \"a.py\"}")}}]})
            } else {
                json!({"content": encrypt_reply(&reply_public, &text)})
            };
            let response = json!({"id": id, "choices": [{"message": message}], "usage": {"prompt_tokens": 11, "completion_tokens": 5}})
                .to_string()
                .into_bytes();
            self.receipts
                .lock()
                .unwrap()
                .insert(id, (r.body.clone(), response.clone()));
            let mut wire = response;
            if self.faults.tamper {
                wire.push(b' ');
            }
            return ResponseTemplate::new(200)
                .insert_header("content-type", "application/json")
                .set_body_bytes(wire);
        }
        if let Some(id) = path.strip_prefix("/v1/signature/") {
            let receipts = self.receipts.lock().unwrap();
            let Some((req, res)) = receipts.get(id) else {
                return ResponseTemplate::new(404);
            };
            let (kind, text, seed) = if self.faults.gateway_signs {
                (
                    "gateway",
                    format!("{}:{}", sha256_hex(req), sha256_hex(res)),
                    GATEWAY_SEED,
                )
            } else {
                let seed = if self.faults.wrong_signer {
                    OTHER_SEED
                } else {
                    MODEL_SEED
                };
                (
                    "provider_tee",
                    format!("{MODEL}:{}:{}", sha256_hex(req), sha256_hex(res)),
                    seed,
                )
            };
            let k = key(seed);
            return ResponseTemplate::new(200).set_body_json(json!({"text": text,
                "signature": hex::encode(k.sign(text.as_bytes()).to_bytes()),
                "signing_address": hex::encode(k.verifying_key().as_bytes()),
                "signing_algo": "ed25519", "signature_kind": kind}));
        }
        ResponseTemplate::new(404)
    }
}

impl Gateway {
    pub async fn start(faults: Faults) -> Gateway {
        let server = MockServer::start().await;
        let chats = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let wire = Arc::new(Mutex::new(Vec::new()));
        Mock::given(wiremock::matchers::any())
            .respond_with(Fixture {
                faults,
                chats: chats.clone(),
                seen: seen.clone(),
                wire: wire.clone(),
                receipts: Mutex::new(HashMap::new()),
            })
            .mount(&server)
            .await;
        Gateway {
            server,
            chats,
            seen,
            wire,
        }
    }

    pub fn options(&self) -> nearai_inference_sdk::InferenceClientOptions {
        use nearai_inference_sdk::{
            GatewayVerificationOptions, InferenceClientOptions, ModelVerificationOptions,
        };
        InferenceClientOptions {
            api_key: Some("fixture-key".into()),
            base_url: format!("{}/v1", self.server.uri()),
            e2ee: true,
            gateway_verification: GatewayVerificationOptions {
                include_spki_fingerprint: false,
                tdx_quote: Some(Arc::new(FixtureQuote)),
                ..Default::default()
            },
            model_verification: ModelVerificationOptions {
                tdx_quote: Some(Arc::new(FixtureQuote)),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}
