#![deny(clippy::print_stdout)]

use crate::approve::{connect, Admin};
use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};

const PROTOCOL: &str = "2025-06-18";
/// Versions this server speaks. It only offers tools, which these three revisions describe the same way.
const SUPPORTED: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// The version to answer with: the client's own when this server speaks it, otherwise the newest this server speaks.
fn negotiate(requested: &str) -> &'static str {
    SUPPORTED
        .iter()
        .copied()
        .find(|v| *v == requested)
        .unwrap_or(PROTOCOL)
}
const MAX_LINE: usize = 1 << 20;
const MAX_FILE: u64 = 20 << 20;
const ID_PATTERN: &str = "^[A-Za-z0-9_-]{1,64}$";

fn string_prop(desc: &str, max: u64) -> Value {
    json!({"type": "string", "description": desc, "minLength": 1, "maxLength": max})
}

fn job_id_prop() -> Value {
    json!({"type": "string", "description": "A review id such as job-3.", "pattern": ID_PATTERN})
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

fn out_obj(props: Value, required: &[&str]) -> Value {
    // Output objects may carry more fields than listed; the listed ones are the stable contract.
    json!({"type": "object", "properties": props, "required": required})
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
            out_obj(json!({"version": {"type": "string"}, "ready": {"type": "boolean"}, "approvals_require_signature": {"type": "boolean"}, "setup_problems": {"type": "array", "items": {"type": "string"}}, "reviews": {"type": "object"}, "memories_by_level": {"type": "object"}, "log": {"type": "object"}}), &["ready", "reviews"]), true),
        tool("list_reviews", "List reviews", "List reviews with state and counts only. Never returns source text.", obj(json!({}), &[]),
            out_obj(json!({"jobs": {"type": "array", "items": {"type": "object"}}}), &["jobs"]), true),
        tool("start_review", "Start a review", "Start a private review of a project folder for a named recipient. Nothing is released by starting a review.",
            obj(json!({"repo": string_prop("Absolute path of the project folder, inside a root the owner allowed.", 4096), "recipient": string_prop("Who will receive the result.", 128)}), &["repo", "recipient"]),
            out_obj(json!({"id": {"type": "string"}}), &["id"]), false),
        tool("review_status", "Review status", "State and counts of one review.", obj(json!({"id": job_id_prop()}), &["id"]),
            out_obj(json!({"id": {"type": "string"}, "state": {"type": "string"}, "stage": {}, "files_reviewed": {}, "files_withheld": {}, "findings_reported": {}, "findings_confirmed": {}, "approval": {}, "delivered": {}}), &["id", "state"]), true),
        tool("request_approval", "Request approval", "Ask the owner to approve the release. This tool cannot approve: the owner approves in the desktop window, the terminal workspace or `kaku approve`, after seeing the exact files.", obj(json!({"id": job_id_prop()}), &["id"]),
            out_obj(json!({"approval": {"type": "string"}, "digest": {"type": "string"}, "how": {"type": "string"}}), &["approval"]), false),
        tool("verify_capsule", "Verify a capsule", "Check a capsule file (.kkg) offline and report what it does and does not establish.", obj(json!({"path": string_prop("Path of a .kkg capsule file.", 4096)}), &["path"]),
            out_obj(json!({"valid": {"type": "boolean"}, "reason": {"type": "string"}, "signer": {}, "recipient": {}, "established": {"type": "array"}, "not_established": {"type": "array"}}), &["valid"]), true),
        tool("verify_log", "Verify the event log", "Check a saved signed event log (.json) offline, optionally against a pinned key and an anchor.", obj(json!({"path": string_prop("Path of a saved event log.", 4096), "pin": string_prop("Base64url Ed25519 public key to require.", 64), "anchor": string_prop("COUNT:HEAD printed by `kaku log export`.", 200)}), &["path"]),
            out_obj(json!({"valid": {"type": "boolean"}, "reason": {"type": "string"}, "entries": {"type": "integer"}, "head": {"type": "string"}, "signer": {"type": "string"}}), &["valid"]), true)
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
                let count = s.chars().count() as u64;
                if count < p["minLength"].as_u64().unwrap_or(1)
                    || count > p["maxLength"].as_u64().unwrap_or(4096)
                    || s.chars().any(|c| c.is_control())
                {
                    return Err(format!(
                        "`{k}` must be {} to {} characters without control characters",
                        p["minLength"].as_u64().unwrap_or(1),
                        p["maxLength"].as_u64().unwrap_or(4096)
                    ));
                }
                if let Some(pat) = p["pattern"].as_str() {
                    if pat != ID_PATTERN {
                        return Err(format!(
                            "`{k}` has a rule this server cannot check, so it is refused"
                        ));
                    }
                    if count > 64
                        || !s
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

/// Where the verify tools may read from: the folder the agent was started in and the owner's export folder. Anything
/// else, and anything missing or unreadable, gets the same answer, so a path cannot be probed.
pub struct Ctx {
    roots: Vec<std::path::PathBuf>,
    /// Tier 0: only the tools that read. The tools that start a review or ask for an approval are neither listed nor callable.
    read_only: bool,
    /// When set, only these tools are listed and callable, whatever else the tool list holds.
    allow: Option<Vec<String>>,
}

impl Ctx {
    pub fn new(data_dir: &std::path::Path) -> Ctx {
        let mut roots = Vec::new();
        if let Ok(cwd) = std::env::current_dir().and_then(|d| d.canonicalize()) {
            roots.push(cwd);
        }
        if let Ok(exports) = data_dir.join("exports").canonicalize() {
            roots.push(exports);
        }
        Ctx {
            roots,
            read_only: false,
            allow: None,
        }
    }

    /// Limits this connection to exactly the named tools.
    pub fn allow(mut self, names: Vec<String>) -> Ctx {
        self.allow = Some(names);
        self
    }

    pub fn read_only(mut self, on: bool) -> Ctx {
        self.read_only = on;
        self
    }

    /// The tools this connection may see and call.
    fn visible(&self) -> Value {
        Value::Array(
            tools()
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|t| !self.read_only || t["annotations"]["readOnlyHint"] == true)
                .filter(|t| {
                    self.allow
                        .as_ref()
                        .is_none_or(|names| names.iter().any(|n| t["name"] == n.as_str()))
                })
                .collect(),
        )
    }

    fn allows(&self, canonical: &std::path::Path) -> bool {
        self.roots.iter().any(|r| canonical.starts_with(r))
    }
}

fn read_small(ctx: &Ctx, path: &str, suffixes: &[&str]) -> Result<Vec<u8>, String> {
    let p = std::path::Path::new(path);
    let canonical = p.canonicalize().ok().filter(|c| ctx.allows(c));
    let Some(canonical) = canonical else {
        return Err("cannot read that file".into());
    };
    let p = canonical.as_path();
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

fn call(admin: &dyn Fn() -> Result<Admin, String>, ctx: &Ctx, name: &str, a: &Value) -> Value {
    let all = ctx.visible();
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
            let bytes = match read_small(ctx, &s("path").unwrap_or_default(), &["kkg", "json"]) {
                Ok(b) => b,
                Err(e) => return fail(&e),
            };
            return match crate::verify::check_bytes(&bytes) {
                Ok(v) => reply(true, v),
                Err(e) => fail(&e),
            };
        }
        "verify_log" => {
            let bytes = match read_small(ctx, &s("path").unwrap_or_default(), &["json"]) {
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
    // Looked up for every call: the agent may have been started before `kaku serve`, and serve may have restarted since,
    // with a new port and token.
    let admin = match admin() {
        Ok(a) => a,
        Err(e) => return fail(&e),
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

const PARSE_ERROR: &str =
    r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}}"#;

fn invalid_request(id: Value) -> Option<Value> {
    Some(
        json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32600, "message": "invalid request"}}),
    )
}

