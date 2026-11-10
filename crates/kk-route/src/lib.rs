use kk_types::label::{Label, Level};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteClass {
    DeviceOnly,
    VerifiedTee,
    Public,
}

impl RouteClass {
    pub fn as_str(self) -> &'static str {
        match self {
            RouteClass::DeviceOnly => "device_only",
            RouteClass::VerifiedTee => "verified_tee",
            RouteClass::Public => "public",
        }
    }
}

pub use kk_types::chat::{ChatMessage, ToolCall, ToolSpec};

pub mod retry;
pub mod text_calls;
pub mod tokens;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
    pub temperature: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub message: ChatMessage,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteEvidence {
    pub route_id: String,
    pub class: String,
    pub model: String,
    pub model_digest: Option<String>,
    pub endpoint: String,
    pub tee_verified: bool,
    pub simulated: bool,
    /// What a verified route can show about the last answer it gave: who signed it, the hardware proof behind it, the checks it
    /// passed. `None` for a route that has nothing to show (a local model, a scripted one, or no answer yet).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<Value>,
}

#[derive(Debug)]
pub enum RouteError {
    EndpointNotLoopback(String),
    Unreachable(String),
    /// The model answered that it is busy or restarting (429, 502, 503, 504): worth trying again in a moment.
    Busy(String),
    /// The call ran out of time. Not retried: the model was working, and a second try would wait as long again.
    TimedOut(String),
    BadResponse(String),
    NoEligibleRoute,
    ScriptExhausted,
    /// The conversation is larger than the model's context window. Ollama would otherwise cut it silently.
    ContextTooLong {
        requested: u64,
        window: u64,
    },
    /// The model service said the conversation is too large for it, without saying by how much (413, or an error text such as
    /// "maximum context length is ...").
    ContextExceeded,
    /// 401 or 403: the model service did not accept the credentials it was given. Never retried.
    Unauthorized(u16),
    /// 402: the account behind the model service has no credit left. Never retried.
    OutOfCredit,
    /// 404: no such model or address at the model service.
    NotFound,
    /// The model's hardware proof, or the proof of one answer, did not verify. The answer is not given to anyone.
    NotVerified(String),
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RouteError::EndpointNotLoopback(u) => {
                write!(f, "device-only route refuses a non-loopback endpoint: {u}")
            }
            RouteError::Unreachable(m) => write!(f, "model unreachable: {m}"),
            RouteError::Busy(m) => write!(f, "model busy: {m}"),
            RouteError::TimedOut(m) => write!(f, "model took too long: {m}"),
            RouteError::BadResponse(m) => write!(f, "unexpected model response: {m}"),
            RouteError::NoEligibleRoute => {
                f.write_str("no route is eligible for this data; refusing instead of falling back")
            }
            RouteError::ScriptExhausted => f.write_str("the scripted route has no more responses"),
            RouteError::ContextTooLong { requested, window } => write!(
                f,
                "the conversation needs {requested} tokens but the model's window holds {window}"
            ),
            RouteError::ContextExceeded => f.write_str(
                "the conversation is longer than this model accepts: start a new chat or shorten it",
            ),
            RouteError::Unauthorized(code) => write!(
                f,
                "the model service did not accept the credentials it was given (status {code})"
            ),
            RouteError::OutOfCredit => {
                f.write_str("the model service says the account has no credit left (status 402)")
            }
            RouteError::NotFound => f.write_str(
                "the model service has no such model or address (status 404): check the model name",
            ),
            RouteError::NotVerified(why) => write!(
                f,
                "the answer was held back because its proof did not verify: {why}"
            ),
        }
    }
}

impl std::error::Error for RouteError {}

/// The text of a message as it is sent to any model: the tokens a chat template reserves for its own structure are replaced.
pub fn neutralised(content: &str) -> std::borrow::Cow<'_, str> {
    outgoing(content)
}

