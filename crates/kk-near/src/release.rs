//! Whether the software a verified deployment runs was built by NEAR's own public build workflows. The hardware proof says
//! which images a deployment measured; this check asks, for the images that handle a request, whether GitHub's signed build
//! record (Sigstore, through the official SDK) names the NEAR repository and workflow they should come from.
//!
//! It establishes where an image was built. It does not establish that the build is reproducible, that every image in the
//! deployment is covered, or what a model launcher loaded later.

use async_trait::async_trait;
use nearai_inference_sdk::{
    verify_compose_manager_deployment_image_provenance, verify_deployment_image_provenance,
    ApiError, ComposeManagerImagePolicy, DeploymentImagesFailureReason, DeploymentVerifier,
    ImageProvenanceFailureReason, ImageProvenancePolicy, MeasuredDeployment, VerificationError,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// What the check found for one deployment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseCheck {
    /// Every required image carries a valid build record from the named NEAR repository and workflow.
    Verified,
    /// The question could not be answered (the build records could not be fetched, or the deployment's last recorded
    /// action did not include the image). Nothing was shown to be wrong.
    NotEstablished(String),
    /// A required image is unpinned, absent where it must be present, has no build record, or was built somewhere else.
    Mismatch(String),
}

impl ReleaseCheck {
    /// One word for records and screens: `verified`, `not_established` or `mismatch`.
    pub fn word(&self) -> &'static str {
        match self {
            ReleaseCheck::Verified => "verified",
            ReleaseCheck::NotEstablished(_) => "not_established",
            ReleaseCheck::Mismatch(_) => "mismatch",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            ReleaseCheck::Verified => None,
            ReleaseCheck::NotEstablished(r) | ReleaseCheck::Mismatch(r) => Some(r),
        }
    }
}

/// What a route does with the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReleaseRule {
    /// The check is not run.
    Off,
    /// A mismatch refuses the deployment; an unanswered question is recorded and shown.
    #[default]
    RefuseMismatch,
    /// Anything but a verified result refuses the deployment.
    Require,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Gateway,
    Model,
}

const BUILD: &str = ".github/workflows/build.yml";

fn policy(repository: &str, workflow: &str) -> ImageProvenancePolicy {
    ImageProvenancePolicy::new(repository.to_owned(), workflow.to_owned())
}

/// The gateway's own images and the NEAR repositories that build them (read from the live measured Compose and from the
/// build records of the deployed digests).
pub fn gateway_images() -> BTreeMap<String, ImageProvenancePolicy> {
    BTreeMap::from([
        (
            "nearaidev/cloud-api".to_owned(),
            policy("nearai/cloud-api", BUILD),
        ),
        (
            "nearaidev/cvm-ingress".to_owned(),
            policy("nearai/cvm-ingress", ".github/workflows/build-push.yml"),
        ),
    ])
}

/// The image every model request passes through, as the SDK's guide names it.
pub fn model_images() -> BTreeMap<String, ImageProvenancePolicy> {
    BTreeMap::from([(
        "nearaidev/vllm-proxy-rs".to_owned(),
        policy("nearai/inference-proxy", BUILD),
    )])
}

pub fn compose_manager() -> ComposeManagerImagePolicy {
    ComposeManagerImagePolicy {
        image_repository: "nearaidev/compose-manager".to_owned(),
        build: policy("nearai/compose-manager", BUILD),
        signer_identity: None,
    }
}

