use crate::b64;
use crate::canon::{self, Value};
use crate::keys::{KeyId, Signer, TrustedKeys};
use ed25519_dalek::Signature;

pub const MAX_ENVELOPE_BYTES: usize = 32 << 20;

pub fn pae(payload_type: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + payload_type.len() + 32);
    out.extend_from_slice(b"DSSEv1 ");
    out.extend_from_slice(payload_type.len().to_string().as_bytes());
    out.push(b' ');
    out.extend_from_slice(payload_type.as_bytes());
    out.push(b' ');
    out.extend_from_slice(payload.len().to_string().as_bytes());
    out.push(b' ');
    out.extend_from_slice(payload);
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    pub payload_type: String,
    pub payload: Vec<u8>,
    pub signatures: Vec<(KeyId, [u8; 64])>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeError {
    TooLarge,
    NotJson,
    WrongShape,
    BadBase64,
    BadSignatureLength,
    BadKeyId,
    NotCanonical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyError {
    UnknownPayloadType,
    WrongSignatureCount,
    UnknownKey,
    BadSignature,
    PayloadNotCanonical,
}

pub fn sign(payload_type: &str, canonical_payload: &[u8], signer: &Signer) -> Envelope {
    let sig = signer.sign(&pae(payload_type, canonical_payload));
    Envelope {
        payload_type: payload_type.to_string(),
        payload: canonical_payload.to_vec(),
        signatures: vec![(signer.key_id(), sig)],
    }
}

pub fn to_value(env: &Envelope) -> Value {
    Value::Object(vec![
        ("payload".into(), Value::Str(b64::encode(&env.payload))),
        ("payloadType".into(), Value::Str(env.payload_type.clone())),
        (
            "signatures".into(),
            Value::Array(
                env.signatures
                    .iter()
                    .map(|(id, sig)| {
                        Value::Object(vec![
                            ("keyid".into(), Value::Str(id.as_str().to_string())),
                            ("sig".into(), Value::Str(b64::encode(sig))),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])
}

pub fn encode(env: &Envelope) -> String {
    canon::serialize(&to_value(env))
}

fn field<'a>(members: &'a [(String, Value)], name: &str) -> Option<&'a Value> {
    members.iter().find(|(k, _)| k == name).map(|(_, v)| v)
}

pub fn from_value(v: &Value) -> Result<Envelope, EnvelopeError> {
    let Value::Object(members) = v else {
        return Err(EnvelopeError::WrongShape);
    };
    if members.len() != 3 {
        return Err(EnvelopeError::WrongShape);
    }
    let (Some(Value::Str(payload)), Some(Value::Str(payload_type)), Some(Value::Array(sigs))) = (
        field(members, "payload"),
        field(members, "payloadType"),
        field(members, "signatures"),
    ) else {
        return Err(EnvelopeError::WrongShape);
    };
    let payload = b64::decode(payload).map_err(|_| EnvelopeError::BadBase64)?;
    let mut signatures = Vec::new();
    for s in sigs {
        let Value::Object(m) = s else {
            return Err(EnvelopeError::WrongShape);
        };
        if m.len() != 2 {
            return Err(EnvelopeError::WrongShape);
        }
        let (Some(Value::Str(keyid)), Some(Value::Str(sig))) = (field(m, "keyid"), field(m, "sig"))
        else {
            return Err(EnvelopeError::WrongShape);
        };
        let keyid = KeyId::parse(keyid).ok_or(EnvelopeError::BadKeyId)?;
        let raw = b64::decode(sig).map_err(|_| EnvelopeError::BadBase64)?;
        let raw: [u8; 64] = raw
            .try_into()
            .map_err(|_| EnvelopeError::BadSignatureLength)?;
        signatures.push((keyid, raw));
    }
    Ok(Envelope {
        payload_type: payload_type.clone(),
        payload,
        signatures,
    })
}

pub fn decode(bytes: &[u8]) -> Result<Envelope, EnvelopeError> {
    if bytes.len() > MAX_ENVELOPE_BYTES {
        return Err(EnvelopeError::TooLarge);
    }
    let v = canon::parse_strict(bytes).map_err(|_| EnvelopeError::NotJson)?;
    if canon::serialize(&v).as_bytes() != bytes {
        return Err(EnvelopeError::NotCanonical);
    }
    from_value(&v)
}

pub struct Verified {
    pub key_id: KeyId,
    pub payload_type: String,
}

pub fn verify(
    env: &Envelope,
    trusted: &TrustedKeys,
    allowed_types: &[&str],
) -> Result<Verified, VerifyError> {
    if !allowed_types.contains(&env.payload_type.as_str()) {
        return Err(VerifyError::UnknownPayloadType);
    }
    if env.signatures.len() != 1 {
        return Err(VerifyError::WrongSignatureCount);
    }
    let (id, raw) = &env.signatures[0];
    let key = trusted.get(id).ok_or(VerifyError::UnknownKey)?;
    let sig = Signature::from_bytes(raw);
    key.verify_strict(&pae(&env.payload_type, &env.payload), &sig)
        .map_err(|_| VerifyError::BadSignature)?;
    let text = std::str::from_utf8(&env.payload).map_err(|_| VerifyError::PayloadNotCanonical)?;
    match canon::canonicalize(text.as_bytes()) {
        Ok(c) if c == text => {}
        _ => return Err(VerifyError::PayloadNotCanonical),
    }
    Ok(Verified {
        key_id: id.clone(),
        payload_type: env.payload_type.clone(),
    })
}

/// Why a threshold check refused an envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThresholdError {
    UnknownPayloadType,
    /// A threshold below one would accept an envelope nobody signed.
    BadThreshold,
    TooManySignatures,
    /// The same key id appears twice: one key must never count as two signers.
    DuplicateSignature,
    /// A key this installation trusts signed something else, or the signature is damaged.
    BadSignature,
    ThresholdNotMet {
        valid: usize,
        needed: usize,
    },
    PayloadNotCanonical,
}

/// Most signatures one envelope may carry; a root has at most this many keys.
pub const MAX_SIGNATURES: usize = 16;

/// Checks that at least `threshold` DIFFERENT trusted keys signed the envelope. A signature from a key that is not trusted
/// is ignored (it neither counts nor fails the envelope); a bad signature from a trusted key fails it, because it means the
/// envelope was altered. The same key id twice fails it. Returns the key ids that counted, sorted.
pub fn verify_threshold(
    env: &Envelope,
    trusted: &TrustedKeys,
    threshold: usize,
    allowed_types: &[&str],
) -> Result<Vec<KeyId>, ThresholdError> {
    if !allowed_types.contains(&env.payload_type.as_str()) {
        return Err(ThresholdError::UnknownPayloadType);
    }
    if threshold < 1 {
        return Err(ThresholdError::BadThreshold);
    }
    if env.signatures.len() > MAX_SIGNATURES {
        return Err(ThresholdError::TooManySignatures);
    }
    let mut seen = std::collections::BTreeSet::new();
    for (id, _) in &env.signatures {
        if !seen.insert(id.clone()) {
            return Err(ThresholdError::DuplicateSignature);
        }
    }
    let text =
        std::str::from_utf8(&env.payload).map_err(|_| ThresholdError::PayloadNotCanonical)?;
    match canon::canonicalize(text.as_bytes()) {
        Ok(c) if c == text => {}
        _ => return Err(ThresholdError::PayloadNotCanonical),
    }
    let message = pae(&env.payload_type, &env.payload);
    let mut counted: Vec<KeyId> = Vec::new();
    for (id, raw) in &env.signatures {
        let Some(key) = trusted.get(id) else { continue };
        let sig = Signature::from_bytes(raw);
        key.verify_strict(&message, &sig)
            .map_err(|_| ThresholdError::BadSignature)?;
        counted.push(id.clone());
    }
    if counted.len() < threshold {
        return Err(ThresholdError::ThresholdNotMet {
            valid: counted.len(),
            needed: threshold,
        });
    }
    counted.sort();
    Ok(counted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer() -> Signer {
        Signer::from_seed([7u8; 32])
    }

    fn trusted(s: &Signer) -> TrustedKeys {
        let mut t = TrustedKeys::new();
        t.add(&s.public()).unwrap();
        t
    }

    const TYPE: &str = "application/vnd.kakurega.test.v1+json";

    #[test]
    fn pae_matches_the_dsse_spec_example() {
        assert_eq!(
            pae("application/example", b"hello"),
            b"DSSEv1 19 application/example 5 hello"
        );
    }

    #[test]
    fn sign_encode_decode_verify_roundtrip() {
        let s = signer();
        let env = sign(TYPE, br#"{"a":1}"#, &s);
        let bytes = encode(&env);
        let back = decode(bytes.as_bytes()).unwrap();
        assert_eq!(back, env);
        assert!(verify(&back, &trusted(&s), &[TYPE]).is_ok());
    }

    #[test]
    fn type_confusion_and_tampering_fail() {
        let s = signer();
        let env = sign(TYPE, br#"{"a":1}"#, &s);
        let mut changed_type = env.clone();
        changed_type.payload_type = "application/vnd.kakurega.other.v1+json".into();
        let allowed = [TYPE, "application/vnd.kakurega.other.v1+json"];
        assert_eq!(
            verify(&changed_type, &trusted(&s), &allowed).err(),
            Some(VerifyError::BadSignature)
        );
        let mut changed_payload = env.clone();
        changed_payload.payload = br#"{"a":2}"#.to_vec();
        assert_eq!(
            verify(&changed_payload, &trusted(&s), &[TYPE]).err(),
            Some(VerifyError::BadSignature)
        );
        assert_eq!(
            verify(&env, &trusted(&s), &["something/else"]).err(),
            Some(VerifyError::UnknownPayloadType)
        );
    }

    #[test]
    fn unknown_key_wrong_count_and_noncanonical_payload_fail() {
        let s = signer();
        let other = Signer::from_seed([8u8; 32]);
        let env = sign(TYPE, br#"{"a":1}"#, &s);
        assert_eq!(
            verify(&env, &trusted(&other), &[TYPE]).err(),
            Some(VerifyError::UnknownKey)
        );
        let mut two = env.clone();
        two.signatures.push(two.signatures[0].clone());
        assert_eq!(
            verify(&two, &trusted(&s), &[TYPE]).err(),
            Some(VerifyError::WrongSignatureCount)
        );
        let mut none = env.clone();
        none.signatures.clear();
        assert_eq!(
            verify(&none, &trusted(&s), &[TYPE]).err(),
            Some(VerifyError::WrongSignatureCount)
        );
        let loose = sign(TYPE, br#"{ "a": 1 }"#, &s);
        assert_eq!(
            verify(&loose, &trusted(&s), &[TYPE]).err(),
            Some(VerifyError::PayloadNotCanonical)
        );
    }

    #[test]
    fn strict_envelope_parsing() {
        let s = signer();
        let good = encode(&sign(TYPE, br#"{"a":1}"#, &s));
        assert!(decode(good.as_bytes()).is_ok());
        assert_eq!(
            decode(format!(" {good}").as_bytes()).err(),
            Some(EnvelopeError::NotCanonical)
        );
        assert_eq!(
            decode(format!("{good}x").as_bytes()).err(),
            Some(EnvelopeError::NotJson)
        );
        let extra = good.replacen('{', r#"{"extra":1,"#, 1);
        assert!(decode(extra.as_bytes()).is_err());
        let pad = good.replacen("\"payload\":\"", "\"payload\":\"=", 1);
        assert!(decode(pad.as_bytes()).is_err());
    }

    #[test]
    fn a_weak_public_key_cannot_be_trusted() {
        let mut t = TrustedKeys::new();
        assert!(t.add(&[0u8; 32]).is_none());
        let mut identity = [0u8; 32];
        identity[0] = 1;
        assert!(t.add(&identity).is_none());
    }
}