/// What of a message is sent to a model. The tokens a chat template reserves for its own structure are replaced, because a
/// document, web page or note must never be able to write a system turn into the conversation.
fn outgoing(content: &str) -> std::borrow::Cow<'_, str> {
    kk_types::injection::neutralise_special_tokens(content)
}

pub trait Route: Send + Sync {
    fn id(&self) -> &str;
    fn class(&self) -> RouteClass;
    fn tee_verified(&self) -> bool;
    /// Whether the reply key is bound to the encrypted request (NEAR E2EE v3), so a party holding the ciphertext cannot
    /// replay it and read the answer with a key of its own. Without it a verified route is capped below confidential data.
    fn reply_key_bound(&self) -> bool {
        false
    }
    fn evidence(&self) -> RouteEvidence;
    fn complete(&self, request: &CompletionRequest) -> Result<Completion, RouteError>;
}

/// The most sensitive data a route may receive, decided by the kernel from the route's properties, never claimed by the
/// route itself. `None` means no data at all is sent to it without a separate, explicit approval.
pub fn route_ceiling(r: &dyn Route) -> Option<Level> {
    match r.class() {
        RouteClass::DeviceOnly => Some(Level::Restricted),
        RouteClass::VerifiedTee if r.tee_verified() && r.reply_key_bound() => {
            Some(Level::Restricted)
        }
        RouteClass::VerifiedTee if r.tee_verified() => Some(Level::Internal),
        RouteClass::VerifiedTee => None,
        // public routes take only sanitised or public artifacts the owner approved, which is not this path
        RouteClass::Public => None,
    }
}

/// What a non-success answer from Ollama means. A conversation larger than the window is reported as its own error, so
/// the owner is told to shorten it instead of being shown a bare status.
fn refusal(status: impl fmt::Display, body: &mut ureq::Body) -> RouteError {
    use std::io::Read;
    if matches!(
        status.to_string().split_whitespace().next(),
        Some("429" | "502" | "503" | "504")
    ) {
        return RouteError::Busy(format!("status {status}"));
    }
    let mut text = String::new();
    let _ = body.as_reader().take(4096).read_to_string(&mut text);
    let message = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| {
            v["error"]
                .as_str()
                .or_else(|| v["error"]["message"].as_str())
                .map(str::to_string)
        })
        .unwrap_or(text);
    let code: Option<u16> = status
        .to_string()
        .split_whitespace()
        .next()
        .and_then(|c| c.parse().ok());
    // only fixed words and the status number are kept: what the service wrote is not repeated, it may quote the conversation
    match (overflow_message(&message), code) {
        (Some((requested, window)), _) => RouteError::ContextTooLong { requested, window },
        (None, Some(413)) => RouteError::ContextExceeded,
        (None, Some(401 | 403)) => RouteError::Unauthorized(code.unwrap_or(401)),
        (None, Some(402)) => RouteError::OutOfCredit,
        (None, Some(404)) => RouteError::NotFound,
        (None, Some(400)) if looks_like_context_overflow(&message) => RouteError::ContextExceeded,
        (None, _) => RouteError::BadResponse(format!("status {status}")),
    }
}