/// Answers one JSON-RPC line, or returns None when the message gets no answer. Nothing runs for a message without an
/// `id`: it is a notification, and a notification never gets a reply, so a tool started by one could not report back.
pub fn handle(admin: &dyn Fn() -> Result<Admin, String>, ctx: &Ctx, line: &str) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line.trim_start_matches('\u{feff}')) {
        Ok(v) => v,
        Err(_) => return serde_json::from_str(PARSE_ERROR).ok(),
    };
    if !msg.is_object() || msg["jsonrpc"] != "2.0" {
        return invalid_request(Value::Null);
    }
    // An answer to a request this server never sent (it sends none).
    if msg.get("method").is_none() && (msg.get("result").is_some() || msg.get("error").is_some()) {
        return None;
    }
    let id = match msg.get("id") {
        None => None,
        Some(v) if v.is_string() || v.is_i64() || v.is_u64() => Some(v.clone()),
        Some(_) => return invalid_request(Value::Null),
    };
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        return invalid_request(id.unwrap_or(Value::Null));
    };
    let id = id?;
    let ok = |result: Value| Some(json!({"jsonrpc": "2.0", "id": id.clone(), "result": result}));
    let err = |code: i64, m: &str| {
        Some(json!({"jsonrpc": "2.0", "id": id.clone(), "error": {"code": code, "message": m}}))
    };
    match method {
        "initialize" => {
            let requested = match msg["params"].get("protocolVersion") {
                None => PROTOCOL,
                Some(Value::String(v)) => negotiate(v),
                Some(_) => return err(-32602, "protocolVersion must be text"),
            };
            ok(
                json!({"protocolVersion": requested, "capabilities": {"tools": {"listChanged": false}}, "serverInfo": {"name": "kakurega", "title": "Kakurega", "version": env!("CARGO_PKG_VERSION")}, "instructions": "Counts and statuses only. Source text, memory text and findings are never returned. Approvals happen in a window or terminal the owner controls."}),
            )
        }
        "ping" => ok(json!({})),
        // All tools fit in one page, so a cursor can only be wrong.
        "tools/list" if msg["params"].get("cursor").is_some_and(|c| !c.is_null()) => err(
            -32602,
            "there is only one page of tools; no cursor is needed",
        ),
        "tools/list" => ok(json!({"tools": ctx.visible()})),
        // Not offered, but clients that probe for them get an empty answer instead of an error to log.
        "resources/list" => ok(json!({"resources": []})),
        "resources/templates/list" => ok(json!({"resourceTemplates": []})),
        "prompts/list" => ok(json!({"prompts": []})),
        "tools/call" => {
            let p = msg.get("params").cloned().unwrap_or(Value::Null);
            let Some(name) = p.get("name").and_then(Value::as_str) else {
                return err(-32602, "params.name is required");
            };
            if !ctx
                .visible()
                .as_array()
                .is_some_and(|t| t.iter().any(|t| t["name"] == name))
            {
                return err(-32602, "unknown tool");
            }
            let args = p
                .get("arguments")
                .cloned()
                .unwrap_or(Value::Object(Map::new()));
            ok(call(admin, ctx, name, &args))
        }
        m if m.starts_with("notifications/") => None,
        _ => err(-32601, "method not found"),
    }
}

