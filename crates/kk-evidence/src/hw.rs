use crate::b64;
use crate::dsse::pae;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

pub const ALG: &str = "p256-se";
pub const PUBLIC_LEN: usize = 65;

pub type P256Public = [u8; PUBLIC_LEN];

pub fn key_id(public: &P256Public) -> String {
    let mut s = String::from("p256:");
    for b in Sha256::digest(public) {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub fn valid_public(public: &[u8]) -> Option<P256Public> {
    let arr: P256Public = public.try_into().ok()?;
    VerifyingKey::from_sec1_bytes(&arr).ok()?;
    Some(arr)
}

pub fn envelope(
    payload_type: &str,
    payload: &[u8],
    public: &P256Public,
    signature: &[u8; 64],
) -> String {
    format!(
        "{{\"alg\":\"{ALG}\",\"payload\":\"{}\",\"public\":\"{}\",\"sig\":\"{}\",\"type\":\"{payload_type}\"}}",
        b64::encode(payload),
        b64::encode(public),
        b64::encode(signature)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwReject {
    NotAnEnvelope,
    WrongType,
    UnknownKey,
    BadSignature,
}

pub struct Parsed {
    pub payload: Vec<u8>,
    pub public: P256Public,
}

/// Checks the signature and that the signing key is one of `trusted`. The caller compares the payload.
pub fn verify(proof: &str, allowed_type: &str, trusted: &[P256Public]) -> Result<Parsed, HwReject> {
    let v: serde_like::Fields = serde_like::parse(proof).ok_or(HwReject::NotAnEnvelope)?;
    if v.alg != ALG {
        return Err(HwReject::NotAnEnvelope);
    }
    if v.kind != allowed_type {
        return Err(HwReject::WrongType);
    }
    let payload = b64::decode(&v.payload).map_err(|_| HwReject::NotAnEnvelope)?;
    let public: P256Public = b64::decode(&v.public)
        .ok()
        .and_then(|p| valid_public(&p))
        .ok_or(HwReject::NotAnEnvelope)?;
    let sig_raw: [u8; 64] = b64::decode(&v.sig)
        .ok()
        .and_then(|s| s.try_into().ok())
        .ok_or(HwReject::NotAnEnvelope)?;
    if !trusted.contains(&public) {
        return Err(HwReject::UnknownKey);
    }
    let key = VerifyingKey::from_sec1_bytes(&public).map_err(|_| HwReject::NotAnEnvelope)?;
    let sig = Signature::from_slice(&sig_raw).map_err(|_| HwReject::BadSignature)?;
    key.verify(&pae(&v.kind, &payload), &sig)
        .map_err(|_| HwReject::BadSignature)?;
    Ok(Parsed { payload, public })
}

mod serde_like {
    use crate::canon::{self, Value};

    pub struct Fields {
        pub alg: String,
        pub kind: String,
        pub payload: String,
        pub public: String,
        pub sig: String,
    }

    pub fn parse(text: &str) -> Option<Fields> {
        if text.len() > 16 * 1024 {
            return None;
        }
        let Ok(Value::Object(m)) = canon::parse_strict(text.as_bytes()) else {
            return None;
        };
        let get = |k: &str| match m.iter().find(|(n, _)| n == k) {
            Some((_, Value::Str(s))) => Some(s.clone()),
            _ => None,
        };
        if m.len() != 5 {
            return None;
        }
        Some(Fields {
            alg: get("alg")?,
            kind: get("type")?,
            payload: get("payload")?,
            public: get("public")?,
            sig: get("sig")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::signature::Signer;
    use p256::ecdsa::SigningKey;

    fn key(seed: u8) -> (SigningKey, P256Public) {
        let sk = SigningKey::from_slice(&[seed; 32]).unwrap();
        let public: P256Public = sk
            .verifying_key()
            .to_encoded_point(false)
            .as_bytes()
            .try_into()
            .unwrap();
        (sk, public)
    }

    fn make(sk: &SigningKey, public: &P256Public, kind: &str, payload: &[u8]) -> String {
        let sig: Signature = sk.sign(&pae(kind, payload));
        envelope(kind, payload, public, &sig.to_bytes().into())
    }

    #[test]
    fn hw01_a_p256_proof_verifies_only_for_a_trusted_key_the_right_type_and_untouched_bytes() {
        let (sk, pk) = key(3);
        let (_, other) = key(4);
        let proof = make(&sk, &pk, "t/x", b"payload");
        assert_eq!(verify(&proof, "t/x", &[pk]).unwrap().payload, b"payload");
        assert_eq!(
            verify(&proof, "t/x", &[other]).err(),
            Some(HwReject::UnknownKey)
        );
        assert_eq!(
            verify(&proof, "t/y", &[pk]).err(),
            Some(HwReject::WrongType)
        );
        assert_eq!(
            verify(
                &proof.replace(&b64::encode(b"payload"), &b64::encode(b"payloaD")),
                "t/x",
                &[pk]
            )
            .err(),
            Some(HwReject::BadSignature)
        );
        for junk in [
            "",
            "{}",
            "null",
            "[]",
            &"x".repeat(20000),
            r#"{"alg":"ed25519","payload":"","public":"","sig":"","type":"t/x"}"#,
        ] {
            assert!(verify(junk, "t/x", &[pk]).is_err());
        }
        assert!(
            valid_public(&[4u8; 65]).is_none(),
            "a point not on the curve is refused"
        );
        assert!(valid_public(&[0u8; 65]).is_none());
        assert_eq!(key_id(&pk).len(), 5 + 64);
    }

    #[test]
    #[ignore = "needs a proof made by spikes/enclave/kaku-enclave on a Mac; set KK_SE_PROOF to its output file"]
    fn hw02_a_proof_from_the_real_secure_enclave_helper_verifies_and_changes_do_not() {
        let text =
            std::fs::read_to_string(std::env::var("KK_SE_PROOF").expect("KK_SE_PROOF")).unwrap();
        let text = text.trim();
        let fields = serde_like::parse(text).unwrap();
        let public = valid_public(&b64::decode(&fields.public).unwrap()).unwrap();
        let ok = verify(text, &fields.kind, &[public]).unwrap();
        assert_eq!(ok.payload, b64::decode(&fields.payload).unwrap());
        let tampered = text.replace(&fields.payload, &b64::encode(b"other"));
        assert_eq!(
            verify(&tampered, &fields.kind, &[public]).err(),
            Some(HwReject::BadSignature)
        );
    }
}
