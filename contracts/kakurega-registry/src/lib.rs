//! Kakurega capsule registry.
//!
//! A proof capsule is a signed file. This contract makes three public statements about one, each checked on chain:
//!
//! 1. **When and by whom**: a NEAR account registered the capsule's SHA-256 digest, with a signing key, at a block time. First
//!    registration of that digest under that key wins and can never be overwritten. A different key registering the same digest is a
//!    separate entry, so nobody can take a capsule's place by registering its digest first.
//! 2. **Whose key**: the registering account controls the key that signed the capsule. The caller proves it with an Ed25519
//!    signature, made by that key, over `kakurega-register-v1:<digest>:<account id>`. The message names the account, so the proof
//!    cannot be replayed by another account.
//! 3. **What a model signed**: each receipt (a model enclave's signature over `MODEL:SHA256(request):SHA256(answer)`, the format
//!    NEAR AI Cloud signs) verifies with the runtime's own Ed25519 check.
//!
//! It does NOT say that a receipt's signer key belongs to attested confidential hardware (that needs the hardware quote, checked
//! off chain), and it says nothing about whether a claim in the capsule is true. There is no owner, no admin call and no upgrade.
//!
//! The receipt checks follow the verification in SealedBounty's `tee.rs` (MIT): the signed text is rebuilt from its parts and
//! checked with `env::ed25519_verify`. The code here is written for this contract.

use near_sdk::json_types::U64;
use near_sdk::store::LookupMap;
use near_sdk::{
    AccountId, BorshStorageKey, NearToken, PanicOnDefault, Promise, env, near, require,
};

/// The most receipts one registration may carry. A review makes a handful of model calls; a larger set is split over capsules.
pub const MAX_RECEIPTS: usize = 16;
/// What a binding signature covers is prefixed, so a signature made for another purpose can never be reused here.
pub const BINDING_PREFIX: &str = "kakurega-register-v2";
const MAX_MODEL_LEN: usize = 200;

#[derive(BorshStorageKey)]
#[near(serializers = [borsh])]
enum Key {
    Entries,
}

/// One model signature, as a caller presents it. All hex is lowercase.
#[near(serializers = [json])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceiptArg {
    /// The model enclave's Ed25519 public key, 64 hex digits.
    pub signer: String,
    pub model: String,
    /// SHA-256 of the exact request bytes, 64 hex digits.
    pub request_sha256: String,
    /// SHA-256 of the exact answer bytes, 64 hex digits.
    pub response_sha256: String,
    /// The signature over `MODEL:REQUEST:ANSWER`, 128 hex digits.
    pub signature: String,
}

/// A receipt whose signature this contract verified.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedReceipt {
    pub signer: String,
    pub model: String,
    pub request_sha256: String,
    pub response_sha256: String,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub registered_by: AccountId,
    /// Block time of the registration, in nanoseconds since 1970.
    pub registered_at: U64,
    /// The Ed25519 key that signed the capsule, proven by the registering account.
    pub capsule_signer: String,
    pub receipts: Vec<VerifiedReceipt>,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Registry {
    entries: LookupMap<String, Entry>,
    total: u64,
}

/// The key an entry is stored under: the capsule's digest and the key that signed it. Two parties registering the same digest with
/// different keys do not collide, so nobody can take a capsule's place by registering its digest first with a key of their own.
fn entry_key(capsule_sha256: &str, capsule_signer: &str) -> String {
    format!("{capsule_sha256}:{capsule_signer}")
}

/// SHA-256, in lowercase hex, of the receipts as presented: one line per receipt, `signer:model:request:response:signature`.
/// The binding signature covers it, so the receipts cannot be swapped after the capsule's key signed.
pub fn receipts_digest(receipts: &[ReceiptArg]) -> String {
    let mut text = String::new();
    for r in receipts {
        text.push_str(&format!(
            "{}:{}:{}:{}:{}\n",
            r.signer, r.model, r.request_sha256, r.response_sha256, r.signature
        ));
    }
    let h = env::sha256_array(text.as_bytes());
    h.iter().map(|b| format!("{b:02x}")).collect()
}

fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

fn hex_to<const N: usize>(s: &str) -> Option<[u8; N]> {
    if !is_lower_hex(s, N * 2) {
        return None;
    }
    let mut out = [0u8; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// The y coordinates (sign bit ignored) of every point of order 1, 2, 4 and 8 on Curve25519, including the non-canonical
/// encodings of 0 and 1 (p and p+1). A key like these lets anyone forge a signature that the runtime's check accepts, so
/// none is ever taken as a signer. Cross-checked against dalek's `EIGHT_TORSION` in the tests.
const SMALL_ORDER_Y: [[u8; 32]; 7] = [
    [0; 32],
    {
        let mut a = [0u8; 32];
        a[0] = 1;
        a
    },
    [
        0x26, 0xe8, 0x95, 0x8f, 0xc2, 0xb2, 0x27, 0xb0, 0x45, 0xc3, 0xf4, 0x89, 0xf2, 0xef, 0x98,
        0xf0, 0xd5, 0xdf, 0xac, 0x05, 0xd3, 0xc6, 0x33, 0x39, 0xb1, 0x38, 0x02, 0x88, 0x6d, 0x53,
        0xfc, 0x05,
    ],
    [
        0xc7, 0x17, 0x6a, 0x70, 0x3d, 0x4d, 0xd8, 0x4f, 0xba, 0x3c, 0x0b, 0x76, 0x0d, 0x10, 0x67,
        0x0f, 0x2a, 0x20, 0x53, 0xfa, 0x2c, 0x39, 0xcc, 0xc6, 0x4e, 0xc7, 0xfd, 0x77, 0x92, 0xac,
        0x03, 0x7a,
    ],
    {
        let mut a = [0xffu8; 32];
        a[0] = 0xec;
        a[31] = 0x7f;
        a
    },
    {
        let mut a = [0xffu8; 32];
        a[0] = 0xed;
        a[31] = 0x7f;
        a
    },
    {
        let mut a = [0xffu8; 32];
        a[0] = 0xee;
        a[31] = 0x7f;
        a
    },
];

/// Whether this public key encodes a point of small order.
pub fn is_small_order(key: &[u8; 32]) -> bool {
    let mut y = *key;
    y[31] &= 0x7f;
    SMALL_ORDER_Y.contains(&y)
}

fn plain_model(m: &str) -> bool {
    !m.is_empty() && m.len() <= MAX_MODEL_LEN && !m.chars().any(|c| c.is_control() || c == ':')
}

/// The text a model enclave signs: the model name and the SHA-256 of the exact request and answer bytes.
pub fn receipt_text(r: &ReceiptArg) -> String {
    format!("{}:{}:{}", r.model, r.request_sha256, r.response_sha256)
}

/// Whether one receipt is well formed and its signature verifies. Never panics.
fn receipt_holds(r: &ReceiptArg) -> bool {
    if !plain_model(&r.model)
        || !is_lower_hex(&r.request_sha256, 64)
        || !is_lower_hex(&r.response_sha256, 64)
    {
        return false;
    }
    let (Some(key), Some(sig)) = (hex_to::<32>(&r.signer), hex_to::<64>(&r.signature)) else {
        return false;
    };
    !is_small_order(&key) && env::ed25519_verify(&sig, receipt_text(r).as_bytes(), &key)
}

#[near]
impl Registry {
    #[init]
    pub fn new() -> Self {
        Self {
            entries: LookupMap::new(Key::Entries),
            total: 0,
        }
    }

    /// The exact message the capsule's signing key must sign to bind this digest and these receipts to `account`.
    pub fn binding_message(
        &self,
        capsule_sha256: String,
        receipts: Vec<ReceiptArg>,
        account_id: AccountId,
    ) -> String {
        format!(
            "{BINDING_PREFIX}:{capsule_sha256}:{}:{account_id}",
            receipts_digest(&receipts)
        )
    }

    /// Which of these receipts verify, in order. A malformed receipt is `false`, not a failure. Free to call.
    pub fn verify_receipts(&self, receipts: Vec<ReceiptArg>) -> Vec<bool> {
        require!(receipts.len() <= 64, "at most 64 receipts per check");
        receipts.iter().map(receipt_holds).collect()
    }

    pub fn get(&self, capsule_sha256: String, capsule_signer: String) -> Option<Entry> {
        self.entries
            .get(&entry_key(&capsule_sha256, &capsule_signer))
            .cloned()
    }

    pub fn total(&self) -> U64 {
        U64(self.total)
    }

    /// Registers a capsule. The attached deposit pays for the storage it takes; what is left over goes back.
    ///
    /// `binding_signature` is the capsule signing key's signature over [`Self::binding_message`] for the calling account.
    #[payable]
    pub fn register(
        &mut self,
        capsule_sha256: String,
        capsule_signer: String,
        binding_signature: String,
        receipts: Vec<ReceiptArg>,
    ) -> Entry {
        require!(
            is_lower_hex(&capsule_sha256, 64),
            "the capsule digest is 64 lowercase hex digits"
        );
        require!(receipts.len() <= MAX_RECEIPTS, "too many receipts");
        let account = env::predecessor_account_id();
        let (Some(key), Some(sig)) = (
            hex_to::<32>(&capsule_signer),
            hex_to::<64>(&binding_signature),
        ) else {
            env::panic_str("the signer is 64 and the signature 128 lowercase hex digits");
        };
        require!(
            !is_small_order(&key),
            "the capsule signer is not a usable key"
        );
        let message = format!(
            "{BINDING_PREFIX}:{capsule_sha256}:{}:{account}",
            receipts_digest(&receipts)
        );
        require!(
            env::ed25519_verify(&sig, message.as_bytes(), &key),
            "the binding signature does not verify for this account"
        );
        let mut verified: Vec<VerifiedReceipt> = Vec::with_capacity(receipts.len());
        for r in &receipts {
            require!(
                receipt_holds(r),
                "a receipt is malformed or its signature does not verify"
            );
            let one = VerifiedReceipt {
                signer: r.signer.clone(),
                model: r.model.clone(),
                request_sha256: r.request_sha256.clone(),
                response_sha256: r.response_sha256.clone(),
            };
            require!(!verified.contains(&one), "a receipt is listed twice");
            verified.push(one);
        }
        let key_text = entry_key(&capsule_sha256, &capsule_signer);
        require!(
            self.entries.get(&key_text).is_none(),
            "this capsule is already registered under this signing key"
        );
        let entry = Entry {
            registered_by: account.clone(),
            registered_at: U64(env::block_timestamp()),
            capsule_signer,
            receipts: verified,
        };
        let before = env::storage_usage();
        self.entries.insert(key_text, entry.clone());
        self.total += 1;
        self.entries.flush();
        let used = env::storage_usage().saturating_sub(before);
        let cost = env::storage_byte_cost().saturating_mul(u128::from(used));
        let paid = env::attached_deposit();
        require!(
            paid >= cost,
            format!("attach at least {cost} yoctoNEAR for the storage this takes")
        );
        let extra = paid.saturating_sub(cost);
        if extra > NearToken::from_yoctonear(0) {
            Promise::new(account.clone()).transfer(extra).detach();
        }
        env::log_str(&format!(
            "EVENT_JSON:{{\"standard\":\"kakurega\",\"version\":\"1.0.0\",\"event\":\"registered\",\"data\":[{{\"capsule_sha256\":\"{capsule_sha256}\",\"registered_by\":\"{account}\",\"receipts\":{}}}]}}",
            entry.receipts.len()
        ));
        entry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use near_sdk::test_utils::VMContextBuilder;
    use near_sdk::testing_env;

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DEPOSIT: u128 = 10_000_000_000_000_000_000_000; // 0.01 NEAR

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }
    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
    fn as_account(who: &str, deposit: u128) {
        testing_env!(
            VMContextBuilder::new()
                .predecessor_account_id(who.parse().unwrap())
                .attached_deposit(NearToken::from_yoctonear(deposit))
                .block_timestamp(1_700_000_000_000_000_000)
                .build()
        );
    }
    /// The capsule key's signature over the binding message for these receipts and this account.
    fn bind(k: &SigningKey, digest: &str, receipts: &[ReceiptArg], who: &str) -> String {
        hex(&k
            .sign(
                format!(
                    "{BINDING_PREFIX}:{digest}:{}:{who}",
                    receipts_digest(receipts)
                )
                .as_bytes(),
            )
            .to_bytes())
    }
    fn receipt(k: &SigningKey, model: &str, req: &str, resp: &str) -> ReceiptArg {
        let text = format!("{model}:{req}:{resp}");
        ReceiptArg {
            signer: hex(k.verifying_key().as_bytes()),
            model: model.into(),
            request_sha256: req.into(),
            response_sha256: resp.into(),
            signature: hex(&k.sign(text.as_bytes()).to_bytes()),
        }
    }
    fn h(c: char) -> String {
        c.to_string().repeat(64)
    }
    fn signer_hex(k: &SigningKey) -> String {
        hex(k.verifying_key().as_bytes())
    }
    fn fresh() -> Registry {
        as_account("alice.testnet", DEPOSIT);
        Registry::new()
    }
    /// Registers `receipts` for `digest` as `who`, signed by `cap`.
    fn reg(
        r: &mut Registry,
        cap: &SigningKey,
        digest: &str,
        receipts: Vec<ReceiptArg>,
        who: &str,
    ) -> Entry {
        let sig = bind(cap, digest, &receipts, who);
        r.register(digest.into(), signer_hex(cap), sig, receipts)
    }

    #[test]
    fn rg01_a_bound_capsule_with_a_real_receipt_registers() {
        let mut r = fresh();
        let (cap, model) = (key(1), key(2));
        let rc = receipt(&model, "Qwen/Qwen3.8-27B", &h('b'), &h('c'));
        let e = reg(&mut r, &cap, DIGEST, vec![rc], "alice.testnet");
        assert_eq!(e.registered_by.as_str(), "alice.testnet");
        assert_eq!(e.registered_at.0, 1_700_000_000_000_000_000);
        assert_eq!(e.receipts.len(), 1);
        assert_eq!(r.get(DIGEST.into(), signer_hex(&cap)).unwrap(), e);
        assert_eq!(r.total().0, 1);
    }

    #[test]
    #[should_panic(expected = "binding signature does not verify")]
    fn rg02_another_account_cannot_replay_the_binding() {
        let mut r = fresh();
        let cap = key(1);
        let sig = bind(&cap, DIGEST, &[], "alice.testnet");
        as_account("mallory.testnet", DEPOSIT);
        r.register(DIGEST.into(), signer_hex(&cap), sig, vec![]);
    }

    #[test]
    #[should_panic(expected = "binding signature does not verify")]
    fn rg03_a_binding_for_another_digest_is_refused() {
        let mut r = fresh();
        let cap = key(1);
        let sig = bind(&cap, &h('d'), &[], "alice.testnet");
        r.register(DIGEST.into(), signer_hex(&cap), sig, vec![]);
    }

    #[test]
    #[should_panic(expected = "binding signature does not verify")]
    fn rg04_a_binding_by_a_different_key_is_refused() {
        let mut r = fresh();
        let sig = bind(&key(9), DIGEST, &[], "alice.testnet");
        r.register(DIGEST.into(), signer_hex(&key(1)), sig, vec![]);
    }

    #[test]
    #[should_panic(expected = "already registered under this signing key")]
    fn rg05_the_first_registration_by_a_key_wins() {
        let mut r = fresh();
        let cap = key(1);
        reg(&mut r, &cap, DIGEST, vec![], "alice.testnet");
        as_account("bob.testnet", DEPOSIT);
        reg(&mut r, &cap, DIGEST, vec![], "bob.testnet");
    }

    #[test]
    fn rg05b_another_key_registering_the_same_digest_first_takes_nobodys_place() {
        let mut r = fresh();
        let (squatter, owner) = (key(7), key(1));
        // the squatter learned the capsule's digest and registers it first, with a key of their own
        as_account("squatter.testnet", DEPOSIT);
        reg(&mut r, &squatter, DIGEST, vec![], "squatter.testnet");
        // the real owner still registers the same digest under their own key
        as_account("alice.testnet", DEPOSIT);
        let e = reg(&mut r, &owner, DIGEST, vec![], "alice.testnet");
        assert_eq!(e.registered_by.as_str(), "alice.testnet");
        assert_eq!(
            r.get(DIGEST.into(), signer_hex(&owner))
                .unwrap()
                .registered_by
                .as_str(),
            "alice.testnet"
        );
        assert_eq!(
            r.get(DIGEST.into(), signer_hex(&squatter))
                .unwrap()
                .registered_by
                .as_str(),
            "squatter.testnet"
        );
        assert_eq!(r.total().0, 2);
    }

    #[test]
    #[should_panic(expected = "binding signature does not verify")]
    fn rg05c_the_receipts_cannot_be_swapped_after_the_capsule_key_signed() {
        let mut r = fresh();
        let (cap, model) = (key(1), key(2));
        let signed_for = vec![receipt(&model, "m", &h('b'), &h('c'))];
        let sig = bind(&cap, DIGEST, &signed_for, "alice.testnet");
        let swapped = vec![receipt(&model, "m", &h('b'), &h('e'))];
        r.register(DIGEST.into(), signer_hex(&cap), sig, swapped);
    }

    #[test]
    #[should_panic(expected = "receipt is malformed or its signature does not verify")]
    fn rg06_a_receipt_with_a_changed_answer_is_refused() {
        let mut r = fresh();
        let (cap, model) = (key(1), key(2));
        let mut rc = receipt(&model, "m", &h('b'), &h('c'));
        rc.response_sha256 = h('e');
        reg(&mut r, &cap, DIGEST, vec![rc], "alice.testnet");
    }

    #[test]
    #[should_panic(expected = "receipt is malformed or its signature does not verify")]
    fn rg07_a_receipt_with_a_changed_model_name_is_refused() {
        let mut r = fresh();
        let (cap, model) = (key(1), key(2));
        let mut rc = receipt(&model, "good-model", &h('b'), &h('c'));
        rc.model = "other-model".into();
        reg(&mut r, &cap, DIGEST, vec![rc], "alice.testnet");
    }

    #[test]
    #[should_panic(expected = "listed twice")]
    fn rg08_the_same_receipt_twice_is_refused() {
        let mut r = fresh();
        let (cap, model) = (key(1), key(2));
        let rc = receipt(&model, "m", &h('b'), &h('c'));
        reg(&mut r, &cap, DIGEST, vec![rc.clone(), rc], "alice.testnet");
    }

    #[test]
    #[should_panic(expected = "too many receipts")]
    fn rg09_more_than_the_cap_is_refused() {
        let mut r = fresh();
        let (cap, model) = (key(1), key(2));
        let many: Vec<ReceiptArg> = (0..=MAX_RECEIPTS)
            .map(|i| receipt(&model, "m", &h('b'), &format!("{i:064x}")))
            .collect();
        reg(&mut r, &cap, DIGEST, many, "alice.testnet");
    }

    #[test]
    #[should_panic(expected = "attach at least")]
    fn rg10_a_deposit_below_the_storage_cost_is_refused() {
        let mut r = fresh();
        as_account("alice.testnet", 1);
        reg(&mut r, &key(1), DIGEST, vec![], "alice.testnet");
    }

    #[test]
    #[should_panic(expected = "64 lowercase hex")]
    fn rg11_an_uppercase_digest_is_refused() {
        let mut r = fresh();
        let up = DIGEST.to_uppercase();
        reg(&mut r, &key(1), &up, vec![], "alice.testnet");
    }

    #[test]
    fn rg12_verify_receipts_marks_each_one_and_never_panics_on_junk() {
        let r = fresh();
        let model = key(2);
        let good = receipt(&model, "m", &h('b'), &h('c'));
        let mut bad_sig = good.clone();
        bad_sig.signature = "0".repeat(128);
        let mut colon = receipt(&model, "a:b", &h('b'), &h('c'));
        colon.model = "a:b".into();
        let mut upper = good.clone();
        upper.signer = upper.signer.to_uppercase();
        let mut short = good.clone();
        short.request_sha256 = "abc".into();
        let mut empty_model = good.clone();
        empty_model.model = String::new();
        let out = r.verify_receipts(vec![good, bad_sig, colon, upper, short, empty_model]);
        assert_eq!(out, vec![true, false, false, false, false, false]);
    }

    #[test]
    fn rg13_a_key_that_is_not_on_the_curve_does_not_verify_and_does_not_crash() {
        let r = fresh();
        let mut rc = receipt(&key(2), "m", &h('b'), &h('c'));
        rc.signer = "ff".repeat(32);
        assert_eq!(r.verify_receipts(vec![rc]), vec![false]);
    }

    #[test]
    fn rg14_the_binding_message_matches_what_register_checks() {
        let r = fresh();
        let rc = receipt(&key(2), "m", &h('b'), &h('c'));
        let m = r.binding_message(
            DIGEST.into(),
            vec![rc.clone()],
            "alice.testnet".parse().unwrap(),
        );
        assert_eq!(
            m,
            format!(
                "{BINDING_PREFIX}:{DIGEST}:{}:alice.testnet",
                receipts_digest(&[rc])
            )
        );
    }

    #[test]
    fn rg15_an_unknown_digest_reads_as_none() {
        let r = fresh();
        assert!(r.get(h('9'), signer_hex(&key(1))).is_none());
        assert_eq!(r.total().0, 0);
    }

    #[test]
    fn rg16_every_small_order_point_is_refused_as_a_signer() {
        use curve25519_dalek::constants::EIGHT_TORSION;
        for p in EIGHT_TORSION.iter() {
            let b = p.compress().to_bytes();
            assert!(is_small_order(&b), "{b:?}");
            let mut flipped = b;
            flipped[31] ^= 0x80;
            assert!(is_small_order(&flipped));
        }
        for tail in [0xed_u8, 0xee] {
            let mut nc = [0xffu8; 32];
            nc[0] = tail;
            nc[31] = 0x7f;
            assert!(is_small_order(&nc));
        }
        for seed in 1..40u8 {
            assert!(!is_small_order(key(seed).verifying_key().as_bytes()));
        }
    }

    #[test]
    fn rg17_a_receipt_under_a_small_order_key_is_not_accepted() {
        let r = fresh();
        let mut rc = receipt(&key(2), "m", &h('b'), &h('c'));
        rc.signer = "00".repeat(32);
        rc.signature = "00".repeat(64);
        assert_eq!(r.verify_receipts(vec![rc]), vec![false]);
    }

    #[test]
    #[should_panic(expected = "not a usable key")]
    fn rg18_a_small_order_capsule_signer_is_refused() {
        let mut r = fresh();
        r.register(DIGEST.into(), "00".repeat(32), "00".repeat(64), vec![]);
    }
}
