//! Prints the container images named in the measured Compose of NEAR AI Cloud's gateway and of one model. Needs no key.
use nearai_inference_sdk::{
    verify_gateway_attestation, verify_model_attestation, AttestationClient, AttestationVerifiers,
    DefaultTdxQuoteVerifier, GatewayAttestationFetchOptions, ModelAttestationVerifiers,
    NrasGpuEvidenceVerifier,
};

fn images(app_compose: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(app_compose).unwrap_or_default();
    v["docker_compose_file"]
        .as_str()
        .unwrap_or("")
        .lines()
        .filter(|l| l.trim_start().starts_with("image:"))
        .map(|l| l.trim().to_string())
        .collect()
}

#[tokio::main]
async fn main() {
    let model = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Qwen/Qwen3.8-27B".into());
    let client = AttestationClient::new(None);
    let tdx = DefaultTdxQuoteVerifier::default();
    let gpu = NrasGpuEvidenceVerifier::default();
    let g = client
        .fetch_gateway_attestation(GatewayAttestationFetchOptions::default())
        .await
        .expect("gateway evidence");
    let gv = verify_gateway_attestation(
        &g.attestation,
        &g.client_binding,
        None,
        AttestationVerifiers {
            tdx_quote: Some(&tdx),
            deployment: None,
        },
    )
    .await
    .expect("gateway verifies");
    println!(
        "gateway compose manager: {}",
        gv.evidence.deployment.compose_manager.is_some()
    );
    let mut gp = std::collections::BTreeMap::new();
    for (img, repo) in [
        ("nearaidev/cloud-api", "nearai/cloud-api"),
        ("nearaidev/cvm-ingress", "nearai/cvm-ingress"),
    ] {
        let mut one = std::collections::BTreeMap::new();
        one.insert(
            img.to_string(),
            nearai_inference_sdk::ImageProvenancePolicy::new(
                repo.into(),
                ".github/workflows/build.yml".into(),
            ),
        );
        println!(
            "gateway provenance {img}: {:?}",
            nearai_inference_sdk::verify_deployment_image_provenance(
                &gv.evidence.deployment.app_compose,
                &one,
                None
            )
            .await
        );
        gp.extend(one);
    }
    for i in images(&gv.evidence.deployment.app_compose) {
        println!("gateway {i}");
    }
    let m = client
        .fetch_model_attestations(&model, None, None)
        .await
        .expect("model evidence");
    for a in &m.attestations {
        let v = verify_model_attestation(
            a,
            &m.client_binding,
            None,
            ModelAttestationVerifiers {
                tdx_quote: Some(&tdx),
                gpu_evidence: Some(&gpu),
                ..Default::default()
            },
        )
        .await
        .expect("model verifies");
        println!(
            "model compose manager: {}",
            v.evidence.deployment.compose_manager.is_some()
        );
        if let Some(cm) = &v.evidence.deployment.compose_manager {
            for a in &cm.actions {
                println!(
                    "  action {} file={:?} image={:?} services={:?}",
                    a.action, a.file, a.image, a.services
                );
            }
        }
        let mut mp = std::collections::BTreeMap::new();
        mp.insert(
            "nearaidev/vllm-proxy-rs".to_string(),
            nearai_inference_sdk::ImageProvenancePolicy::new(
                "nearai/inference-proxy".into(),
                ".github/workflows/build.yml".into(),
            ),
        );
        let manager = nearai_inference_sdk::ComposeManagerImagePolicy {
            image_repository: "nearaidev/compose-manager".into(),
            build: nearai_inference_sdk::ImageProvenancePolicy::new(
                "nearai/compose-manager".into(),
                ".github/workflows/build.yml".into(),
            ),
            signer_identity: None,
        };
        println!(
            "model provenance: {:?}",
            nearai_inference_sdk::verify_compose_manager_deployment_image_provenance(
                &v.evidence.deployment,
                &mp,
                &manager,
                Default::default()
            )
            .await
        );
        for i in images(&v.evidence.deployment.app_compose) {
            println!("model {i}");
        }
    }
}
