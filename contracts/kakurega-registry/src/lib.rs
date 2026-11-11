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
//! 4. **On what hardware**: a model's signing key is bound into a quote that NEAR's verifier contract checked against Intel's
//!    collateral at a block time ([`Registry::attest_signer`]). A receipt records whether its signer had such a record when the
//!    capsule was registered.
//!
//! 5. **Paid on a verified result**: a client sets money aside for a named reviewer and capsule key ([`Registry::open_order`], or
//!    a transfer of the accepted token). It is paid when that reviewer registers a capsule for the order, signed by that key and,
//!    if the client asked, carrying only model receipts whose signer's hardware quote this contract saw verified after the order
//!    was opened. After the deadline an unpaid order goes back to the client.
//!
//! It does NOT judge which software a quote measured (the measurements are recorded, none is approved here), it does not check
//! NVIDIA GPU evidence, and it says nothing about whether a claim in the capsule is true. There is no owner, no admin call and
//! no upgrade; the verifier's account is fixed when the contract is created.
//!
//! The receipt checks follow the verification in SealedBounty's `tee.rs` (MIT): the signed text is rebuilt from its parts and
//! checked with `env::ed25519_verify`. The code here is written for this contract.

use near_sdk::json_types::{Base64VecU8, U64, U128};
use near_sdk::store::LookupMap;
use near_sdk::{
    AccountId, BorshStorageKey, Gas, NearToken, PanicOnDefault, Promise, PromiseError,
    PromiseOrValue, env, near, require,
};

pub mod hardware;
pub use hardware::Attested;

/// The most receipts one registration may carry. A review makes a handful of model calls; a larger set is split over capsules.
pub const MAX_RECEIPTS: usize = 16;
/// What a binding signature covers is prefixed, so a signature made for another purpose can never be reused here.
pub const BINDING_PREFIX: &str = "kakurega-register-v2";
const MAX_MODEL_LEN: usize = 200;
/// A TDX quote with its certificates is about 5 KB and Intel's collateral about 14 KB; anything far larger is not one.
pub const MAX_QUOTE_BYTES: usize = 16 * 1024;
pub const MAX_COLLATERAL_BYTES: usize = 64 * 1024;
/// The verifier's own work on a quote (about 176 Tgas measured by others on testnet), with room to spare.
const VERIFY_GAS: Gas = Gas::from_tgas(200);
const ATTESTED_CALLBACK_GAS: Gas = Gas::from_tgas(30);
const TOKEN_TRANSFER_GAS: Gas = Gas::from_tgas(15);
const PAYOUT_CALLBACK_GAS: Gas = Gas::from_tgas(10);
/// The smallest NEAR order, after its own storage is paid: an order below this is not worth a record.
pub const MIN_NEAR_ORDER: NearToken = NearToken::from_millinear(10);
/// An order may wait at most this long, and must leave at least a minute to be met.
pub const MAX_ORDER_NANOS: u64 = 366 * 24 * 60 * 60 * 1_000_000_000;
pub const MIN_ORDER_NANOS: u64 = 60 * 1_000_000_000;
/// A registration for an order signs this after the usual binding message, so a capsule key commits to the order it is paid by.
pub const ORDER_SUFFIX: &str = "order";

#[derive(BorshStorageKey)]
#[near(serializers = [borsh])]
enum Key {
    Entries,
    Attested,
    Orders,
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
    /// When this contract last saw the signer's hardware quote verified, if it had by the time of registration: block time in
    /// nanoseconds. `None` says only that no such record existed here, not that the hardware is absent.
    pub signer_attested_at: Option<U64>,
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
    /// The order this capsule was registered for, if any.
    pub order: Option<U64>,
}

#[near(serializers = [json, borsh])]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderState {
    /// Money is set aside and waits for the reviewer's capsule.
    Open,
    /// The capsule was registered; the payout has not gone through yet and anyone may retry it.
    Earned,
    Paid,
    Refunded,
}

/// Money a client set aside for one review.
#[near(serializers = [json, borsh])]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    pub client: AccountId,
    pub reviewer: AccountId,
    /// The capsule signing key the client expects, 64 hex digits.
    pub reviewer_key: String,
    /// SHA-256 of what was asked, as the two agreed it. Recorded for a recipient to compare; not judged here.
    pub scope_sha256: String,
    /// In yoctoNEAR, or in the token's smallest unit.
    pub amount: U128,
    /// `None` for NEAR, or the accepted token's account.
    pub token: Option<AccountId>,
    pub opened_at: U64,
    pub deadline: U64,
    /// Pay only for a capsule whose every model receipt is signed by a key attested here, up to date, after the order opened.
    pub require_hardware: bool,
    pub state: OrderState,
    pub capsule_sha256: Option<String>,
}

/// The terms of an order, as a client gives them (and as the message of a token transfer, in JSON).
#[near(serializers = [json])]
#[derive(Clone, Debug)]
pub struct OrderTerms {
    pub reviewer: AccountId,
    pub reviewer_key: String,
    pub scope_sha256: String,
    pub deadline: U64,
    pub require_hardware: bool,
}

#[near(contract_state)]
#[derive(PanicOnDefault)]
pub struct Registry {
    entries: LookupMap<String, Entry>,
    total: u64,
    /// Signing keys whose hardware quote the verifier accepted, by key.
    attested: LookupMap<String, Attested>,
    /// NEAR's quote verifier. Fixed for the life of the contract.
    verifier: AccountId,
    orders: LookupMap<u64, Order>,
    next_order: u64,
    /// The one NEP-141 token orders may be paid in besides NEAR, and its smallest order. Fixed at creation.
    token: Option<AccountId>,
    min_token_order: u128,
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
    pub fn new(
        verifier: AccountId,
        token: Option<AccountId>,
        min_token_order: Option<U128>,
    ) -> Self {
        require!(
            token.is_none() || min_token_order.is_some_and(|m| m.0 > 0),
            "a token needs a smallest order above zero"
        );
        Self {
            entries: LookupMap::new(Key::Entries),
            total: 0,
            attested: LookupMap::new(Key::Attested),
            verifier,
            orders: LookupMap::new(Key::Orders),
            next_order: 0,
            token,
            min_token_order: min_token_order.map_or(0, |m| m.0),
        }
    }

    /// The token orders may be paid in besides NEAR, if any.
    pub fn token(&self) -> Option<AccountId> {
        self.token.clone()
    }

    pub fn order(&self, order_id: U64) -> Option<Order> {
        self.orders.get(&order_id.0).cloned()
    }

    pub fn orders_total(&self) -> U64 {
        U64(self.next_order)
    }

