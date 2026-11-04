use kk_near::sdk::{
    DeploymentProvenanceStatus, GpuEvidenceStatus, MeasuredDeployment, RuntimeMeasurements,
    SigningAlgo, SigningIdentity, TcbStatus, VerifiedAttestationEvidence, VerifiedModelAttestation,
};
use kk_near::{judge, NearPolicy, Rejection};

fn verified(
    tcb: TcbStatus,
    gpu: GpuEvidenceStatus,
    compose: Option<&str>,
    os: Option<&str>,
) -> VerifiedModelAttestation {
    VerifiedModelAttestation {
        signing_public_key: None,
        evidence: VerifiedAttestationEvidence {
            signer: SigningIdentity {
                signing_algo: SigningAlgo::Ecdsa,
                signing_address: "0xabc".into(),
            },
            tcb_status: tcb,
            advisory_ids: vec![],
            deployment: MeasuredDeployment {
                compose_manager: None,
                app_compose: "{}".into(),
                runtime_measurements: RuntimeMeasurements {
                    os_image_hash: os.map(String::from),
                    compose_hash: compose.map(String::from),
                },
            },
            deployment_provenance: DeploymentProvenanceStatus::NotChecked,
        },
        gpu_evidence: gpu,
    }
}

#[test]
fn the_default_policy_accepts_only_an_up_to_date_gpu_verified_deployment() {
    let p = NearPolicy::default();
    let ok = judge(
        "m",
        "n",
        &verified(
            TcbStatus::UpToDate,
            GpuEvidenceStatus::Verified,
            Some("c1"),
            Some("o1"),
        ),
        &p,
    )
    .unwrap();
    assert!(
        ok.gpu_verified && ok.compose_hash.as_deref() == Some("c1") && ok.signer_address == "0xabc"
    );
    assert_eq!(
        judge(
            "m",
            "n",
            &verified(
                TcbStatus::OutOfDate,
                GpuEvidenceStatus::Verified,
                None,
                None
            ),
            &p
        ),
        Err(Rejection::TcbNotAccepted("OutOfDate".into()))
    );
    assert_eq!(
        judge(
            "m",
            "n",
            &verified(
                TcbStatus::SwHardeningNeeded,
                GpuEvidenceStatus::Verified,
                None,
                None
            ),
            &p
        ),
        Err(Rejection::TcbNotAccepted("SwHardeningNeeded".into()))
    );
    assert_eq!(
        judge(
            "m",
            "n",
            &verified(
                TcbStatus::UpToDate,
                GpuEvidenceStatus::NotProvided,
                None,
                None
            ),
            &p
        ),
        Err(Rejection::GpuEvidenceMissing)
    );
}

#[test]
fn pinned_measurements_must_match_and_must_exist() {
    let p = NearPolicy {
        allowed_compose_hashes: Some(vec!["c1".into()]),
        allowed_os_image_hashes: Some(vec!["o1".into()]),
        ..NearPolicy::default()
    };
    assert!(judge(
        "m",
        "n",
        &verified(
            TcbStatus::UpToDate,
            GpuEvidenceStatus::Verified,
            Some("c1"),
            Some("o1")
        ),
        &p
    )
    .is_ok());
    assert_eq!(
        judge(
            "m",
            "n",
            &verified(
                TcbStatus::UpToDate,
                GpuEvidenceStatus::Verified,
                Some("c2"),
                Some("o1")
            ),
            &p
        ),
        Err(Rejection::ComposeHashNotAllowed("c2".into()))
    );
    assert_eq!(
        judge(
            "m",
            "n",
            &verified(
                TcbStatus::UpToDate,
                GpuEvidenceStatus::Verified,
                Some("c1"),
                Some("o2")
            ),
            &p
        ),
        Err(Rejection::OsImageNotAllowed("o2".into()))
    );
    assert_eq!(
        judge(
            "m",
            "n",
            &verified(
                TcbStatus::UpToDate,
                GpuEvidenceStatus::Verified,
                None,
                Some("o1")
            ),
            &p
        ),
        Err(Rejection::MeasurementsMissing)
    );
}

#[test]
fn a_relaxed_policy_can_accept_a_missing_gpu_report_but_the_default_never_does() {
    let relaxed = NearPolicy {
        require_gpu: false,
        accepted_tcb: vec![TcbStatus::UpToDate, TcbStatus::SwHardeningNeeded],
        ..NearPolicy::default()
    };
    let r = judge(
        "m",
        "n",
        &verified(
            TcbStatus::SwHardeningNeeded,
            GpuEvidenceStatus::NotProvided,
            None,
            None,
        ),
        &relaxed,
    )
    .unwrap();
    assert!(
        !r.gpu_verified,
        "the evidence record says exactly what was and was not verified"
    );
}

