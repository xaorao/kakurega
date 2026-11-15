//! A capsule from a real review that ran on NEAR AI Cloud's confidential hardware (`Qwen/Qwen3.8-27B`), kept as a fixture so
//! the live run can be checked again with no account and no network. The same capsule is registered on NEAR testnet at
//! `kakurega-registry-c68af3.testnet` for order 1, which required a hardware record made after the order opened; the
//! values below were read from that registry. That registry was built on the developer's computer; the reproducibly built
//! one is `kakurega-registry-1ba6b2.testnet`, where the public sample below is registered for paid order 0.

use kk_evidence::capsule::{sha256_hex, verify, VerifyOptions};
use kk_evidence::registry::receipts_digest;

const CAPSULE: &[u8] = include_bytes!("../../../fixtures/near-live/review.kkg");
/// The model signing key NEAR's own verifier recorded at the registry after checking its Intel TDX quote.
const NEAR_MODEL_KEY: &str = "89154a43a47684742c3d1753252bb6f49f61da9d8c84fc9b1def2df130cb2d93";
/// Created 2026-11-13 03:08:06 UTC; checked as of a minute later, since a capsule expires a day after it is made.
const CHECKED_AT: i64 = 1_794_539_346;

#[test]
fn nl01_a_real_near_review_capsule_verifies_offline_with_every_answer_signed_by_the_attested_model_key(
) {
    let r = verify(
        CAPSULE,
        &VerifyOptions {
            pinned_keys: None,
            now: CHECKED_AT,
            expected_recipient: None,
        },
    );
    assert!(r.valid, "{:?}", r.reason);
    assert_eq!(r.route.as_deref(), Some("verified_tee"));
    assert_eq!(r.receipts.len(), 20);
    for rc in &r.receipts {
        assert_eq!(
            (rc.signer.as_str(), rc.model.as_str()),
            (NEAR_MODEL_KEY, "Qwen/Qwen3.8-27B")
        );
    }
    // the values the registry on NEAR testnet holds for this capsule
    assert_eq!(
        sha256_hex(CAPSULE),
        "2d2f9450a88fab653909a09c3c1a992929e308a0120feb5380a13ea381b99f8b"
    );
    assert_eq!(
        r.signer_public
            .map(|k| k.iter().map(|b| format!("{b:02x}")).collect::<String>()),
        Some("d48949fd672796dd58880341608d6a2fc26e28ade3f323a24792226a324a43ee".into())
    );
    assert_eq!(
        receipts_digest(&r.receipts),
        "50478a39a3f00251f96b254d87b43cbbf9589e47ddd06faa3751d8d4ee6028aa",
        "every one of the 20 receipts is the one the contract verified"
    );
}

#[test]
fn nl02_one_changed_byte_in_a_model_answer_digest_breaks_it() {
    let text = String::from_utf8(CAPSULE.to_vec()).unwrap();
    // the capsule's payload is base64url, so a changed letter in it changes the signed bytes
    let at = text.find("\"payload\":\"").unwrap() + 40;
    let mut bytes = text.into_bytes();
    bytes[at] = if bytes[at] == b'A' { b'B' } else { b'A' };
    let r = verify(
        &bytes,
        &VerifyOptions {
            pinned_keys: None,
            now: CHECKED_AT,
            expected_recipient: None,
        },
    );
    assert!(!r.valid);
}

/// The public sample on the site: a later real review, made to stay valid for two years, in the capsule form whose release
/// names its claims. It is registered on the reproducibly built registry `kakurega-registry-1ba6b2.testnet` for order 0,
/// which required a hardware record made after it opened; the values below were read from that registry.
const SAMPLE: &[u8] = include_bytes!("../../../fixtures/near-live/sample.kkg");
/// Created at 1_794_695_293 (Unix time) and valid for two years; checked a minute after it was made, so the test does not
/// depend on the day it runs.
const SAMPLE_CHECKED_AT: i64 = 1_794_695_353;

#[test]
fn nl03_the_public_sample_verifies_offline_names_its_claims_and_matches_what_the_reproducible_registry_holds(
) {
    let r = verify(
        SAMPLE,
        &VerifyOptions {
            pinned_keys: None,
            now: SAMPLE_CHECKED_AT,
            expected_recipient: None,
        },
    );
    assert!(r.valid, "{:?}", r.reason);
    assert_eq!(r.route.as_deref(), Some("verified_tee"));
    assert_eq!(r.receipts.len(), 20);
    assert!(r
        .receipts
        .iter()
        .all(|rc| rc.signer == NEAR_MODEL_KEY && rc.model == "Qwen/Qwen3.8-27B"));
    assert!(
        r.established
            .iter()
            .any(|e| e.contains("exactly those are here")),
        "the release names its claims, so a claim taken out would be seen"
    );
    assert_eq!(r.claims.len(), 2);
    assert_eq!(
        sha256_hex(SAMPLE),
        "71cb95bf7ca3ad984ff889c403bca5a83217533718aa95d747f0c4d604520aee"
    );
    assert_eq!(
        r.signer_public
            .map(|k| k.iter().map(|b| format!("{b:02x}")).collect::<String>()),
        Some("f3e907329c0dafea7ee8f7fbf009ae16e410159f95466e2b7fd2a20786e36039".into())
    );
    assert_eq!(
        receipts_digest(&r.receipts),
        "1c880d21a6311c452e75b252ca205e81295ffd340803f3608f13428c10c6093b",
        "every one of the 20 receipts is the one the contract verified"
    );
}
