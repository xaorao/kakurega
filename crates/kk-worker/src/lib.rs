use kk_evidence::b64;
use kk_types::chat::{ChatMessage, ToolCall, ToolSpec};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

pub const STATE_VERSION: u64 = 1;

#[derive(Debug)]
pub enum WorkerError {
    Link(String),
    BadReply(String),
    Refused { code: String, detail: String },
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkerError::Link(m) => write!(f, "broker link failed: {m}"),
            WorkerError::BadReply(m) => write!(f, "unexpected broker reply: {m}"),
            WorkerError::Refused { code, detail } => write!(f, "refused ({code}): {detail}"),
        }
    }
}

impl std::error::Error for WorkerError {}

pub trait Link {
    fn call(&mut self, request: Value) -> Result<Value, WorkerError>;
}

pub struct StdioLink<R: BufRead, W: Write> {
    next_id: u64,
    reader: R,
    writer: W,
}

impl<R: BufRead, W: Write> StdioLink<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        StdioLink {
            next_id: 1,
            reader,
            writer,
        }
    }
}

impl<R: BufRead, W: Write> Link for StdioLink<R, W> {
    fn call(&mut self, mut request: Value) -> Result<Value, WorkerError> {
        let id = self.next_id;
        self.next_id += 1;
        request["id"] = json!(id);
        let line = serde_json::to_string(&request).map_err(|e| WorkerError::Link(e.to_string()))?;
        writeln!(self.writer, "{line}").map_err(|e| WorkerError::Link(e.to_string()))?;
        self.writer
            .flush()
            .map_err(|e| WorkerError::Link(e.to_string()))?;
        let mut reply = String::new();
        let n = self
            .reader
            .read_line(&mut reply)
            .map_err(|e| WorkerError::Link(e.to_string()))?;
        if n == 0 {
            return Err(WorkerError::Link("the broker closed the channel".into()));
        }
        serde_json::from_str(&reply).map_err(|e| WorkerError::BadReply(e.to_string()))
    }
}

fn ok_result(reply: Value) -> Result<Value, WorkerError> {
    if reply["ok"].as_bool() == Some(true) {
        return Ok(reply["result"].clone());
    }
    Err(WorkerError::Refused {
        code: reply["error"]["code"]
            .as_str()
            .unwrap_or("UNKNOWN")
            .to_string(),
        detail: reply["error"]["detail"].as_str().unwrap_or("").to_string(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentState {
    pub version: u64,
    pub messages: Vec<ChatMessage>,
    pub turn: u32,
}

impl AgentState {
    pub fn new(system: &str, task: &str) -> Self {
        AgentState {
            version: STATE_VERSION,
            messages: vec![
                ChatMessage::new("system", system),
                ChatMessage::new("user", task),
            ],
            turn: 0,
        }
    }

    fn pending_calls(&self) -> Vec<ToolCall> {
        let Some(idx) = self.messages.iter().rposition(|m| m.role == "assistant") else {
            return Vec::new();
        };
        let calls = &self.messages[idx].tool_calls;
        let answered: Vec<&str> = self.messages[idx + 1..]
            .iter()
            .filter(|m| m.role == "tool")
            .filter_map(|m| m.tool_call_id.as_deref())
            .collect();
        calls
            .iter()
            .filter(|c| !answered.contains(&c.id.as_str()))
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Finished(String),
    TurnLimit,
    Cancelled,
}

pub struct AgentConfig {
    pub tools: Vec<ToolSpec>,
    pub local_tools: Vec<ToolSpec>,
    pub max_turns: u32,
    pub max_tokens: u32,
}

pub fn finding_tool() -> ToolSpec {
    ToolSpec {
        name: "report_finding".into(),
        description: "Record one confirmed defect. Give the file, the line, a short title, why it is a defect and, if you can, a reproducer script.".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "file": {"type": "string"},
                "line": {"type": "integer"},
                "explanation": {"type": "string"},
                "reproducer": {"type": "string"}
            },
            "required": ["title", "file", "line", "explanation"]
        }),
    }
}

pub fn fix_tool() -> ToolSpec {
    ToolSpec {
        name: "propose_fix".into(),
        description: "Propose a fix for a finding you already reported. Give the finding's exact title, the file, the exact existing text to replace (it must appear exactly once in that file) and the new text. The kernel checks the change against your reproducer; it is shown to the owner only and never sent to anyone else.".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "file": {"type": "string"},
                "old_text": {"type": "string"},
                "new_text": {"type": "string"},
                "explanation": {"type": "string"},
                "also": {
                    "type": "array",
                    "description": "Further changes that belong to the same fix, in other files (at most three). Each has file, old_text and new_text, and old_text must appear exactly once in that file.",
                    "items": {"type": "object", "properties": {"file": {"type": "string"}, "old_text": {"type": "string"}, "new_text": {"type": "string"}}, "required": ["file", "old_text", "new_text"]}
                }
            },
            "required": ["title", "file", "old_text", "new_text"]
        }),
    }
}