#[tokio::test]
#[ignore]
async fn live_a_real_public_attestation_verifies_and_passes_the_policy() {
    let r = kk_near::attest_model("Qwen/Qwen3.8-27B", &NearPolicy::default())
        .await
        .expect("verified");
    println!("{r:#?}");
    assert!(
        !r.is_empty()
            && r.iter()
                .all(|a| a.gpu_verified && a.tcb_status == "UpToDate")
    );
}

#[tokio::test]
#[ignore]
async fn live_tampered_or_replayed_evidence_is_rejected() {
    use kk_near::sdk::{
        verify_model_attestation, AttestationClient, DefaultTdxQuoteVerifier,
        ModelAttestationVerifiers, ModelClientBinding,
    };
    let client = AttestationClient::new(None);
    let fetched = client
        .fetch_model_attestations("Qwen/Qwen3.8-27B", None, None)
        .await
        .expect("fetch");
    let tdx = DefaultTdxQuoteVerifier::default();
    let verifiers = || ModelAttestationVerifiers {
        tdx_quote: Some(&tdx),
        ..Default::default()
    };
    let good = &fetched.attestations[0];
    assert!(
        verify_model_attestation(good, &fetched.client_binding, None, verifiers())
            .await
            .is_ok(),
        "control: untouched evidence verifies"
    );
    let replay = ModelClientBinding {
        nonce: "00".repeat(32),
    };
    let r = verify_model_attestation(good, &replay, None, verifiers()).await;
    assert!(r.is_err(), "evidence for another nonce is a replay: {r:?}");
    let mut flipped = good.clone();
    let mid = flipped.evidence.intel_quote.len() / 2;
    let c = flipped.evidence.intel_quote.as_bytes()[mid];
    let swap = if c == b'0' { "1" } else { "0" };
    flipped
        .evidence
        .intel_quote
        .replace_range(mid..mid + 1, swap);
    let r = verify_model_attestation(&flipped, &fetched.client_binding, None, verifiers()).await;
    assert!(
        r.is_err(),
        "a quote with one changed digit must not verify: {r:?}"
    );
    let mut other_signer = good.clone();
    other_signer.evidence.signer.signing_address =
        "0x0000000000000000000000000000000000000001".into();
    let r =
        verify_model_attestation(&other_signer, &fetched.client_binding, None, verifiers()).await;
    assert!(
        r.is_err(),
        "a different claimed signer must not verify: {r:?}"
    );
    let mut other_compose = good.clone();
    other_compose.evidence.app_compose.push(' ');
    let r =
        verify_model_attestation(&other_compose, &fetched.client_binding, None, verifiers()).await;
    assert!(
        r.is_err(),
        "a changed compose file must not match the measured hash: {r:?}"
    );
}

#[tokio::test]
#[ignore]
async fn live_the_gateway_proves_it_runs_in_a_tee_and_that_this_tls_connection_ends_inside_it() {
    let g = kk_near::attest_gateway(&NearPolicy::default())
        .await
        .expect("verified");
    println!("{g:#?}");
    assert_eq!(g.tcb_status, "UpToDate");
    assert!(!g.signer_address.is_empty() && !g.nonce.is_empty());
    assert!(
        g.tls_bound,
        "the quote binds the TLS certificate of this very connection"
    );
}

#[tokio::test]
#[ignore]
async fn live_gateway_evidence_replayed_or_pointed_at_another_connection_is_rejected() {
    use kk_near::sdk::{
        verify_gateway_attestation, AttestationClient, AttestationPolicy, AttestationVerifiers,
        DefaultTdxQuoteVerifier, GatewayAttestationFetchOptions, GatewayClientBinding,
    };
    let fetched = AttestationClient::new(None)
        .fetch_gateway_attestation(GatewayAttestationFetchOptions::default())
        .await
        .expect("fetch");
    let tdx = DefaultTdxQuoteVerifier::default();
    let policy = AttestationPolicy {
        accepted_tcb_statuses: None,
    };
    let run = |binding: GatewayClientBinding| {
        let a = fetched.attestation.clone();
        let tdx = &tdx;
        let policy = &policy;
        async move {
            verify_gateway_attestation(
                &a,
                &binding,
                Some(policy),
                AttestationVerifiers {
                    tdx_quote: Some(tdx),
                    deployment: None,
                },
            )
            .await
        }
    };
    assert!(
        run(fetched.client_binding.clone()).await.is_ok(),
        "control: the untouched evidence verifies"
    );
    let replay = GatewayClientBinding {
        nonce: "0".repeat(64),
        ..fetched.client_binding.clone()
    };
    assert!(
        run(replay).await.is_err(),
        "evidence answered to another challenge is a replay"
    );
    let other_peer = GatewayClientBinding {
        spki_fingerprint: Some("f".repeat(64)),
        ..fetched.client_binding.clone()
    };
    assert!(
        run(other_peer).await.is_err(),
        "a different TLS certificate than the one the quote binds is a man in the middle"
    );
}
