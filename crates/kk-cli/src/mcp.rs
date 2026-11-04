use crate::approve::{connect, Admin};
use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};

const PROTOCOL: &str = "2025-06-18";
const MAX_LINE: usize = 1 << 20;
const MAX_FILE: u64 = 20 << 20;

fn string_prop(desc: &str, max: u64) -> Value {
    json!({"type": "string", "description": desc, "minLength": 1, "maxLength": max})
}

fn job_id_prop() -> Value {
    json!({"type": "string", "description": "A review id such as job-3.", "pattern": "^[A-Za-z0-9_-]{1,64}$"})
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

fn tool(
    name: &str,
    title: &str,
    description: &str,
    input: Value,
    output: Value,
    read_only: bool,
) -> Value {
    json!({
        "name": name, "title": title, "description": description,
        "inputSchema": input, "outputSchema": output,
        "annotations": {"readOnlyHint": read_only, "destructiveHint": false, "idempotentHint": read_only, "openWorldHint": false}
    })
}

pub fn tools() -> Value {
    let error_out = json!({"type": "object", "properties": {"error": {"type": "string"}}, "required": ["error"]});
    let _ = error_out;
    json!([
        tool("overview", "Overview", "Counts only: setup problems, reviews by state, memories by privacy level and the signed log's latest entry. Never returns source text, memory text or findings.", obj(json!({}), &[]),
            obj(json!({"version": {"type": "string"}, "ready": {"type": "boolean"}, "setup_problems": {"type": "array", "items": {"type": "string"}}, "reviews": {"type": "object"}, "memories_by_level": {"type": "object"}, "log": {"type": "object"}}), &["ready", "reviews"]), true),
        tool("list_reviews", "List reviews", "List reviews with state and counts only. Never returns source text.", obj(json!({}), &[]),
            obj(json!({"jobs": {"type": "array", "items": {"type": "object"}}}), &["jobs"]), true),
        tool("start_review", "Start a review", "Start a private review of a project folder for a named recipient. Nothing is released by starting a review.",
            obj(json!({"repo": string_prop("Absolute path of the project folder, inside a root the owner allowed.", 4096), "recipient": string_prop("Who will receive the result.", 128)}), &["repo", "recipient"]),
            obj(json!({"id": {"type": "string"}}), &["id"]), false),
        tool("review_status", "Review status", "State and counts of one review.", obj(json!({"id": job_id_prop()}), &["id"]),
            obj(json!({"id": {"type": "string"}, "state": {"type": "string"}, "stage": {}, "files_reviewed": {}, "files_withheld": {}, "findings_reported": {}, "findings_confirmed": {}, "approval": {}, "delivered": {}}), &["id", "state"]), true),
        tool("request_approval", "Request approval", "Ask the owner to approve the release. This tool cannot approve: the owner approves in the desktop window, the terminal workspace or `kaku approve`, after seeing the exact files.", obj(json!({"id": job_id_prop()}), &["id"]),
            obj(json!({"approval": {"type": "string"}, "digest": {"type": "string"}, "how": {"type": "string"}}), &["approval"]), false),
        tool("verify_capsule", "Verify a capsule", "Check a capsule file (.kkg) offline and report what it does and does not establish.", obj(json!({"path": string_prop("Path of a .kkg capsule file.", 4096)}), &["path"]),
            obj(json!({"valid": {"type": "boolean"}, "reason": {"type": "string"}, "signer": {}, "recipient": {}, "established": {"type": "array"}, "not_established": {"type": "array"}}), &["valid"]), true),
        tool("verify_log", "Verify the event log", "Check a saved signed event log (.json) offline, optionally against a pinned key and an anchor.", obj(json!({"path": string_prop("Path of a saved event log.", 4096), "pin": string_prop("Base64url Ed25519 public key to require.", 64), "anchor": string_prop("COUNT:HEAD printed by `kaku log export`.", 200)}), &["path"]),
            obj(json!({"valid": {"type": "boolean"}, "reason": {"type": "string"}, "entries": {"type": "integer"}, "head": {"type": "string"}, "signer": {"type": "string"}}), &["valid"]), true)
    ])
}

fn reply(ok: bool, v: Value) -> Value {
    if ok {
        json!({"content": [{"type": "text", "text": v.to_string()}], "structuredContent": v, "isError": false})
    } else {
        json!({"content": [{"type": "text", "text": v.to_string()}], "isError": true})
    }
}

fn fail(msg: &str) -> Value {
    reply(false, json!({"error": msg}))
}

fn check_args(schema: &Value, args: &Value) -> Result<(), String> {
    let Some(map) = args.as_object() else {
        return Err("arguments must be an object".into());
    };
    let props = schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    for k in map.keys() {
        if !props.contains_key(k) {
            return Err(format!("unknown argument `{k}`"));
        }
    }
    for r in schema["required"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !map.contains_key(r) {
            return Err(format!("`{r}` is required"));
        }
    }
    for (k, v) in map {
        let p = &props[k];
        match p["type"].as_str() {
            Some("string") => {
                let Some(s) = v.as_str() else {
                    return Err(format!("`{k}` must be a string"));
                };
                if s.is_empty()
                    || s.chars().count() as u64 > p["maxLength"].as_u64().unwrap_or(4096)
                    || s.chars().any(|c| c.is_control())
                {
                    return Err(format!(
                        "`{k}` must be 1 to {} characters without control characters",
                        p["maxLength"]
                    ));
                }
                if let Some(pat) = p["pattern"].as_str() {
                    if pat == "^[A-Za-z0-9_-]{1,64}$"
                        && !s
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    {
                        return Err(format!("`{k}` is not a valid id"));
                    }
                }
            }
            _ => return Err(format!("`{k}` has an unsupported type")),
        }
    }
    Ok(())
}

fn read_small(path: &str, suffixes: &[&str]) -> Result<Vec<u8>, String> {
    let p = std::path::Path::new(path);
    let ext_ok = p
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| suffixes.contains(&e.to_ascii_lowercase().as_str()));
    let meta = std::fs::metadata(p)
        .ok()
        .filter(|m| m.is_file() && m.len() <= MAX_FILE);
    if !ext_ok || meta.is_none() {
        return Err("cannot read that file".into());
    }
    std::fs::read(p).map_err(|_| "cannot read that file".to_string())
}

