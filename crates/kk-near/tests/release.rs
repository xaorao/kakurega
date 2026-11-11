use kk_near::release::{classify, refuses, Part, ReleaseCheck, ReleaseRule};
use kk_near::sdk::{
    DeploymentImagesFailureReason as Images, ImageProvenanceFailureReason as Build,
    VerificationError,
};

fn build_failed(reasons: Vec<Build>) -> Result<(), VerificationError> {
    Err(VerificationError::ImageProvenanceVerificationFailed {
        digest: "sha256:aa".into(),
        reasons,
    })
}

fn images(reason: Images) -> Result<(), VerificationError> {
    Err(VerificationError::DeploymentImagesInvalid {
        reason,
        image_repository: Some("nearaidev/vllm-proxy-rs".into()),
        service: None,
    })
}

fn mismatch(c: &ReleaseCheck) -> bool {
    matches!(c, ReleaseCheck::Mismatch(_))
}

fn open(c: &ReleaseCheck) -> bool {
    matches!(c, ReleaseCheck::NotEstablished(_))
}

#[test]
fn rl01_a_passing_check_is_verified_for_both_parts() {
    for part in [Part::Gateway, Part::Model] {
        assert_eq!(classify(part, &Ok(())), ReleaseCheck::Verified);
    }
}

#[test]
fn rl02_an_image_built_elsewhere_or_with_no_record_is_a_mismatch() {
    for reason in [
        Build::NoAttestations,
        Build::InvalidBundle,
        Build::UntrustedIdentity,
        Build::InvalidStatement,
        Build::DigestMismatch,
        Build::SourceMismatch,
        Build::CommitMismatch,
    ] {
        for part in [Part::Gateway, Part::Model] {
            let c = classify(part, &build_failed(vec![reason]));
            assert!(mismatch(&c), "{reason:?} {part:?} gave {c:?}");
        }
    }
    // one real failure next to an unavailable trust root is still a failure
    let c = classify(
        Part::Gateway,
        &build_failed(vec![Build::TrustRootUnavailable, Build::UntrustedIdentity]),
    );
    assert!(mismatch(&c), "{c:?}");
    // an empty reason list proves nothing in the image's favour
    assert!(mismatch(&classify(Part::Gateway, &build_failed(vec![]))));
}

#[test]
fn rl09_no_build_record_in_the_named_repository_is_a_mismatch_and_an_outage_is_not() {
    use kk_near::sdk::{ApiError, ApiResource};
    let request = |status| {
        Err(VerificationError::ImageProvenanceRequestFailed {
            image_repository: "nearaidev/cloud-api".into(),
            digest: "sha256:aa".into(),
            source: Box::new(ApiError::HttpStatus {
                resource: ApiResource::ImageProvenance,
                status,
            }),
        })
    };
    for part in [Part::Gateway, Part::Model] {
        assert!(mismatch(&classify(part, &request(404))), "{part:?}");
        for status in [403, 429, 500, 502, 503] {
            assert!(open(&classify(part, &request(status))), "{part:?} {status}");
        }
    }
    let unparsed = Err(VerificationError::ImageProvenanceRequestFailed {
        image_repository: "nearaidev/cloud-api".into(),
        digest: "sha256:aa".into(),
        source: Box::new(ApiError::InvalidJson {
            resource: ApiResource::ImageProvenance,
        }),
    });
    assert!(open(&classify(Part::Gateway, &unparsed)));
}

#[test]
fn rl03_an_unavailable_trust_root_alone_leaves_the_question_open() {
    let c = classify(
        Part::Model,
        &build_failed(vec![Build::TrustRootUnavailable]),
    );
    assert!(open(&c), "{c:?}");
}

#[test]
fn rl04_a_missing_image_is_open_for_a_model_and_a_mismatch_for_the_gateway() {
    for reason in [Images::ImageMissing, Images::ServiceMissing] {
        assert!(open(&classify(Part::Model, &images(reason))), "{reason:?}");
        assert!(
            mismatch(&classify(Part::Gateway, &images(reason))),
            "{reason:?}"
        );
    }
}

#[test]
fn rl05_an_unpinned_or_unreadable_deployment_is_a_mismatch_everywhere() {
    for reason in [
        Images::UnresolvedImage,
        Images::ImageNotPinned,
        Images::InvalidDockerCompose,
        Images::InvalidAppCompose,
    ] {
        for part in [Part::Gateway, Part::Model] {
            let c = classify(part, &images(reason));
            assert!(mismatch(&c), "{reason:?} {part:?} gave {c:?}");
        }
    }
}

#[test]
fn rl06_deployment_record_gaps_are_open_and_contradictions_are_mismatches() {
    let cm = |reason| Err(VerificationError::ComposeManagerDeploymentInvalid { reason });
    for reason in [
        "attestation_missing",
        "compose_up_missing",
        "manager_image_missing",
    ] {
        assert!(open(&classify(Part::Model, &cm(reason))), "{reason}");
    }
    for reason in [
        "manager_image_mismatch",
        "invalid_file_reference",
        "anything_new",
    ] {
        assert!(mismatch(&classify(Part::Model, &cm(reason))), "{reason}");
    }
    let changed = Err(VerificationError::ComposeFileHashMismatch {
        repository: "nearai/cvm-compose-files".into(),
        commit: "c".into(),
        file: "prod/x.yaml".into(),
    });
    assert!(mismatch(&classify(Part::Model, &changed)));
    let unreachable = Err(VerificationError::ComposeFileRequestFailed {
        repository: "nearai/cvm-compose-files".into(),
        commit: "c".into(),
        file: "prod/x.yaml".into(),
        status: Some(503),
        retryable: true,
    });
    assert!(open(&classify(Part::Model, &unreachable)));
}

