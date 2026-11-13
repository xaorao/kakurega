use crate::collateral::{self, CollateralError};
use base64::Engine as _;
use near_api::{AccountId, NearGas, NearToken, NetworkConfig, Signer};
use serde_json::{json, Value};
use std::fmt;
use std::sync::Arc;

/// Gas for a call that has the verifier check a quote (the verifier alone uses about 176 Tgas).
const ATTEST_GAS: u64 = 300;
const CALL_GAS: u64 = 100;
/// What is attached for a record's storage; the contract returns what it does not use.
const STORAGE_DEPOSIT_MILLINEAR: u128 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Network {
    Testnet,
    Mainnet,
}

impl Network {
    pub fn parse(name: &str) -> Option<Network> {
        match name {
            "testnet" => Some(Network::Testnet),
            "mainnet" => Some(Network::Mainnet),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Network::Testnet => "testnet",
            Network::Mainnet => "mainnet",
        }
    }
}

#[derive(Debug)]
pub enum ChainError {
    /// Something the caller gave is not usable; says what.
    Input(String),
    /// A transaction was asked for on a network this build only reads.
    ReadOnlyNetwork,
    /// The network or a service could not be reached, or answered with an error.
    Unreachable(String),
    /// The contract refused the call; carries its reason.
    Refused(String),
    Collateral(CollateralError),
    /// An answer is not in the expected form.
    Answer(String),
}

impl fmt::Display for ChainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChainError::Input(m) => write!(f, "{m}"),
            ChainError::ReadOnlyNetwork => write!(
                f,
                "this build sends transactions on testnet only; on mainnet it reads"
            ),
            ChainError::Unreachable(m) => write!(f, "could not reach {m}"),
            ChainError::Refused(m) => write!(f, "the contract refused: {m}"),
            ChainError::Collateral(e) => write!(f, "{e}"),
            ChainError::Answer(m) => write!(f, "unexpected answer: {m}"),
        }
    }
}

impl std::error::Error for ChainError {}

impl From<CollateralError> for ChainError {
    fn from(e: CollateralError) -> Self {
        ChainError::Collateral(e)
    }
}

/// The account that pays for a transaction, and its key. The key is read from a file the owner names and is held only while
/// the command runs; it is never written, logged or shown.
pub struct Payer {
    pub account: AccountId,
    signer: Arc<Signer>,
}

impl fmt::Debug for Payer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Payer({})", self.account)
    }
}

impl Payer {
    /// Reads a NEAR key file (JSON with `private_key`, as NEAR's tools write it). `account` is needed when the file does not
    /// name one. A file other users can read is refused.
    pub fn from_key_file(
        path: &std::path::Path,
        account: Option<&str>,
    ) -> Result<Payer, ChainError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path)
                .map_err(|_| ChainError::Input("the key file cannot be read".into()))?
                .permissions()
                .mode();
            if mode & 0o077 != 0 {
                return Err(ChainError::Input(
                    "the key file can be read by other users; run chmod 600 on it".into(),
                ));
            }
        }
        let text = std::fs::read_to_string(path)
            .map_err(|_| ChainError::Input("the key file cannot be read".into()))?;
        Payer::from_key_json(&text, account)
    }

    pub fn from_key_json(text: &str, account: Option<&str>) -> Result<Payer, ChainError> {
        let bad = || ChainError::Input("the key file is not a NEAR key file".into());
        let file: Value = serde_json::from_str(text).map_err(|_| bad())?;
        let named = file["account_id"].as_str().or(account).ok_or_else(|| {
            ChainError::Input("the key file names no account; give one with --account".into())
        })?;
        if let (Some(given), Some(in_file)) = (account, file["account_id"].as_str()) {
            if given != in_file {
                return Err(ChainError::Input(
                    "the account given is not the one the key file names".into(),
                ));
            }
        }
        let account: AccountId = named
            .parse()
            .map_err(|_| ChainError::Input("that is not a NEAR account name".into()))?;
        let secret = file["private_key"]
            .as_str()
            .ok_or_else(bad)?
            .parse()
            .map_err(|_| bad())?;
        let signer = Signer::from_secret_key(secret).map_err(|_| bad())?;
        Ok(Payer { account, signer })
    }
}

