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

/// The tools that change something. This list, and nothing else, decides what read-only mode hides and what the
/// `readOnlyHint` of every tool says; the hint is advertised metadata and is never read back to make a decision.
pub const MUTATING: [&str; 2] = ["start_review", "request_approval"];

fn tool(name: &str, title: &str, description: &str, input: Value, output: Value) -> Value {
    let read_only = !MUTATING.contains(&name);
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
            out_obj(json!({"version": {"type": "string"}, "ready": {"type": "boolean"}, "approvals_require_signature": {"type": "boolean"}, "setup_problems": {"type": "array", "items": {"type": "string"}}, "optional_missing": {"type": "array", "items": {"type": "string"}}, "reviews": {"type": "object"}, "memories_by_level": {"type": "object"}, "log": {"type": "object"}}), &["ready", "reviews"])),
        tool("list_reviews", "List reviews", "List reviews with state and counts only. Never returns source text.", obj(json!({}), &[]),
            out_obj(json!({"jobs": {"type": "array", "items": {"type": "object"}}}), &["jobs"])),
        tool("start_review", "Start a review", "Start a private review of a project folder for a named recipient. Nothing is released by starting a review.",
            obj(json!({"repo": string_prop("Absolute path of the project folder, inside a root the owner allowed.", 4096), "recipient": string_prop("Who will receive the result.", 128)}), &["repo", "recipient"]),
            out_obj(json!({"id": {"type": "string"}}), &["id"])),
        tool("review_status", "Review status", "State and counts of one review.", obj(json!({"id": job_id_prop()}), &["id"]),
            out_obj(json!({"id": {"type": "string"}, "state": {"type": "string"}, "stage": {}, "files_reviewed": {}, "files_withheld": {}, "findings_reported": {}, "findings_confirmed": {}, "approval": {}, "delivered": {}}), &["id", "state"])),
        tool("request_approval", "Request approval", "Ask the owner to approve the release. This tool cannot approve: the owner approves in the desktop window, the terminal workspace or `kaku approve`, after seeing the exact files.", obj(json!({"id": job_id_prop()}), &["id"]),
            out_obj(json!({"approval": {"type": "string"}, "digest": {"type": "string"}, "how": {"type": "string"}}), &["approval"])),
        tool("verify_capsule", "Verify a capsule", "Check a capsule file (.kkg) offline and report what it does and does not establish.", obj(json!({"path": string_prop("Path of a .kkg capsule file.", 4096)}), &["path"]),
            out_obj(json!({"valid": {"type": "boolean"}, "reason": {"type": "string"}, "signer": {}, "recipient": {}, "established": {"type": "array"}, "not_established": {"type": "array"}}), &["valid"])),
        tool("verify_log", "Verify the event log", "Check a saved signed event log (.json) offline, optionally against a pinned key and an anchor.", obj(json!({"path": string_prop("Path of a saved event log.", 4096), "pin": string_prop("Base64url Ed25519 public key to require.", 64), "anchor": string_prop("COUNT:HEAD printed by `kaku log export`.", 200)}), &["path"]),
            out_obj(json!({"valid": {"type": "boolean"}, "reason": {"type": "string"}, "entries": {"type": "integer"}, "head": {"type": "string"}, "signer": {"type": "string"}}), &["valid"]))
    ])
}

