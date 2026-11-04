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