/// Reads one line without ever holding more than `max` bytes of it: the rest of an oversized line is discarded as it
/// arrives. Returns None at end of input.
fn read_line_bounded(input: &mut dyn BufRead, max: usize) -> Option<(Vec<u8>, bool)> {
    let mut line = Vec::new();
    let mut too_long = false;
    let mut any = false;
    loop {
        let buf = match input.fill_buf() {
            Ok(b) => b,
            Err(_) => return None,
        };
        if buf.is_empty() {
            return any.then_some((line, too_long));
        }
        any = true;
        let (chunk, found) = match buf.iter().position(|b| *b == b'\n') {
            Some(i) => (&buf[..i], Some(i + 1)),
            None => (buf, None),
        };
        if !too_long {
            if line.len() + chunk.len() > max {
                too_long = true;
                line.clear();
            } else {
                line.extend_from_slice(chunk);
            }
        }
        let used = found.unwrap_or(buf.len());
        input.consume(used);
        if found.is_some() {
            return Some((line, too_long));
        }
    }
}

/// Tools that call the running service over the network. They can be slow, so they run beside the reader and a cancel or
/// a ping is never stuck behind them. The others read a local file or only refuse, and run in order.
const SLOW_TOOLS: [&str; 5] = [
    "overview",
    "list_reviews",
    "start_review",
    "review_status",
    "request_approval",
];
/// At most this many service-backed calls run at once.
const MAX_IN_FLIGHT: usize = 4;
/// At the end of input, work in progress gets this long to answer before the process leaves.
const DRAIN: std::time::Duration = std::time::Duration::from_secs(5);

