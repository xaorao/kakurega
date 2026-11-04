use nearai_inference_sdk::{
    verify_model_attestation, AttestationClient, DefaultTdxQuoteVerifier, GpuEvidenceRequirement,
    ModelAttestationPolicy, ModelAttestationVerifiers, NrasGpuEvidenceVerifier,
};

#[tokio::main]
async fn main() {
    let model = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Qwen/Qwen3.8-27B".into());
    let client = AttestationClient::new(None);
    let fetched = client
        .fetch_model_attestations(&model, None, None)
        .await
        .expect("fetch");
    println!(
        "model {model}: {} attestation(s), nonce {}",
        fetched.attestations.len(),
        fetched.client_binding.nonce
    );
    let tdx = DefaultTdxQuoteVerifier::default();
    let gpu = NrasGpuEvidenceVerifier::default();
    for (i, att) in fetched.attestations.iter().enumerate() {
        let policy = ModelAttestationPolicy {
            accepted_tcb_statuses: None,
            gpu_evidence: GpuEvidenceRequirement::Required,
        };
        let result = verify_model_attestation(
            att,
            &fetched.client_binding,
            Some(&policy),
            ModelAttestationVerifiers {
                tdx_quote: Some(&tdx),
                gpu_evidence: Some(&gpu),
                ..Default::default()
            },
        )
        .await;
        match result {
            Ok(v) => println!(
                "#{i} VERIFIED signer {:?} tcb {:?} gpu {:?}",
                v.evidence.signer, v.evidence.tcb_status, v.gpu_evidence
            ),
            Err(e) => println!("#{i} REJECTED {e:?}"),
        }
    }
}
