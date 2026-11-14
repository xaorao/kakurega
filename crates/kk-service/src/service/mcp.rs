//! The owner's MCP servers: programs the owner chose to run, the tools they offer, and which of those tools may be called.
//!
//! A server is started only for the time one listing or one call takes and is stopped again, so nothing keeps running.
//! Environment values the owner gives a server are kept in the encrypted store and are never returned by any call.

use super::*;
use kk_mcpc::{Limits, Server, ServerConfig, ToolInfo};
use std::collections::BTreeMap;
use std::path::PathBuf;

const MAX_SERVERS: i64 = 20;
const MAX_WAITING: usize = 10;
/// The most characters of a call's arguments the owner is asked to read. A longer call is refused without asking, because an
/// approval of text the owner could not see in full would not be an approval of the call.
const MAX_SHOWN_ARGS: usize = 1500;

/// An outside-tool call that waits for the owner. `decision` is None until the owner answers.
pub(super) struct PendingCall {
    id: String,
    server: String,
    tool: String,
    args: String,
    created: i64,
    decision: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DecideBody {
    id: String,
    approve: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AddBody {
    name: String,
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NameBody {
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ToolBody {
    server: String,
    tool: String,
    mode: String,
    pin: String,
    /// The owner confirms they know a tool that is not read-only may change things.
    #[serde(default)]
    knowing: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TestBody {
    server: String,
    tool: String,
    #[serde(default)]
    args: Value,
}

fn refuse(why: &str) -> Response {
    error(StatusCode::UNPROCESSABLE_ENTITY, "MCP_REFUSED", why)
}

fn limits() -> Limits {
    Limits {
        call_timeout: std::time::Duration::from_secs(30),
        ..Limits::default()
    }
}

/// The input schema as stored for the model: an object of at most 4 KiB, or a bare object schema.
fn schema_text(schema: &Value) -> String {
    let text = schema.to_string();
    if schema.is_object() && text.len() <= 4096 {
        text
    } else {
        "{\"type\":\"object\"}".to_string()
    }
}

impl Service {
    /// How long a call waits for the owner before it counts as refused.
    fn mcp_ask_wait(&self) -> std::time::Duration {
        std::time::Duration::from_secs(if self.test_mode { 3 } else { 120 })
    }

    /// A call a coding task makes: a tool set to ask waits for the owner's answer first; one set to allow does not.
    pub(super) fn mcp_call_for_task(
        &self,
        server: &str,
        tool: &str,
        args: Value,
    ) -> Result<kk_mcpc::Outcome, String> {
        let mode: String = {
            let store = self.store.lock().expect("lock");
            store
                .connection()
                .query_row(
                    "SELECT mode FROM mcp_tools WHERE server = ?1 AND tool = ?2",
                    [server, tool],
                    |r| r.get(0),
                )
                .map_err(|_| "that tool is not one the owner has allowed".to_string())?
        };
        if mode != "ask" {
            return self.mcp_call(server, tool, args, false);
        }
        let shown = kk_mcpc::clean_text(&args.to_string());
        if shown.chars().count() > MAX_SHOWN_ARGS {
            return Err(format!(
                "the arguments are too large to show you in full ({} characters; at most {MAX_SHOWN_ARGS}), so the call was refused without asking",
                shown.chars().count()
            ));
        }
        if !self.mcp_wait_for_owner(server, tool, &shown) {
            return Err("the owner did not approve this call".into());
        }
        self.mcp_call(server, tool, args, true)
    }

    fn mcp_wait_for_owner(&self, server: &str, tool: &str, shown: &str) -> bool {
        let id = format!(
            "mc-{}",
            self.mcp_pending_seq
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                + 1
        );
        {
            let mut g = self.mcp_pending.lock().expect("lock");
            if g.len() >= MAX_WAITING {
                return false;
            }
            let text: String = shown.to_string();
            g.push(PendingCall {
                id: id.clone(),
                server: server.to_string(),
                tool: tool.to_string(),
                args: text,
                created: now(),
                decision: None,
            });
        }
        let until = std::time::Instant::now() + self.mcp_ask_wait();
        let mut answer = false;
        while std::time::Instant::now() < until {
            {
                let g = self.mcp_pending.lock().expect("lock");
                if let Some(p) = g.iter().find(|p| p.id == id) {
                    if let Some(d) = p.decision {
                        answer = d;
                        break;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        self.mcp_pending
            .lock()
            .expect("lock")
            .retain(|p| p.id != id);
        answer
    }

    /// The tools the owner allowed, as the coding loop offers and calls them. None when no tool is allowed.
    pub(super) fn mcp_external(self: &Arc<Self>) -> Option<kk_review::code::External<'static>> {
        let rows: Vec<(String, String, String, String)> = {
            let store = self.store.lock().expect("lock");
            let c = store.connection();
            let Ok(mut st) = c.prepare("SELECT server, tool, description, schema FROM mcp_tools WHERE mode IN ('allow', 'ask') ORDER BY server, tool") else {
                return None;
            };
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .map(|it| it.flatten().collect())
                .unwrap_or_default()
        };
        let mut tools = Vec::new();
        let mut routes: BTreeMap<String, (String, String)> = BTreeMap::new();
        for (server, tool, description, schema) in rows.into_iter().take(40) {
            let name = format!("mcp__{server}__{tool}");
            if name.len() > 64 {
                continue;
            }
            let parameters =
                serde_json::from_str::<Value>(&schema).unwrap_or(json!({"type": "object"}));
            tools.push(kk_types::chat::ToolSpec {
                name: name.clone(),
                description: format!(
                    "From an outside program the owner allowed ({server}): {description}"
                ),
                parameters,
            });
            routes.insert(name, (server, tool));
        }
        if tools.is_empty() {
            return None;
        }
        let svc = self.clone();
        Some(kk_review::code::External {
            tools,
            call: Box::new(move |name: &str, args: &Value| {
                let (server, tool) = routes.get(name).ok_or("that tool is not allowed")?;
                let out = svc.mcp_call_for_task(server, tool, args.clone())?;
                svc.log_event(
                    "mcp.called",
                    &format!("{server}/{tool}"),
                    &[
                        ("bytes", out.text.len().into()),
                        ("by", "coding task".into()),
                    ],
                );
                if out.is_error {
                    Err(format!("the tool reported an error: {}", out.text))
                } else {
                    Ok(out.text)
                }
            }),
            max_calls: 8,
        })
    }

    /// A folder of its own for a server to start in, under the data folder, so it does not start in the service's working folder.
    fn mcp_work_dir(&self, name: &str) -> Option<PathBuf> {
        let dir = self.data_dir.join("mcp-run").join(name);
        std::fs::create_dir_all(&dir).ok()?;
        dir.canonicalize().ok()
    }

    fn mcp_config(&self, name: &str) -> Result<ServerConfig, String> {
        let store = self.store.lock().expect("lock");
        let row: Option<(String, String, String, String)> = store
            .connection()
            .query_row(
                "SELECT program, args, env, sha256 FROM mcp_servers WHERE name = ?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .ok();
        let (program, args, env, sha256) = row.ok_or("there is no server with that name")?;
        let args: Vec<String> = serde_json::from_str(&args).map_err(|e| e.to_string())?;
        let env: BTreeMap<String, String> =
            serde_json::from_str(&env).map_err(|e| e.to_string())?;
        Ok(ServerConfig {
            name: name.to_string(),
            program: PathBuf::from(program),
            args,
            env: env.into_iter().collect(),
            cwd: self.mcp_work_dir(name),
            expected_sha256: (!sha256.is_empty()).then_some(sha256),
        })
    }

    /// Lists the tools of one server and keeps what the owner already chose only for tools whose pin did not change.
    pub(super) fn mcp_discover(&self, name: &str) -> Result<Value, String> {
        let config = self.mcp_config(name)?;
        let mut server = Server::start(config, limits()).map_err(|e| e.to_string())?;
        let tools = server.list_tools().map_err(|e| e.to_string());
        let tail = server.stderr_tail();
        server.stop();
        let tools = tools.map_err(|e| {
            if tail.trim().is_empty() {
                e
            } else {
                format!(
                    "{e} (the server said: {})",
                    tail.chars()
                        .rev()
                        .take(300)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>()
                )
            }
        })?;
        let store = self.store.lock().expect("lock");
        let c = store.connection();
        let old: BTreeMap<String, (String, String)> = {
            let mut st = c
                .prepare("SELECT tool, pin, mode FROM mcp_tools WHERE server = ?1")
                .map_err(|e| e.to_string())?;
            let rows: Vec<(String, String, String)> = st
                .query_map([name], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(|e| e.to_string())?
                .flatten()
                .collect();
            rows.into_iter().map(|(t, p, m)| (t, (p, m))).collect()
        };
        c.execute("DELETE FROM mcp_tools WHERE server = ?1", [name])
            .map_err(|e| e.to_string())?;
        let mut changed = Vec::new();
        for t in &tools {
            let mode = match old.get(&t.name) {
                Some((pin, mode)) if *pin == t.pin => mode.clone(),
                Some(_) => {
                    changed.push(t.name.clone());
                    "off".to_string()
                }
                None => "off".to_string(),
            };
            c.execute(
                "INSERT INTO mcp_tools(server, tool, pin, description, schema, effect, mode) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                kk_vault::rusqlite::params![name, t.name, t.pin, t.description, schema_text(&t.schema), t.effect.as_str(), mode],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(json!({"tools": tools.len(), "changed": changed}))
    }

    /// Calls one tool the owner allowed, with the pin it had when they allowed it. Nothing else can be called.
    pub(super) fn mcp_call(
        &self,
        server: &str,
        tool: &str,
        args: Value,
        approved: bool,
    ) -> Result<kk_mcpc::Outcome, String> {
        let (pin, mode): (String, String) = {
            let store = self.store.lock().expect("lock");
            store
                .connection()
                .query_row(
                    "SELECT pin, mode FROM mcp_tools WHERE server = ?1 AND tool = ?2",
                    [server, tool],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|_| "that tool is not one the owner has allowed".to_string())?
        };
        // `ask` needs an approval of this very call; `allow` does not; anything else is off
        if !(mode == "allow" || (mode == "ask" && approved)) {
            return Err("that tool is not one the owner has allowed".into());
        }
        let config = self.mcp_config(server)?;
        // the program is given these arguments only once the log has recorded the call
        if !self.log_event("mcp.calling", &format!("{server}/{tool}"), &[]) {
            return Err("the signed log could not be written (is the disk full?), so the outside program was not called; free some space and try again".into());
        }
        let mut s = Server::start(config, limits()).map_err(|e| e.to_string())?;
        let listed: Vec<ToolInfo> = s.list_tools().map_err(|e| e.to_string())?;
        // the server may have changed the tool since the owner allowed it: then it is not called, and is put back to waiting
        let still = listed
            .iter()
            .find(|t| t.name == tool)
            .map(|t| t.pin == pin)
            .unwrap_or(false);
        if !still {
            s.stop();
            let store = self.store.lock().expect("lock");
            let _ = store.connection().execute(
                "UPDATE mcp_tools SET mode = 'off' WHERE server = ?1 AND tool = ?2",
                [server, tool],
            );
            return Err(
                "the tool changed since you allowed it; it is off until you read it again".into(),
            );
        }
        let out = s.call(tool, args).map_err(|e| e.to_string());
        s.stop();
        out
    }
}

pub(super) async fn mcp_get(axum::Extension(svc): Svc) -> Response {
    let store = svc.store.lock().expect("lock");
    let c = store.connection();
    let servers: Vec<(String, String, String)> = c
        .prepare("SELECT name, program, args FROM mcp_servers ORDER BY name")
        .ok()
        .map(|mut st| {
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map(|it| it.flatten().collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let env_names: BTreeMap<String, Vec<String>> = {
        let rows: Vec<(String, String)> = c
            .prepare("SELECT name, env FROM mcp_servers")
            .ok()
            .map(|mut st| {
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                    .map(|it| it.flatten().collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        rows.into_iter()
            .map(|(n, e)| {
                (
                    n,
                    serde_json::from_str::<BTreeMap<String, String>>(&e)
                        .map(|m| m.into_keys().collect())
                        .unwrap_or_default(),
                )
            })
            .collect()
    };
    let list: Vec<Value> = servers
        .iter()
        .map(|(name, program, args)| {
            let tools: Vec<Value> = c
                .prepare("SELECT tool, description, pin, mode, effect, schema FROM mcp_tools WHERE server = ?1 ORDER BY tool")
                .ok()
                .map(|mut st| {
                    st.query_map([name], |r| Ok(json!({"name": r.get::<_, String>(0)?, "description": r.get::<_, String>(1)?, "pin": r.get::<_, String>(2)?, "mode": r.get::<_, String>(3)?, "effect": r.get::<_, String>(4)?, "schema": r.get::<_, String>(5)?})))
                        .map(|it| it.flatten().collect())
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            json!({"name": name, "program": program, "args": serde_json::from_str::<Value>(args).unwrap_or(json!([])), "env_names": env_names.get(name), "tools": tools})
        })
        .collect();
    // what packs the owner imported suggest, minus what is already added under that name and what the owner dismissed
    let have: Vec<String> = servers.iter().map(|(n, _, _)| n.to_lowercase()).collect();
    let suggested: Vec<Value> = c
        .prepare("SELECT s.pack, s.name, s.key_id, s.body, p.label FROM pack_suggestions s JOIN pack_publishers p ON p.key_id = s.key_id WHERE s.dismissed = 0 ORDER BY s.pack, p.label, s.key_id, s.name")
        .ok()
        .map(|mut st| {
            st.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
            })
            .map(|it| it.flatten().collect::<Vec<_>>())
            .unwrap_or_default()
        })
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, name, _, _, _)| !have.contains(&name.to_lowercase()))
        .map(|(pack, name, key_id, body, label)| {
            let b: Value = serde_json::from_str(&body).unwrap_or(json!({}));
            json!({"pack": pack, "publisher": key_id, "publisher_label": label, "name": name, "program": b["program"], "args": b["args"], "env": b["env"], "why": b["why"]})
        })
        .collect();
    Json(json!({"servers": list, "suggested": suggested, "limits": {"servers": MAX_SERVERS}}))
        .into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DismissBody {
    pack: String,
    /// The publisher's key: two publishers' packs may share a name.
    publisher: String,
    name: String,
}

/// Puts away a program a pack suggested. It is not shown again until the pack is imported again.
pub(super) async fn suggested_dismiss(
    axum::Extension(svc): Svc,
    Json(b): Json<DismissBody>,
) -> Response {
    let changed = {
        let store = svc.store.lock().expect("lock");
        store
            .connection()
            .execute(
                "UPDATE pack_suggestions SET dismissed = 1 WHERE pack = ?1 AND key_id = ?2 AND name = ?3",
                kk_vault::rusqlite::params![b.pack, b.publisher, b.name],
            )
            .unwrap_or(0)
    };
    if changed != 1 {
        return error(
            StatusCode::NOT_FOUND,
            "NO_SUGGESTION",
            "there is no such suggestion",
        );
    }
    Json(json!({"dismissed": true})).into_response()
}

pub(super) async fn mcp_add(axum::Extension(svc): Svc, Json(b): Json<AddBody>) -> Response {
    let config = ServerConfig {
        name: b.name.clone(),
        program: PathBuf::from(&b.program),
        args: b.args.clone(),
        env: b.env.clone().into_iter().collect(),
        cwd: None,
        expected_sha256: None,
    };
    if let Err(e) = config.check() {
        return refuse(&e.to_string());
    }
    let args_text = b.args.join(" ");
    if !chat_clean(&args_text) || kk_types::injection::hidden_chars(&args_text) {
        return refuse("the arguments contain control or direction-changing characters");
    }
    // arguments are shown on the screen and kept as they are; a key belongs in a setting, which is kept hidden
    if b.args
        .iter()
        .any(|a| !kk_secrets::scan(a).is_empty() || kk_secrets::spoken::holds(a))
    {
        return refuse("an argument looks like a key or a password; put it in a setting (NAME=value) instead, where it is kept hidden and never shown again");
    }
    let program_sha256 = std::fs::read(&b.program)
        .map(|bytes| kk_evidence::capsule::sha256_hex(&bytes))
        .unwrap_or_default();
    let done = {
        let store = svc.store.lock().expect("lock");
        let c = store.connection();
        let known: i64 = c
            .query_row(
                "SELECT count(*) FROM mcp_servers WHERE name = ?1",
                [&b.name],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let all: i64 = c
            .query_row("SELECT count(*) FROM mcp_servers", [], |r| r.get(0))
            .unwrap_or(0);
        if known == 0 && all >= MAX_SERVERS {
            return refuse(&format!("at most {MAX_SERVERS} servers can be saved"));
        }
        let args = serde_json::to_string(&b.args).unwrap_or_else(|_| "[]".into());
        let env = serde_json::to_string(&b.env).unwrap_or_else(|_| "{}".into());
        // a changed program, arguments or environment is a different server: its tools must be read and allowed again
        let _ = c.execute("DELETE FROM mcp_tools WHERE server = ?1", [&b.name]);
        c.execute(
            "INSERT INTO mcp_servers(name, program, args, env, created, sha256) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(name) DO UPDATE SET program = ?2, args = ?3, env = ?4, sha256 = ?6",
            kk_vault::rusqlite::params![b.name, b.program, args, env, now(), program_sha256],
        )
    };
    match done {
        Ok(_) => {
            svc.log_event("mcp.server_saved", &b.name, &[]);
            Json(json!({"name": b.name, "saved": true})).into_response()
        }
        Err(e) => refuse(&e.to_string()),
    }
}

pub(super) async fn mcp_remove(axum::Extension(svc): Svc, Json(b): Json<NameBody>) -> Response {
    let n = {
        let store = svc.store.lock().expect("lock");
        let c = store.connection();
        let _ = c.execute("DELETE FROM mcp_tools WHERE server = ?1", [&b.name]);
        c.execute("DELETE FROM mcp_servers WHERE name = ?1", [&b.name])
            .unwrap_or(0)
    };
    svc.log_event("mcp.server_removed", &b.name, &[]);
    Json(json!({"name": b.name, "removed": n > 0})).into_response()
}

pub(super) async fn mcp_discover_route(
    axum::Extension(svc): Svc,
    Json(b): Json<NameBody>,
) -> Response {
    let svc2 = svc.clone();
    let name = b.name.clone();
    match tokio::task::spawn_blocking(move || svc2.mcp_discover(&name)).await {
        Ok(Ok(v)) => {
            svc.log_event("mcp.discovered", &b.name, &[]);
            Json(v).into_response()
        }
        Ok(Err(e)) => refuse(&e),
        Err(_) => refuse("the check could not run"),
    }
}

/// Sets one tool off or allowed. Allowing needs the pin the owner read, so a tool that changed in between stays off.
pub(super) async fn mcp_tool_set(axum::Extension(svc): Svc, Json(b): Json<ToolBody>) -> Response {
    if b.mode != "off" && b.mode != "allow" && b.mode != "ask" {
        return refuse("a tool is off, allowed, or asks you each time");
    }
    let stored: Option<(String, String)> = {
        let store = svc.store.lock().expect("lock");
        store
            .connection()
            .query_row(
                "SELECT pin, effect FROM mcp_tools WHERE server = ?1 AND tool = ?2",
                [&b.server, &b.tool],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    };
    let Some((pin, effect)) = stored else {
        return error(
            StatusCode::NOT_FOUND,
            "MCP_TOOL_NOT_FOUND",
            "that tool has not been listed; check the server first",
        );
    };
    if pin != b.pin {
        return error(
            StatusCode::CONFLICT,
            "MCP_TOOL_CHANGED",
            "the tool changed after you read it; read it again before allowing it",
        );
    }
    if b.mode != "off" && effect != "read_only" && !b.knowing {
        return error(
            StatusCode::CONFLICT,
            "MCP_MAY_CHANGE",
            "the server does not say this tool only reads; allowing it lets a coding task change things through it. Confirm that you know.",
        );
    }
    {
        let store = svc.store.lock().expect("lock");
        let _ = store.connection().execute(
            "UPDATE mcp_tools SET mode = ?3 WHERE server = ?1 AND tool = ?2",
            [&b.server, &b.tool, &b.mode],
        );
    }
    svc.log_event(
        "mcp.tool_mode",
        &format!("{}/{}", b.server, b.tool),
        &[("mode", b.mode.clone().into())],
    );
    Json(json!({"server": b.server, "tool": b.tool, "mode": b.mode})).into_response()
}

/// The owner's own test of an allowed tool: what it answers, as the model would see it.
pub(super) async fn mcp_test(axum::Extension(svc): Svc, Json(b): Json<TestBody>) -> Response {
    let svc2 = svc.clone();
    let (server, tool, args) = (b.server.clone(), b.tool.clone(), b.args.clone());
    match tokio::task::spawn_blocking(move || svc2.mcp_call(&server, &tool, args, true)).await {
        Ok(Ok(out)) => {
            svc.log_event(
                "mcp.called",
                &format!("{}/{}", b.server, b.tool),
                &[("bytes", out.text.len().into())],
            );
            Json(json!({"text": out.text, "is_error": out.is_error, "truncated": out.truncated, "omitted_parts": out.omitted_parts})).into_response()
        }
        Ok(Err(e)) => refuse(&e),
        Err(_) => refuse("the call could not run"),
    }
}

pub(super) async fn mcp_pending_get(axum::Extension(svc): Svc) -> Response {
    let g = svc.mcp_pending.lock().expect("lock");
    let t = now();
    Json(
        json!({"pending": g.iter().filter(|p| p.decision.is_none()).map(|p| json!({
        "id": p.id, "server": p.server, "tool": p.tool, "args": p.args, "age_secs": t - p.created
    })).collect::<Vec<_>>()}),
    )
    .into_response()
}

/// The owner's yes or no for one waiting call. A decision is used once.
pub(super) async fn mcp_pending_decide(
    axum::Extension(svc): Svc,
    Json(b): Json<DecideBody>,
) -> Response {
    let found = {
        let mut g = svc.mcp_pending.lock().expect("lock");
        match g.iter_mut().find(|p| p.id == b.id && p.decision.is_none()) {
            Some(p) => {
                p.decision = Some(b.approve);
                Some((p.server.clone(), p.tool.clone()))
            }
            None => None,
        }
    };
    match found {
        Some((server, tool)) => {
            svc.log_event(
                if b.approve {
                    "mcp.call_approved"
                } else {
                    "mcp.call_denied"
                },
                &format!("{server}/{tool}"),
                &[],
            );
            Json(json!({"id": b.id, "approved": b.approve})).into_response()
        }
        None => error(
            StatusCode::NOT_FOUND,
            "MCP_NOTHING_WAITING",
            "no call is waiting with that id",
        ),
    }
}
