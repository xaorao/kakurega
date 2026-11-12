use kk_route::{ChatMessage, ToolSpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_LINE_BYTES: usize = 8 << 20;

#[derive(Debug, Clone)]
pub struct Envelope {
    pub id: u64,
    pub request: Request,
}

pub fn parse_line(line: &str) -> Result<Envelope, String> {
    let mut v: Value = serde_json::from_str(line).map_err(|e| e.to_string())?;
    let obj = v.as_object_mut().ok_or("a request is a JSON object")?;
    let id = obj
        .remove("id")
        .and_then(|i| i.as_u64())
        .ok_or("`id` must be a non-negative integer")?;
    let request: Request = serde_json::from_value(v).map_err(|e| e.to_string())?;
    Ok(Envelope { id, request })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
pub enum Request {
    #[serde(rename = "read.artifact")]
    ReadArtifact { handle: u64 },
    #[serde(rename = "write.artifact")]
    WriteArtifact {
        bytes_b64: String,
        #[serde(default)]
        derived_from_hint: Vec<u64>,
        kind: String,
    },
    #[serde(rename = "model.complete")]
    ModelComplete {
        messages: Vec<ChatMessage>,
        #[serde(default)]
        tools: Vec<ToolSpec>,
        max_tokens: u32,
    },
    #[serde(rename = "tool.call")]
    ToolCall {
        request_id: String,
        name: String,
        args: Value,
    },
    #[serde(rename = "checkpoint")]
    Checkpoint { state_b64: String },
    #[serde(rename = "checkpoint.load")]
    CheckpointLoad {},
    #[serde(rename = "log.note")]
    LogNote { code: String },
    #[serde(rename = "release.propose")]
    ReleasePropose {
        handles: Vec<u64>,
        recipient: String,
    },
}

impl Request {
    pub fn op_name(&self) -> &'static str {
        match self {
            Request::ReadArtifact { .. } => "read.artifact",
            Request::WriteArtifact { .. } => "write.artifact",
            Request::ModelComplete { .. } => "model.complete",
            Request::ToolCall { .. } => "tool.call",
            Request::Checkpoint { .. } => "checkpoint",
            Request::CheckpointLoad { .. } => "checkpoint.load",
            Request::LogNote { .. } => "log.note",
            Request::ReleasePropose { .. } => "release.propose",
        }
    }

    /// What was asked, in a few words for the request list a person reads: the tool and the file or text it was given, how
    /// many messages went to the model, how many bytes were written. Never a file's contents or a message's text.
    pub fn summary(&self) -> String {
        fn short(s: &str) -> String {
            let clean: String = s
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            if clean.chars().count() > 80 {
                format!("{}…", clean.chars().take(80).collect::<String>())
            } else {
                clean
            }
        }
        let bytes = |b64: &str| b64.len() / 4 * 3;
        match self {
            Request::ReadArtifact { handle } => format!("file {handle}"),
            Request::WriteArtifact {
                bytes_b64, kind, ..
            } => format!("{} ({} bytes)", short(kind), bytes(bytes_b64)),
            Request::ModelComplete {
                messages, tools, ..
            } => format!(
                "{} message{}, {} tool{} offered",
                messages.len(),
                if messages.len() == 1 { "" } else { "s" },
                tools.len(),
                if tools.len() == 1 { "" } else { "s" }
            ),
            Request::ToolCall { name, args, .. } => {
                let text = |k: &str| {
                    args.get(k)
                        .and_then(Value::as_str)
                        .filter(|v| !v.is_empty())
                };
                match (text("path"), text("needle"), text("prefix")) {
                    (Some(path), _, _) => format!("{} {}", short(name), short(path)),
                    (None, Some(needle), Some(prefix)) => {
                        format!("{} \"{}\" in {}", short(name), short(needle), short(prefix))
                    }
                    (None, Some(needle), None) => format!("{} \"{}\"", short(name), short(needle)),
                    (None, None, Some(prefix)) => format!("{} {}", short(name), short(prefix)),
                    (None, None, None) => short(name),
                }
            }
            Request::Checkpoint { state_b64 } => format!("{} bytes", bytes(state_b64)),
            Request::CheckpointLoad {} => String::new(),
            Request::LogNote { code } => short(code),
            Request::ReleasePropose { handles, recipient } => format!(
                "{} file{} for {}",
                handles.len(),
                if handles.len() == 1 { "" } else { "s" },
                short(recipient)
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ErrorCode {
    #[serde(rename = "BAD_REQUEST")]
    BadRequest,
    #[serde(rename = "DENY_POLICY")]
    DenyPolicy,
    #[serde(rename = "DENY_ERROR")]
    DenyError,
    #[serde(rename = "OP_NOT_ALLOWED")]
    OpNotAllowed,
    #[serde(rename = "LABEL_EXCEEDS_CLEARANCE")]
    LabelExceedsClearance,
    #[serde(rename = "BUDGET_EXCEEDED")]
    BudgetExceeded,
    #[serde(rename = "EXPIRED")]
    Expired,
    #[serde(rename = "REPLAY")]
    Replay,
    #[serde(rename = "UNKNOWN_HANDLE")]
    UnknownHandle,
    #[serde(rename = "NO_ELIGIBLE_ROUTE")]
    NoEligibleRoute,
    #[serde(rename = "TOOL_ERROR")]
    ToolError,
    #[serde(rename = "UNKNOWN_TOOL")]
    UnknownTool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Reply {
    pub id: u64,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl Reply {
    pub fn success(id: u64, result: Value) -> Reply {
        Reply {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: u64, code: ErrorCode, detail: &str) -> Reply {
        Reply {
            id,
            ok: false,
            result: None,
            error: Some(ErrorBody {
                code,
                detail: detail.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod summary_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pr01_a_request_summary_names_the_tool_and_file_but_never_contents_or_message_text() {
        let secret = format!(
            "{}-{}",
            "sk-ant", "api03-abcdefghijklmnopqrstuvwxyz0123456789"
        );
        let b64 = kk_evidence::b64::encode(secret.as_bytes());
        let write = Request::WriteArtifact {
            bytes_b64: b64.clone(),
            derived_from_hint: vec![],
            kind: "finding".into(),
        };
        let s = write.summary();
        assert!(
            s.starts_with("finding (") && !s.contains(&b64) && !s.contains(&secret),
            "{s}"
        );
        let model = Request::ModelComplete {
            messages: vec![
                ChatMessage::new("system", "rules"),
                ChatMessage::new("user", &format!("the key is {secret}")),
            ],
            tools: vec![],
            max_tokens: 100,
        };
        let s = model.summary();
        assert_eq!(s, "2 messages, 0 tools offered");
        let read = Request::ToolCall {
            request_id: "r1".into(),
            name: "read_file".into(),
            args: json!({"path": "src/a.rs\u{1b}[2J"}),
        };
        assert_eq!(
            read.summary(),
            "read_file src/a.rs [2J",
            "control characters are blanked"
        );
        let search = Request::ToolCall {
            request_id: "r2".into(),
            name: "search_text".into(),
            args: json!({"needle": "x".repeat(200), "prefix": "src/"}),
        };
        let s = search.summary();
        assert!(s.chars().count() < 110 && s.ends_with("in src/"), "{s}");
        let checkpoint = Request::Checkpoint {
            state_b64: b64.clone(),
        };
        assert!(!checkpoint.summary().contains(&b64));
    }
}