/// Whether `v` fits `schema`, for the few keywords the output schemas use (`type`, `properties`, `required`, `items`; a test
/// keeps it that way). Fields the schema does not list are allowed: the listed ones are the stable contract.
fn conforms(schema: &Value, v: &Value) -> Result<(), String> {
    if let Some(t) = schema["type"].as_str() {
        let fits = match t {
            "object" => v.is_object(),
            "array" => v.is_array(),
            "string" => v.is_string(),
            "boolean" => v.is_boolean(),
            "integer" => v.is_i64() || v.is_u64(),
            "number" => v.is_number(),
            "null" => v.is_null(),
            other => {
                return Err(format!(
                    "the schema uses a type this server does not check: {other}"
                ))
            }
        };
        if !fits {
            return Err(format!("expected {t}"));
        }
    }
    if let Some(map) = v.as_object() {
        for r in schema["required"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !map.contains_key(r) {
                return Err(format!("`{r}` is missing"));
            }
        }
        if let Some(props) = schema["properties"].as_object() {
            for (k, p) in props {
                if let Some(x) = map.get(k) {
                    conforms(p, x).map_err(|e| format!("`{k}`: {e}"))?;
                }
            }
        }
    }
    if let (Some(items), Some(list)) = (schema.get("items"), v.as_array()) {
        for (i, x) in list.iter().enumerate() {
            conforms(items, x).map_err(|e| format!("item {i}: {e}"))?;
        }
    }
    Ok(())
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
    /// The tools that act are listed and callable only while the owner holds a lease for them, and every call spends a use.
    leased: bool,
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
            leased: false,
        }
    }

    /// Makes the tools that act depend on the owner's leases.
    pub fn leased(mut self, on: bool) -> Ctx {
        self.leased = on;
        self
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
                .filter(|t| {
                    !self.read_only || !MUTATING.contains(&t["name"].as_str().unwrap_or_default())
                })
                .filter(|t| {
                    self.allow
                        .as_ref()
                        .is_none_or(|names| names.iter().any(|n| t["name"] == n.as_str()))
                })
                .collect(),
        )
    }

    /// What this connection is shown right now. In leased mode a tool that acts appears only while the service reports a lease
    /// for it; when the service cannot be asked, none does.
    fn listed(&self, admin: &dyn Fn() -> Result<Admin, String>) -> Value {
        let all = self.visible();
        if !self.leased {
            return all;
        }
        let leased: Vec<String> = admin()
            .and_then(|a| a.get("/admin/agent-access"))
            .ok()
            .filter(|(code, _)| *code == 200)
            .and_then(|(_, v)| v["leases"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter(|l| l["remaining"].as_i64().unwrap_or(0) > 0)
            .filter_map(|l| l["tool"].as_str().map(str::to_string))
            .collect();
        Value::Array(
            all.as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|t| {
                    let name = t["name"].as_str().unwrap_or_default();
                    !MUTATING.contains(&name) || leased.iter().any(|l| l == name)
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
    let result = call_unchecked(admin, ctx, name, a);
    // A success reply is only passed on as structured data if it fits the tool's own output schema. The data comes from
    // another process, so a wrong shape is refused here rather than handed to an agent that trusts the schema.
    if result["isError"] == false {
        let schema = tools()
            .as_array()
            .and_then(|t| {
                t.iter()
                    .find(|t| t["name"] == name)
                    .map(|t| t["outputSchema"].clone())
            })
            .unwrap_or(Value::Null);
        if let Err(why) = conforms(&schema, &result["structuredContent"]) {
            return fail(&format!(
                "the answer did not match this tool's output schema ({why}), so it was not used"
            ));
        }
    }
    result
}

/// How a finished call is described in the signed log: it worked, it failed, or it was refused (a lease, or the arguments).
fn outcome_of(result: &Value) -> &'static str {
    if result["isError"] != true {
        return "ok";
    }
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    if text.contains("acts on the owner's behalf") {
        "refused"
    } else {
        "error"
    }
}

/// Tells the service a tool was used and how it ended, so the owner's signed log shows what an agent did. Only the tool's
/// name and the outcome go: never the arguments or the answer. A service that cannot be reached changes nothing for the agent.
fn report(admin: &dyn Fn() -> Result<Admin, String>, tool: &str, outcome: &str) {
    if let Ok(a) = admin() {
        let _ = a.post_within(
            "/admin/agent-calls",
            json!({"tool": tool, "outcome": outcome}),
            2,
        );
    }
}

/// Why a lease did not let a call through, in words the agent can pass on to the owner.
fn lease_refusal(tool: &str, reason: &str) -> String {
    let why = match reason {
        "expired" => "the owner's lease for it has run out of time".to_string(),
        "exhausted" => "the owner's lease for it has no uses left".to_string(),
        _ => "the owner has not allowed it".to_string(),
    };
    format!(
        "{tool} acts on the owner's behalf and {why}. Nothing was started. The owner can allow it with: kaku agent grant {tool} --uses 3 --minutes 60"
    )
}

/// In leased mode a tool that acts must first win a use from the service, which keeps the count where a restart of this
/// process cannot reset it. A use spent on a call that then fails is given back.
fn call_unchecked(
    admin: &dyn Fn() -> Result<Admin, String>,
    ctx: &Ctx,
    name: &str,
    a: &Value,
) -> Value {
    if !(ctx.leased && MUTATING.contains(&name)) {
        return call_inner(admin, ctx, name, a);
    }
    // arguments are checked first, so a malformed call costs nothing and asks nothing
    let def = ctx.visible();
    let Some(def) = def
        .as_array()
        .and_then(|t| t.iter().find(|t| t["name"] == name).cloned())
    else {
        return fail("unknown tool");
    };
    if let Err(e) = check_args(&def["inputSchema"], a) {
        return fail(&e);
    }
    let service = match admin() {
        Ok(s) => s,
        Err(_) => return fail("the owner's service cannot be reached, so the owner's lease cannot be checked. Nothing was started"),
    };
    match service.post("/admin/agent-access/use", json!({"tool": name})) {
        Ok((200, v)) if v["allowed"] == true => {}
        Ok((200, v)) => return fail(&lease_refusal(name, v["reason"].as_str().unwrap_or(""))),
        _ => return fail("the owner's lease could not be checked. Nothing was started"),
    }
    let result = call_inner(admin, ctx, name, a);
    if result["isError"] == true {
        let _ = service.post("/admin/agent-access/refund", json!({"tool": name}));
    }
    result
}

fn call_inner(
    admin: &dyn Fn() -> Result<Admin, String>,
    ctx: &Ctx,
    name: &str,
    a: &Value,
) -> Value {
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
                json!({"protocolVersion": requested, "capabilities": {"tools": {"listChanged": false}, "resources": {"listChanged": false, "subscribe": false}, "prompts": {"listChanged": false}}, "serverInfo": {"name": "kakurega", "title": "Kakurega", "version": env!("CARGO_PKG_VERSION")}, "instructions": "Counts and statuses only. Source text, memory text and findings are never returned. Approvals happen in a window or terminal the owner controls."}),
            )
        }
        "ping" => ok(json!({})),
        // All tools fit in one page, so a cursor can only be wrong.
        "tools/list" if msg["params"].get("cursor").is_some_and(|c| !c.is_null()) => err(
            -32602,
            "there is only one page of tools; no cursor is needed",
        ),
        "tools/list" => ok(json!({"tools": ctx.listed(admin)})),
        "resources/list" | "resources/templates/list" | "prompts/list"
            if msg["params"].get("cursor").is_some_and(|c| !c.is_null()) =>
        {
            err(-32602, "there is only one page; no cursor is needed")
        }
        "resources/list" => ok(json!({"resources": resources_listed(ctx)})),
        "resources/templates/list" => ok(json!({"resourceTemplates": templates_listed(ctx)})),
        "resources/read" => {
            let uri = match msg["params"].get("uri") {
                Some(Value::String(u)) => u.as_str(),
                _ => return err(-32602, "params.uri is required and must be text"),
            };
            let Some((tool, args)) = resource_target(ctx, uri) else {
                // an address that is not exactly a listed resource is never written anywhere
                report(admin, "unknown", "refused");
                return err(-32002, "resource not found");
            };
            let result = call(admin, ctx, tool, &args);
            report(admin, tool, outcome_of(&result));
            if result["isError"] == false {
                ok(
                    json!({"contents": [{"uri": uri, "mimeType": "application/json", "text": result["structuredContent"].to_string()}]}),
                )
            } else {
                let why = result["content"][0]["text"]
                    .as_str()
                    .unwrap_or("the service could not be asked");
                err(-32603, why)
            }
        }
        "prompts/list" => ok(json!({"prompts": prompts_listed(ctx)})),
        "prompts/get" => prompt_get(ctx, &msg["params"]).map_or_else(|m| err(-32602, &m), ok),
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
                // a name the agent made up is never written anywhere
                report(admin, "unknown", "refused");
                return err(-32602, "unknown tool");
            }
            let args = p
                .get("arguments")
                .cloned()
                .unwrap_or(Value::Object(Map::new()));
            let result = call(admin, ctx, name, &args);
            report(admin, name, outcome_of(&result));
            ok(result)
        }
        m if m.starts_with("notifications/") => None,
        _ => err(-32601, "method not found"),
    }
}