/// Whether an error text from a model service says the conversation was too large for the model, in the many ways services say
/// so ("maximum context length", "prompt is too long", "input token count exceeds the limit", "request body too large").
/// Adapted from goose `http_status.rs` (Apache-2.0); the phrase lists are rebuilt here.
#[doc(hidden)]
pub fn looks_like_context_overflow(text: &str) -> bool {
    let t = text.to_lowercase();
    let any = |phrases: &[&str]| phrases.iter().any(|p| t.contains(p));
    if any(&[
        "context length",
        "context_length_exceeded",
        "context window",
        "context_window_exceeded",
        "context limit",
        "maximum context",
        "max context",
        "maximum prompt length",
        "max prompt length",
        "input is too long",
        "input too long",
        "prompt is too long",
        "prompt too long",
    ]) {
        return true;
    }
    if t.contains("reduce the length") && any(&["message", "messages", "input", "prompt"]) {
        return true;
    }
    let overflow = any(&["exceed", "too long", "too large", "over the limit"]);
    let tokens = any(&[
        "input token",
        "input length",
        "prompt token",
        "prompt length",
        "message token",
        "messages token",
        "request token",
        "total token",
    ]);
    let limit = any(&[
        "model limit",
        "model's limit",
        "maximum allowed",
        "max allowed",
        "maximum number of tokens",
        "token limit",
        "tokens limit",
    ]);
    if tokens && limit && overflow {
        return true;
    }
    let words: Vec<&str> = t.split(|c: char| !c.is_ascii_alphanumeric()).collect();
    let bytes_or_request =
        words.contains(&"request") || words.contains(&"byte") || words.contains(&"bytes");
    let size = any(&[
        "request size",
        "requestsize",
        "request body size",
        "request payload size",
        "payload size",
        "body size",
    ]) || any(&[
        "request body is too large",
        "request body too large",
        "request payload is too large",
        "request payload too large",
        "payload is too large",
        "payload too large",
    ]) || (any(&["content length", "content-length"]) && bytes_or_request);
    size && overflow
}

/// Reads "request (N tokens) exceeds the available context size (W tokens)" and returns (N, W).
fn overflow_message(message: &str) -> Option<(u64, u64)> {
    fn number_after(text: &str, marker: &str) -> Option<u64> {
        let rest = &text[text.find(marker)? + marker.len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    }
    Some((
        number_after(message, "request (")?,
        number_after(message, "exceeds the available context size (")?,
    ))
}

pub fn is_loopback_http(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let host = if let Some(stripped) = authority.strip_prefix('[') {
        stripped.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    host == "127.0.0.1" || host == "::1" || host == "localhost"
}

/// The context window asked of Ollama for every request. Ollama's own default is 4096 tokens on most machines, and it
/// cuts a longer conversation from the middle without saying so.
pub const DEFAULT_CONTEXT_TOKENS: u32 = 16_384;

/// The client for a model on this computer: no redirects, no proxy from the environment (a proxy would carry the prompts
/// somewhere else), and a status that is read, not turned into an error.
fn local_agent(limit: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(limit))
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into()
}

/// The client for a streamed call: connecting, sending and waiting for the first reply are bound by the call limit; only the body
/// may go on longer, and its silences are watched by the caller.
fn stream_agent(limit: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(STREAM_TOTAL))
        .timeout_connect(Some(limit))
        .timeout_send_request(Some(limit))
        .timeout_recv_response(Some(limit))
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into()
}

pub struct OllamaRoute {
    id: String,
    base: String,
    model: String,
    agent: ureq::Agent,
    context_tokens: u32,
    retry: retry::Retry,
    /// How long a call may take before its first answer, and (for a call that is not streamed) in all.
    limit: Duration,
    /// How long a streamed answer may go quiet once it has started.
    stall: Duration,
}

/// A streamed answer that has begun may take as long as it likes, but not stay silent longer than this.
pub const DEFAULT_STALL: Duration = Duration::from_secs(120);

/// The most a streamed call is allowed in all: a bound for a connection nobody is reading any more, not a cap on a living answer.
const STREAM_TOTAL: Duration = Duration::from_secs(3600);

impl OllamaRoute {
    pub fn new(base_url: &str, model: &str) -> Result<Self, RouteError> {
        if !is_loopback_http(base_url) {
            return Err(RouteError::EndpointNotLoopback(base_url.to_string()));
        }
        let agent = local_agent(Duration::from_secs(300));
        Ok(OllamaRoute {
            id: format!("ollama:{model}"),
            base: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            agent,
            context_tokens: DEFAULT_CONTEXT_TOKENS,
            retry: retry::Retry::default(),
            limit: Duration::from_secs(300),
            stall: DEFAULT_STALL,
        })
    }

