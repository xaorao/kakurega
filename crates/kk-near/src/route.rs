//! A model route that runs on NEAR AI Cloud's confidential hardware, and gives an answer to nobody until both halves of its
//! proof have verified: the deployment before the request (the gateway and the model's attestation, with the policy of this
//! product on top) and the particular answer after it (the signature over exactly the bytes that were sent and received, which
//! must be the model's own, not the gateway's). The request is encrypted to the attested model key (end to end, with the reply
//! key bound to the request); every answer leaves a receipt that a recipient can read.

use crate::release::{self, Outcomes, Part, ReleaseRule, ReleaseVerifier};
use crate::NearPolicy;
use kk_route::{
    neutralised, ChatMessage, Completion, CompletionRequest, Route, RouteClass, RouteError,
    RouteEvidence, ToolCall,
};
use nearai_inference_sdk::{
    ApiError, AttestationPolicy, CompletionSignatureKind, DefaultTdxQuoteVerifier,
    GatewayVerificationOptions, GpuEvidenceRequirement, GpuEvidenceStatus, InferenceClient,
    InferenceClientOptions, InferenceError, ModelAttestationPolicy, ModelVerificationOptions,
    NrasGpuEvidenceVerifier, SigningAlgo,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// The most that is read of one answer's text, so a hostile or broken service cannot hand back an unbounded reply.
const MAX_REPLY_BYTES: usize = 1 << 20;

pub struct NearRoute {
    id: String,
    model: String,
    base_url: String,
    client: InferenceClient,
    rt: tokio::runtime::Runtime,
    last: Mutex<Option<Value>>,
    /// Whether an answer signed by the gateway (and not by the model's hardware) may be given. Never for data this route is
    /// allowed to carry above internal.
    require_model_signature: bool,
    /// What the build-record check found for the gateway and for the model, when the check is on.
    release: Option<(Outcomes, Outcomes)>,
}

/// What a verified route needs to start: where it is, the account's key (read from the environment by the caller, never stored
/// here beyond the client), the model, and the policy for the hardware proof.
pub struct NearConfig {
    pub base_url: Option<String>,
    pub api_key: String,
    pub model: String,
    pub policy: NearPolicy,
}

/// The SDK options this product uses for a configuration: end-to-end encryption on, the gateway pinned to its attested TLS key, the
/// hardware policy applied to the gateway and the model.
pub fn client_options(c: &NearConfig) -> InferenceClientOptions {
    client_options_with_release(c).0
}

/// The same options, with the handles that say what the build-record check found.
pub fn client_options_with_release(
    c: &NearConfig,
) -> (InferenceClientOptions, Option<(Outcomes, Outcomes)>) {
    let policy = &c.policy;
    let (gateway_release, model_release, outcomes) = match policy.release {
        ReleaseRule::Off => (None, None, None),
        rule => {
            let (g, go) = ReleaseVerifier::new(Part::Gateway, rule);
            let (m, mo) = ReleaseVerifier::new(Part::Model, rule);
            (
                Some(g as Arc<dyn nearai_inference_sdk::DeploymentVerifier>),
                Some(m as Arc<dyn nearai_inference_sdk::DeploymentVerifier>),
                Some((go, mo)),
            )
        }
    };
    let tdx = Arc::new(DefaultTdxQuoteVerifier::default());
    let gpu = Arc::new(NrasGpuEvidenceVerifier::default());
    let mut o = InferenceClientOptions {
        api_key: Some(c.api_key.clone()),
        e2ee: true,
        signing_algo: SigningAlgo::Ed25519,
        gateway_verification: GatewayVerificationOptions {
            include_spki_fingerprint: true,
            policy: Some(AttestationPolicy {
                accepted_tcb_statuses: Some(policy.accepted_tcb.clone()),
            }),
            tdx_quote: Some(tdx.clone()),
            deployment: gateway_release,
        },
        model_verification: ModelVerificationOptions {
            policy: Some(ModelAttestationPolicy {
                accepted_tcb_statuses: Some(policy.accepted_tcb.clone()),
                gpu_evidence: if policy.require_gpu {
                    GpuEvidenceRequirement::Required
                } else {
                    GpuEvidenceRequirement::IfPresent
                },
            }),
            tdx_quote: Some(tdx),
            gpu_evidence: Some(gpu),
            deployment: model_release,
        },
        ..Default::default()
    };
    if let Some(u) = &c.base_url {
        o.base_url = u.clone();
    }
    (o, outcomes)
}

impl NearRoute {
    pub fn new(c: NearConfig) -> Result<NearRoute, RouteError> {
        let (options, outcomes) = client_options_with_release(&c);
        let mut route = Self::with_options(options, &c.model)?;
        route.release = outcomes;
        Ok(route)
    }

    /// The same route over caller-built client options: the tests point it at a local fixture gateway with fixture verifiers.
    pub fn with_options(o: InferenceClientOptions, model: &str) -> Result<NearRoute, RouteError> {
        if !o.e2ee {
            return Err(RouteError::NotVerified(
                "this route requires end-to-end encryption".into(),
            ));
        }
        if model.is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
            return Err(RouteError::BadResponse(
                "a model name is 1 to 200 plain characters".into(),
            ));
        }
        let base_url = o.base_url.clone();
        let client = InferenceClient::with_options(o)
            .map_err(|e| RouteError::NotVerified(e.code().into()))?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| RouteError::Unreachable(e.to_string()))?;
        Ok(NearRoute {
            id: format!("near:{model}"),
            model: model.to_string(),
            base_url,
            client,
            rt,
            last: Mutex::new(None),
            require_model_signature: true,
            release: None,
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Verifies the deployment now (the SDK keeps a verified session for an hour). `Ok` carries what was proven.
    pub fn verify_deployment(&self) -> Result<Value, RouteError> {
        let r = self
            .rt
            .block_on(self.client.verify(&self.model))
            .map_err(|e| RouteError::NotVerified(describe(&e)))?;
        let mut proven = deployment_json(&r);
        if let Some((gateway, model)) = &self.release {
            proven["release"] = json!({
                "gateway": release_json(release::latest(gateway)),
                "model": release_json(release::latest(model)),
            });
        }
        Ok(proven)
    }
}

/// How a build-record result is written into a receipt: the word, and the reason when there is one.
pub fn release_json(found: Option<release::ReleaseCheck>) -> Value {
    match found {
        None => Value::Null,
        Some(c) => json!({"result": c.word(), "reason": c.reason()}),
    }
}

fn describe(e: &InferenceError) -> String {
    format!("{} ({})", e.code(), e)
}

fn deployment_json(r: &nearai_inference_sdk::AttestationVerificationResult) -> Value {
    let g = &r.gateway.evidence;
    let gm = &g.deployment.runtime_measurements;
    json!({
        "gateway": {
            "signer": g.signer.signing_address,
            "tcb_status": format!("{:?}", g.tcb_status),
            "compose_hash": gm.compose_hash,
            "os_image_hash": gm.os_image_hash,
            "tls_bound": matches!(r.gateway.tls_binding, nearai_inference_sdk::GatewayTlsBinding::Attested { .. }),
        },
        "models": r.models.iter().map(|m| {
            let mm = &m.evidence.deployment.runtime_measurements;
            json!({
                "signer": m.evidence.signer.signing_address,
                "tcb_status": format!("{:?}", m.evidence.tcb_status),
                "gpu_verified": m.gpu_evidence == GpuEvidenceStatus::Verified,
                "compose_hash": mm.compose_hash,
                "os_image_hash": mm.os_image_hash,
            })
        }).collect::<Vec<_>>(),
        "verified_at": r.verified_at,
    })
}

/// The status the model service answered, when the failure was an answer and not a broken connection.
fn http_status(e: &InferenceError) -> Option<u16> {
    match e {
        InferenceError::Api(ApiError::HttpStatus { status, .. }) => Some(*status),
        _ => None,
    }
}

fn map_error(e: InferenceError) -> RouteError {
    match http_status(&e) {
        Some(401 | 403) => RouteError::Unauthorized(http_status(&e).unwrap_or(401)),
        Some(402) => RouteError::OutOfCredit,
        Some(404) => RouteError::NotFound,
        Some(413) => RouteError::ContextExceeded,
        Some(429 | 502 | 503 | 504) => {
            RouteError::Busy(format!("status {}", http_status(&e).unwrap_or(0)))
        }
        Some(s) => RouteError::BadResponse(format!("status {s}")),
        None => match e {
            InferenceError::Api(ApiError::Transport { .. }) => {
                RouteError::Unreachable(describe(&e))
            }
            _ => RouteError::NotVerified(describe(&e)),
        },
    }
}

fn to_openai(m: &ChatMessage) -> Value {
    let mut o = json!({"role": m.role, "content": neutralised(&m.content)});
    if !m.tool_calls.is_empty() {
        o["tool_calls"] = Value::Array(
            m.tool_calls
                .iter()
                .map(|c| {
                    json!({"id": c.id, "type": "function",
                        "function": {"name": c.name, "arguments": c.arguments.to_string()}})
                })
                .collect(),
        );
    }
    if let Some(id) = &m.tool_call_id {
        o["tool_call_id"] = json!(id);
    }
    o
}

impl Route for NearRoute {
    fn id(&self) -> &str {
        &self.id
    }

    fn class(&self) -> RouteClass {
        RouteClass::VerifiedTee
    }

    /// True only while the deployment verifies: a gateway and a model whose hardware proof checks out under this product's
    /// policy, with a fresh challenge.
    fn tee_verified(&self) -> bool {
        self.verify_deployment().is_ok()
    }

    /// The request is encrypted with the reply key bound to it, so a party that holds the ciphertext cannot replay it and read
    /// the answer.
    fn reply_key_bound(&self) -> bool {
        true
    }

    fn evidence(&self) -> RouteEvidence {
        RouteEvidence {
            route_id: self.id.clone(),
            class: "verified_tee".into(),
            model: self.model.clone(),
            model_digest: None,
            endpoint: self.base_url.clone(),
            tee_verified: self.last.lock().map(|l| l.is_some()).unwrap_or(false),
            simulated: false,
            receipt: self.last.lock().ok().and_then(|l| l.clone()),
        }
    }

    fn complete(&self, request: &CompletionRequest) -> Result<Completion, RouteError> {
        // 1. before the request: the deployment must verify, or nothing is sent
        let deployment = self.verify_deployment()?;
        let mut body = json!({
            "model": self.model,
            "stream": false,
            "messages": request.messages.iter().map(to_openai).collect::<Vec<_>>(),
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
        });
        if !request.tools.is_empty() {
            body["tools"] = Value::Array(
                request
                    .tools
                    .iter()
                    .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}}))
                    .collect(),
            );
        }
        let v = self
            .rt
            .block_on(self.client.chat_completions(body))
            .map_err(map_error)?;
        let id = v["id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 200)
            .ok_or_else(|| RouteError::BadResponse("the answer has no id".into()))?
            .to_string();
        // 2. after the answer: the signature over exactly these bytes, and whose it is
        let verified = self
            .rt
            .block_on(self.client.verify_response(&id))
            .map_err(|e| RouteError::NotVerified(describe(&e)))?;
        let kind = verified.signature_kind();
        if self.require_model_signature && kind != CompletionSignatureKind::ProviderTee {
            return Err(RouteError::NotVerified(
                "the answer was signed by the gateway, not by the model's own hardware".into(),
            ));
        }
        let choice = &v["choices"][0]["message"];
        let content = choice["content"].as_str().unwrap_or("");
        if content.len() > MAX_REPLY_BYTES {
            return Err(RouteError::BadResponse(
                "the answer is larger than this product reads".into(),
            ));
        }
        let mut tool_calls = Vec::new();
        if let Some(calls) = choice["tool_calls"].as_array() {
            for (i, c) in calls.iter().enumerate().take(16) {
                let name = c["function"]["name"]
                    .as_str()
                    .ok_or_else(|| RouteError::BadResponse("tool call without a name".into()))?;
                let raw = &c["function"]["arguments"];
                let arguments = match raw.as_str() {
                    Some(t) => serde_json::from_str(t).unwrap_or(Value::String(t.to_string())),
                    None => raw.clone(),
                };
                tool_calls.push(ToolCall {
                    id: c["id"]
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| format!("call_{i}")),
                    name: name.to_string(),
                    arguments,
                });
            }
        }
        let receipt = json!({
            "completion_id": verified.id,
            "signature_kind": match kind { CompletionSignatureKind::ProviderTee => "provider_tee", CompletionSignatureKind::Gateway => "gateway" },
            "signer": verified.signature.signer.signing_address,
            "signing_algo": verified.signature.signer.signing_algo.to_string(),
            "signed_text": verified.signature.signed_text,
            "signature": verified.signature.signature,
            "e2ee": true,
            "reply_key_bound": true,
            "deployment": deployment,
        });
        if let Ok(mut last) = self.last.lock() {
            *last = Some(receipt);
        }
        Ok(Completion {
            message: ChatMessage {
                role: "assistant".into(),
                content: content.to_string(),
                tool_calls,
                tool_call_id: None,
            },
            prompt_tokens: v["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
            completion_tokens: v["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        })
    }
}
