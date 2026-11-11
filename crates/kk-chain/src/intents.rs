//! What it would cost to pay for an order from another chain, through NEAR Intents' 1Click service.
//!
//! A 1Click quote names what the payer sends (any supported coin on any supported chain), what arrives (here: the registry's
//! token on NEAR, an exact amount) and a message handed to the receiving contract with the transfer; the registry reads that
//! message as the order's terms. This module only ever asks for a dry quote: a price, with no deposit address, so nothing can
//! be paid by mistake. 1Click has no test network, so a real payment needs the registry on mainnet and the owner's word.

use crate::ChainError;
use serde_json::{json, Value};

pub const ONE_CLICK: &str = "https://1click.chaindefuser.com";

/// One coin 1Click can take or deliver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub asset_id: String,
    pub symbol: String,
    pub blockchain: String,
    pub decimals: u32,
}

/// A price, not an offer to pay: nothing in it can be deposited to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryQuote {
    /// What the payer would send, in the coin's own units, as 1Click writes it (for example `25.255482`).
    pub pay: String,
    pub pay_symbol: String,
    pub pay_chain: String,
    /// What would arrive at the registry, in the token's smallest units.
    pub arrive_units: String,
    pub arrive: String,
    /// 1Click's estimate of the time from deposit to arrival, in seconds.
    pub seconds: u64,
}

/// Reads 1Click's token list, keeping only well-formed entries.
pub fn read_tokens(list: &Value) -> Vec<Token> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| {
            let text = |k: &str| {
                t[k].as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 200)
                    .map(String::from)
            };
            Some(Token {
                asset_id: text("assetId")?,
                symbol: text("symbol")?,
                blockchain: text("blockchain")?,
                decimals: u32::try_from(t["decimals"].as_u64()?)
                    .ok()
                    .filter(|d| *d <= 36)?,
            })
        })
        .collect()
}

/// The token named `SYMBOL@chain` (for example `USDC@sol`), when the list has exactly one.
pub fn find<'a>(tokens: &'a [Token], name: &str) -> Result<&'a Token, ChainError> {
    let (symbol, chain) = name.split_once('@').ok_or_else(|| {
        ChainError::Input("name a coin as SYMBOL@chain, for example USDC@sol".into())
    })?;
    let mut found = tokens.iter().filter(|t| {
        t.symbol.eq_ignore_ascii_case(symbol) && t.blockchain.eq_ignore_ascii_case(chain)
    });
    match (found.next(), found.next()) {
        (Some(t), None) => Ok(t),
        (None, _) => Err(ChainError::Input(format!("1Click lists no {symbol} on {chain}"))),
        (Some(_), Some(_)) => Err(ChainError::Input(format!(
            "1Click lists more than one {symbol} on {chain}; this build does not choose between them"
        ))),
    }
}

/// What a payer asks a price for: `arrive_units` of `arrive` delivered to `registry`, with `terms` as the transfer's message.
#[derive(Debug, Clone, Copy)]
pub struct Ask<'a> {
    pub pay: &'a Token,
    pub arrive: &'a Token,
    pub arrive_units: u128,
    /// The registry's account on mainnet. 1Click refuses a recipient that does not exist.
    pub registry: &'a str,
    /// Where 1Click would return the payer's coins, on the paying chain.
    pub refund_to: &'a str,
    pub terms: &'a Value,
    pub deadline_rfc3339: &'a str,
}

/// The request for a dry quote. `dry` is always true here.
pub fn dry_request(ask: &Ask<'_>) -> Value {
    json!({
        "dry": true,
        "swapType": "EXACT_OUTPUT",
        "originAsset": ask.pay.asset_id,
        "destinationAsset": ask.arrive.asset_id,
        "amount": ask.arrive_units.to_string(),
        "recipient": ask.registry,
        "refundTo": ask.refund_to,
        "depositType": "ORIGIN_CHAIN",
        "recipientType": "DESTINATION_CHAIN",
        "refundType": "ORIGIN_CHAIN",
        "slippageTolerance": 100,
        "deadline": ask.deadline_rfc3339,
        "customRecipientMsg": ask.terms.to_string(),
    })
}

/// Reads 1Click's answer to a dry request. An answer that carries a deposit address is refused: it would not be dry.
pub fn read_quote(answer: &Value, pay: &Token, sent_terms: &Value) -> Result<DryQuote, ChainError> {
    let q = &answer["quote"];
    if !q["depositAddress"].is_null() {
        return Err(ChainError::Answer(
            "the quote carries a deposit address; it is not a dry quote".into(),
        ));
    }
    if answer["quoteRequest"]["dry"] != json!(true) {
        return Err(ChainError::Answer(
            "the service did not confirm the quote as dry".into(),
        ));
    }
    if answer["quoteRequest"]["customRecipientMsg"] != json!(sent_terms.to_string()) {
        return Err(ChainError::Answer(
            "the service did not keep the order's terms as the transfer's message".into(),
        ));
    }
    let text = |k: &str| {
        q[k].as_str()
            .filter(|s| {
                !s.is_empty() && s.len() <= 60 && s.bytes().all(|c| c.is_ascii_digit() || c == b'.')
            })
            .map(String::from)
            .ok_or_else(|| ChainError::Answer(format!("the quote has no readable {k}")))
    };
    Ok(DryQuote {
        pay: text("amountInFormatted")?,
        pay_symbol: pay.symbol.clone(),
        pay_chain: pay.blockchain.clone(),
        arrive_units: text("amountOut")?,
        arrive: text("amountOutFormatted")?,
        seconds: q["timeEstimate"].as_u64().unwrap_or(0),
    })
}

async fn get_json(
    http: &reqwest::Client,
    url: String,
    body: Option<&Value>,
) -> Result<Value, ChainError> {
    let req = match body {
        Some(b) => http.post(&url).json(b),
        None => http.get(&url),
    };
    let r = req
        .send()
        .await
        .map_err(|_| ChainError::Unreachable("NEAR Intents (1Click)".into()))?;
    let status = r.status();
    let v: Value = r
        .json()
        .await
        .map_err(|_| ChainError::Answer("1Click's answer is not JSON".into()))?;
    if !status.is_success() {
        let why: String = v["message"]
            .as_str()
            .unwrap_or("no reason given")
            .chars()
            .filter(|c| !c.is_control())
            .take(160)
            .collect();
        return Err(ChainError::Refused(format!(
            "1Click answered {}: {why}",
            status.as_u16()
        )));
    }
    Ok(v)
}

pub async fn tokens(http: &reqwest::Client) -> Result<Vec<Token>, ChainError> {
    Ok(read_tokens(
        &get_json(http, format!("{ONE_CLICK}/v0/tokens"), None).await?,
    ))
}

/// Asks 1Click for a dry quote. No deposit address is requested and none is accepted in the answer.
pub async fn dry_quote(http: &reqwest::Client, ask: &Ask<'_>) -> Result<DryQuote, ChainError> {
    let answer = get_json(
        http,
        format!("{ONE_CLICK}/v0/quote"),
        Some(&dry_request(ask)),
    )
    .await?;
    read_quote(&answer, ask.pay, ask.terms)
}