/// A model's hardware quote as NEAR AI Cloud reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelQuote {
    /// The model's Ed25519 signing key, 64 hex digits.
    pub signer: String,
    pub quote: Vec<u8>,
}

/// What a sent transaction did.
#[derive(Debug, Clone, PartialEq)]
pub struct Sent {
    pub transaction: String,
    pub gas_tgas: f64,
}

pub struct Chain {
    network: Network,
    net: NetworkConfig,
    registry: AccountId,
    http: reqwest::Client,
    near_ai: String,
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

/// Reads the first model's key and quote out of a NEAR AI Cloud report, and checks they have the expected form.
pub fn read_model_quote(report: &Value) -> Result<ModelQuote, ChainError> {
    let m = &report["model_attestations"][0];
    let signer = m["signing_address"]
        .as_str()
        .filter(|s| is_hex(s, 64))
        .ok_or_else(|| ChainError::Answer("the report names no Ed25519 signing key".into()))?
        .to_string();
    let quote = m["intel_quote"]
        .as_str()
        .and_then(unhex)
        .filter(|q| (1024..=16 * 1024).contains(&q.len()))
        .ok_or_else(|| {
            ChainError::Answer("the report carries no quote of a plausible size".into())
        })?;
    Ok(ModelQuote { signer, quote })
}

impl Chain {
    pub fn new(network: Network, registry: &str) -> Result<Chain, ChainError> {
        let registry: AccountId = registry
            .parse()
            .map_err(|_| ChainError::Input("the registry is not a NEAR account name".into()))?;
        let net = match network {
            Network::Testnet => NetworkConfig::testnet(),
            Network::Mainnet => NetworkConfig::mainnet(),
        };
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(40))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| ChainError::Unreachable(e.to_string()))?;
        Ok(Chain {
            network,
            net,
            registry,
            http,
            near_ai: crate::NEAR_AI.to_string(),
        })
    }

    /// The HTTP client this chain uses for the services beside NEAR's RPC.
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn network(&self) -> &Network {
        &self.network
    }

    pub fn registry(&self) -> &str {
        self.registry.as_str()
    }

    /// Calls a read-only method of the registry. Costs nothing and needs no account.
    pub async fn view(&self, method: &str, args: Value) -> Result<Value, ChainError> {
        near_api::Contract(self.registry.clone())
            .call_function(method, args)
            .read_only::<Value>()
            .fetch_from(&self.net)
            .await
            .map(|d| d.data)
            .map_err(|e| {
                // the node answered, but the account runs no such method (an older registry, or not a registry at all)
                let text = format!("{e:?}");
                if text.contains("MethodNotFound") || text.contains("MethodResolveError") {
                    ChainError::Answer(format!(
                        "{} on {} has no `{method}`: it is not a registry this build knows, or an older one",
                        self.registry,
                        self.network.name()
                    ))
                } else if text.contains("CodeDoesNotExist") || text.contains("UnknownAccount") {
                    ChainError::Answer(format!(
                        "{} on {} holds no contract",
                        self.registry,
                        self.network.name()
                    ))
                } else {
                    ChainError::Unreachable(format!("the registry on {}", self.network.name()))
                }
            })
    }

    /// What the registry recorded about a model signing key's hardware quote, or `None`.
    pub async fn hardware(&self, signer: &str) -> Result<Option<Value>, ChainError> {
        if !is_hex(signer, 64) {
            // an Ethereum-style address is the ECDSA form of a key, which the registry does not record
            return Err(ChainError::Input(if signer.starts_with("0x") {
                "that is an Ethereum-style address (an ECDSA key); the registry records the Ed25519 key that signs the model's answers: 64 lowercase hex digits, as the hardware panel in Chat shows it".into()
            } else {
                "a signing key is 64 lowercase hex digits (the Ed25519 key that signs the model's answers)".into()
            }));
        }
        let v = self.view("hardware", json!({"signer": signer})).await?;
        Ok((!v.is_null()).then_some(v))
    }

    pub async fn order(&self, id: u64) -> Result<Option<Value>, ChainError> {
        let v = self
            .view("order", json!({"order_id": id.to_string()}))
            .await?;
        Ok((!v.is_null()).then_some(v))
    }

    pub async fn entry(
        &self,
        capsule_sha256: &str,
        capsule_signer: &str,
    ) -> Result<Option<Value>, ChainError> {
        if !is_hex(capsule_sha256, 64) || !is_hex(capsule_signer, 64) {
            return Err(ChainError::Input(
                "a capsule digest and a signing key are 64 lowercase hex digits each".into(),
            ));
        }
        let v = self
            .view(
                "get",
                json!({"capsule_sha256": capsule_sha256, "capsule_signer": capsule_signer}),
            )
            .await?;
        Ok((!v.is_null()).then_some(v))
    }

    /// Asks NEAR AI Cloud for a model's hardware quote that answers `challenge` (64 hex digits). Needs no key.
    pub async fn model_quote(
        &self,
        model: &str,
        challenge: &str,
    ) -> Result<ModelQuote, ChainError> {
        if model.is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
            return Err(ChainError::Input(
                "a model name is 1 to 200 plain characters".into(),
            ));
        }
        if !is_hex(challenge, 64) {
            return Err(ChainError::Answer(
                "the registry's challenge is not 64 hex digits".into(),
            ));
        }
        let r = self
            .http
            .get(format!("{}/attestation/report", self.near_ai))
            .query(&[
                ("model", model),
                ("provider", "near"),
                ("nonce", challenge),
                ("include_tls_fingerprint", "false"),
                ("signing_algo", "ed25519"),
            ])
            .send()
            .await
            .map_err(|_| ChainError::Unreachable("NEAR AI Cloud".into()))?;
        if !r.status().is_success() {
            return Err(ChainError::Unreachable(format!(
                "NEAR AI Cloud (it answered {})",
                r.status().as_u16()
            )));
        }
        let report: Value = r
            .json()
            .await
            .map_err(|_| ChainError::Answer("NEAR AI Cloud's report is not JSON".into()))?;
        read_model_quote(&report)
    }

    fn writable(&self) -> Result<(), ChainError> {
        match self.network {
            Network::Testnet => Ok(()),
            Network::Mainnet => Err(ChainError::ReadOnlyNetwork),
        }
    }

    async fn send(
        &self,
        payer: &Payer,
        method: &str,
        args: Value,
        gas_tgas: u64,
        deposit: NearToken,
    ) -> Result<Sent, ChainError> {
        self.writable()?;
        let done = near_api::Contract(self.registry.clone())
            .call_function(method, args)
            .transaction()
            .gas(NearGas::from_tgas(gas_tgas))
            .deposit(deposit)
            .with_signer(payer.account.clone(), payer.signer.clone())
            .send_to(&self.net)
            .await
            .map_err(|_| ChainError::Unreachable(format!("NEAR {}", self.network.name())))?
            .into_full()
            .ok_or_else(|| ChainError::Answer("the transaction has no final result yet".into()))?;
        let sent = Sent {
            transaction: done.outcome().transaction_hash.to_string(),
            gas_tgas: done.total_gas_burnt.as_gas() as f64 / 1e12,
        };
        done.into_result()
            .map_err(|e| ChainError::Refused(reason_of(&format!("{e:?}"))))?;
        Ok(sent)
    }

    /// Has NEAR's verifier check `model`'s hardware quote and the registry record its signing key, paid by `payer`.
    /// Returns the transaction and the record as the chain now holds it.
    pub async fn attest_model(
        &self,
        payer: &Payer,
        model: &str,
    ) -> Result<(Sent, Value), ChainError> {
        self.writable()?;
        let challenge = self
            .view("challenge_for", json!({"account_id": payer.account}))
            .await?;
        let challenge = challenge
            .as_str()
            .ok_or_else(|| ChainError::Answer("the registry gave no challenge".into()))?;
        let q = self.model_quote(model, challenge).await?;
        let parts = collateral::fetch(&self.http, &q.quote).await?;
        let b64 = base64::engine::general_purpose::STANDARD;
        let args = json!({
            "quote": b64.encode(&q.quote),
            "collateral": b64.encode(collateral::encode(&parts)?),
        });
        let sent = self
            .send(
                payer,
                "attest_signer",
                args,
                ATTEST_GAS,
                NearToken::from_millinear(STORAGE_DEPOSIT_MILLINEAR),
            )
            .await?;
        let record = self.hardware(&q.signer).await?.ok_or_else(|| {
            ChainError::Refused(
                "the verifier did not accept the quote, or it did not answer this account's challenge; nothing was recorded and the deposit was returned"
                    .into(),
            )
        })?;
        if record["attested_by"] != json!(payer.account.as_str()) {
            return Err(ChainError::Refused(
                "the registry holds a record for this key made by another account, and did not replace it".into(),
            ));
        }
        Ok((sent, record))
    }

    /// What the registry takes on top of each paid order, and the only account it sends that to.
    pub async fn fee_terms(&self) -> Result<Value, ChainError> {
        self.view("fee_terms", json!({})).await
    }

    /// What a client sets aside so the reviewer receives `amount` (yoctoNEAR, or the token's smallest unit): the amount,
    /// the fee on it and their sum, as the registry computes them.
    pub async fn price(&self, amount: u128) -> Result<(u128, u128, u128), ChainError> {
        let p = self
            .view("price", json!({"amount": amount.to_string()}))
            .await?;
        let num = |k: &str| {
            p[k].as_str()
                .and_then(|s| s.parse::<u128>().ok())
                .ok_or_else(|| ChainError::Answer(format!("the registry's price has no {k}")))
        };
        Ok((num("amount")?, num("fee")?, num("total")?))
    }

    /// NEAR an account set aside at the registry for its token orders' storage (or change it was owed back).
    pub async fn storage_balance(&self, account: &str) -> Result<u128, ChainError> {
        self.view("storage_balance_of", json!({"account_id": account}))
            .await?
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| ChainError::Answer("the registry gave no storage balance".into()))
    }

    /// Sets `amount` of NEAR aside at the registry for the payer's token orders.
    pub async fn storage_deposit(
        &self,
        payer: &Payer,
        amount: NearToken,
    ) -> Result<Sent, ChainError> {
        self.send(payer, "storage_deposit", json!({}), CALL_GAS, amount)
            .await
    }

    /// Takes the payer's whole storage balance back.
    pub async fn storage_withdraw(&self, payer: &Payer) -> Result<Sent, ChainError> {
        self.send(
            payer,
            "storage_withdraw",
            json!({}),
            CALL_GAS,
            NearToken::from_yoctonear(0),
        )
        .await
    }

    /// Fees earned and not yet sent to the fee account.
    pub async fn fees_owed(&self) -> Result<Value, ChainError> {
        self.view("fees_owed", json!({})).await
    }

    /// Sends the fees owed, in NEAR or in the token, to the registry's fee account. Anyone may send it; the money only goes there.
    pub async fn withdraw_fees(&self, payer: &Payer, in_token: bool) -> Result<Sent, ChainError> {
        self.send(
            payer,
            "withdraw_fees",
            json!({"in_token": in_token}),
            CALL_GAS,
            NearToken::from_yoctonear(0),
        )
        .await
    }

    /// Opens an order that pays the reviewer `amount` of NEAR for a review. The registry's price for that amount (the amount
    /// and its fee) is attached with room for the order's storage; the contract keeps exactly the price and the storage and
    /// hands the rest back. Returns the transaction and the new order's number.
    pub async fn open_order(
        &self,
        payer: &Payer,
        terms: Value,
        amount: NearToken,
    ) -> Result<(Sent, u64), ChainError> {
        self.writable()?;
        let (priced, fee, total) = self.price(amount.as_yoctonear()).await?;
        // what an RPC reports is checked before anything is signed: the amount asked for, a fee no registry can exceed
        let want = amount.as_yoctonear();
        let most = crate::price_with(want, crate::MAX_FEE_BPS).map(|(_, f)| f);
        if priced != want || most.is_none_or(|m| fee > m) || priced.checked_add(fee) != Some(total)
        {
            return Err(ChainError::Answer(format!(
                "the registry's price does not add up (amount {priced}, fee {fee}, total {total} for {want}); nothing was sent"
            )));
        }
        let mut terms = terms;
        terms["amount"] = json!(amount.as_yoctonear().to_string());
        let attach =
            NearToken::from_yoctonear(total.saturating_add(
                NearToken::from_millinear(STORAGE_DEPOSIT_MILLINEAR).as_yoctonear(),
            ));
        let before: u64 = self
            .view("orders_total", json!({}))
            .await?
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| ChainError::Answer("the registry gave no order count".into()))?;
        let sent = self
            .send(
                payer,
                "open_order",
                json!({"terms": terms}),
                CALL_GAS,
                attach,
            )
            .await?;
        // the order is the newest one this account opened at or after the count read before sending
        let after: u64 = self
            .view("orders_total", json!({}))
            .await?
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(before);
        for id in (before..after).rev() {
            if let Some(o) = self.order(id).await? {
                if o["client"] == json!(payer.account.as_str())
                    && o["scope_sha256"] == terms["scope_sha256"]
                {
                    return Ok((sent, id));
                }
            }
        }
        Err(ChainError::Answer(
            "the order was sent but could not be found afterwards".into(),
        ))
    }

    /// Registers a capsule with what the owner's service prepared for it (`ticket`: the digest, the signing key, its signature
    /// over the registry's message for the payer's account, and the model receipts). With an order in the ticket, the
    /// registration is for that order and the contract pays it out. Returns the transaction and the entry as the chain holds it.
    pub async fn register(
        &self,
        payer: &Payer,
        ticket: &Value,
    ) -> Result<(Sent, Value), ChainError> {
        self.writable()?;
        let field = |name: &str, len: usize| -> Result<&str, ChainError> {
            ticket[name]
                .as_str()
                .filter(|s| is_hex(s, len))
                .ok_or_else(|| {
                    ChainError::Input(format!("the ticket's {name} is not {len} hex digits"))
                })
        };
        let (digest, signer, signature) = (
            field("capsule_sha256", 64)?,
            field("capsule_signer", 64)?,
            field("binding_signature", 128)?,
        );
        if ticket["account"] != json!(payer.account.as_str()) {
            return Err(ChainError::Input(
                "the ticket was made for another account than the one that pays".into(),
            ));
        }
        let receipts = ticket["receipts"]
            .as_array()
            .ok_or_else(|| ChainError::Input("the ticket carries no receipt list".into()))?;
        let mut args = json!({
            "capsule_sha256": digest,
            "capsule_signer": signer,
            "binding_signature": signature,
            "receipts": receipts,
        });
        let method = match ticket["order"].as_u64() {
            Some(id) => {
                args["order_id"] = json!(id.to_string());
                "register_for_order"
            }
            None if ticket["order"].is_null() => "register",
            None => {
                return Err(ChainError::Input(
                    "the ticket's order is not a number".into(),
                ))
            }
        };
        let sent = self
            .send(
                payer,
                method,
                args,
                CALL_GAS,
                NearToken::from_millinear(STORAGE_DEPOSIT_MILLINEAR),
            )
            .await?;
        let entry = self.entry(digest, signer).await?.ok_or_else(|| {
            ChainError::Answer("the registration was sent but is not on the chain".into())
        })?;
        Ok((sent, entry))
    }

    /// Returns an unmet order to its client after the deadline. Anyone may send it.
    pub async fn refund_order(&self, payer: &Payer, id: u64) -> Result<Sent, ChainError> {
        self.send(
            payer,
            "refund_order",
            json!({"order_id": id.to_string()}),
            CALL_GAS,
            NearToken::from_yoctonear(0),
        )
        .await
    }

    /// Retries the payout of an order whose capsule is registered. Anyone may send it.
    pub async fn claim_order(&self, payer: &Payer, id: u64) -> Result<Sent, ChainError> {
        self.send(
            payer,
            "claim_order",
            json!({"order_id": id.to_string()}),
            CALL_GAS,
            NearToken::from_yoctonear(0),
        )
        .await
    }
}

/// The contract's own words out of a failed transaction's description, or a plain fallback.
pub fn reason_of(debug: &str) -> String {
    const MARK: &str = "Smart contract panicked: ";
    match debug.find(MARK) {
        Some(i) => {
            let rest = &debug[i + MARK.len()..];
            let end = rest.find(['"', '\\']).unwrap_or(rest.len());
            rest[..end].trim().chars().take(200).collect()
        }
        None => "the transaction failed".into(),
    }
}