    /// Sets how a call that fails for a passing reason is tried again.
    pub fn with_retry(mut self, retry: retry::Retry) -> Self {
        self.retry = retry;
        self
    }

    /// Sets how long one call may take.
    pub fn with_timeout(mut self, limit: Duration) -> Self {
        self.agent = local_agent(limit);
        self.limit = limit;
        self
    }

    /// Sets how long a streamed answer may stay silent once it has started.
    pub fn with_stall(mut self, stall: Duration) -> Self {
        self.stall = stall;
        self
    }

    /// Posts a chat request, trying again while the model is starting up or busy. Returns once a response with a success status
    /// has arrived; nothing of the answer has been read, so a streamed reply is never started twice.
    fn post_chat(&self, body: &Value) -> Result<ureq::http::Response<ureq::Body>, RouteError> {
        self.post_chat_with(&self.agent, body)
    }

    fn post_chat_with(
        &self,
        agent: &ureq::Agent,
        body: &Value,
    ) -> Result<ureq::http::Response<ureq::Body>, RouteError> {
        retry::run(&self.retry, &std::thread::sleep, || {
            let mut resp = agent
                .post(&format!("{}/api/chat", self.base))
                .send_json(body)
                .map_err(|e| match e {
                    ureq::Error::Timeout(_) => RouteError::TimedOut(e.to_string()),
                    other => RouteError::Unreachable(other.to_string()),
                })?;
            if !resp.status().is_success() {
                return Err(refusal(resp.status(), resp.body_mut()));
            }
            Ok(resp)
        })
    }

    /// Sets the context window (in tokens) asked of the model.
    pub fn with_context_tokens(mut self, tokens: u32) -> Self {
        self.context_tokens = tokens;
        self
    }

    pub fn context_tokens(&self) -> u32 {
        self.context_tokens
    }

    fn options(&self, request: &CompletionRequest) -> Value {
        json!({"num_predict": request.max_tokens, "temperature": request.temperature, "num_ctx": self.context_tokens})
    }

    fn model_digest(&self) -> Option<String> {
        let mut resp = self
            .agent
            .get(&format!("{}/api/tags", self.base))
            .call()
            .ok()?;
        let v: Value = resp.body_mut().read_json().ok()?;
        v["models"]
            .as_array()?
            .iter()
            .find(|m| m["name"] == self.model.as_str())
            .and_then(|m| m["digest"].as_str().map(String::from))
    }
}

impl Route for OllamaRoute {
    fn id(&self) -> &str {
        &self.id
    }

    fn class(&self) -> RouteClass {
        RouteClass::DeviceOnly
    }

    fn tee_verified(&self) -> bool {
        false
    }

    fn evidence(&self) -> RouteEvidence {
        RouteEvidence {
            route_id: self.id.clone(),
            class: "device_only".into(),
            model: self.model.clone(),
            model_digest: self.model_digest(),
            endpoint: self.base.clone(),
            tee_verified: false,
            simulated: false,
            receipt: None,
        }
    }

    fn complete(&self, request: &CompletionRequest) -> Result<Completion, RouteError> {
        let messages: Vec<Value> = request
            .messages
            .iter()
            .map(|m| {
                let mut o = json!({"role": m.role, "content": outgoing(&m.content)});
                if !m.tool_calls.is_empty() {
                    o["tool_calls"] = Value::Array(
                        m.tool_calls
                            .iter()
                            .map(
                                |c| json!({"function": {"name": c.name, "arguments": c.arguments}}),
                            )
                            .collect(),
                    );
                }
                o
            })
            .collect();
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}}))
            .collect();
        let mut body = json!({
            "model": self.model,
            "stream": false,
            "truncate": false,
            "messages": messages,
            "options": self.options(request),
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
        }
        let mut resp = self.post_chat(&body)?;
        let v: Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| RouteError::BadResponse(e.to_string()))?;
        let msg = &v["message"];
        let content = msg["content"].as_str().unwrap_or("").to_string();
        let mut tool_calls = Vec::new();
        if let Some(calls) = msg["tool_calls"].as_array() {
            for (i, c) in calls.iter().enumerate() {
                let name = c["function"]["name"]
                    .as_str()
                    .ok_or_else(|| RouteError::BadResponse("tool call without a name".into()))?;
                tool_calls.push(ToolCall {
                    id: format!("call_{i}"),
                    name: name.to_string(),
                    arguments: c["function"]["arguments"].clone(),
                });
            }
        }
        Ok(Completion {
            message: ChatMessage {
                role: "assistant".into(),
                content,
                tool_calls,
                tool_call_id: None,
            },
            prompt_tokens: v["prompt_eval_count"].as_u64().unwrap_or(0),
            completion_tokens: v["eval_count"].as_u64().unwrap_or(0),
        })
    }
}