pub fn memory_tool() -> ToolSpec {
    ToolSpec {
        name: "suggest_memory".into(),
        description: "Suggest one durable fact about this project that would help future reviews. The owner decides whether it is kept; at most three suggestions. Never include secrets or credentials.".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "text": {"type": "string"},
                "why": {"type": "string"}
            },
            "required": ["text", "why"]
        }),
    }
}

fn save(link: &mut dyn Link, state: &AgentState) -> Result<(), WorkerError> {
    let bytes = serde_json::to_vec(state).map_err(|e| WorkerError::Link(e.to_string()))?;
    ok_result(link.call(json!({"op": "checkpoint", "state_b64": b64::encode(&bytes)}))?)?;
    Ok(())
}

pub fn load_state(link: &mut dyn Link) -> Result<Option<AgentState>, WorkerError> {
    let r = ok_result(link.call(json!({"op": "checkpoint.load"}))?)?;
    let Some(text) = r["state_b64"].as_str() else {
        return Ok(None);
    };
    let bytes = b64::decode(text)
        .map_err(|_| WorkerError::BadReply("checkpoint is not base64url".into()))?;
    let state: AgentState =
        serde_json::from_slice(&bytes).map_err(|e| WorkerError::BadReply(e.to_string()))?;
    if state.version != STATE_VERSION {
        return Err(WorkerError::BadReply(
            "checkpoint version is not supported".into(),
        ));
    }
    Ok(Some(state))
}

fn fatal(code: &str) -> bool {
    matches!(
        code,
        "EXPIRED"
            | "BUDGET_EXCEEDED"
            | "REPLAY"
            | "OP_NOT_ALLOWED"
            | "NO_ELIGIBLE_ROUTE"
            | "DENY_ERROR"
    )
}

