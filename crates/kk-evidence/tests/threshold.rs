use kk_evidence::dsse::{self, Envelope, ThresholdError, MAX_SIGNATURES};
use kk_evidence::keys::{KeyId, Signer, TrustedKeys};

const TYPE: &str = "application/vnd.kakurega.test.v1+json";
const PAYLOAD: &[u8] = br#"{"a":1}"#;

fn signer(n: u8) -> Signer {
    Signer::from_seed([n; 32])
}

fn trusting(signers: &[&Signer]) -> TrustedKeys {
    let mut t = TrustedKeys::new();
    for s in signers {
        t.add(&s.public()).unwrap();
    }
    t
}

fn signed_by(signers: &[&Signer]) -> Envelope {
    let mut env = dsse::sign(TYPE, PAYLOAD, signers[0]);
    for s in &signers[1..] {
        env.signatures
            .push((s.key_id(), s.sign(&dsse::pae(TYPE, PAYLOAD))));
    }
    env
}

#[test]
fn th01_two_of_three_distinct_trusted_keys_pass_and_one_does_not() {
    let (a, b, c) = (signer(1), signer(2), signer(3));
    let trusted = trusting(&[&a, &b, &c]);
    let ok = dsse::verify_threshold(&signed_by(&[&a, &c]), &trusted, 2, &[TYPE]).unwrap();
    let mut want = vec![a.key_id(), c.key_id()];
    want.sort();
    assert_eq!(ok, want);
    assert_eq!(
        dsse::verify_threshold(&signed_by(&[&b]), &trusted, 2, &[TYPE]),
        Err(ThresholdError::ThresholdNotMet {
            valid: 1,
            needed: 2
        })
    );
    // asking for more than the keys can ever give
    assert_eq!(
        dsse::verify_threshold(&signed_by(&[&a, &b, &c]), &trusted, 4, &[TYPE]),
        Err(ThresholdError::ThresholdNotMet {
            valid: 3,
            needed: 4
        })
    );
}

#[test]
fn th02_one_key_can_never_count_as_two_signers() {
    let (a, b) = (signer(1), signer(2));
    let trusted = trusting(&[&a, &b]);
    let mut env = signed_by(&[&a]);
    env.signatures.push(env.signatures[0].clone());
    assert_eq!(
        dsse::verify_threshold(&env, &trusted, 2, &[TYPE]),
        Err(ThresholdError::DuplicateSignature)
    );
    // the same holds with a threshold of one: a repeated id is a malformed envelope, not a bonus
    assert_eq!(
        dsse::verify_threshold(&env, &trusted, 1, &[TYPE]),
        Err(ThresholdError::DuplicateSignature)
    );
}

#[test]
fn th03_signers_this_installation_does_not_trust_are_ignored_and_never_help() {
    let (a, b, stranger) = (signer(1), signer(2), signer(9));
    let trusted = trusting(&[&a, &b]);
    // a stranger's valid signature does not count towards the threshold
    assert_eq!(
        dsse::verify_threshold(&signed_by(&[&a, &stranger]), &trusted, 2, &[TYPE]),
        Err(ThresholdError::ThresholdNotMet {
            valid: 1,
            needed: 2
        })
    );
    // and does not break an envelope that already has enough
    assert!(dsse::verify_threshold(&signed_by(&[&a, &b, &stranger]), &trusted, 2, &[TYPE]).is_ok());
}

#[test]
fn th04_a_bad_signature_from_a_trusted_key_fails_the_whole_envelope() {
    let (a, b) = (signer(1), signer(2));
    let trusted = trusting(&[&a, &b]);
    // b signed different bytes: the envelope was altered after signing
    let mut env = signed_by(&[&a]);
    env.signatures
        .push((b.key_id(), b.sign(&dsse::pae(TYPE, br#"{"a":2}"#))));
    assert_eq!(
        dsse::verify_threshold(&env, &trusted, 1, &[TYPE]),
        Err(ThresholdError::BadSignature)
    );
    // a signature made by a under b's key id
    let mut env = signed_by(&[&a]);
    env.signatures
        .push((b.key_id(), a.sign(&dsse::pae(TYPE, PAYLOAD))));
    assert_eq!(
        dsse::verify_threshold(&env, &trusted, 1, &[TYPE]),
        Err(ThresholdError::BadSignature)
    );
    // a signature that is signed for another payload type
    let mut env = signed_by(&[&a]);
    env.signatures.push((
        b.key_id(),
        b.sign(&dsse::pae("application/vnd.other+json", PAYLOAD)),
    ));
    assert_eq!(
        dsse::verify_threshold(&env, &trusted, 1, &[TYPE]),
        Err(ThresholdError::BadSignature)
    );
}

#[test]
fn th05_a_threshold_of_zero_a_wrong_type_a_loose_payload_and_a_flood_are_refused() {
    let a = signer(1);
    let trusted = trusting(&[&a]);
    let env = signed_by(&[&a]);
    assert_eq!(
        dsse::verify_threshold(&env, &trusted, 0, &[TYPE]),
        Err(ThresholdError::BadThreshold),
        "nobody signed is not enough, and zero would accept that"
    );
    assert_eq!(
        dsse::verify_threshold(&env, &trusted, 1, &["application/other"]),
        Err(ThresholdError::UnknownPayloadType)
    );
    // a payload that is not in canonical form
    let loose = dsse::sign(TYPE, br#"{ "a": 1 }"#, &a);
    assert_eq!(
        dsse::verify_threshold(&loose, &trusted, 1, &[TYPE]),
        Err(ThresholdError::PayloadNotCanonical)
    );
    let mut flood = signed_by(&[&a]);
    for i in 0..MAX_SIGNATURES {
        let other = signer(100 + i as u8);
        flood
            .signatures
            .push((other.key_id(), other.sign(&dsse::pae(TYPE, PAYLOAD))));
    }
    assert_eq!(
        dsse::verify_threshold(&flood, &trusted, 1, &[TYPE]),
        Err(ThresholdError::TooManySignatures)
    );
}

#[test]
fn th06_an_envelope_with_no_signature_never_passes_and_the_single_signature_rule_is_unchanged() {
    let a = signer(1);
    let trusted = trusting(&[&a]);
    let mut none = signed_by(&[&a]);
    none.signatures.clear();
    assert_eq!(
        dsse::verify_threshold(&none, &trusted, 1, &[TYPE]),
        Err(ThresholdError::ThresholdNotMet {
            valid: 0,
            needed: 1
        })
    );
    // the existing one-signature check is untouched
    let two = signed_by(&[&a, &signer(2)]);
    assert!(dsse::verify(&two, &trusted, &[TYPE]).is_err());
    assert!(dsse::verify(&signed_by(&[&a]), &trusted, &[TYPE]).is_ok());
    let _ = KeyId::of(&a.public());
}
