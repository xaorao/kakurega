use nearai_inference_sdk::{
    verify_gateway_attestation, verify_model_attestation, AttestationClient, AttestationPolicy,
    AttestationVerifiers, DefaultTdxQuoteVerifier, GatewayAttestationFetchOptions,
    GatewayTlsBinding, GpuEvidenceRequirement, GpuEvidenceStatus, ModelAttestationPolicy,
    ModelAttestationVerifiers, NrasGpuEvidenceVerifier, SigningAlgo, TcbStatus,
    VerifiedModelAttestation,
};
use std::fmt;

pub mod release;
pub mod route;

pub use nearai_inference_sdk as sdk;

#[derive(Debug, Clone)]
pub struct NearPolicy {
    pub accepted_tcb: Vec<TcbStatus>,
    pub require_gpu: bool,
    pub allowed_compose_hashes: Option<Vec<String>>,
    pub allowed_os_image_hashes: Option<Vec<String>>,
    /// What to do with the check that the deployment's images were built by NEAR's public workflows.
    pub release: release::ReleaseRule,
}

impl Default for NearPolicy {
    fn default() -> Self {
        NearPolicy {
            accepted_tcb: vec![TcbStatus::UpToDate],
            require_gpu: true,
            allowed_compose_hashes: None,
            allowed_os_image_hashes: None,
            release: release::ReleaseRule::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteAttestation {
    pub model: String,
    pub signer_address: String,
    pub tcb_status: String,
    pub advisories: Vec<String>,
    pub gpu_verified: bool,
    pub compose_hash: Option<String>,
    pub os_image_hash: Option<String>,
    pub nonce: String,
    /// Whether the images were built by NEAR's public workflows; `None` when the check is off.
    pub release: Option<release::ReleaseCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    NoAttestations,
    TcbNotAccepted(String),
    GpuEvidenceMissing,
    MeasurementsMissing,
    ComposeHashNotAllowed(String),
    OsImageNotAllowed(String),
}

#[derive(Debug)]
pub enum NearError {
    Fetch(String),
    Verification(String),
    Policy(Rejection),
}

impl fmt::Display for NearError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NearError::Fetch(m) => write!(f, "could not fetch the attestation: {m}"),
            NearError::Verification(m) => write!(f, "attestation did not verify: {m}"),
            NearError::Policy(r) => write!(
                f,
                "attestation verified but the route policy refuses it: {r:?}"
            ),
        }
    }
}

impl std::error::Error for NearError {}

pub fn judge(
    model: &str,
    nonce: &str,
    v: &VerifiedModelAttestation,
    policy: &NearPolicy,
) -> Result<RouteAttestation, Rejection> {
    if !policy.accepted_tcb.contains(&v.evidence.tcb_status) {
        return Err(Rejection::TcbNotAccepted(format!(
            "{:?}",
            v.evidence.tcb_status
        )));
    }
    let gpu = v.gpu_evidence == GpuEvidenceStatus::Verified;
    if policy.require_gpu && !gpu {
        return Err(Rejection::GpuEvidenceMissing);
    }
    let m = &v.evidence.deployment.runtime_measurements;
    if (policy.allowed_compose_hashes.is_some() && m.compose_hash.is_none())
        || (policy.allowed_os_image_hashes.is_some() && m.os_image_hash.is_none())
    {
        return Err(Rejection::MeasurementsMissing);
    }
    if let (Some(allowed), Some(h)) = (&policy.allowed_compose_hashes, &m.compose_hash) {
        if !allowed.iter().any(|a| a == h) {
            return Err(Rejection::ComposeHashNotAllowed(h.clone()));
        }
    }
    if let (Some(allowed), Some(h)) = (&policy.allowed_os_image_hashes, &m.os_image_hash) {
        if !allowed.iter().any(|a| a == h) {
            return Err(Rejection::OsImageNotAllowed(h.clone()));
        }
    }
    Ok(RouteAttestation {
        model: model.to_string(),
        signer_address: v.evidence.signer.signing_address.clone(),
        tcb_status: format!("{:?}", v.evidence.tcb_status),
        advisories: v.evidence.advisory_ids.clone(),
        gpu_verified: gpu,
        compose_hash: m.compose_hash.clone(),
        os_image_hash: m.os_image_hash.clone(),
        nonce: nonce.to_string(),
        release: None,
    })
}