/// Turns the SDK's answer into this product's three states. A gateway must name its images in its own measured Compose, so
/// a missing one is a mismatch there; a model deployment is checked through its last recorded action, which may not have
/// touched the proxy, so a missing image leaves the question open.
pub fn classify(part: Part, result: &Result<(), VerificationError>) -> ReleaseCheck {
    use DeploymentImagesFailureReason as Images;
    use ImageProvenanceFailureReason as Build;
    let e = match result {
        Ok(()) => return ReleaseCheck::Verified,
        Err(e) => e,
    };
    match e {
        VerificationError::ImageProvenanceVerificationFailed { reasons, .. } => {
            let words = reasons
                .iter()
                .map(|r| format!("{r:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            if !reasons.is_empty() && reasons.iter().all(|r| *r == Build::TrustRootUnavailable) {
                ReleaseCheck::NotEstablished(format!(
                    "build record trust root unavailable ({words})"
                ))
            } else {
                ReleaseCheck::Mismatch(format!("no trusted build record ({words})"))
            }
        }
        // GitHub answers "not found" when the named repository holds no build record for this digest: that is an image
        // nobody there built, not a question left open. Other answers (rate limits, outages) leave it open.
        VerificationError::ImageProvenanceRequestFailed {
            image_repository,
            source,
            ..
        } => match source.as_ref() {
            ApiError::HttpStatus { status: 404, .. } => ReleaseCheck::Mismatch(format!(
                "the named repository holds no build record for {image_repository}"
            )),
            _ => ReleaseCheck::NotEstablished(format!(
                "the build record for {image_repository} could not be fetched"
            )),
        },
        VerificationError::ComposeFileRequestFailed { file, .. } => ReleaseCheck::NotEstablished(
            format!("the recorded compose file {file} could not be fetched"),
        ),
        VerificationError::ComposeFileHashMismatch { file, .. } => ReleaseCheck::Mismatch(format!(
            "the compose file {file} does not match its recorded hash"
        )),
        VerificationError::DeploymentImagesInvalid {
            reason,
            image_repository,
            ..
        } => {
            let image = image_repository.as_deref().unwrap_or("an image");
            match (reason, part) {
                (Images::ImageMissing | Images::ServiceMissing, Part::Model) => {
                    ReleaseCheck::NotEstablished(format!(
                        "the last recorded deployment action did not include {image}"
                    ))
                }
                (Images::EmptyPolicy, _) => {
                    ReleaseCheck::NotEstablished("no image policy was given".to_owned())
                }
                _ => ReleaseCheck::Mismatch(format!("{image}: {reason:?}")),
            }
        }
        VerificationError::ComposeManagerDeploymentInvalid { reason } => match *reason {
            "attestation_missing" | "compose_up_missing" | "manager_image_missing" => {
                ReleaseCheck::NotEstablished(format!("deployment record incomplete ({reason})"))
            }
            other => ReleaseCheck::Mismatch(format!("deployment record invalid ({other})")),
        },
        other => ReleaseCheck::NotEstablished(format!("the check could not run ({other})")),
    }
}

/// Whether a result refuses the deployment under a rule.
pub fn refuses(rule: ReleaseRule, check: &ReleaseCheck) -> bool {
    match (rule, check) {
        (ReleaseRule::Off, _) | (_, ReleaseCheck::Verified) => false,
        (ReleaseRule::RefuseMismatch, ReleaseCheck::NotEstablished(_)) => false,
        (ReleaseRule::RefuseMismatch, ReleaseCheck::Mismatch(_)) => true,
        (ReleaseRule::Require, _) => true,
    }
}

/// Runs the SDK's build-record check on one measured deployment.
pub async fn check(part: Part, deployment: &MeasuredDeployment) -> ReleaseCheck {
    let result = run(part, deployment).await;
    classify(part, &result)
}

async fn run(part: Part, deployment: &MeasuredDeployment) -> Result<(), VerificationError> {
    match part {
        Part::Gateway => {
            verify_deployment_image_provenance(&deployment.app_compose, &gateway_images(), None)
                .await
        }
        Part::Model => {
            verify_compose_manager_deployment_image_provenance(
                deployment,
                &model_images(),
                &compose_manager(),
                Default::default(),
            )
            .await
        }
    }
}

/// The results a verifier has seen, newest last, shared with whoever installed it.
pub type Outcomes = Arc<Mutex<Vec<ReleaseCheck>>>;

/// The SDK calls this after the quote and the Compose measurement have verified. It records what it found and refuses the
/// deployment when the rule says so.
pub struct ReleaseVerifier {
    part: Part,
    rule: ReleaseRule,
    outcomes: Outcomes,
}

impl ReleaseVerifier {
    pub fn new(part: Part, rule: ReleaseRule) -> (Arc<ReleaseVerifier>, Outcomes) {
        let outcomes: Outcomes = Arc::new(Mutex::new(Vec::new()));
        (
            Arc::new(ReleaseVerifier {
                part,
                rule,
                outcomes: outcomes.clone(),
            }),
            outcomes,
        )
    }
}

#[async_trait]
impl DeploymentVerifier for ReleaseVerifier {
    async fn verify(&self, deployment: &MeasuredDeployment) -> Result<(), VerificationError> {
        let result = run(self.part, deployment).await;
        let found = classify(self.part, &result);
        let refuse = refuses(self.rule, &found);
        if let Ok(mut seen) = self.outcomes.lock() {
            if seen.len() >= 64 {
                seen.remove(0);
            }
            seen.push(found);
        }
        match (refuse, result) {
            (true, Err(e)) => Err(e),
            _ => Ok(()),
        }
    }
}

/// The newest result a verifier recorded, if it has run.
pub fn latest(outcomes: &Outcomes) -> Option<ReleaseCheck> {
    outcomes.lock().ok().and_then(|o| o.last().cloned())
}