struct Shared {
    out: std::sync::Mutex<std::io::Stdout>,
    /// Requests the client cancelled, by id as JSON text. An answer for one of them is not sent.
    cancelled: std::sync::Mutex<std::collections::HashSet<String>>,
    in_flight: std::sync::atomic::AtomicUsize,
}

impl Shared {
    fn send(&self, frame: &Value) -> bool {
        let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        writeln!(out, "{frame}").and_then(|_| out.flush()).is_ok()
    }
}

/// The id of a service-backed `tools/call` that will really call the service: the tool is visible and its arguments are
/// valid. Those are the only calls that run beside the reader. A refusal is answered in order, at once.
fn slow_call(ctx: &Ctx, line: &str) -> Option<Value> {
    let msg: Value = serde_json::from_str(line.trim_start_matches('\u{feff}')).ok()?;
    let id = msg.get("id")?;
    if !(id.is_string() || id.is_i64() || id.is_u64()) || msg["method"] != "tools/call" {
        return None;
    }
    let name = msg["params"]["name"].as_str()?;
    if !SLOW_TOOLS.contains(&name) {
        return None;
    }
    let tools = ctx.visible();
    let def = tools.as_array()?.iter().find(|t| t["name"] == name)?;
    let empty = Value::Object(Map::new());
    let args = msg["params"].get("arguments").unwrap_or(&empty);
    check_args(&def["inputSchema"], args).ok()?;
    Some(id.clone())
}

/// The id a `notifications/cancelled` names, as JSON text.
fn cancelled_id(line: &str) -> Option<String> {
    let msg: Value = serde_json::from_str(line.trim_start_matches('\u{feff}')).ok()?;
    if msg["method"] != "notifications/cancelled" || msg.get("id").is_some() {
        return None;
    }
    let id = msg["params"].get("requestId")?;
    (id.is_string() || id.is_i64() || id.is_u64()).then(|| id.to_string())
}