const OVERVIEW_URI: &str = "kakurega://overview";
const STATUS_PREFIX: &str = "kakurega://review/";
const STATUS_SUFFIX: &str = "/status";
const PROMPT_NAME: &str = "private_review";
const PROMPT_TOOLS: [&str; 3] = ["start_review", "review_status", "request_approval"];

impl Ctx {
    fn sees(&self, tool: &str) -> bool {
        self.visible()
            .as_array()
            .is_some_and(|t| t.iter().any(|t| t["name"] == tool))
    }
}

fn resources_listed(ctx: &Ctx) -> Value {
    if !ctx.sees("overview") {
        return json!([]);
    }
    json!([{
        "uri": OVERVIEW_URI,
        "name": "overview",
        "title": "Overview",
        "description": "Counts and statuses of the reviews on this computer: how many are running, ready or delivered. Never source text.",
        "mimeType": "application/json"
    }])
}

fn templates_listed(ctx: &Ctx) -> Value {
    if !ctx.sees("review_status") {
        return json!([]);
    }
    json!([{
        "uriTemplate": format!("{STATUS_PREFIX}{{id}}{STATUS_SUFFIX}"),
        "name": "review_status",
        "title": "Review status",
        "description": "State and counts of one review, for example kakurega://review/job-3/status.",
        "mimeType": "application/json"
    }])
}

