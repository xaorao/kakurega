use kk_types::label::Label;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn new(role: &str, content: &str) -> Self {
        ChatMessage {
            role: role.into(),
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

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
}

#[derive(Debug)]
pub enum RouteError {
    EndpointNotLoopback(String),
    Unreachable(String),
    BadResponse(String),
    NoEligibleRoute,
    ScriptExhausted,
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RouteError::EndpointNotLoopback(u) => {
                write!(f, "device-only route refuses a non-loopback endpoint: {u}")
            }
            RouteError::Unreachable(m) => write!(f, "model unreachable: {m}"),
            RouteError::BadResponse(m) => write!(f, "unexpected model response: {m}"),
            RouteError::NoEligibleRoute => {
                f.write_str("no route is eligible for this data; refusing instead of falling back")
            }
            RouteError::ScriptExhausted => f.write_str("the scripted route has no more responses"),
        }
    }
}

impl std::error::Error for RouteError {}

pub trait Route: Send + Sync {
    fn id(&self) -> &str;
    fn class(&self) -> RouteClass;
    fn tee_verified(&self) -> bool;
    fn evidence(&self) -> RouteEvidence;
    fn complete(&self, request: &CompletionRequest) -> Result<Completion, RouteError>;
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

pub struct OllamaRoute {
    id: String,
    base: String,
    model: String,
    agent: ureq::Agent,
}

impl OllamaRoute {
    pub fn new(base_url: &str, model: &str) -> Result<Self, RouteError> {
        if !is_loopback_http(base_url) {
            return Err(RouteError::EndpointNotLoopback(base_url.to_string()));
        }
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(300)))
            .max_redirects(0)
            .http_status_as_error(false)
            .build();
        Ok(OllamaRoute {
            id: format!("ollama:{model}"),
            base: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            agent: config.into(),
        })
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
        }
    }

    fn complete(&self, request: &CompletionRequest) -> Result<Completion, RouteError> {
        let messages: Vec<Value> = request
            .messages
            .iter()
            .map(|m| {
                let mut o = json!({"role": m.role, "content": m.content});
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
            "messages": messages,
            "options": {"num_predict": request.max_tokens, "temperature": request.temperature},
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools);
        }
        let mut resp = self
            .agent
            .post(&format!("{}/api/chat", self.base))
            .send_json(&body)
            .map_err(|e| RouteError::Unreachable(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(RouteError::BadResponse(format!("status {}", resp.status())));
        }
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

pub struct ScriptedRoute {
    id: String,
    class: RouteClass,
    tee_verified: bool,
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
            script: Mutex::new(script),
            seen: Mutex::new(Vec::new()),
        }
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

    fn evidence(&self) -> RouteEvidence {
        RouteEvidence {
            route_id: self.id.clone(),
            class: self.class.as_str().into(),
            model: "scripted".into(),
            model_digest: None,
            endpoint: "in-process".into(),
            tee_verified: self.tee_verified,
            simulated: true,
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

    pub fn eligible_for(&self, _taint: &Label) -> Option<&dyn Route> {
        self.routes
            .iter()
            .map(|r| r.as_ref())
            .find(|r| match r.class() {
                RouteClass::DeviceOnly => true,
                RouteClass::VerifiedTee => r.tee_verified(),
                RouteClass::Public => false,
            })
    }

    pub fn select(&self, taint: &Label) -> Result<&dyn Route, RouteError> {
        self.eligible_for(taint).ok_or(RouteError::NoEligibleRoute)
    }
}