    /// The message a capsule key signs to register for an order.
    pub fn order_binding_message(
        &self,
        order_id: U64,
        capsule_sha256: String,
        receipts: Vec<ReceiptArg>,
        account_id: AccountId,
    ) -> String {
        format!(
            "{BINDING_PREFIX}:{capsule_sha256}:{}:{account_id}:{ORDER_SUFFIX}:{}",
            receipts_digest(&receipts),
            order_id.0
        )
    }

    /// Sets the attached NEAR aside for one review. The storage the order takes is paid from it; the rest is the order's amount.
    #[payable]
    pub fn open_order(&mut self, terms: OrderTerms) -> U64 {
        let client = env::predecessor_account_id();
        let paid = env::attached_deposit();
        let before = env::storage_usage();
        let id = self
            .add_order(client, &terms, 0, None)
            .unwrap_or_else(|e| env::panic_str(e));
        let used = env::storage_usage().saturating_sub(before);
        let cost = env::storage_byte_cost().saturating_mul(u128::from(used));
        let amount = paid.saturating_sub(cost);
        require!(
            paid > cost && amount >= MIN_NEAR_ORDER,
            "attach at least 0.01 NEAR beyond the storage the order takes"
        );
        if let Some(o) = self.orders.get_mut(&id) {
            o.amount = U128(amount.as_yoctonear());
        }
        self.orders.flush();
        U64(id)
    }