/// Fetch fresh evidence for a model without an API key, verify every deployment behind it through the official SDK (Intel TDX chain and NVIDIA GPU evidence), then apply the Kakurega policy. Every deployment must pass.
pub async fn attest_model(
    model: &str,
    policy: &NearPolicy,
) -> Result<Vec<RouteAttestation>, NearError> {
    let client = AttestationClient::new(None);
    // asked for the Ed25519 key, the one the answering route has answers signed with: the key shown is then the key that
    // signs answers, and the one the NEAR registry records
    let fetched = client
        .fetch_model_attestations(model, Some(SigningAlgo::Ed25519), None)
        .await
        .map_err(|e| NearError::Fetch(format!("{e:?}")))?;
    if fetched.attestations.is_empty() {
        return Err(NearError::Policy(Rejection::NoAttestations));
    }
    let tdx = DefaultTdxQuoteVerifier::default();
    let gpu = NrasGpuEvidenceVerifier::default();
    let sdk_policy = ModelAttestationPolicy {
        accepted_tcb_statuses: Some(policy.accepted_tcb.clone()),
        gpu_evidence: if policy.require_gpu {
            GpuEvidenceRequirement::Required
        } else {
            GpuEvidenceRequirement::IfPresent
        },
    };
    let mut out = Vec::new();
    for att in &fetched.attestations {
        let (verifier, outcomes) =
            release::ReleaseVerifier::new(release::Part::Model, policy.release);
        let deployment: Option<&dyn nearai_inference_sdk::DeploymentVerifier> = match policy.release
        {
            release::ReleaseRule::Off => None,
            _ => Some(&*verifier),
        };
        let v = verify_model_attestation(
            att,
            &fetched.client_binding,
            Some(&sdk_policy),
            ModelAttestationVerifiers {
                tdx_quote: Some(&tdx),
                gpu_evidence: Some(&gpu),
                deployment,
            },
        )
        .await
        .map_err(|e| NearError::Verification(format!("{e:?}")))?;
        let mut judged =
            judge(model, &fetched.client_binding.nonce, &v, policy).map_err(NearError::Policy)?;
        judged.release = release::latest(&outcomes);
        out.push(judged);
    }
    Ok(out)
}

/// What the gateway (the front door every request passes through) proved about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayProof {
    pub signer_address: String,
    pub tcb_status: String,
    pub advisories: Vec<String>,
    pub compose_hash: Option<String>,
    pub os_image_hash: Option<String>,
    /// True when the quote binds the TLS certificate this very connection used, so the encrypted channel ends inside the
    /// verified software rather than at a proxy in front of it.
    pub tls_bound: bool,
    pub tls_fingerprint: Option<String>,
    pub nonce: String,
    /// Whether the gateway's images were built by NEAR's public workflows; `None` when the check is off.
    pub release: Option<release::ReleaseCheck>,
}

/// Fetches fresh Gateway evidence with a random challenge, records the TLS peer of that request, verifies the Intel
/// TDX chain through the official SDK, and applies the policy's patch-level rule. Needs no API key.
pub async fn attest_gateway(policy: &NearPolicy) -> Result<GatewayProof, NearError> {
    let client = AttestationClient::new(None);
    let fetched = client
        .fetch_gateway_attestation(GatewayAttestationFetchOptions::default())
        .await
        .map_err(|e| NearError::Fetch(format!("{e:?}")))?;
    let tdx = DefaultTdxQuoteVerifier::default();
    let (verifier, outcomes) =
        release::ReleaseVerifier::new(release::Part::Gateway, policy.release);
    let deployment: Option<&dyn nearai_inference_sdk::DeploymentVerifier> = match policy.release {
        release::ReleaseRule::Off => None,
        _ => Some(&*verifier),
    };
    let v = verify_gateway_attestation(
        &fetched.attestation,
        &fetched.client_binding,
        Some(&AttestationPolicy {
            accepted_tcb_statuses: Some(policy.accepted_tcb.clone()),
        }),
        AttestationVerifiers {
            tdx_quote: Some(&tdx),
            deployment,
        },
    )
    .await
    .map_err(|e| NearError::Verification(format!("{e:?}")))?;
    let m = &v.evidence.deployment.runtime_measurements;
    let (tls_bound, fp) = match &v.tls_binding {
        GatewayTlsBinding::Attested { spki_fingerprint } => (true, Some(spki_fingerprint.clone())),
        GatewayTlsBinding::None => (false, None),
    };
    if !policy.accepted_tcb.contains(&v.evidence.tcb_status) {
        return Err(NearError::Policy(Rejection::TcbNotAccepted(format!(
            "{:?}",
            v.evidence.tcb_status
        ))));
    }
    Ok(GatewayProof {
        signer_address: v.evidence.signer.signing_address.clone(),
        tcb_status: format!("{:?}", v.evidence.tcb_status),
        advisories: v.evidence.advisory_ids.clone(),
        compose_hash: m.compose_hash.clone(),
        os_image_hash: m.os_image_hash.clone(),
        tls_bound,
        tls_fingerprint: fp,
        nonce: fetched.client_binding.nonce.clone(),
        release: release::latest(&outcomes),
    })
}