impl OllamaRoute {
    /// Streams a reply. `on_chunk` receives each piece of text as the model writes it and returns `false` to stop: the
    /// connection is then dropped, which makes the model stop generating. Returns the text produced so far with
    /// `stopped` set when `on_chunk` ended it early. No tools are offered in a streamed chat.
    pub fn stream(
        &self,
        request: &CompletionRequest,
        on_chunk: &mut dyn FnMut(&str) -> bool,
    ) -> Result<Streamed, RouteError> {
        use std::io::{BufRead, Read};
        let messages: Vec<Value> = request
            .messages
            .iter()
            .map(|m| json!({"role": m.role, "content": outgoing(&m.content)}))
            .collect();
        let body = json!({
            "model": self.model,
            "stream": true,
            "truncate": false,
            "messages": messages,
            "options": self.options(request),
        });
        // The call may take as long as a living answer needs. The first line is waited for as long as the call limit (a model that is
        // loading holds the request open); after that the answer may not stay silent longer than `stall`. The lines are read on
        // a thread of their own so that waiting can be given up without waiting for the connection.
        let long = stream_agent(self.limit);
        let mut resp = self.post_chat_with(&long, &body)?;
        let (tx, rx) = std::sync::mpsc::sync_channel::<std::io::Result<String>>(8);
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(resp.body_mut().as_reader());
            loop {
                let mut line = String::new();
                let n = match reader.by_ref().take(1 << 20).read_line(&mut line) {
                    Ok(n) => n,
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                };
                if n == 0 {
                    return;
                }
                if !line.ends_with('\n') && n >= (1 << 20) {
                    let _ = tx.send(Err(std::io::Error::other("a streamed line was too long")));
                    return;
                }
                if tx.send(Ok(line)).is_err() {
                    return;
                }
            }
        });
        let mut text = String::new();
        let (mut prompt_tokens, mut completion_tokens) = (0, 0);
        let mut started = false;
        loop {
            let wait = if started { self.stall } else { self.limit };
            let line = match rx.recv_timeout(wait) {
                Ok(Ok(line)) => line,
                Ok(Err(e)) => {
                    return Err(if e.to_string().contains("too long") {
                        RouteError::BadResponse("a streamed line was too long".into())
                    } else {
                        RouteError::BadResponse(e.to_string())
                    })
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    return Err(RouteError::TimedOut(if started {
                        format!(
                            "the model went quiet for {} seconds in the middle of its answer",
                            wait.as_secs()
                        )
                    } else {
                        format!(
                            "the model did not start answering within {} seconds",
                            wait.as_secs()
                        )
                    }))
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(RouteError::BadResponse(
                        "the model stopped answering before it finished".into(),
                    ))
                }
            };
            started = true;
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(line.trim())
                .map_err(|e| RouteError::BadResponse(format!("unreadable stream line: {e}")))?;
            if let Some(err) = v["error"].as_str() {
                return Err(overflow_message(err)
                    .map(|(requested, window)| RouteError::ContextTooLong { requested, window })
                    .unwrap_or_else(|| RouteError::BadResponse(err.to_string())));
            }
            if let Some(piece) = v["message"]["content"].as_str() {
                if !piece.is_empty() {
                    text.push_str(piece);
                    if text.len() > MAX_STREAMED_BYTES {
                        return Err(RouteError::BadResponse(
                            "the reply was longer than allowed".into(),
                        ));
                    }
                    if !on_chunk(piece) {
                        return Ok(Streamed {
                            text,
                            stopped: true,
                            prompt_tokens,
                            completion_tokens,
                        });
                    }
                }
            }
            if v["done"].as_bool() == Some(true) {
                prompt_tokens = v["prompt_eval_count"].as_u64().unwrap_or(0);
                completion_tokens = v["eval_count"].as_u64().unwrap_or(0);
                return Ok(Streamed {
                    text,
                    stopped: false,
                    prompt_tokens,
                    completion_tokens,
                });
            }
        }
    }
}

