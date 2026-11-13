//! An outside model for testing: a hosted model reached over the internet with the owner's own key. It is not private and
//! says so everywhere: its class is `public`, so the kernel sends it nothing from a review, the service gives it only notes
//! that are public, and every screen names the host the question goes to. It exists so the quality of answers, roundtables and
//! team runs can be judged with a strong model before the verified NEAR route is wired; it is never the default.
//!
//! The key is read from the environment, held in memory, sent only to the model's own host, and never written to a log, an
//! error or a record.

use kk_route::{
    ChatMessage, Completion, CompletionRequest, Route, RouteClass, RouteError, RouteEvidence,
};
use serde_json::{json, Value};
use std::time::Duration;

pub const HOST: &str = "https://api.anthropic.com";
/// The API version the request shapes below are written for.
const API_VERSION: &str = "2026-10-21";
/// Lets the service fall back to another model of the same provider when a safety filter declines a request.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// The window used for sizing a conversation. The model's own is larger; this bounds what one question can cost.
pub const CONTEXT_TOKENS: u32 = 200_000;
const MAX_REPLY_BYTES: u64 = 4 * 1024 * 1024;

pub struct OutsideRoute {
    id: String,
    base: String,
    model: String,
    key: String,
    agent: ureq::Agent,
}

fn model_ok(m: &str) -> bool {
    !m.is_empty()
        && m.len() <= 64
        && m.starts_with("claude-")
        && m.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn base_ok(base: &str) -> bool {
    // the real host over TLS, or a stand-in on this computer for tests; nothing else can be given the key
    base == HOST
        || base
            .strip_prefix("http://127.0.0.1:")
            .is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

impl OutsideRoute {
    /// `spec` is `anthropic:MODEL`. The key must look like a key; it is not checked against the service here.
    pub fn new(spec: &str, key: &str, base: &str) -> Result<OutsideRoute, String> {
        let model = spec
            .strip_prefix("anthropic:")
            .filter(|m| model_ok(m))
            .ok_or("the outside model is written as anthropic:MODEL, for example anthropic:claude-sonnet-5-5")?;
        let key = key.trim();
        if key.len() < 20
            || key.len() > 300
            || key.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err("ANTHROPIC_API_KEY is not set to a key".into());
        }
        if !base_ok(base) {
            return Err(
                "the outside model's address is not one this program sends a key to".into(),
            );
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(300)))
            .proxy(None)
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .into();
        Ok(OutsideRoute {
            id: format!("anthropic:{model}"),
            base: base.to_string(),
            model: model.to_string(),
            key: key.to_string(),
            agent,
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    /// The request body for a conversation: system messages become the one system text, the rest keep their order. Two
    /// messages of the same side in a row are joined, because the service wants the sides to alternate.
    pub fn body(&self, request: &CompletionRequest) -> Value {
        let mut system: Vec<&str> = Vec::new();
        let mut turns: Vec<(String, String)> = Vec::new();
        for m in &request.messages {
            match m.role.as_str() {
                "system" => system.push(&m.content),
                role => {
                    let side = if role == "assistant" {
                        "assistant"
                    } else {
                        "user"
                    };
                    match turns.last_mut() {
                        Some((last, text)) if last == side => {
                            text.push_str("\n\n");
                            text.push_str(&m.content);
                        }
                        _ => turns.push((side.to_string(), m.content.clone())),
                    }
                }
            }
        }
        if turns.first().is_none_or(|(side, _)| side != "user") {
            turns.insert(0, ("user".into(), "(continue)".into()));
        }
        let mut body = json!({
            "model": self.model,
            // room for the model's own reasoning as well as the answer it writes
            "max_tokens": u64::from(request.max_tokens).max(1024) + 4096,
            "output_config": {"effort": "medium"},
            "fallbacks": "default",
            "messages": turns.iter().map(|(role, content)| json!({"role": role, "content": content})).collect::<Vec<_>>(),
        });
        if !system.is_empty() {
            body["system"] = json!(system.join("\n\n"));
        }
        body
    }
}

/// The environment variable the key is read from. There is no flag for it: a key on a command line is readable by other
/// programs on the computer.
pub const KEY_VAR: &str = "ANTHROPIC_API_KEY";

/// The outside route when `--route outside` was given: `--outside-model anthropic:MODEL`, the key from the environment,
/// and for tests `--outside-url` naming a stand-in on this computer.
pub fn from_flags(args: &[String]) -> Result<Option<std::sync::Arc<OutsideRoute>>, String> {
    if crate::flag(args, "--route").as_deref() != Some("outside") {
        return Ok(None);
    }
    let spec = crate::flag(args, "--outside-model")
        .ok_or("--route outside needs --outside-model anthropic:MODEL, for example anthropic:claude-sonnet-5-5")?;
    let key = std::env::var(KEY_VAR)
        .map_err(|_| format!("--route outside needs the key in {KEY_VAR}"))?;
    let base = crate::flag(args, "--outside-url").unwrap_or_else(|| HOST.to_string());
    OutsideRoute::new(&spec, &key, &base).map(|r| Some(std::sync::Arc::new(r)))
}

/// Reads the service's answer. Only text blocks are the answer; the model's reasoning blocks are not shown or kept.
pub fn read_reply(status: u16, v: &Value) -> Result<Completion, RouteError> {
    match status {
        200 => {}
        401 | 403 => return Err(RouteError::Unauthorized(status)),
        402 => return Err(RouteError::OutOfCredit),
        404 => return Err(RouteError::NotFound),
        413 => return Err(RouteError::ContextExceeded),
        429 | 500..=599 => {
            return Err(RouteError::Busy(format!(
                "the outside model answered {status}"
            )))
        }
        _ => {
            // the service's own words say what was wrong with the request; they name no secret
            let why: String = v["error"]["message"]
                .as_str()
                .unwrap_or("")
                .chars()
                .filter(|c| !c.is_control())
                .take(200)
                .collect();
            return Err(RouteError::BadResponse(format!(
                "the outside model refused the request ({status}): {why}"
            )));
        }
    }
    if v["stop_reason"] == "refusal" {
        return Err(RouteError::BadResponse(
            "the outside model declined this request".into(),
        ));
    }
    let text: String = v["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("");
    Ok(Completion {
        message: ChatMessage::new("assistant", &text),
        prompt_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0)
            + v["usage"]["cache_read_input_tokens"].as_u64().unwrap_or(0)
            + v["usage"]["cache_creation_input_tokens"]
                .as_u64()
                .unwrap_or(0),
        completion_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0),
        cut_short: v["stop_reason"] == "max_tokens",
        receipt: None,
    })
}

impl Route for OutsideRoute {
    fn id(&self) -> &str {
        &self.id
    }

    fn class(&self) -> RouteClass {
        RouteClass::Public
    }

    fn tee_verified(&self) -> bool {
        false
    }

    fn evidence(&self) -> RouteEvidence {
        RouteEvidence {
            route_id: self.id.clone(),
            class: "public".into(),
            model: self.model.clone(),
            model_digest: None,
            endpoint: self.base.clone(),
            tee_verified: false,
            simulated: false,
            receipt: None,
        }
    }

    fn complete(&self, request: &CompletionRequest) -> Result<Completion, RouteError> {
        if !request.tools.is_empty() {
            // this route answers questions; it is never an agent with tools
            return Err(RouteError::NoEligibleRoute);
        }
        let mut resp = self
            .agent
            .post(&format!("{}/v1/messages", self.base))
            .header("x-api-key", &self.key)
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", FALLBACK_BETA)
            .send_json(self.body(request))
            .map_err(|e| match e {
                ureq::Error::Timeout(_) => {
                    RouteError::TimedOut("the outside model did not answer in time".into())
                }
                // the transport's own message can hold the address but never a header; it is not passed on all the same
                _ => RouteError::Unreachable("the outside model could not be reached".into()),
            })?;
        let status = resp.status().as_u16();
        let v: Value = resp
            .body_mut()
            .with_config()
            .limit(MAX_REPLY_BYTES)
            .read_json()
            .unwrap_or(Value::Null);
        read_reply(status, &v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "test-key-0123456789abcdefghij";

    fn req(messages: Vec<ChatMessage>) -> CompletionRequest {
        CompletionRequest {
            messages,
            tools: vec![],
            max_tokens: 600,
            temperature: 0.2,
        }
    }

    #[test]
    fn ou01_only_a_named_model_a_real_looking_key_and_the_one_host_make_a_route() {
        assert!(OutsideRoute::new("anthropic:claude-sonnet-5-5", KEY, HOST).is_ok());
        assert!(
            OutsideRoute::new("anthropic:claude-sonnet-5-5", KEY, "http://127.0.0.1:18080").is_ok()
        );
        for spec in [
            "claude-sonnet-5-5",
            "anthropic:",
            "anthropic:gpt-9",
            "anthropic:claude-x/../y",
            "openai:claude-sonnet-5-5",
            "anthropic:Claude-Sonnet",
        ] {
            assert!(OutsideRoute::new(spec, KEY, HOST).is_err(), "{spec}");
        }
        for key in [
            "",
            "short",
            "has a space in it 0123456789abcdef",
            "line\nbreak-0123456789abcdefghij",
        ] {
            assert!(
                OutsideRoute::new("anthropic:claude-sonnet-5-5", key, HOST).is_err(),
                "{key:?}"
            );
        }
        // the key goes to the model's own host or to a stand-in on this computer, never anywhere else
        for base in [
            "http://api.anthropic.com",
            "https://api.anthropic.com.evil.example",
            "https://evil.example",
            "http://127.0.0.1",
            "http://127.0.0.1:80/x",
            "http://localhost:8080",
            "https://api.anthropic.com/",
        ] {
            assert!(
                OutsideRoute::new("anthropic:claude-sonnet-5-5", KEY, base).is_err(),
                "{base}"
            );
        }
    }

    #[test]
    fn ou02_it_is_a_public_route_that_the_kernel_gives_nothing_to() {
        let r = OutsideRoute::new("anthropic:claude-sonnet-5-5", KEY, HOST).unwrap();
        assert_eq!(r.class(), RouteClass::Public);
        assert!(!r.tee_verified() && !r.reply_key_bound());
        assert_eq!(
            kk_route::route_ceiling(&r),
            None,
            "no labelled data may be sent to it by the broker"
        );
        let ev = r.evidence();
        assert_eq!(
            (
                ev.class.as_str(),
                ev.endpoint.as_str(),
                ev.tee_verified,
                ev.simulated
            ),
            ("public", HOST, false, false)
        );
        assert!(!format!("{ev:?}").contains(KEY), "the key is in no record");
        // it is never an agent: a request that offers tools is refused before anything is sent
        let mut with_tools = req(vec![ChatMessage::new("user", "hi")]);
        with_tools.tools = vec![kk_route::ToolSpec {
            name: "read_file".into(),
            description: "x".into(),
            parameters: json!({}),
        }];
        assert!(matches!(
            r.complete(&with_tools),
            Err(RouteError::NoEligibleRoute)
        ));
    }

    #[test]
    fn ou03_the_request_holds_the_conversation_in_the_form_the_service_reads() {
        let r = OutsideRoute::new("anthropic:claude-sonnet-5-5", KEY, HOST).unwrap();
        let b = r.body(&req(vec![
            ChatMessage::new("system", "Be brief."),
            ChatMessage::new("system", "You are the security reviewer."),
            ChatMessage::new("user", "first"),
            ChatMessage::new("user", "second"),
            ChatMessage::new("assistant", "answer"),
            ChatMessage::new("user", "third"),
        ]));
        assert_eq!(b["model"], "claude-sonnet-5-5");
        assert_eq!(b["system"], "Be brief.\n\nYou are the security reviewer.");
        assert_eq!(
            b["messages"],
            json!([{"role": "user", "content": "first\n\nsecond"}, {"role": "assistant", "content": "answer"}, {"role": "user", "content": "third"}])
        );
        assert_eq!(b["max_tokens"], 1024 + 4096);
        assert_eq!(b["fallbacks"], "default");
        // settings this version of the service rejects are never sent, and the key is not in the body
        for gone in ["temperature", "top_p", "top_k", "thinking", "tool_choice"] {
            assert!(b.get(gone).is_none(), "{gone}");
        }
        assert!(!b.to_string().contains(KEY));
        // a conversation that starts with the assistant gets a first user line, as the service requires
        let b = r.body(&req(vec![ChatMessage::new("assistant", "hello")]));
        assert_eq!(b["messages"][0]["role"], "user");
        assert!(b.get("system").is_none());
    }

    #[test]
    fn ou04_an_answer_is_its_text_blocks_and_every_other_ending_is_named() {
        let ok = json!({"content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": "Use "}, {"type": "text", "text": "a vault."}], "stop_reason": "end_turn", "usage": {"input_tokens": 12, "cache_read_input_tokens": 3, "output_tokens": 7}});
        let c = read_reply(200, &ok).unwrap();
        assert_eq!(
            (
                c.message.content.as_str(),
                c.prompt_tokens,
                c.completion_tokens,
                c.cut_short
            ),
            ("Use a vault.", 15, 7, false)
        );
        let mut cut = ok.clone();
        cut["stop_reason"] = json!("max_tokens");
        assert!(read_reply(200, &cut).unwrap().cut_short);
        let refused = json!({"content": [], "stop_reason": "refusal", "stop_details": {"type": "refusal", "category": "cyber"}});
        assert!(
            matches!(read_reply(200, &refused), Err(RouteError::BadResponse(m)) if m == "the outside model declined this request")
        );
        assert!(matches!(
            read_reply(401, &Value::Null),
            Err(RouteError::Unauthorized(401))
        ));
        assert!(matches!(
            read_reply(402, &Value::Null),
            Err(RouteError::OutOfCredit)
        ));
        assert!(matches!(
            read_reply(404, &Value::Null),
            Err(RouteError::NotFound)
        ));
        assert!(matches!(
            read_reply(413, &Value::Null),
            Err(RouteError::ContextExceeded)
        ));
        assert!(matches!(
            read_reply(429, &Value::Null),
            Err(RouteError::Busy(_))
        ));
        assert!(matches!(
            read_reply(529, &Value::Null),
            Err(RouteError::Busy(_))
        ));
        // the service's reason for a bad request is passed on cleaned and cut, with nothing of ours in it
        let bad = json!({"error": {"type": "invalid_request_error", "message": format!("bad\u{1b}[2J {}", "x".repeat(500))}});
        match read_reply(400, &bad) {
            Err(RouteError::BadResponse(m)) => assert!(
                m.starts_with("the outside model refused the request (400): bad[2J ")
                    && m.len() < 280
                    && !m.contains('\u{1b}'),
                "{m}"
            ),
            other => panic!("{other:?}"),
        }
    }
}