#[test]
fn rl07_the_rule_decides_what_refuses_a_deployment() {
    let verified = ReleaseCheck::Verified;
    let unknown = ReleaseCheck::NotEstablished("x".into());
    let wrong = ReleaseCheck::Mismatch("x".into());
    for c in [&verified, &unknown, &wrong] {
        assert!(!refuses(ReleaseRule::Off, c));
    }
    assert!(!refuses(ReleaseRule::RefuseMismatch, &verified));
    assert!(!refuses(ReleaseRule::RefuseMismatch, &unknown));
    assert!(refuses(ReleaseRule::RefuseMismatch, &wrong));
    assert!(!refuses(ReleaseRule::Require, &verified));
    assert!(refuses(ReleaseRule::Require, &unknown));
    assert!(refuses(ReleaseRule::Require, &wrong));
    assert_eq!(ReleaseRule::default(), ReleaseRule::RefuseMismatch);
}

#[test]
fn rl08_a_result_names_itself_in_one_word_and_keeps_its_reason() {
    assert_eq!(ReleaseCheck::Verified.word(), "verified");
    assert_eq!(ReleaseCheck::Verified.reason(), None);
    let c = ReleaseCheck::Mismatch("built elsewhere".into());
    assert_eq!(
        (c.word(), c.reason()),
        ("mismatch", Some("built elsewhere"))
    );
    let c = ReleaseCheck::NotEstablished("offline".into());
    assert_eq!((c.word(), c.reason()), ("not_established", Some("offline")));
}

#[tokio::test]
#[ignore]
async fn live_the_real_gateway_images_were_built_by_nears_public_workflows() {
    let g = kk_near::attest_gateway(&kk_near::NearPolicy::default())
        .await
        .expect("the gateway verifies under the default rule");
    assert_eq!(g.release, Some(ReleaseCheck::Verified), "{:?}", g.release);
}

#[tokio::test]
#[ignore]
async fn live_a_gateway_held_to_the_wrong_workflow_is_refused() {
    use kk_near::sdk::{
        verify_deployment_image_provenance, verify_gateway_attestation, AttestationClient,
        AttestationVerifiers, DefaultTdxQuoteVerifier, GatewayAttestationFetchOptions,
        ImageProvenancePolicy,
    };
    let client = AttestationClient::new(None);
    let tdx = DefaultTdxQuoteVerifier::default();
    let f = client
        .fetch_gateway_attestation(GatewayAttestationFetchOptions::default())
        .await
        .expect("gateway evidence");
    let v = verify_gateway_attestation(
        &f.attestation,
        &f.client_binding,
        None,
        AttestationVerifiers {
            tdx_quote: Some(&tdx),
            deployment: None,
        },
    )
    .await
    .expect("gateway verifies");
    // the right repository with a workflow that did not build the image, and a repository that never built it
    for (repository, workflow) in [
        ("nearai/cloud-api", ".github/workflows/test.yml"),
        ("nearai/inference-proxy", ".github/workflows/build.yml"),
    ] {
        let wrong = std::collections::BTreeMap::from([(
            "nearaidev/cloud-api".to_owned(),
            ImageProvenancePolicy::new(repository.into(), workflow.into()),
        )]);
        let result =
            verify_deployment_image_provenance(&v.evidence.deployment.app_compose, &wrong, None)
                .await;
        let found = classify(Part::Gateway, &result);
        assert!(mismatch(&found), "{repository} {workflow}: {found:?}");
        assert!(refuses(ReleaseRule::RefuseMismatch, &found));
    }
}

#[tokio::test]
#[ignore]
async fn live_a_real_model_deployment_is_never_a_mismatch() {
    let r = kk_near::attest_model("Qwen/Qwen3.8-27B", &kk_near::NearPolicy::default())
        .await
        .expect("the model verifies under the default rule");
    for a in &r {
        let found = a.release.clone().expect("the check ran");
        println!("model release: {found:?}");
        assert!(!mismatch(&found), "{found:?}");
    }
}

#[tokio::test]
async fn rl10_the_verifier_refuses_by_its_rule_and_records_what_it_found() {
    use kk_near::release::{latest, ReleaseVerifier};
    use kk_near::sdk::{DeploymentVerifier, MeasuredDeployment};
    let unreadable = MeasuredDeployment {
        compose_manager: None,
        app_compose: "not json".into(),
        runtime_measurements: Default::default(),
    };
    // a gateway whose measured Compose cannot be read: a mismatch, refused under both active rules
    for rule in [ReleaseRule::RefuseMismatch, ReleaseRule::Require] {
        let (v, seen) = ReleaseVerifier::new(Part::Gateway, rule);
        assert!(latest(&seen).is_none());
        assert!(v.verify(&unreadable).await.is_err(), "{rule:?}");
        assert!(mismatch(&latest(&seen).expect("recorded")));
    }
    // a model with no deployment record: the question stays open, refused only when the rule requires an answer
    let (v, seen) = ReleaseVerifier::new(Part::Model, ReleaseRule::RefuseMismatch);
    assert!(v.verify(&unreadable).await.is_ok());
    assert!(open(&latest(&seen).expect("recorded")));
    let (v, seen) = ReleaseVerifier::new(Part::Model, ReleaseRule::Require);
    assert!(v.verify(&unreadable).await.is_err());
    assert!(open(&latest(&seen).expect("recorded")));
    // the record is bounded
    let (v, seen) = ReleaseVerifier::new(Part::Model, ReleaseRule::RefuseMismatch);
    for _ in 0..70 {
        let _ = v.verify(&unreadable).await;
    }
    assert_eq!(seen.lock().expect("lock").len(), 64);
}
