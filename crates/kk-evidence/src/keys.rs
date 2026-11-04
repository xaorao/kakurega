use crate::b64;
use ed25519_dalek::{Signer as _, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use zeroize::ZeroizeOnDrop;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct KeyId(String);

impl KeyId {
    pub fn of(public: &[u8; 32]) -> KeyId {
        let digest = Sha256::digest(public);
        let mut s = String::from("ed25519:");
        for b in digest {
            s.push_str(&format!("{b:02x}"));
        }
        KeyId(s)
    }

    pub fn parse(text: &str) -> Option<KeyId> {
        let hex = text.strip_prefix("ed25519:")?;
        let ok = hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        ok.then(|| KeyId(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(ZeroizeOnDrop)]
pub struct Signer {
    key: SigningKey,
}

impl Signer {
    pub fn from_seed(seed: [u8; 32]) -> Signer {
        Signer {
            key: SigningKey::from_bytes(&seed),
        }
    }

    pub fn public(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    pub fn key_id(&self) -> KeyId {
        KeyId::of(&self.public())
    }

    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.key.sign(message).to_bytes()
    }
}

impl fmt::Debug for Signer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signer({})", self.key_id())
    }
}

#[derive(Debug, Clone, Default)]
pub struct TrustedKeys {
    keys: BTreeMap<KeyId, VerifyingKey>,
}

impl TrustedKeys {
    pub fn new() -> Self {
        TrustedKeys::default()
    }

    pub fn add(&mut self, public: &[u8; 32]) -> Option<KeyId> {
        let key = VerifyingKey::from_bytes(public).ok()?;
        if key.is_weak() {
            return None;
        }
        let id = KeyId::of(public);
        self.keys.insert(id.clone(), key);
        Some(id)
    }

    pub fn get(&self, id: &KeyId) -> Option<&VerifyingKey> {
        self.keys.get(id)
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

pub fn public_key_b64(public: &[u8; 32]) -> String {
    b64::encode(public)
}
