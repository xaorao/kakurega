//! The owner's MCP servers: programs the owner chose to run, the tools they offer, and which of those tools may be called.
//!
//! A server is started only for the time one listing or one call takes and is stopped again, so nothing keeps running.
//! Environment values the owner gives a server are kept in the encrypted store and are never returned by any call.

use super::*;
use kk_mcpc::{Limits, Server, ServerConfig, ToolInfo};
use std::collections::BTreeMap;
use std::path::PathBuf;

const MAX_SERVERS: i64 = 20;

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
    /// The tools the owner allowed, as the coding loop offers and calls them. None when no tool is allowed.
    pub(super) fn mcp_external(self: &Arc<Self>) -> Option<kk_review::code::External<'static>> {
        let rows: Vec<(String, String, String, String)> = {
            let store = self.store.lock().expect("lock");
            let c = store.connection();
            let Ok(mut st) = c.prepare("SELECT server, tool, description, schema FROM mcp_tools WHERE mode = 'allow' ORDER BY server, tool") else {
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
                let out = svc.mcp_call(server, tool, args.clone())?;
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

    fn mcp_config(&self, name: &str) -> Result<ServerConfig, String> {
        let store = self.store.lock().expect("lock");
        let row: Option<(String, String, String)> = store
            .connection()
            .query_row(
                "SELECT program, args, env FROM mcp_servers WHERE name = ?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        let (program, args, env) = row.ok_or("there is no server with that name")?;
        let args: Vec<String> = serde_json::from_str(&args).map_err(|e| e.to_string())?;
        let env: BTreeMap<String, String> =
            serde_json::from_str(&env).map_err(|e| e.to_string())?;
        Ok(ServerConfig {
            name: name.to_string(),
            program: PathBuf::from(program),
            args,
            env: env.into_iter().collect(),
            cwd: None,
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
                "INSERT INTO mcp_tools(server, tool, pin, description, schema, mode) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                kk_vault::rusqlite::params![name, t.name, t.pin, t.description, schema_text(&t.schema), mode],
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
        if mode != "allow" {
            return Err("that tool is not one the owner has allowed".into());
        }
        let config = self.mcp_config(server)?;
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
                .prepare("SELECT tool, description, pin, mode FROM mcp_tools WHERE server = ?1 ORDER BY tool")
                .ok()
                .map(|mut st| {
                    st.query_map([name], |r| Ok(json!({"name": r.get::<_, String>(0)?, "description": r.get::<_, String>(1)?, "pin": r.get::<_, String>(2)?, "mode": r.get::<_, String>(3)?})))
                        .map(|it| it.flatten().collect())
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            json!({"name": name, "program": program, "args": serde_json::from_str::<Value>(args).unwrap_or(json!([])), "env_names": env_names.get(name), "tools": tools})
        })
        .collect();
    Json(json!({"servers": list, "limits": {"servers": MAX_SERVERS}})).into_response()
}

pub(super) async fn mcp_add(axum::Extension(svc): Svc, Json(b): Json<AddBody>) -> Response {
    let config = ServerConfig {
        name: b.name.clone(),
        program: PathBuf::from(&b.program),
        args: b.args.clone(),
        env: b.env.clone().into_iter().collect(),
        cwd: None,
    };
    if let Err(e) = config.check() {
        return refuse(&e.to_string());
    }
    let args_text = b.args.join(" ");
    if !chat_clean(&args_text) || kk_types::injection::hidden_chars(&args_text) {
        return refuse("the arguments contain control or direction-changing characters");
    }
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
            "INSERT INTO mcp_servers(name, program, args, env, created) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(name) DO UPDATE SET program = ?2, args = ?3, env = ?4",
            kk_vault::rusqlite::params![b.name, b.program, args, env, now()],
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
    if b.mode != "off" && b.mode != "allow" {
        return refuse("a tool is either off or allowed");
    }
    let stored: Option<String> = {
        let store = svc.store.lock().expect("lock");
        store
            .connection()
            .query_row(
                "SELECT pin FROM mcp_tools WHERE server = ?1 AND tool = ?2",
                [&b.server, &b.tool],
                |r| r.get(0),
            )
            .ok()
    };
    let Some(pin) = stored else {
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
    match tokio::task::spawn_blocking(move || svc2.mcp_call(&server, &tool, args)).await {
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