pub fn run(args: &[String]) -> u8 {
    use std::sync::atomic::Ordering;
    let args: std::sync::Arc<Vec<String>> = std::sync::Arc::new(args.to_vec());
    let mut ctx = Ctx::new(&crate::data_dir(&args)).read_only(
        args.iter().any(|a| a == "--read-only")
            || std::env::var("KAKU_MCP_READ_ONLY").is_ok_and(|v| v == "1"),
    );
    if let Some(list) = crate::flag(&args, "--allow") {
        let names: Vec<String> = list
            .split(',')
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .collect();
        let known: Vec<String> = tools()
            .as_array()
            .map(|t| {
                t.iter()
                    .filter_map(|t| t["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(bad) = names.iter().find(|n| !known.contains(n)) {
            eprintln!(
                "--allow names a tool that does not exist: {bad}\nthe tools are: {}",
                known.join(", ")
            );
            return 2;
        }
        if names.is_empty() {
            eprintln!("--allow needs at least one tool name");
            return 2;
        }
        ctx = ctx.allow(names);
    }
    let ctx = std::sync::Arc::new(ctx);
    let shared = std::sync::Arc::new(Shared {
        out: std::sync::Mutex::new(std::io::stdout()),
        cancelled: Default::default(),
        in_flight: std::sync::atomic::AtomicUsize::new(0),
    });
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    while let Some((bytes, too_long)) = read_line_bounded(&mut input, MAX_LINE) {
        if too_long {
            let frame = json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "request too large"}});
            if !shared.send(&frame) {
                break;
            }
            continue;
        }
        let Ok(line) = String::from_utf8(bytes) else {
            if !shared.send(&serde_json::from_str(PARSE_ERROR).unwrap_or(Value::Null)) {
                break;
            }
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(id) = cancelled_id(&line) {
            shared
                .cancelled
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id);
            continue;
        }
        if let Some(id) = slow_call(&ctx, &line) {
            if shared.in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
                shared.in_flight.fetch_sub(1, Ordering::SeqCst);
                let frame = json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": "too many requests are in progress; try again in a moment"}});
                if !shared.send(&frame) {
                    break;
                }
                continue;
            }
            let (shared, ctx, args) = (shared.clone(), ctx.clone(), args.clone());
            std::thread::spawn(move || {
                let admin = || connect(&args);
                let reply = handle(&admin, &ctx, &line);
                let key = id.to_string();
                let was_cancelled = shared
                    .cancelled
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&key);
                if let (Some(frame), false) = (reply, was_cancelled) {
                    shared.send(&frame);
                }
                shared.in_flight.fetch_sub(1, Ordering::SeqCst);
            });
            continue;
        }
        let admin = || connect(&args);
        if let Some(r) = handle(&admin, &ctx, &line) {
            if !shared.send(&r) {
                break;
            }
        }
    }
    // input ended: let work in progress answer, but never hold the process longer than DRAIN
    let waited = std::time::Instant::now();
    while shared.in_flight.load(Ordering::SeqCst) > 0 && waited.elapsed() < DRAIN {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_in_the_tool_list_is_one_this_server_checks() {
        for t in tools().as_array().unwrap() {
            let schema = &t["inputSchema"];
            for (name, p) in schema["properties"].as_object().unwrap() {
                assert_eq!(
                    p["type"], "string",
                    "{}: `{name}` has a type the checker does not know",
                    t["name"]
                );
                if let Some(pat) = p.get("pattern") {
                    assert_eq!(
                        pat, ID_PATTERN,
                        "{}: `{name}` has a pattern the checker would skip",
                        t["name"]
                    );
                }
            }
        }
    }

    fn schema_of(name: &str) -> Value {
        tools()
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap()["inputSchema"]
            .clone()
    }

    #[test]
    fn a_rule_the_checker_does_not_know_refuses_instead_of_passing() {
        let schema = json!({"type": "object", "properties": {"x": {"type": "string", "pattern": "^[a-z]+$", "maxLength": 10}}, "required": ["x"]});
        let e = check_args(&schema, &json!({"x": "abc"})).unwrap_err();
        assert!(e.contains("cannot check"), "{e}");
    }

    #[test]
    fn minimum_length_is_honoured_and_counts_characters() {
        let schema = json!({"type": "object", "properties": {"x": {"type": "string", "minLength": 3, "maxLength": 5}}, "required": ["x"]});
        assert!(check_args(&schema, &json!({"x": "ab"})).is_err());
        assert!(check_args(&schema, &json!({"x": "abc"})).is_ok());
        assert!(
            check_args(&schema, &json!({"x": "вгд"})).is_ok(),
            "three characters, six bytes"
        );
        assert!(check_args(&schema, &json!({"x": "abcdef"})).is_err());
    }

    #[test]
    fn a_review_id_is_at_most_64_characters_of_letters_digits_dash_and_underscore() {
        let schema = schema_of("review_status");
        assert!(check_args(&schema, &json!({"id": "a".repeat(64)})).is_ok());
        assert!(check_args(&schema, &json!({"id": "a".repeat(65)})).is_err());
        assert!(check_args(&schema, &json!({"id": "job/../3"})).is_err());
        assert!(check_args(&schema, &json!({"id": ""})).is_err());
    }
}