fn run_local_tool(link: &mut dyn Link, call: &ToolCall) -> Result<Value, WorkerError> {
    match call.name.as_str() {
        "report_finding" => {
            let a = &call.arguments;
            let (Some(title), Some(file), Some(line), Some(explanation)) = (
                a["title"].as_str(),
                a["file"].as_str(),
                a["line"].as_i64(),
                a["explanation"].as_str(),
            ) else {
                return Ok(
                    json!({"error": "report_finding needs title, file, line and explanation"}),
                );
            };
            let finding = json!({
                "title": title,
                "file": file,
                "line": line,
                "explanation": explanation,
                "reproducer": a["reproducer"].as_str().unwrap_or(""),
            });
            let bytes =
                serde_json::to_vec(&finding).map_err(|e| WorkerError::Link(e.to_string()))?;
            let r = ok_result(link.call(json!({"op": "write.artifact", "bytes_b64": b64::encode(&bytes), "kind": "finding"}))?)?;
            Ok(json!({"recorded": true, "handle": r["handle"]}))
        }
        "propose_fix" => {
            let a = &call.arguments;
            let (Some(title), Some(file), Some(old), Some(new)) = (
                a["title"].as_str(),
                a["file"].as_str(),
                a["old_text"].as_str(),
                a["new_text"].as_str(),
            ) else {
                return Ok(
                    json!({"error": "propose_fix needs title, file, old_text and new_text"}),
                );
            };
            let mut doc = json!({"title": title, "file": file, "old_text": old, "new_text": new, "explanation": a["explanation"].as_str().unwrap_or("")});
            if let Some(also) = a.get("also").filter(|v| v.is_array()) {
                doc["also"] = also.clone();
            }
            let bytes = serde_json::to_vec(&doc).map_err(|e| WorkerError::Link(e.to_string()))?;
            let r = ok_result(link.call(
                json!({"op": "write.artifact", "bytes_b64": b64::encode(&bytes), "kind": "fix"}),
            )?)?;
            Ok(json!({"proposed": true, "handle": r["handle"]}))
        }
        "suggest_memory" => {
            let a = &call.arguments;
            let (Some(text), Some(why)) = (a["text"].as_str(), a["why"].as_str()) else {
                return Ok(json!({"error": "suggest_memory needs text and why"}));
            };
            let bytes = serde_json::to_vec(&json!({"text": text, "why": why}))
                .map_err(|e| WorkerError::Link(e.to_string()))?;
            let r = ok_result(link.call(json!({"op": "write.artifact", "bytes_b64": b64::encode(&bytes), "kind": "memory_suggestion"}))?)?;
            Ok(json!({"suggested": true, "handle": r["handle"]}))
        }
        other => Ok(json!({"error": format!("no such tool: {other}")})),
    }
}

pub fn run(
    link: &mut dyn Link,
    mut state: AgentState,
    config: &AgentConfig,
    cancelled: &dyn Fn() -> bool,
) -> Result<Outcome, WorkerError> {
    let mut all_tools = config.tools.clone();
    all_tools.extend(config.local_tools.clone());
    loop {
        if cancelled() {
            save(link, &state)?;
            return Ok(Outcome::Cancelled);
        }
        let pending = state.pending_calls();
        if !pending.is_empty() {
            for call in pending {
                if cancelled() {
                    save(link, &state)?;
                    return Ok(Outcome::Cancelled);
                }
                let content = if config.local_tools.iter().any(|t| t.name == call.name) {
                    run_local_tool(link, &call)?
                } else {
                    let request_id = format!("t{}-{}", state.turn, call.id);
                    let reply = link.call(json!({"op": "tool.call", "request_id": request_id, "name": call.name, "args": call.arguments}))?;
                    match ok_result(reply) {
                        Ok(v) => v,
                        Err(WorkerError::Refused { code, detail }) if !fatal(&code) => {
                            json!({"error": code, "detail": detail})
                        }
                        Err(e) => return Err(e),
                    }
                };
                let mut msg = ChatMessage::new("tool", &content.to_string());
                msg.tool_call_id = Some(call.id.clone());
                state.messages.push(msg);
                save(link, &state)?;
            }
            continue;
        }
        if state.turn >= config.max_turns {
            save(link, &state)?;
            return Ok(Outcome::TurnLimit);
        }
        let reply = link.call(json!({"op": "model.complete", "messages": state.messages, "tools": all_tools, "max_tokens": config.max_tokens}))?;
        let result = ok_result(reply)?;
        let message: ChatMessage = serde_json::from_value(result["message"].clone())
            .map_err(|e| WorkerError::BadReply(e.to_string()))?;
        let finished = message.tool_calls.is_empty();
        let text = message.content.clone();
        let mut message = message;
        for (i, c) in message.tool_calls.iter_mut().enumerate() {
            if c.id.is_empty() {
                c.id = format!("call_{i}");
            }
        }
        state.messages.push(message);
        state.turn += 1;
        save(link, &state)?;
        if finished {
            return Ok(Outcome::Finished(text));
        }
    }
}