impl OllamaRoute {
    /// Vectors for texts, from the local Ollama's `/api/embed`. Nothing leaves this computer.
    pub fn embed_with(&self, model: &str, texts: &[String]) -> Result<Vec<Vec<f32>>, RouteError> {
        let mut resp = self
            .agent
            .post(&format!("{}/api/embed", self.base))
            .send_json(json!({"model": model, "input": texts}))
            .map_err(|e| RouteError::Unreachable(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(RouteError::BadResponse(format!("status {}", resp.status())));
        }
        let v: Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| RouteError::BadResponse(e.to_string()))?;
        let rows = v["embeddings"]
            .as_array()
            .ok_or_else(|| RouteError::BadResponse("no embeddings in the answer".into()))?;
        if rows.len() != texts.len() {
            return Err(RouteError::BadResponse(
                "the number of vectors does not match the number of texts".into(),
            ));
        }
        rows.iter()
            .map(|r| {
                r.as_array()
                    .filter(|a| !a.is_empty() && a.len() <= 8192)
                    .and_then(|a| {
                        a.iter()
                            .map(|x| x.as_f64().map(|f| f as f32))
                            .collect::<Option<Vec<f32>>>()
                    })
                    .ok_or_else(|| {
                        RouteError::BadResponse("a vector was missing or malformed".into())
                    })
            })
            .collect()
    }