/// The tool, and its arguments, that produces what this address names. Only the exact listed shapes resolve, and only for a
/// tool this connection may call; anything else is not a resource.
fn resource_target(ctx: &Ctx, uri: &str) -> Option<(&'static str, Value)> {
    if uri == OVERVIEW_URI {
        return ctx.sees("overview").then(|| ("overview", json!({})));
    }
    let id = uri
        .strip_prefix(STATUS_PREFIX)?
        .strip_suffix(STATUS_SUFFIX)?;
    let valid = (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    (valid && ctx.sees("review_status")).then(|| ("review_status", json!({"id": id})))
}

fn prompts_available(ctx: &Ctx) -> bool {
    PROMPT_TOOLS.iter().all(|t| ctx.sees(t))
}

fn prompts_listed(ctx: &Ctx) -> Value {
    if !prompts_available(ctx) {
        return json!([]);
    }
    json!([{
        "name": PROMPT_NAME,
        "title": "Private review",
        "description": "Review a project folder privately for a named recipient. The owner approves what is released.",
        "arguments": [
            {"name": "repo", "description": "Absolute path of the project folder.", "required": true},
            {"name": "recipient", "description": "Who will receive the result.", "required": true}
        ]
    }])
}

fn prompt_get(ctx: &Ctx, params: &Value) -> Result<Value, String> {
    if params.get("name").and_then(Value::as_str) != Some(PROMPT_NAME) || !prompts_available(ctx) {
        return Err("unknown prompt".into());
    }
    let args = params
        .get("arguments")
        .and_then(Value::as_object)
        .ok_or("arguments are required")?;
    if let Some(extra) = args
        .keys()
        .find(|k| !matches!(k.as_str(), "repo" | "recipient"))
    {
        return Err(format!("unknown argument {extra}"));
    }
    let value = |name: &str, max: usize| -> Result<String, String> {
        let v = args
            .get(name)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("{name} is required and must be text"))?;
        if v.chars().count() > max || v.chars().any(char::is_control) {
            return Err(format!("{name} is too long or has control characters"));
        }
        Ok(v.to_string())
    };
    let repo = value("repo", 4096)?;
    let recipient = value("recipient", 128)?;
    // the values go in as quoted JSON text, so neither can add a line or end its own quotes
    let (repo, recipient) = (json!(repo).to_string(), json!(recipient).to_string());
    let text = format!(
        "Review the project folder {repo} privately for the recipient {recipient}.\n\
1. Call start_review with exactly those two values and keep the id it returns.\n\
2. Call review_status with that id until the state is no longer running.\n\
3. When the state is ready, call request_approval with the id. This only asks: nothing is released.\n\
4. Tell the owner the review is waiting and that they approve or refuse it themselves with `kaku approve` or in the app.\n\
You see counts and statuses only. Never try to read, copy or send the project's contents, and do not release anything. If a tool is refused, stop and tell the owner what was refused."
    );
    Ok(json!({
        "description": "Review a project folder privately for a named recipient.",
        "messages": [{"role": "user", "content": {"type": "text", "text": text}}]
    }))
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
    /// The service-backed calls running now, by id as JSON text, each with whether the client cancelled it. A cancel only
    /// ever marks a call that is in this map, so it can never silence a request that was not running; the answer of a
    /// cancelled call is not sent. A finished call leaves the map, which frees its id.
    in_flight: std::sync::Mutex<std::collections::HashMap<String, bool>>,
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

/// The options `kaku mcp` knows. Anything else is refused: an option that is misspelled must never quietly leave an agent
/// with more tools than its owner meant to give.
const MCP_FLAGS: [&str; 3] = ["--read-only", "--leased", "--allow"];

/// Why the arguments are not acceptable, or None. `--data-dir` and `--allow` each take a value.
fn unknown_option(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--data-dir" || a == "--allow" {
            let value = args.get(i + 1).map(String::as_str).unwrap_or("");
            if value.is_empty() || value.starts_with("--") {
                return Some(format!("{a} needs a value"));
            }
            i += 2;
            continue;
        }
        if !MCP_FLAGS.contains(&a) {
            return Some(format!("unknown option `{a}`"));
        }
        i += 1;
    }
    None
}

