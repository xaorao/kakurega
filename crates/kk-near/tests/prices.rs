use kk_near::prices::{next_offset, parse_list};
use serde_json::{json, Value};

fn list() -> Value {
    serde_json::from_str(include_str!("fixtures/model-list.json")).unwrap()
}

#[test]
fn pr01_the_published_prices_are_read_as_amounts_for_a_million_tokens() {
    let prices = parse_list(&list());
    let qwen = prices
        .iter()
        .find(|p| p.model == "Qwen/Qwen3.8-27B")
        .expect("listed");
    // 40 and 1350 at scale 9: 0.04 and 1.35 US dollars a million tokens; 18 for a cached token
    assert_eq!(
        (
            qwen.currency.as_str(),
            qwen.input_per_million,
            qwen.output_per_million,
            qwen.cached_input_per_million
        ),
        ("USD", 0.04, 1.35, Some(0.018))
    );
    assert_eq!(
        (qwen.context_tokens, qwen.verifiable),
        (Some(262_144), true)
    );
    // a model NEAR serves without hardware proof is listed as such
    let gpt = prices.iter().find(|p| p.model == "openai/gpt-5").unwrap();
    assert!(!gpt.verifiable);
    assert!(prices.len() >= 5);
}

#[test]
fn pr02_a_price_that_is_missing_malformed_or_in_two_currencies_is_never_guessed() {
    let page = json!({"models": [
        {"modelId": "a/no-output", "inputCostPerToken": {"amount": 40, "scale": 9, "currency": "USD"}},
        {"modelId": "a/text-amount", "inputCostPerToken": {"amount": "40", "scale": 9, "currency": "USD"}, "outputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"}},
        {"modelId": "a/two-currencies", "inputCostPerToken": {"amount": 40, "scale": 9, "currency": "USD"}, "outputCostPerToken": {"amount": 1, "scale": 9, "currency": "EUR"}},
        {"modelId": "a/huge-scale", "inputCostPerToken": {"amount": 40, "scale": 400, "currency": "USD"}, "outputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"}},
        {"modelId": "", "inputCostPerToken": {"amount": 40, "scale": 9, "currency": "USD"}, "outputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"}},
        {"modelId": "a/ok", "inputCostPerToken": {"amount": 2, "scale": 6, "currency": "USD"}, "outputCostPerToken": {"amount": 3, "scale": 6, "currency": "USD"}, "cacheReadCostPerToken": {"amount": 1, "scale": 6, "currency": "EUR"}}
    ]});
    let prices = parse_list(&page);
    assert_eq!(prices.len(), 1, "{prices:?}");
    assert_eq!(
        (
            prices[0].model.as_str(),
            prices[0].input_per_million,
            prices[0].output_per_million,
            prices[0].cached_input_per_million,
            prices[0].verifiable
        ),
        ("a/ok", 2.0, 3.0, None, false)
    );
    assert!(parse_list(&json!({"error": "x"})).is_empty());
    assert!(parse_list(&json!("not an object")).is_empty());
}

#[test]
fn pr03_the_list_is_read_page_by_page_until_its_total() {
    let page = |offset: u64, n: usize, total: u64| json!({"offset": offset, "total": total, "models": vec![json!({}); n]});
    assert_eq!(next_offset(&page(0, 100, 250)), Some(100));
    assert_eq!(next_offset(&page(200, 50, 250)), None);
    assert_eq!(next_offset(&page(0, 0, 250)), None, "an empty page ends it");
    assert_eq!(next_offset(&json!({"models": []})), None);
    // a list that lies about where it is ends the reading instead of overflowing
    assert_eq!(next_offset(&page(u64::MAX, 3, u64::MAX)), None);
    assert_eq!(next_offset(&list()), None);
}

#[test]
fn pr10_the_list_says_which_models_chat_which_call_tools_and_which_have_a_proof_that_can_be_fetched(
) {
    let page = serde_json::json!({"models": [
        {"modelId": "chat/tools", "inputCostPerToken": {"amount": 40, "scale": 9, "currency": "USD"}, "outputCostPerToken": {"amount": 1350, "scale": 9, "currency": "USD"},
         "metadata": {"verifiable": true, "attestationSupported": true, "architecture": {"inputModalities": ["text", "image"], "outputModalities": ["text"]}, "supportedFeatures": ["tools", "reasoning"]}},
        {"modelId": "image/maker", "inputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"}, "outputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"},
         "metadata": {"verifiable": true, "attestationSupported": true, "architecture": {"inputModalities": ["text"], "outputModalities": ["image"]}}},
        {"modelId": "speech/reader", "inputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"}, "outputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"},
         "metadata": {"verifiable": true, "architecture": {"inputModalities": ["audio"], "outputModalities": ["text"]}}},
        {"modelId": "promised/not-fetchable", "inputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"}, "outputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"},
         "metadata": {"verifiable": true, "attestationSupported": false, "architecture": {"inputModalities": ["text"], "outputModalities": ["text"]}, "supportedFeatures": ["json_mode"]}},
        {"modelId": "old/list-entry", "inputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"}, "outputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"},
         "metadata": {"verifiable": true}},
        {"modelId": "Qwen/Qwen3-Reranker-0.6B", "inputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"}, "outputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"},
         "metadata": {"verifiable": true, "architecture": {"inputModalities": ["text"], "outputModalities": ["text"]}}},
        {"modelId": "openai/privacy-filter", "inputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"}, "outputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"},
         "metadata": {"verifiable": true, "architecture": {"inputModalities": ["text"], "outputModalities": ["text"]}}},
        {"modelId": "Qwen/Qwen3-Embedding-0.6B", "inputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"}, "outputCostPerToken": {"amount": 1, "scale": 9, "currency": "USD"},
         "metadata": {"verifiable": true}}
    ]});
    let by: std::collections::BTreeMap<String, kk_near::prices::PublishedPrice> =
        kk_near::prices::parse_list(&page)
            .into_iter()
            .map(|p| (p.model.clone(), p))
            .collect();
    let got = |m: &str| (by[m].chat, by[m].tools, by[m].verifiable);
    assert_eq!(got("chat/tools"), (true, Some(true), true));
    assert_eq!(got("image/maker"), (false, None, true));
    assert_eq!(got("speech/reader"), (false, None, true));
    // a proof that is promised but cannot be fetched is not a proof this product can check
    assert_eq!(got("promised/not-fetchable"), (true, Some(false), false));
    // an entry that says nothing about what it takes is still offered
    // an entry that does not say what it supports is not called "chat only"
    assert_eq!(got("old/list-entry"), (true, None, true));
    // NEAR lists text in and text out for a reranker and for its PII filter (read from the live list): neither holds a
    // conversation, and an embedding model with no modalities stated does not either
    assert!(!by["Qwen/Qwen3-Reranker-0.6B"].chat);
    assert!(!by["openai/privacy-filter"].chat);
    assert!(!by["Qwen/Qwen3-Embedding-0.6B"].chat);
}
