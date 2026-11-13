//! NEAR AI Cloud's own published prices, read from its public model list (`GET {base}/model/list`, no account key). A price
//! is a whole number of currency units at a decimal scale, per token: 40 at scale 9 is 0.000000040 US dollars a token, or
//! 0.04 for a million. Only what the list says is taken; a model or a field it does not give has no price.

use serde_json::Value;

/// Where the list is read from, after the route's base address.
pub const LIST_PATH: &str = "model/list";

#[derive(Debug, Clone, PartialEq)]
pub struct PublishedPrice {
    pub model: String,
    pub currency: String,
    pub input_per_million: f64,
    pub output_per_million: f64,
    /// What a token read again from NEAR's cache costs, when the list gives it: a prompt that repeats an earlier one is
    /// billed less, so a cost counted at the input price is at most what was charged for it.
    pub cached_input_per_million: Option<f64>,
    /// The longest conversation the model takes, in tokens, when the list says.
    pub context_tokens: Option<u64>,
    /// Whether NEAR marks the model as running in hardware whose proof can be checked.
    pub verifiable: bool,
    /// Whether the list says the model takes text and answers in text. A list that does not say counts as yes, so an
    /// older or shorter list still offers its models; an image, speech or embedding model says otherwise.
    pub chat: bool,
    /// Whether the list says the model can call tools, which a review and an agent need. `None` when the list says
    /// nothing about what the model supports: unknown is not "cannot".
    pub tools: Option<bool>,
}

/// One price field (`{"amount": 40, "scale": 9, "currency": "USD"}`) as an amount for a million tokens and its currency.
fn per_million(v: &Value) -> Option<(f64, String)> {
    let amount = v.get("amount")?.as_u64()?;
    let scale = v.get("scale")?.as_u64()?;
    let currency = v.get("currency")?.as_str()?;
    if scale > 18 || currency.is_empty() || currency.len() > 8 {
        return None;
    }
    // dividing by an exact power of ten gives the nearest number to the written one (18 at scale 9 is 0.018 exactly as shown)
    let per = if scale >= 6 {
        amount as f64 / 10f64.powi(scale as i32 - 6)
    } else {
        amount as f64 * 10f64.powi(6 - scale as i32)
    };
    Some((per, currency.to_string()))
}

/// The prices one page of the list gives. A model whose input or output price is missing, malformed or in two different
/// currencies is left out rather than guessed.
pub fn parse_list(page: &Value) -> Vec<PublishedPrice> {
    let Some(models) = page.get("models").and_then(Value::as_array) else {
        return Vec::new();
    };
    models
        .iter()
        .filter_map(|m| {
            let model = m.get("modelId")?.as_str()?;
            if model.is_empty() || model.len() > 200 {
                return None;
            }
            let (input, cur) = per_million(m.get("inputCostPerToken")?)?;
            let (output, cur_out) = per_million(m.get("outputCostPerToken")?)?;
            if cur != cur_out {
                return None;
            }
            let cached = m
                .get("cacheReadCostPerToken")
                .and_then(per_million)
                .filter(|(_, c)| *c == cur)
                .map(|(a, _)| a);
            let meta = m.get("metadata");
            Some(PublishedPrice {
                model: model.to_string(),
                currency: cur,
                input_per_million: input,
                output_per_million: output,
                cached_input_per_million: cached,
                context_tokens: meta
                    .and_then(|x| x.get("contextLength"))
                    .and_then(Value::as_u64),
                // usable here only when the proof can be fetched as well as promised
                verifiable: meta
                    .and_then(|x| x.get("verifiable"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    && meta
                        .and_then(|x| x.get("attestationSupported"))
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                chat: {
                    let kinds = |key: &str| {
                        meta.and_then(|x| x.get("architecture"))
                            .and_then(|a| a.get(key))
                            .and_then(Value::as_array)
                            .map(|l| l.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                    };
                    match (kinds("inputModalities"), kinds("outputModalities")) {
                        (Some(input), Some(output)) => {
                            input.contains(&"text") && output == ["text"]
                        }
                        _ => true,
                    }
                },
                tools: meta
                    .and_then(|x| x.get("supportedFeatures"))
                    .and_then(Value::as_array)
                    .map(|l| l.iter().any(|f| f.as_str() == Some("tools"))),
            })
        })
        .collect()
}

/// Where the next page starts, or `None` when this page was the last one. A list that does not say how long it is ends here.
pub fn next_offset(page: &Value) -> Option<u64> {
    let offset = page.get("offset")?.as_u64()?;
    let total = page.get("total")?.as_u64()?;
    let got = page.get("models")?.as_array()?.len() as u64;
    let next = offset.checked_add(got)?;
    (got > 0 && next < total).then_some(next)
}
