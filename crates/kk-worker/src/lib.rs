use kk_evidence::b64;
use kk_types::chat::{ChatMessage, ToolCall, ToolSpec};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
pub mod stuck;
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
    /// The agent was going round in circles and did not change course when told; the reason is for the owner.
    Stuck(String),
    Cancelled,
}

pub struct AgentConfig {
    pub tools: Vec<ToolSpec>,
    pub local_tools: Vec<ToolSpec>,
    pub max_turns: u32,
    pub max_tokens: u32,
    /// A model that ends with fewer tool results than this and has reported nothing is asked once to look further. 0 turns it off.
    pub min_looks: usize,
}

pub fn finding_tool() -> ToolSpec {
    ToolSpec {
        name: "report_finding".into(),
        description: "Record one confirmed defect. Give the file, the line, a short title, why it is a defect and, if you can, a reproducer script in the language named by `language` (python for .py files, javascript for .js, .mjs and .cjs files, typescript for .ts, .mts and .cts files, rust for .rs files).".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "file": {"type": "string"},
                "line": {"type": "integer"},
                "explanation": {"type": "string"},
                "reproducer": {"type": "string"},
                "language": {"type": "string", "enum": ["python", "javascript", "typescript", "rust"], "description": "The language of the reproducer; python if omitted."}
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
                    "description": "Further changes that belong to the same fix, in other files or in other places of the same file (at most seven, in at most four different files). Each has file, old_text and new_text; old_text must appear exactly once in that file as it is now, and no two changes may share any of the same text.",
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

pub fn method_tool() -> ToolSpec {
    ToolSpec {
        name: "suggest_method".into(),
        description: "Suggest one reusable way of working that would help future reviews of projects like this one: a short name, one line on what it is for, a few trigger words, and the text of the guidance. The owner reads it and decides whether it is kept; at most two suggestions. Never include secrets.".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "description": {"type": "string"},
                "triggers": {"type": "array", "items": {"type": "string"}},
                "text": {"type": "string"}
            },
            "required": ["name", "text"]
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

/// The finding artifact for a `report_finding` call, or the message to send back to the model. The language is passed on only
/// when the model named one of the two known ones; anything else is sent back to be corrected, never guessed.
pub fn build_finding(a: &Value) -> Result<Value, String> {
    let (Some(title), Some(file), Some(line), Some(explanation)) = (
        a["title"].as_str(),
        a["file"].as_str(),
        a["line"].as_i64(),
        a["explanation"].as_str(),
    ) else {
        return Err("report_finding needs title, file, line and explanation".into());
    };
    let mut finding = json!({
        "title": title,
        "file": file,
        "line": line,
        "explanation": explanation,
        "reproducer": a["reproducer"].as_str().unwrap_or(""),
    });
    if let Some(language) = a.get("language") {
        match language.as_str() {
            Some(l @ ("python" | "javascript" | "typescript" | "rust")) => finding["language"] = json!(l),
            _ => return Err(
                "language must be exactly \"python\", \"javascript\", \"typescript\" or \"rust\" (or left out for python)"
                    .into(),
            ),
        }
    }
    Ok(finding)
}

/// The broker's judgement of what was just written, when it gave one: the model reads it as `check` in the tool result.
fn with_check(mut result: Value, reply: &Value) -> Value {
    if !reply["feedback"].is_null() {
        result["check"] = reply["feedback"].clone();
    }
    result
}

pub fn run_local_tool(link: &mut dyn Link, call: &ToolCall) -> Result<Value, WorkerError> {
    match call.name.as_str() {
        "report_finding" => {
            let finding = match build_finding(&call.arguments) {
                Ok(f) => f,
                Err(message) => return Ok(json!({"error": message})),
            };
            let bytes =
                serde_json::to_vec(&finding).map_err(|e| WorkerError::Link(e.to_string()))?;
            let r = ok_result(link.call(json!({"op": "write.artifact", "bytes_b64": b64::encode(&bytes), "kind": "finding"}))?)?;
            Ok(with_check(
                json!({"recorded": true, "handle": r["handle"]}),
                &r,
            ))
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
            Ok(with_check(
                json!({"proposed": true, "handle": r["handle"]}),
                &r,
            ))
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
        "suggest_method" => {
            let a = &call.arguments;
            let (Some(name), Some(text)) = (a["name"].as_str(), a["text"].as_str()) else {
                return Ok(json!({"error": "suggest_method needs a name and text"}));
            };
            let bytes = serde_json::to_vec(&json!({
                "name": name,
                "description": a["description"].as_str().unwrap_or(""),
                "triggers": a["triggers"].clone(),
                "body": text,
            }))
            .map_err(|e| WorkerError::Link(e.to_string()))?;
            let r = ok_result(link.call(json!({"op": "write.artifact", "bytes_b64": b64::encode(&bytes), "kind": "method_suggestion"}))?)?;
            Ok(json!({"suggested": true, "handle": r["handle"]}))
        }
        other => Ok(json!({"error": format!("no such tool: {other}")})),
    }
}

/// The start of the one message sent to a model that ends a review having looked at almost nothing.
const EARLY_FINISH_NOTICE: &str = "You stopped after reading only ";
/// The message that asks a model to look further, or nothing: it applies once, only when the model reported no finding or fix
/// and made fewer than `min_looks` tool calls, and never again once it was sent (also across a restart from a saved state).
fn early_finish_nudge(messages: &[ChatMessage], min_looks: usize) -> Option<String> {
    if messages
        .iter()
        .any(|m| m.role == "user" && m.content.starts_with(EARLY_FINISH_NOTICE))
    {
        return None;
    }
    let acted = messages
        .iter()
        .flat_map(|m| m.tool_calls.iter())
        .any(|c| matches!(c.name.as_str(), "report_finding" | "propose_fix"));
    let looks = messages.iter().filter(|m| m.role == "tool").count();
    if acted || looks >= min_looks {
        return None;
    }
    Some(format!(
        "{EARLY_FINISH_NOTICE}{looks} tool results and reported nothing. Use list_files and read_file to read the source files that handle input, logins, tokens, files or money before you answer, and report every defect you can confirm with report_finding."
    ))
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
        match stuck::check(&state.messages) {
            stuck::Verdict::Fine => {}
            stuck::Verdict::Nudge(text) => {
                state.messages.push(ChatMessage::new("user", &text));
                save(link, &state)?;
            }
            stuck::Verdict::Stop(reason) => {
                save(link, &state)?;
                return Ok(Outcome::Stuck(reason));
            }
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
            if let Some(notice) = early_finish_nudge(&state.messages, config.min_looks) {
                state.messages.push(ChatMessage::new("user", &notice));
                save(link, &state)?;
                continue;
            }
            return Ok(Outcome::Finished(text));
        }
    }
}