/// The tool names in an `--allow` list: trimmed, without repeats, each one a tool that exists.
pub(crate) fn parse_allow(list: &str) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = Vec::new();
    for n in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        if !names.iter().any(|x| x == n) {
            names.push(n.to_string());
        }
    }
    let known: Vec<String> = tools()
        .as_array()
        .map(|t| {
            t.iter()
                .filter_map(|t| t["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if let Some(bad) = names.iter().find(|n| !known.contains(n)) {
        return Err(format!(
            "--allow names a tool that does not exist: {bad}\nthe tools are: {}",
            known.join(", ")
        ));
    }
    if names.is_empty() {
        return Err("--allow needs at least one tool name".into());
    }
    Ok(names)
}

pub fn run(args: &[String]) -> u8 {
    if args.first().map(String::as_str) == Some("install") {
        return crate::mcp_install::run(&args[1..]);
    }
    if let Some(why) = unknown_option(args) {
        eprintln!(
            "{why}\nusage: kaku mcp [--read-only] [--allow TOOL,TOOL] [--leased] [--data-dir DIR]"
        );
        return 2;
    }
    let args: std::sync::Arc<Vec<String>> = std::sync::Arc::new(args.to_vec());
    let mut ctx = Ctx::new(&crate::data_dir(&args))
        .leased(args.iter().any(|a| a == "--leased"))
        .read_only(
            args.iter().any(|a| a == "--read-only")
                || std::env::var("KAKU_MCP_READ_ONLY").is_ok_and(|v| v == "1"),
        );
    if let Some(list) = crate::flag(&args, "--allow") {
        match parse_allow(&list) {
            Ok(names) => ctx = ctx.allow(names),
            Err(why) => {
                eprintln!("{why}");
                return 2;
            }
        }
    }
    let ctx = std::sync::Arc::new(ctx);
    let shared = std::sync::Arc::new(Shared {
        out: std::sync::Mutex::new(std::io::stdout()),
        in_flight: Default::default(),
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
            if let Some(cancelled) = shared
                .in_flight
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_mut(&id)
            {
                *cancelled = true;
            }
            continue;
        }
        if let Some(id) = slow_call(&ctx, &line) {
            let key = id.to_string();
            let refusal = {
                let mut running = shared.in_flight.lock().unwrap_or_else(|e| e.into_inner());
                if running.contains_key(&key) {
                    Some("that request id is already in use by a request still running")
                } else if running.len() >= MAX_IN_FLIGHT {
                    Some("too many requests are in progress; try again in a moment")
                } else {
                    running.insert(key.clone(), false);
                    None
                }
            };
            if let Some(why) = refusal {
                let (code, why) = if why.starts_with("that request") {
                    (-32600, why)
                } else {
                    (-32000, why)
                };
                let frame =
                    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": why}});
                if !shared.send(&frame) {
                    break;
                }
                continue;
            }
            let (shared, ctx, args) = (shared.clone(), ctx.clone(), args.clone());
            std::thread::spawn(move || {
                let admin = || connect(&args);
                let reply = handle(&admin, &ctx, &line);
                // leave the map and learn whether the client cancelled, in one step
                let was_cancelled = shared
                    .in_flight
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&key)
                    .unwrap_or(false);
                if let (Some(frame), false) = (reply, was_cancelled) {
                    shared.send(&frame);
                }
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
    while !shared
        .in_flight
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_empty()
        && waited.elapsed() < DRAIN
    {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    0
}

#[cfg(test)]
mod tests {
    #[test]
    fn ml01_the_tools_that_act_are_exactly_the_tools_an_owner_can_lease() {
        let mut a: Vec<&str> = MUTATING.to_vec();
        let mut b: Vec<&str> = kk_service::LEASABLE_TOOLS.to_vec();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }

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

    #[test]
    fn read_only_visibility_comes_from_the_explicit_list_and_the_hints_follow_it() {
        let names = |v: Value| -> Vec<String> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string())
                .collect()
        };
        let all = names(tools());
        for t in tools().as_array().unwrap() {
            let name = t["name"].as_str().unwrap();
            assert_eq!(
                t["annotations"]["readOnlyHint"],
                !MUTATING.contains(&name),
                "{name}: the hint must be derived from the explicit list"
            );
        }
        for m in MUTATING {
            assert!(
                all.iter().any(|n| n == m),
                "{m} is in MUTATING but is not a tool"
            );
        }
        let dir = std::env::temp_dir();
        let ro = Ctx::new(&dir).read_only(true);
        let visible = names(ro.visible());
        let expected: Vec<String> = all
            .iter()
            .filter(|n| !MUTATING.contains(&n.as_str()))
            .cloned()
            .collect();
        assert_eq!(visible, expected);
        assert!(!visible.iter().any(|n| MUTATING.contains(&n.as_str())));
        assert_eq!(
            names(Ctx::new(&dir).visible()),
            all,
            "without read-only everything is offered"
        );
    }

    #[test]
    fn the_output_check_follows_the_schema_and_knows_every_keyword_the_tools_use() {
        let schema = json!({"type": "object", "properties": {
            "ok": {"type": "boolean"}, "n": {"type": "integer"}, "s": {"type": "string"},
            "list": {"type": "array", "items": {"type": "string"}}, "o": {"type": "object"}, "any": {}
        }, "required": ["ok"]});
        assert!(conforms(&schema, &json!({"ok": true})).is_ok());
        assert!(conforms(&schema, &json!({"ok": false, "n": 3, "s": "x", "list": ["a"], "o": {}, "any": [1, null], "extra": 1})).is_ok());
        for bad in [
            json!({}),
            json!({"ok": 1}),
            json!({"ok": "true"}),
            json!({"ok": null}),
            json!({"ok": true, "n": 1.5}),
            json!({"ok": true, "n": true}),
            json!({"ok": true, "n": "3"}),
            json!({"ok": true, "s": 3}),
            json!({"ok": true, "list": "a"}),
            json!({"ok": true, "list": [1]}),
            json!({"ok": true, "o": []}),
            json!([]),
            json!("x"),
            json!(null),
        ] {
            assert!(conforms(&schema, &bad).is_err(), "{bad}");
        }
        // every keyword used by an output schema is one the check understands, as for input schemas
        fn keywords(v: &Value, out: &mut Vec<String>) {
            if let Some(o) = v.as_object() {
                for (k, x) in o {
                    if k == "properties" {
                        for p in x.as_object().into_iter().flat_map(|m| m.values()) {
                            keywords(p, out);
                        }
                    } else {
                        out.push(k.clone());
                        if k == "items" {
                            keywords(x, out);
                        }
                    }
                }
            }
        }
        for t in tools().as_array().unwrap() {
            let mut ks = Vec::new();
            keywords(&t["outputSchema"], &mut ks);
            for k in ks {
                assert!(
                    ["type", "properties", "required", "items"].contains(&k.as_str()),
                    "{}: output keyword {k} is not checked",
                    t["name"]
                );
            }
        }
    }
}
