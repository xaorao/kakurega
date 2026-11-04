use crate::approve::{connect, Admin};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const PROTOCOL: &str = "2025-06-18";

fn tools() -> Value {
    json!([
        {"name": "list_reviews", "description": "List reviews with status and counts only. Never returns source text.", "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}},
        {"name": "start_review", "description": "Start a private review of a project folder for a named recipient.", "inputSchema": {"type": "object", "properties": {"repo": {"type": "string"}, "recipient": {"type": "string"}}, "required": ["repo", "recipient"], "additionalProperties": false}},
        {"name": "review_status", "description": "Status and counts of one review.", "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"], "additionalProperties": false}},
        {"name": "request_approval", "description": "Ask the owner to approve the release. The owner approves in a terminal with `kaku approve`; this tool cannot approve.", "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"], "additionalProperties": false}},
        {"name": "verify_capsule", "description": "Check a capsule file offline and report what it does and does not establish.", "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"], "additionalProperties": false}}
    ])
}

fn text(ok: bool, v: Value) -> Value {
    json!({"content": [{"type": "text", "text": v.to_string()}], "isError": !ok})
}

fn call(admin: &Result<Admin, String>, name: &str, a: &Value) -> Value {
    let s = |k: &str| a.get(k).and_then(Value::as_str).map(str::to_string);
    if name == "verify_capsule" {
        let Some(p) = s("path") else {
            return text(false, json!({"error": "path is required"}));
        };
        return match std::fs::read(&p) {
            Ok(bytes) => match crate::verify::check_bytes(&bytes) {
                Ok(v) => text(true, v),
                Err(e) => text(false, json!({"error": e})),
            },
            Err(e) => text(false, json!({"error": format!("cannot read file: {e}")})),
        };
    }
    let admin = match admin {
        Ok(a) => a,
        Err(e) => return text(false, json!({"error": e})),
    };
    let ids_ok = |id: &str| {
        !id.is_empty()
            && id.len() <= 64
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    };
    let r = match name {
        "list_reviews" => admin.get("/admin/jobs"),
        "start_review" => match (s("repo"), s("recipient")) {
            (Some(repo), Some(recipient)) => {
                admin.post("/admin/jobs", json!({"repo": repo, "recipient": recipient}))
            }
            _ => return text(false, json!({"error": "repo and recipient are required"})),
        },
        "review_status" | "request_approval" => match s("id") {
            Some(id) if ids_ok(&id) => {
                if name == "review_status" {
                    admin.get(&format!("/admin/jobs/{id}"))
                } else {
                    admin.post(&format!("/admin/jobs/{id}/approval"), json!({}))
                }
            }
            _ => return text(false, json!({"error": "a valid review id is required"})),
        },
        _ => return text(false, json!({"error": "unknown tool"})),
    };
    match r {
        Ok((c, v)) if c < 300 => text(true, v),
        Ok((c, v)) => text(false, json!({"status": c, "body": v})),
        Err(e) => text(false, json!({"error": e})),
    }
}

pub fn handle(admin: &Result<Admin, String>, line: &str) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => {
            return Some(
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}}),
            )
        }
    };
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let reply = |result: Value| {
        id.clone()
            .map(|id| json!({"jsonrpc": "2.0", "id": id, "result": result}))
    };
    let fail = |code: i64, m: &str| {
        id.clone()
            .map(|id| json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": m}}))
    };
    match method {
        "initialize" => reply(
            json!({"protocolVersion": PROTOCOL, "capabilities": {"tools": {}}, "serverInfo": {"name": "kakurega", "version": env!("CARGO_PKG_VERSION")}}),
        ),
        "ping" => reply(json!({})),
        "tools/list" => reply(json!({"tools": tools()})),
        "tools/call" => {
            let p = msg.get("params").cloned().unwrap_or(Value::Null);
            let name = p.get("name").and_then(Value::as_str).unwrap_or("");
            let args = p.get("arguments").cloned().unwrap_or(json!({}));
            reply(call(admin, name, &args))
        }
        m if m.starts_with("notifications/") => None,
        _ => fail(-32601, "method not found"),
    }
}

pub fn run(args: &[String]) -> u8 {
    let admin = connect(args);
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(r) = handle(&admin, &line) {
            if writeln!(out, "{r}").and_then(|_| out.flush()).is_err() {
                break;
            }
        }
    }
    0
}