    /// NEP-141 receiver: a transfer of the accepted token with the order's terms as its JSON message opens an order. Anything
    /// else is handed straight back (the returned number is what the token contract refunds to the sender).
    pub fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        let from_token = env::predecessor_account_id();
        if self.token.as_ref() != Some(&from_token)
            || amount.0 < self.min_token_order
            || amount.0 == 0
        {
            return PromiseOrValue::Value(amount);
        }
        let Ok(terms) = near_sdk::serde_json::from_str::<OrderTerms>(&msg) else {
            return PromiseOrValue::Value(amount);
        };
        match self.add_order(sender_id, &terms, amount.0, Some(from_token)) {
            Ok(_) => PromiseOrValue::Value(U128(0)),
            Err(_) => PromiseOrValue::Value(amount),
        }
    }

    /// Checks the terms and stores the order. `Err` says what is wrong; nothing is stored then.
    fn add_order(
        &mut self,
        client: AccountId,
        terms: &OrderTerms,
        amount: u128,
        token: Option<AccountId>,
    ) -> Result<u64, &'static str> {
        let now = env::block_timestamp();
        let Some(key) = hex_to::<32>(&terms.reviewer_key) else {
            return Err("the reviewer key is 64 lowercase hex digits");
        };
        if is_small_order(&key) {
            return Err("the reviewer key is not a usable key");
        }
        if !is_lower_hex(&terms.scope_sha256, 64) {
            return Err("the scope digest is 64 lowercase hex digits");
        }
        if terms.deadline.0 < now.saturating_add(MIN_ORDER_NANOS)
            || terms.deadline.0 > now.saturating_add(MAX_ORDER_NANOS)
        {
            return Err("the deadline is between a minute and a year from now");
        }
        if terms.reviewer == client {
            return Err("the client and the reviewer are the same account");
        }
        let id = self.next_order;
        self.next_order += 1;
        self.orders.insert(
            id,
            Order {
                client: client.clone(),
                reviewer: terms.reviewer.clone(),
                reviewer_key: terms.reviewer_key.clone(),
                scope_sha256: terms.scope_sha256.clone(),
                amount: U128(amount),
                token,
                opened_at: U64(now),
                deadline: terms.deadline,
                require_hardware: terms.require_hardware,
                state: OrderState::Open,
                capsule_sha256: None,
            },
        );
        self.orders.flush();
        env::log_str(&format!(
            "EVENT_JSON:{{\"standard\":\"kakurega\",\"version\":\"1.0.0\",\"event\":\"order_opened\",\"data\":[{{\"order\":{id},\"client\":\"{client}\",\"reviewer\":\"{}\"}}]}}",
            terms.reviewer
        ));
        Ok(id)
    }

    /// The reviewer registers the capsule an order pays for. The binding signature covers [`Self::order_binding_message`].
    /// On success the order is paid out in the same transaction; if that payment does not go through, the order stays earned
    /// and [`Self::claim_order`] retries it. Attach the storage deposit as for [`Self::register`], and 100 Tgas.
    #[payable]
    pub fn register_for_order(
        &mut self,
        order_id: U64,
        capsule_sha256: String,
        capsule_signer: String,
        binding_signature: String,
        receipts: Vec<ReceiptArg>,
    ) -> Entry {
        let order = self
            .orders
            .get(&order_id.0)
            .cloned()
            .unwrap_or_else(|| env::panic_str("no such order"));
        require!(order.state == OrderState::Open, "the order is not open");
        require!(
            env::block_timestamp() <= order.deadline.0,
            "the order's deadline has passed"
        );
        require!(
            env::predecessor_account_id() == order.reviewer,
            "only the order's reviewer registers for it"
        );
        require!(
            capsule_signer == order.reviewer_key,
            "the capsule is not signed by the key the order names"
        );
        let entry = self.register_inner(
            capsule_sha256.clone(),
            capsule_signer,
            binding_signature,
            receipts,
            Some(order_id.0),
        );
        if order.require_hardware {
            require!(
                !entry.receipts.is_empty(),
                "the order asks for model receipts and the capsule carries none"
            );
            for r in &entry.receipts {
                let fresh = self.attested.get(&r.signer).is_some_and(|a| {
                    a.tcb_status == "UpToDate" && a.attested_at.0 >= order.opened_at.0
                });
                require!(
                    fresh,
                    "a receipt's signer has no up-to-date hardware record made after the order opened"
                );
            }
        }
        if let Some(o) = self.orders.get_mut(&order_id.0) {
            o.state = OrderState::Earned;
            o.capsule_sha256 = Some(capsule_sha256);
        }
        self.orders.flush();
        self.pay_out(order_id.0);
        entry
    }

    /// Retries the payout of an order whose capsule is registered. Anyone may call it; the money only ever goes to the reviewer.
    pub fn claim_order(&mut self, order_id: U64) {
        let state = self.orders.get(&order_id.0).map(|o| o.state);
        require!(
            state == Some(OrderState::Earned),
            "the order is not waiting for a payout"
        );
        self.pay_out(order_id.0);
    }

    /// Returns an open order's money to the client once its deadline has passed. Anyone may call it.
    pub fn refund_order(&mut self, order_id: U64) {
        let order = self
            .orders
            .get(&order_id.0)
            .cloned()
            .unwrap_or_else(|| env::panic_str("no such order"));
        require!(order.state == OrderState::Open, "the order is not open");
        require!(
            env::block_timestamp() > order.deadline.0,
            "the order's deadline has not passed"
        );
        self.send(
            order_id.0,
            &order,
            order.client.clone(),
            OrderState::Refunded,
            OrderState::Open,
        );
    }

    fn pay_out(&mut self, id: u64) {
        let Some(order) = self.orders.get(&id).cloned() else {
            return;
        };
        self.send(
            id,
            &order,
            order.reviewer.clone(),
            OrderState::Paid,
            OrderState::Earned,
        );
    }

    /// Marks the order `done` first, then sends its money; if the transfer fails, the callback puts `back` the earlier state.
    fn send(&mut self, id: u64, order: &Order, to: AccountId, done: OrderState, back: OrderState) {
        if let Some(o) = self.orders.get_mut(&id) {
            o.state = done;
        }
        self.orders.flush();
        let transfer = match &order.token {
            None => Promise::new(to).transfer(NearToken::from_yoctonear(order.amount.0)),
            Some(token) => Promise::new(token.clone()).function_call(
                "ft_transfer".to_string(),
                near_sdk::serde_json::json!({"receiver_id": to, "amount": order.amount})
                    .to_string()
                    .into_bytes(),
                NearToken::from_yoctonear(1),
                TOKEN_TRANSFER_GAS,
            ),
        };
        transfer
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(PAYOUT_CALLBACK_GAS)
                    .on_order_sent(U64(id), done, back),
            )
            .detach();
    }

    /// After an order's transfer. A failed transfer leaves the money here, so the order goes back to where it was.
    #[private]
    pub fn on_order_sent(
        &mut self,
        order_id: U64,
        done: OrderState,
        back: OrderState,
        #[callback_result] result: Result<(), PromiseError>,
    ) -> bool {
        if result.is_ok() {
            env::log_str(&format!(
                "EVENT_JSON:{{\"standard\":\"kakurega\",\"version\":\"1.0.0\",\"event\":\"order_settled\",\"data\":[{{\"order\":{},\"state\":\"{done:?}\"}}]}}",
                order_id.0
            ));
            return true;
        }
        if let Some(o) = self.orders.get_mut(&order_id.0)
            && o.state == done
        {
            o.state = back;
        }
        self.orders.flush();
        env::log_str(&format!(
            "EVENT_JSON:{{\"standard\":\"kakurega\",\"version\":\"1.0.0\",\"event\":\"order_transfer_failed\",\"data\":[{{\"order\":{}}}]}}",
            order_id.0
        ));
        false
    }

    /// The account of the quote verifier this contract asks.
    pub fn verifier(&self) -> AccountId {
        self.verifier.clone()
    }

    /// What is recorded about a signing key's hardware quote, if anything.
    pub fn hardware(&self, signer: String) -> Option<Attested> {
        self.attested.get(&signer).cloned()
    }

    /// The challenge, in hex, that a quote must carry to be recorded for `account_id`: ask the model service for an attestation
    /// with this value as the nonce.
    pub fn challenge_for(&self, account_id: AccountId) -> String {
        hardware::challenge(&account_id)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// Has NEAR's verifier check a TDX quote against Intel's collateral, and records the signing key bound into it when the
    /// quote answers the caller's challenge. `collateral` is the verifier's Borsh `Collateral`. The deposit pays for the storage
    /// the record takes; the rest, or all of it when nothing is recorded, goes back. Attach 300 Tgas.
    #[payable]
    pub fn attest_signer(&mut self, quote: Base64VecU8, collateral: Base64VecU8) -> Promise {
        require!(
            !quote.0.is_empty() && quote.0.len() <= MAX_QUOTE_BYTES,
            "the quote is empty or too large"
        );
        require!(
            !collateral.0.is_empty() && collateral.0.len() <= MAX_COLLATERAL_BYTES,
            "the collateral is empty or too large"
        );
        let account = env::predecessor_account_id();
        let paid = env::attached_deposit();
        Promise::new(self.verifier.clone())
            .function_call(
                "verify_quote".to_string(),
                hardware::verifier_args(&quote.0, &collateral.0),
                NearToken::from_yoctonear(0),
                VERIFY_GAS,
            )
            .then(
                Self::ext(env::current_account_id())
                    .with_static_gas(ATTESTED_CALLBACK_GAS)
                    .on_attested(account, paid),
            )
    }

    /// The verifier's answer. Never panics: a panic here would keep the caller's deposit.
    #[private]
    pub fn on_attested(
        &mut self,
        account: AccountId,
        paid: NearToken,
        #[callback_result]
        #[serializer(borsh)]
        result: Result<hardware::VerificationResultIn, PromiseError>,
    ) -> Option<Attested> {
        let refuse = |reason: &str| {
            if paid > NearToken::from_yoctonear(0) {
                Promise::new(account.clone()).transfer(paid).detach();
            }
            env::log_str(&format!(
                "EVENT_JSON:{{\"standard\":\"kakurega\",\"version\":\"1.0.0\",\"event\":\"attest_refused\",\"data\":[{{\"account\":\"{account}\",\"reason\":\"{reason}\"}}]}}"
            ));
            None
        };
        let report = match result {
            Ok(hardware::VerificationResultIn::Verified(v)) => v,
            Ok(hardware::VerificationResultIn::Rejected(_)) => {
                return refuse("the verifier rejected the quote");
            }
            Err(_) => return refuse("the verifier could not be asked"),
        };
        let found = match hardware::accept(&report, &account, env::block_timestamp()) {
            Ok(a) => a,
            Err(reason) => return refuse(reason),
        };
        let before = env::storage_usage();
        let previous = self.attested.insert(found.signer.clone(), found.clone());
        self.attested.flush();
        let used = env::storage_usage().saturating_sub(before);
        let cost = env::storage_byte_cost().saturating_mul(u128::from(used));
        if paid < cost {
            match previous {
                Some(p) => {
                    self.attested.insert(found.signer.clone(), p);
                }
                None => {
                    self.attested.remove(&found.signer);
                }
            }
            self.attested.flush();
            return refuse("the deposit does not cover the storage this record takes");
        }
        let extra = paid.saturating_sub(cost);
        if extra > NearToken::from_yoctonear(0) {
            Promise::new(account.clone()).transfer(extra).detach();
        }
        env::log_str(&format!(
            "EVENT_JSON:{{\"standard\":\"kakurega\",\"version\":\"1.0.0\",\"event\":\"attested\",\"data\":[{{\"signer\":\"{}\",\"tcb_status\":\"{}\",\"account\":\"{account}\"}}]}}",
            found.signer, found.tcb_status
        ));
        Some(found)
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
        self.register_inner(
            capsule_sha256,
            capsule_signer,
            binding_signature,
            receipts,
            None,
        )
    }

    fn register_inner(
        &mut self,
        capsule_sha256: String,
        capsule_signer: String,
        binding_signature: String,
        receipts: Vec<ReceiptArg>,
        order: Option<u64>,
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
        let mut message = format!(
            "{BINDING_PREFIX}:{capsule_sha256}:{}:{account}",
            receipts_digest(&receipts)
        );
        if let Some(id) = order {
            message.push_str(&format!(":{ORDER_SUFFIX}:{id}"));
        }
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
                signer_attested_at: self.attested.get(&r.signer).map(|a| a.attested_at),
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
            order: order.map(U64),
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
        Registry::new(
            "verifier.testnet".parse().unwrap(),
            Some("usdc.testnet".parse().unwrap()),
            Some(U128(1_000_000)),
        )
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

    // ---- the hardware check ----

    use crate::hardware::{
        ReportIn, TDReport15In, VerificationResultIn, VerifiedReportIn, VerifierErrorIn, accept,
        challenge, verifier_args,
    };
    use near_sdk::borsh;

    /// What NEAR's verifier on testnet answered for a real NEAR AI Cloud model quote (see the engineering record).
    const REAL_ANSWER: &[u8] = include_bytes!("../tests/fixtures/verified_report.borsh");
    const REAL_SIGNER: &str = "89154a43a47684742c3d1753252bb6f49f61da9d8c84fc9b1def2df130cb2d93";
    const REAL_NONCE: &str = "1b1c7a663fdc23e87a40182598facae1d31ffba719555b247dbb021557ebfd34";

    fn real_report() -> VerifiedReportIn {
        match borsh::from_slice::<VerificationResultIn>(REAL_ANSWER).expect("the mirror decodes") {
            VerificationResultIn::Verified(v) => v,
            VerificationResultIn::Rejected(_) => panic!("the fixture is a verified answer"),
        }
    }
    /// The real report, re-addressed to `who`'s challenge (as if `who` had asked for the quote).
    fn report_for(who: &str) -> VerifiedReportIn {
        let mut v = real_report();
        let c = challenge(&who.parse().unwrap());
        let ReportIn::TD10(td) = &mut v.report else {
            panic!("the fixture is a TD 1.0 report")
        };
        td.report_data[32..].copy_from_slice(&c);
        v
    }
    fn td_mut(v: &mut VerifiedReportIn) -> &mut crate::hardware::TDReport10In {
        match &mut v.report {
            ReportIn::TD10(td) => td,
            _ => panic!("not TD 1.0"),
        }
    }
    fn attest(r: &mut Registry, who: &str, deposit: u128, v: VerifiedReportIn) -> Option<Attested> {
        as_account("registry.testnet", 0);
        r.on_attested(
            who.parse().unwrap(),
            NearToken::from_yoctonear(deposit),
            Ok(VerificationResultIn::Verified(v)),
        )
    }

    #[test]
    fn hw01_the_real_verifier_answer_decodes_and_carries_the_models_key_and_the_challenge() {
        let v = real_report();
        assert_eq!((v.status.as_str(), v.advisory_ids.len()), ("UpToDate", 0));
        let td = v.report.as_td10().expect("a TDX report");
        assert_eq!(hex(&td.report_data[..32]), REAL_SIGNER);
        assert_eq!(hex(&td.report_data[32..]), REAL_NONCE);
        assert_eq!(td.mr_config_id[0], 1);
    }

    #[test]
    fn hw02_a_quote_made_for_no_account_here_is_refused_as_is() {
        fresh();
        for who in ["alice.testnet", "mallory.testnet"] {
            assert_eq!(
                accept(&real_report(), &who.parse().unwrap(), 5),
                Err("the quote does not answer this account's challenge")
            );
        }
    }

    #[test]
    fn hw03_a_quote_answering_the_callers_challenge_is_read_in_full() {
        fresh();
        let a = accept(
            &report_for("alice.testnet"),
            &"alice.testnet".parse().unwrap(),
            77,
        )
        .unwrap();
        assert_eq!(a.signer, REAL_SIGNER);
        assert_eq!((a.tcb_status.as_str(), a.attested_at.0), ("UpToDate", 77));
        assert_eq!(a.attested_by.as_str(), "alice.testnet");
        assert!(a.mr_td.len() == 96 && a.rt_mr3.len() == 96 && a.mr_td != a.rt_mr0);
        let td = real_report();
        let td = td.report.as_td10().unwrap();
        assert_eq!(a.compose_sha256, Some(hex(&td.mr_config_id[1..33])));
    }

    #[test]
    fn hw04_another_accounts_quote_cannot_be_recorded_by_someone_else() {
        fresh();
        assert!(
            accept(
                &report_for("alice.testnet"),
                &"mallory.testnet".parse().unwrap(),
                1
            )
            .is_err()
        );
    }

    #[test]
    fn hw05_a_weak_key_an_enclave_report_and_an_unrecordable_status_are_refused() {
        fresh();
        let who: AccountId = "alice.testnet".parse().unwrap();
        let mut weak = report_for("alice.testnet");
        td_mut(&mut weak).report_data[..32].copy_from_slice(&[0u8; 32]);
        assert_eq!(
            accept(&weak, &who, 1),
            Err("the bound key is not a usable signing key")
        );
        let mut sgx = report_for("alice.testnet");
        sgx.report = ReportIn::SgxEnclave(crate::hardware::EnclaveReportIn {
            cpu_svn: [0; 16],
            misc_select: 0,
            reserved1: [0; 28],
            attributes: [0; 16],
            mr_enclave: [0; 32],
            reserved2: [0; 32],
            mr_signer: [0; 32],
            reserved3: [0; 96],
            isv_prod_id: 0,
            isv_svn: 0,
            reserved4: [0; 60],
            report_data: [7; 64],
        });
        assert_eq!(accept(&sgx, &who, 1), Err("not a TDX report"));
        let mut long = report_for("alice.testnet");
        long.status = "x".repeat(65);
        assert!(accept(&long, &who, 1).is_err());
        let mut empty = report_for("alice.testnet");
        empty.status.clear();
        assert!(accept(&empty, &who, 1).is_err());
        let mut many = report_for("alice.testnet");
        many.advisory_ids = vec!["INTEL-SA-1".into(); 65];
        assert!(accept(&many, &who, 1).is_err());
    }

    #[test]
    fn hw06_a_newer_report_kind_is_read_through_its_td_body_and_an_unknown_config_form_gives_no_compose()
     {
        fresh();
        let who: AccountId = "alice.testnet".parse().unwrap();
        let mut v = report_for("alice.testnet");
        let base = td_mut(&mut v).clone();
        v.report = ReportIn::TD15(TDReport15In {
            base,
            tee_tcb_svn2: [0; 16],
            mr_service_td: [0; 48],
        });
        assert_eq!(accept(&v, &who, 1).unwrap().signer, REAL_SIGNER);
        let mut other = report_for("alice.testnet");
        td_mut(&mut other).mr_config_id[0] = 2;
        assert_eq!(accept(&other, &who, 1).unwrap().compose_sha256, None);
    }

    #[test]
    fn hw07_a_verified_quote_is_recorded_and_a_receipt_registered_afterwards_says_so() {
        let mut r = fresh();
        assert!(r.hardware(REAL_SIGNER.into()).is_none());
        let got = attest(
            &mut r,
            "alice.testnet",
            DEPOSIT,
            report_for("alice.testnet"),
        )
        .unwrap();
        assert_eq!(r.hardware(REAL_SIGNER.into()), Some(got.clone()));
        assert_eq!(got.attested_at.0, 1_700_000_000_000_000_000);
        // a receipt by a key with no record says None; the registry cannot sign for the real key, so the lookup is checked
        // through a key it can sign with, recorded by hand
        let model = key(2);
        let mut mine = got.clone();
        mine.signer = signer_hex(&model);
        r.attested.insert(mine.signer.clone(), mine);
        let (cap, other) = (key(1), key(3));
        as_account("alice.testnet", DEPOSIT);
        let e = reg(
            &mut r,
            &cap,
            DIGEST,
            vec![
                receipt(&model, "m", &h('b'), &h('c')),
                receipt(&other, "m", &h('d'), &h('e')),
            ],
            "alice.testnet",
        );
        assert_eq!(
            e.receipts[0].signer_attested_at,
            Some(U64(1_700_000_000_000_000_000))
        );
        assert_eq!(e.receipts[1].signer_attested_at, None);
    }

    #[test]
    fn hw08_a_rejected_quote_an_unreachable_verifier_and_a_foreign_quote_record_nothing() {
        let mut r = fresh();
        as_account("registry.testnet", 0);
        let who: AccountId = "alice.testnet".parse().unwrap();
        let paid = NearToken::from_yoctonear(DEPOSIT);
        let rejected = Ok(VerificationResultIn::Rejected(
            VerifierErrorIn::DcapVerification("bad".into()),
        ));
        assert!(r.on_attested(who.clone(), paid, rejected).is_none());
        assert!(
            r.on_attested(who.clone(), paid, Err(PromiseError::Failed))
                .is_none()
        );
        assert!(
            attest(
                &mut r,
                "mallory.testnet",
                DEPOSIT,
                report_for("alice.testnet")
            )
            .is_none()
        );
        assert!(attest(&mut r, "alice.testnet", DEPOSIT, real_report()).is_none());
        assert!(r.hardware(REAL_SIGNER.into()).is_none());
    }

    #[test]
    fn hw08b_every_refusal_says_why_and_sends_the_whole_deposit_back() {
        use near_sdk::test_utils::{get_created_receipts, get_logs};
        let who: AccountId = "alice.testnet".parse().unwrap();
        let paid = NearToken::from_yoctonear(DEPOSIT);
        let cases: Vec<(Result<VerificationResultIn, PromiseError>, &str)> = vec![
            (
                Ok(VerificationResultIn::Rejected(
                    VerifierErrorIn::DcapVerification("bad".into()),
                )),
                "the verifier rejected the quote",
            ),
            (Err(PromiseError::Failed), "the verifier could not be asked"),
            (
                Ok(VerificationResultIn::Verified(real_report())),
                "the quote does not answer this account's challenge",
            ),
        ];
        for (answer, reason) in cases {
            let mut r = fresh();
            as_account("registry.testnet", 0);
            assert!(r.on_attested(who.clone(), paid, answer).is_none());
            let logs = get_logs().join("\n");
            assert!(
                logs.contains("attest_refused") && logs.contains(reason),
                "{reason}: {logs}"
            );
            let sent = format!("{:?}", get_created_receipts());
            assert!(
                sent.contains("alice.testnet") && sent.contains(&DEPOSIT.to_string()),
                "{reason}: the deposit goes back: {sent}"
            );
        }
        // an underpaid record is refused the same way
        let mut r = fresh();
        assert!(attest(&mut r, "alice.testnet", 1, report_for("alice.testnet")).is_none());
        assert!(get_logs().join("\n").contains("does not cover the storage"));
        // and a recorded one says so and returns only what is left over
        let mut r = fresh();
        attest(
            &mut r,
            "alice.testnet",
            DEPOSIT,
            report_for("alice.testnet"),
        )
        .unwrap();
        let logs = get_logs().join("\n");
        assert!(
            logs.contains("\"event\":\"attested\"") && logs.contains(REAL_SIGNER),
            "{logs}"
        );
        let sent = format!("{:?}", get_created_receipts());
        assert!(
            sent.contains("alice.testnet") && !sent.contains(&DEPOSIT.to_string()),
            "{sent}"
        );
    }

    #[test]
    fn hw09_a_deposit_below_the_storage_cost_records_nothing_and_keeps_an_earlier_record() {
        let mut r = fresh();
        assert!(attest(&mut r, "alice.testnet", 1, report_for("alice.testnet")).is_none());
        assert!(r.hardware(REAL_SIGNER.into()).is_none());
        let first = attest(
            &mut r,
            "alice.testnet",
            DEPOSIT,
            report_for("alice.testnet"),
        )
        .unwrap();
        // a later, longer record with too little attached leaves the first one in place
        let mut later = report_for("bob.testnet");
        later.advisory_ids = vec!["INTEL-SA-00000".into(); 40];
        assert!(attest(&mut r, "bob.testnet", 1, later).is_none());
        assert_eq!(r.hardware(REAL_SIGNER.into()), Some(first));
    }

    #[test]
    fn hw10_a_later_verified_quote_for_the_same_key_replaces_the_record() {
        let mut r = fresh();
        attest(
            &mut r,
            "alice.testnet",
            DEPOSIT,
            report_for("alice.testnet"),
        )
        .unwrap();
        let again = attest(&mut r, "bob.testnet", DEPOSIT, report_for("bob.testnet")).unwrap();
        assert_eq!(again.attested_by.as_str(), "bob.testnet");
        assert_eq!(
            r.hardware(REAL_SIGNER.into()).unwrap().attested_by.as_str(),
            "bob.testnet"
        );
    }

    #[test]
    fn hw11_the_challenge_names_the_account_and_the_verifier_gets_the_quote_then_the_collateral() {
        let r = fresh();
        let a = r.challenge_for("alice.testnet".parse().unwrap());
        let b = r.challenge_for("bob.testnet".parse().unwrap());
        assert!(a.len() == 64 && a != b);
        assert_eq!(a, hex(&challenge(&"alice.testnet".parse().unwrap())));
        assert_eq!(
            verifier_args(&[1, 2, 3], &[9, 8]),
            vec![3, 0, 0, 0, 1, 2, 3, 9, 8]
        );
        assert_eq!(r.verifier().as_str(), "verifier.testnet");
    }

    #[test]
    #[should_panic(expected = "the quote is empty or too large")]
    fn hw12_an_empty_quote_is_refused_before_the_verifier_is_asked() {
        let mut r = fresh();
        let _ = r.attest_signer(Base64VecU8(vec![]), Base64VecU8(vec![1]));
    }

    #[test]
    #[should_panic(expected = "the quote is empty or too large")]
    fn hw13_an_oversized_quote_is_refused() {
        let mut r = fresh();
        let _ = r.attest_signer(
            Base64VecU8(vec![0; MAX_QUOTE_BYTES + 1]),
            Base64VecU8(vec![1]),
        );
    }

    #[test]
    #[should_panic(expected = "the collateral is empty or too large")]
    fn hw14_an_oversized_collateral_is_refused() {
        let mut r = fresh();
        let _ = r.attest_signer(
            Base64VecU8(vec![1]),
            Base64VecU8(vec![0; MAX_COLLATERAL_BYTES + 1]),
        );
    }

    // ---- orders: paid on a verified capsule ----

    const T0: u64 = 1_700_000_000_000_000_000;
    const HOUR: u64 = 3_600_000_000_000;
    const ONE_NEAR: u128 = 1_000_000_000_000_000_000_000_000;

    fn at(who: &str, deposit: u128, time: u64) {
        testing_env!(
            VMContextBuilder::new()
                .predecessor_account_id(who.parse().unwrap())
                .current_account_id("registry.testnet".parse().unwrap())
                .attached_deposit(NearToken::from_yoctonear(deposit))
                .block_timestamp(time)
                .build()
        );
    }
    fn terms(cap: &SigningKey, hardware: bool) -> OrderTerms {
        OrderTerms {
            reviewer: "rita.testnet".parse().unwrap(),
            reviewer_key: signer_hex(cap),
            scope_sha256: h('5'),
            deadline: U64(T0 + HOUR),
            require_hardware: hardware,
        }
    }
    /// A registry with one NEAR order from carl to rita, opened at T0.
    fn with_order(cap: &SigningKey, hardware: bool) -> (Registry, U64) {
        let mut r = fresh();
        at("carl.testnet", ONE_NEAR, T0);
        let id = r.open_order(terms(cap, hardware));
        (r, id)
    }
    fn bind_order(
        k: &SigningKey,
        digest: &str,
        receipts: &[ReceiptArg],
        who: &str,
        id: u64,
    ) -> String {
        hex(&k
            .sign(
                format!(
                    "{BINDING_PREFIX}:{digest}:{}:{who}:{ORDER_SUFFIX}:{id}",
                    receipts_digest(receipts)
                )
                .as_bytes(),
            )
            .to_bytes())
    }
    fn deliver(
        r: &mut Registry,
        cap: &SigningKey,
        id: U64,
        receipts: Vec<ReceiptArg>,
        time: u64,
    ) -> Entry {
        at("rita.testnet", DEPOSIT, time);
        let sig = bind_order(cap, DIGEST, &receipts, "rita.testnet", id.0);
        r.register_for_order(id, DIGEST.into(), signer_hex(cap), sig, receipts)
    }
    /// What the contract sent: one line per action, with method names and arguments as text.
    fn sent() -> String {
        use near_sdk::mock::MockAction;
        let mut out = String::new();
        for r in near_sdk::test_utils::get_created_receipts() {
            for a in &r.actions {
                match a {
                    MockAction::Transfer { deposit, .. } => {
                        out.push_str(&format!(
                            "transfer {} to {}\n",
                            deposit.as_yoctonear(),
                            r.receiver_id
                        ));
                    }
                    MockAction::FunctionCallWeight {
                        method_name,
                        args,
                        attached_deposit,
                        ..
                    } => {
                        out.push_str(&format!(
                            "call {} on {} with {} deposit {}\n",
                            String::from_utf8_lossy(method_name),
                            r.receiver_id,
                            String::from_utf8_lossy(args),
                            attached_deposit.as_yoctonear()
                        ));
                    }
                    other => out.push_str(&format!("{other:?}\n")),
                }
            }
        }
        out
    }
    /// Runs one case on its own thread, so its contract state and its panic stay its own. `Err` when it panicked.
    /// The reason a case panicked with, or an empty string when it did not panic.
    fn why<T>(r: std::thread::Result<T>) -> String {
        match r {
            Ok(_) => String::new(),
            Err(e) => e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "panicked".into()),
        }
    }
    fn isolated<T: Send + 'static>(
        f: impl FnOnce() -> T + Send + 'static,
    ) -> std::thread::Result<T> {
        std::thread::spawn(f).join()
    }
    /// Records a hardware entry for a key the tests can sign with.
    fn attested_at(r: &mut Registry, model: &SigningKey, time: u64, status: &str) {
        let mut a = accept(
            &report_for("alice.testnet"),
            &"alice.testnet".parse().unwrap(),
            time,
        )
        .unwrap();
        a.signer = signer_hex(model);
        a.tcb_status = status.into();
        r.attested.insert(a.signer.clone(), a);
    }

    #[test]
    fn od01_an_order_sets_the_deposit_aside_less_its_own_storage() {
        let cap = key(1);
        let (r, id) = with_order(&cap, false);
        let o = r.order(id).unwrap();
        assert_eq!(
            (o.client.as_str(), o.reviewer.as_str()),
            ("carl.testnet", "rita.testnet")
        );
        assert_eq!(
            (o.state, o.token.clone(), o.opened_at.0),
            (OrderState::Open, None, T0)
        );
        assert!(
            o.amount.0 < ONE_NEAR && o.amount.0 > ONE_NEAR - ONE_NEAR / 100,
            "{}",
            o.amount.0
        );
        assert_eq!(r.orders_total().0, 1);
        assert!(r.order(U64(7)).is_none());
    }

    #[test]
    fn od02_terms_that_cannot_be_met_or_checked_are_refused() {
        let bad = |change: fn(&mut OrderTerms), deposit: u128| {
            why(isolated(move || {
                let mut r = fresh();
                at("carl.testnet", deposit, T0);
                let mut t = terms(&key(1), false);
                change(&mut t);
                r.open_order(t)
            }))
        };
        assert!(bad(|t| t.reviewer_key = "zz".into(), ONE_NEAR).contains("reviewer key is 64"));
        assert!(bad(|t| t.reviewer_key = "00".repeat(32), ONE_NEAR).contains("not a usable key"));
        assert!(bad(|t| t.scope_sha256 = "ABC".into(), ONE_NEAR).contains("scope digest"));
        assert!(
            bad(|t| t.deadline = U64(T0 + 1), ONE_NEAR).contains("between a minute and a year")
        );
        assert!(
            bad(|t| t.deadline = U64(T0 + MAX_ORDER_NANOS + 1), ONE_NEAR)
                .contains("between a minute and a year")
        );
        assert!(
            bad(|t| t.reviewer = "carl.testnet".parse().unwrap(), ONE_NEAR)
                .contains("same account")
        );
        assert!(bad(|_| {}, 1_000).contains("at least 0.01 NEAR"));
        assert_eq!(bad(|_| {}, ONE_NEAR), "");
    }

    #[test]
    fn od03_the_reviewers_capsule_for_the_order_earns_it_and_the_money_goes_to_the_reviewer() {
        let cap = key(1);
        let (mut r, id) = with_order(&cap, false);
        let amount = r.order(id).unwrap().amount.0;
        let e = deliver(&mut r, &cap, id, vec![], T0 + 10);
        assert_eq!(e.order, Some(id));
        let o = r.order(id).unwrap();
        assert_eq!(
            (o.state, o.capsule_sha256.as_deref()),
            (OrderState::Paid, Some(DIGEST))
        );
        let out = sent();
        assert!(
            out.contains(&format!("transfer {amount} to rita.testnet")),
            "{out}"
        );
        assert!(
            out.contains("call on_order_sent on registry.testnet"),
            "the transfer is followed by its check: {out}"
        );
    }

    #[test]
    fn od04_nobody_else_no_other_key_no_other_order_and_no_late_capsule_is_paid() {
        let refused = |who: &'static str, signer: u8, sign_for: Option<u64>, time: u64| {
            why(isolated(move || {
                let cap = key(1);
                let (mut r, id) = with_order(&cap, false);
                at(who, DEPOSIT, time);
                let k = key(signer);
                let sig = match sign_for {
                    Some(n) => bind_order(&k, DIGEST, &[], who, n),
                    None => bind(&k, DIGEST, &[], who),
                };
                r.register_for_order(id, DIGEST.into(), signer_hex(&k), sig, vec![])
            }))
        };
        assert!(
            refused("mallory.testnet", 1, Some(0), T0 + 10).contains("only the order's reviewer")
        );
        assert!(
            refused("rita.testnet", 9, Some(0), T0 + 10)
                .contains("not signed by the key the order names")
        );
        assert!(
            refused("rita.testnet", 1, Some(1), T0 + 10)
                .contains("binding signature does not verify")
        );
        assert!(
            refused("rita.testnet", 1, None, T0 + 10).contains("binding signature does not verify")
        );
        assert!(refused("rita.testnet", 1, Some(0), T0 + HOUR + 1).contains("deadline has passed"));
        assert_eq!(
            refused("rita.testnet", 1, Some(0), T0 + HOUR),
            "",
            "at the deadline it still counts"
        );
    }

    #[test]
    #[should_panic(expected = "binding signature does not verify")]
    fn od05_a_signature_made_for_an_order_cannot_register_a_capsule_outside_it() {
        let cap = key(1);
        let (mut r, id) = with_order(&cap, false);
        at("rita.testnet", DEPOSIT, T0 + 10);
        let sig = bind_order(&cap, DIGEST, &[], "rita.testnet", id.0);
        r.register(DIGEST.into(), signer_hex(&cap), sig, vec![]);
    }

    #[test]
    #[should_panic(expected = "the order is not open")]
    fn od06_an_order_is_earned_once() {
        let cap = key(1);
        let (mut r, id) = with_order(&cap, false);
        deliver(&mut r, &cap, id, vec![], T0 + 10);
        at("rita.testnet", DEPOSIT, T0 + 20);
        let sig = bind_order(&cap, &h('9'), &[], "rita.testnet", id.0);
        r.register_for_order(id, h('9'), signer_hex(&cap), sig, vec![]);
    }

    #[test]
    fn od07_an_order_that_asks_for_hardware_is_paid_only_for_receipts_by_keys_attested_since_it_opened()
     {
        let run = |setup: fn(&mut Registry, &SigningKey), with_receipt: bool| {
            isolated(move || {
                let (cap, model) = (key(1), key(2));
                let (mut r, id) = with_order(&cap, true);
                setup(&mut r, &model);
                let receipts = if with_receipt {
                    vec![receipt(&model, "m", &h('b'), &h('c'))]
                } else {
                    vec![]
                };
                deliver(&mut r, &cap, id, receipts, T0 + 100);
                r.order(id).unwrap().state
            })
        };
        let no_record = "no up-to-date hardware record made after the order opened";
        assert!(why(run(|_, _| {}, false)).contains("carries none"));
        assert!(
            why(run(|_, _| {}, true)).contains(no_record),
            "a signer with no hardware record"
        );
        assert!(
            why(run(|r, m| attested_at(r, m, T0 - 1, "UpToDate"), true)).contains(no_record),
            "older than the order"
        );
        assert!(
            why(run(|r, m| attested_at(r, m, T0 + 50, "OutOfDate"), true)).contains(no_record),
            "behind on patches"
        );
        assert_eq!(
            run(|r, m| attested_at(r, m, T0 + 50, "UpToDate"), true).unwrap(),
            OrderState::Paid
        );
        assert_eq!(
            run(|r, m| attested_at(r, m, T0, "UpToDate"), true).unwrap(),
            OrderState::Paid
        );
    }

    #[test]
    fn od08_a_failed_payout_leaves_the_order_earned_and_anyone_can_retry_it() {
        let cap = key(1);
        let (mut r, id) = with_order(&cap, false);
        deliver(&mut r, &cap, id, vec![], T0 + 10);
        at("registry.testnet", 0, T0 + 11);
        assert!(!r.on_order_sent(
            id,
            OrderState::Paid,
            OrderState::Earned,
            Err(PromiseError::Failed)
        ));
        assert_eq!(r.order(id).unwrap().state, OrderState::Earned);
        at("anyone.testnet", 0, T0 + 12);
        r.claim_order(id);
        assert_eq!(r.order(id).unwrap().state, OrderState::Paid);
        assert!(
            sent().contains("to rita.testnet"),
            "the retry pays the reviewer, whoever asked"
        );
        at("registry.testnet", 0, T0 + 13);
        assert!(r.on_order_sent(id, OrderState::Paid, OrderState::Earned, Ok(())));
        assert_eq!(r.order(id).unwrap().state, OrderState::Paid);
    }

    #[test]
    #[should_panic(expected = "not waiting for a payout")]
    fn od09_a_paid_order_cannot_be_claimed_again() {
        let cap = key(1);
        let (mut r, id) = with_order(&cap, false);
        deliver(&mut r, &cap, id, vec![], T0 + 10);
        at("rita.testnet", 0, T0 + 12);
        r.claim_order(id);
    }

    #[test]
    #[should_panic(expected = "not waiting for a payout")]
    fn od10_an_open_order_cannot_be_claimed() {
        let cap = key(1);
        let (mut r, id) = with_order(&cap, false);
        at("rita.testnet", 0, T0 + 12);
        r.claim_order(id);
    }

    #[test]
    fn od11_after_the_deadline_an_unmet_order_goes_back_to_the_client_and_only_then() {
        let cap = key(1);
        let early = isolated(|| {
            let (mut r, id) = with_order(&key(1), false);
            at("carl.testnet", 0, T0 + HOUR);
            r.refund_order(id);
        });
        assert!(why(early).contains("deadline has not passed"));
        let (mut r, id) = with_order(&cap, false);
        let amount = r.order(id).unwrap().amount.0;
        at("anyone.testnet", 0, T0 + HOUR + 1);
        r.refund_order(id);
        assert_eq!(r.order(id).unwrap().state, OrderState::Refunded);
        let out = sent();
        assert!(
            out.contains(&format!("transfer {amount} to carl.testnet")),
            "{out}"
        );
        // a refund that did not go through leaves the order open, and it can be asked for again
        at("registry.testnet", 0, T0 + HOUR + 2);
        assert!(!r.on_order_sent(
            id,
            OrderState::Refunded,
            OrderState::Open,
            Err(PromiseError::Failed)
        ));
        assert_eq!(r.order(id).unwrap().state, OrderState::Open);
        at("anyone.testnet", 0, T0 + HOUR + 3);
        r.refund_order(id);
        assert_eq!(r.order(id).unwrap().state, OrderState::Refunded);
    }

    #[test]
    fn od12_a_paid_or_refunded_order_is_never_sent_twice() {
        let cap = key(1);
        let twice = isolated(|| {
            let cap = key(1);
            let (mut r, id) = with_order(&cap, false);
            deliver(&mut r, &cap, id, vec![], T0 + 10);
            at("carl.testnet", 0, T0 + HOUR + 1);
            r.refund_order(id);
        });
        assert!(
            why(twice).contains("the order is not open"),
            "a paid order cannot also be refunded"
        );
        let again = isolated(|| {
            let (mut r, id) = with_order(&key(1), false);
            at("carl.testnet", 0, T0 + HOUR + 1);
            r.refund_order(id);
            r.refund_order(id);
        });
        assert!(
            why(again).contains("the order is not open"),
            "a refunded order cannot be refunded again"
        );
        // a late success report for one state does not disturb another
        let (mut r, id) = with_order(&cap, false);
        at("registry.testnet", 0, T0 + 1);
        assert!(!r.on_order_sent(
            id,
            OrderState::Paid,
            OrderState::Earned,
            Err(PromiseError::Failed)
        ));
        assert_eq!(
            r.order(id).unwrap().state,
            OrderState::Open,
            "an order that was never marked paid is left as it is"
        );
    }

    #[test]
    fn od13_a_transfer_of_the_accepted_token_with_terms_opens_an_order_and_anything_else_is_handed_back()
     {
        let cap = key(1);
        let mut r = fresh();
        let msg = |t: &OrderTerms| {
            near_sdk::serde_json::json!({"reviewer": t.reviewer, "reviewer_key": t.reviewer_key, "scope_sha256": t.scope_sha256,
                "deadline": t.deadline, "require_hardware": t.require_hardware})
            .to_string()
        };
        let back = |v: PromiseOrValue<U128>| match v {
            PromiseOrValue::Value(u) => u.0,
            PromiseOrValue::Promise(_) => panic!("no promise expected"),
        };
        let carl: AccountId = "carl.testnet".parse().unwrap();
        let good = msg(&terms(&cap, false));
        at("other-token.testnet", 0, T0);
        assert_eq!(
            back(r.ft_on_transfer(carl.clone(), U128(5_000_000), good.clone())),
            5_000_000,
            "another token"
        );
        at("usdc.testnet", 0, T0);
        assert_eq!(
            back(r.ft_on_transfer(carl.clone(), U128(999_999), good.clone())),
            999_999,
            "below the smallest order"
        );
        assert_eq!(
            back(r.ft_on_transfer(carl.clone(), U128(5_000_000), "pay rita".into())),
            5_000_000,
            "no terms"
        );
        let mut weak = terms(&cap, false);
        weak.reviewer_key = "00".repeat(32);
        assert_eq!(
            back(r.ft_on_transfer(carl.clone(), U128(5_000_000), msg(&weak))),
            5_000_000,
            "bad terms"
        );
        assert_eq!(r.orders_total().0, 0, "nothing was stored for any of them");
        assert_eq!(
            back(r.ft_on_transfer(carl.clone(), U128(5_000_000), good)),
            0
        );
        let o = r.order(U64(0)).unwrap();
        assert_eq!(
            (
                o.amount.0,
                o.token.as_ref().map(|t| t.as_str()),
                o.client.as_str()
            ),
            (5_000_000, Some("usdc.testnet"), "carl.testnet")
        );
        // it is paid in that token, to the reviewer, with the one yoctoNEAR a token transfer needs
        deliver(&mut r, &cap, U64(0), vec![], T0 + 10);
        let out = sent();
        assert!(
            out.contains(r#"call ft_transfer on usdc.testnet with {"amount":"5000000","receiver_id":"rita.testnet"} deposit 1"#)
                || out.contains(r#"call ft_transfer on usdc.testnet with {"receiver_id":"rita.testnet","amount":"5000000"} deposit 1"#),
            "{out}"
        );
    }

    #[test]
    fn od14_the_order_message_names_the_order_and_differs_from_the_plain_one() {
        let r = fresh();
        let who: AccountId = "rita.testnet".parse().unwrap();
        let plain = r.binding_message(DIGEST.into(), vec![], who.clone());
        let a = r.order_binding_message(U64(0), DIGEST.into(), vec![], who.clone());
        let b = r.order_binding_message(U64(1), DIGEST.into(), vec![], who);
        assert!(a.starts_with(&plain) && a != plain && a != b && a.ends_with(":order:0"));
        assert_eq!(r.token().unwrap().as_str(), "usdc.testnet");
    }
}