    /// Asks the local Ollama to download this model. Ollama fetches it from its own registry; nothing of the owner's is
    /// sent. `on_progress` gets the step name and the percentage when known, and returns `false` to give up waiting.
    pub fn pull(
        &self,
        on_progress: &mut dyn FnMut(&str, Option<u8>) -> bool,
    ) -> Result<(), RouteError> {
        use std::io::{BufRead, Read};
        let mut resp = self
            .agent
            .post(&format!("{}/api/pull", self.base))
            .send_json(json!({"model": self.model, "stream": true}))
            .map_err(|e| RouteError::Unreachable(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(RouteError::BadResponse(format!("status {}", resp.status())));
        }
        let mut reader = std::io::BufReader::new(resp.body_mut().as_reader());
        let mut line = String::new();
        loop {
            line.clear();
            let n = reader
                .by_ref()
                .take(1 << 20)
                .read_line(&mut line)
                .map_err(|e| RouteError::BadResponse(e.to_string()))?;
            if n == 0 {
                return Err(RouteError::BadResponse(
                    "the download stopped before it finished".into(),
                ));
            }
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(line.trim())
                .map_err(|e| RouteError::BadResponse(format!("unreadable progress line: {e}")))?;
            if let Some(err) = v["error"].as_str() {
                return Err(RouteError::BadResponse(err.to_string()));
            }
            let status = v["status"].as_str().unwrap_or("");
            let pct = match (v["completed"].as_u64(), v["total"].as_u64()) {
                (Some(c), Some(t)) if t > 0 => Some(((c.min(t) * 100) / t) as u8),
                _ => None,
            };
            if !on_progress(status, pct) {
                return Err(RouteError::BadResponse(
                    "stopped waiting for the download".into(),
                ));
            }
            if status == "success" {
                return Ok(());
            }
        }
    }
}

/// The most text one streamed reply may hold.
pub const MAX_STREAMED_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Streamed {
    pub text: String,
    pub stopped: bool,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

pub struct ScriptedRoute {
    id: String,
    class: RouteClass,
    tee_verified: bool,
    reply_key_bound: bool,
    script: Mutex<Vec<ChatMessage>>,
    pub seen: Mutex<Vec<CompletionRequest>>,
}

impl ScriptedRoute {
    pub fn new(
        id: &str,
        class: RouteClass,
        tee_verified: bool,
        mut script: Vec<ChatMessage>,
    ) -> Self {
        script.reverse();
        ScriptedRoute {
            id: id.into(),
            class,
            tee_verified,
            reply_key_bound: false,
            script: Mutex::new(script),
            seen: Mutex::new(Vec::new()),
        }
    }
}

impl ScriptedRoute {
    /// A scripted stand-in for a route whose reply key is bound to the request.
    pub fn with_reply_key_bound(mut self) -> Self {
        self.reply_key_bound = true;
        self
    }
}

impl Route for ScriptedRoute {
    fn id(&self) -> &str {
        &self.id
    }

    fn class(&self) -> RouteClass {
        self.class
    }

    fn tee_verified(&self) -> bool {
        self.tee_verified
    }

    fn reply_key_bound(&self) -> bool {
        self.reply_key_bound
    }

    fn evidence(&self) -> RouteEvidence {
        RouteEvidence {
            route_id: self.id.clone(),
            class: self.class.as_str().into(),
            model: "scripted".into(),
            model_digest: None,
            endpoint: "in-process".into(),
            tee_verified: self.tee_verified,
            simulated: true,
            receipt: None,
        }
    }

    fn complete(&self, request: &CompletionRequest) -> Result<Completion, RouteError> {
        self.seen
            .lock()
            .map_err(|_| RouteError::ScriptExhausted)?
            .push(request.clone());
        let message = self
            .script
            .lock()
            .map_err(|_| RouteError::ScriptExhausted)?
            .pop()
            .ok_or(RouteError::ScriptExhausted)?;
        Ok(Completion {
            message,
            prompt_tokens: 0,
            completion_tokens: 0,
        })
    }
}

pub struct RouteSet {
    routes: Vec<Box<dyn Route>>,
}

impl RouteSet {
    pub fn new(routes: Vec<Box<dyn Route>>) -> Self {
        RouteSet { routes }
    }

    /// The first route, in the order given, whose ceiling covers the data. Data of unknown sensitivity counts as
    /// restricted. Nothing weaker is ever chosen when no route qualifies.
    pub fn eligible_for(&self, taint: &Label) -> Option<&dyn Route> {
        let level = taint.effective_level();
        self.routes
            .iter()
            .map(|r| r.as_ref())
            .find(|r| route_ceiling(*r).is_some_and(|max| level <= max))
    }

    pub fn select(&self, taint: &Label) -> Result<&dyn Route, RouteError> {
        self.eligible_for(taint).ok_or(RouteError::NoEligibleRoute)
    }
}

#[cfg(test)]
mod tests {
    use super::overflow_message;

    #[test]
    fn ollama_overflow_text_is_read_and_nothing_else_is() {
        let ok = "request (7517 tokens) exceeds the available context size (4096 tokens), try increasing it";
        assert_eq!(overflow_message(ok), Some((7517, 4096)));
        assert_eq!(
            overflow_message(
                "request (abc tokens) exceeds the available context size (4096 tokens)"
            ),
            None
        );
        assert_eq!(overflow_message("request (12 tokens) is fine"), None);
        assert_eq!(
            overflow_message("exceeds the available context size (4096 tokens)"),
            None
        );
        assert_eq!(overflow_message("request (99999999999999999999999 tokens) exceeds the available context size (4 tokens)"), None);
        assert_eq!(overflow_message(""), None);
    }
}