fn call(admin: &Result<Admin, String>, name: &str, a: &Value) -> Value {
    let all = tools();
    let Some(def) = all
        .as_array()
        .and_then(|t| t.iter().find(|t| t["name"] == name))
    else {
        return fail("unknown tool");
    };
    if let Err(e) = check_args(&def["inputSchema"], a) {
        return fail(&e);
    }
    let s = |k: &str| a.get(k).and_then(Value::as_str).map(str::to_string);
    match name {
        "verify_capsule" => {
            let bytes = match read_small(&s("path").unwrap_or_default(), &["kkg", "json"]) {
                Ok(b) => b,
                Err(e) => return fail(&e),
            };
            return match crate::verify::check_bytes(&bytes) {
                Ok(v) => reply(true, v),
                Err(e) => fail(&e),
            };
        }
        "verify_log" => {
            let bytes = match read_small(&s("path").unwrap_or_default(), &["json"]) {
                Ok(b) => b,
                Err(e) => return fail(&e),
            };
            let text = String::from_utf8_lossy(&bytes).to_string();
            let pin = match s("pin") {
                Some(k) => match kk_evidence::b64::decode(&k)
                    .ok()
                    .and_then(|r| <[u8; 32]>::try_from(r).ok())
                {
                    Some(p) => Some(p),
                    None => return fail("`pin` must be a base64url Ed25519 public key (32 bytes)"),
                },
                None => None,
            };
            let anchor = match s("anchor") {
                Some(x) => match x.split_once(':').and_then(|(c, h)| {
                    Some(kk_eventlog::Anchor {
                        count: c.parse().ok()?,
                        head: h.to_string(),
                    })
                }) {
                    Some(a) => Some(a),
                    None => return fail("`anchor` must be COUNT:HEAD"),
                },
                None => None,
            };
            return match kk_eventlog::verify_export(&text, pin.as_ref(), anchor.as_ref()) {
                Ok(r) => reply(
                    true,
                    json!({"valid": true, "entries": r.count, "head": r.head, "signer": r.signer, "signer_pinned": r.signer_pinned}),
                ),
                Err(f) => reply(true, json!({"valid": false, "reason": f.as_str()})),
            };
        }
        _ => {}
    }
    let admin = match admin {
        Ok(a) => a,
        Err(e) => return fail(e),
    };
    let r = match name {
        "overview" => admin.get("/admin/overview"),
        "list_reviews" => admin.get("/admin/jobs"),
        "start_review" => admin.post(
            "/admin/jobs",
            json!({"repo": s("repo"), "recipient": s("recipient")}),
        ),
        "review_status" => admin.get(&format!("/admin/jobs/{}", s("id").unwrap_or_default())),
        "request_approval" => admin.post(
            &format!("/admin/jobs/{}/approval", s("id").unwrap_or_default()),
            json!({}),
        ),
        _ => return fail("unknown tool"),
    };
    match r {
        Ok((c, v)) if c < 300 => reply(true, v),
        Ok((c, v)) => fail(&format!(
            "the service refused ({c}): {}",
            v["error"]["message"].as_str().unwrap_or("no reason given")
        )),
        Err(e) => fail(&e),
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
    if !msg.is_object() || msg["jsonrpc"] != "2.0" {
        return Some(
            json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "invalid request"}}),
        );
    }
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let ok = |result: Value| {
        id.clone()
            .map(|id| json!({"jsonrpc": "2.0", "id": id, "result": result}))
    };
    let err = |code: i64, m: &str| {
        id.clone()
            .map(|id| json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": m}}))
    };
    match method {
        "initialize" => ok(
            json!({"protocolVersion": PROTOCOL, "capabilities": {"tools": {"listChanged": false}}, "serverInfo": {"name": "kakurega", "title": "Kakurega", "version": env!("CARGO_PKG_VERSION")}, "instructions": "Counts and statuses only. Source text, memory text and findings are never returned. Approvals happen in a window or terminal the owner controls."}),
        ),
        "ping" => ok(json!({})),
        "tools/list" => ok(json!({"tools": tools()})),
        "tools/call" => {
            let p = msg.get("params").cloned().unwrap_or(Value::Null);
            let Some(name) = p.get("name").and_then(Value::as_str) else {
                return err(-32602, "params.name is required");
            };
            let args = p
                .get("arguments")
                .cloned()
                .unwrap_or(Value::Object(Map::new()));
            ok(call(admin, name, &args))
        }
        m if m.starts_with("notifications/") => None,
        _ => err(-32601, "method not found"),
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
        if line.len() > MAX_LINE {
            let _ = writeln!(
                out,
                "{}",
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "request too large"}})
            );
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
