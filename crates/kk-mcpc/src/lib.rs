//! MCP client: runs one owner-approved server program as a child process and calls its tools with bounds.
//!
//! The server is a program the owner chose to run on their own computer. It gets only the variables the owner named and a fixed
//! path, its stderr is kept small and never shown to a model, every call has a time limit, and its descriptions and results come
//! back as cleaned plain text. A server that times out or dies is not used again.

use kk_evidence::capsule::sha256_hex;
use rmcp::model::{CallToolRequestParams, CallToolResponse};
use rmcp::service::{RunningService, ServiceError};
use rmcp::transport::async_rw::AsyncRwTransport;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::runtime::Runtime;

const STDERR_KEEP: usize = 8 * 1024;
const MAX_DESCRIPTION: usize = 1000;
const FIXED_PATH: &str = "/usr/bin:/bin";

#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// A short name for the owner's list: letters, digits, dot, dash, underscore.
    pub name: String,
    /// The program, as an absolute path to a regular file. A bare name is refused so no search path chooses the program.
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The only variables the server gets besides a fixed PATH; the values are already filled in by the caller.
    pub env: Vec<(String, String)>,
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub start_timeout: Duration,
    pub call_timeout: Duration,
    pub max_result_bytes: usize,
    pub max_line_bytes: usize,
    pub max_tools: usize,
    pub max_args_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            start_timeout: Duration::from_secs(20),
            call_timeout: Duration::from_secs(60),
            max_result_bytes: 64 * 1024,
            max_line_bytes: 1024 * 1024,
            max_tools: 100,
            max_args_bytes: 256 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpError {
    BadProgram(String),
    Start(String),
    Timeout,
    Dead,
    TooManyTools,
    UnknownTool,
    Failed(String),
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpError::BadProgram(w) => write!(f, "{w}"),
            McpError::Start(w) => write!(f, "the server did not start: {w}"),
            McpError::Timeout => write!(f, "the server did not answer in time and was stopped"),
            McpError::Dead => write!(f, "the server is not running"),
            McpError::TooManyTools => write!(f, "the server lists more tools than are allowed"),
            McpError::UnknownTool => write!(f, "that tool is not one the server listed"),
            McpError::Failed(w) => write!(f, "{w}"),
        }
    }
}

impl std::error::Error for McpError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInfo {
    pub name: String,
    /// Cleaned and cut for showing.
    pub description: String,
    pub schema: Value,
    /// Hash of the name, the description exactly as the server sent it, and the input schema. A change of any of them changes it.
    pub pin: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub text: String,
    pub is_error: bool,
    pub truncated: bool,
    /// Parts that were not text (images, audio, resources) and were left out.
    pub omitted_parts: usize,
}

pub struct Server {
    client: Option<RunningService<RoleClient, ()>>,
    child: Child,
    stderr: Arc<Mutex<Vec<u8>>>,
    listed: Option<Vec<String>>,
    limits: Limits,
    dead: bool,
    /// Last, so it is dropped after everything that needs it.
    rt: Runtime,
}

fn plain_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn env_name_ok(n: &str) -> bool {
    let mut c = n.chars();
    c.next().is_some_and(|f| f.is_ascii_uppercase() || f == '_')
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn hidden(c: char) -> bool {
    c.is_control() && c != '\n' && c != '\t'
        || matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
}

fn clean(text: &str) -> String {
    text.chars().filter(|c| !hidden(*c)).collect()
}

fn clip(mut text: String, max: usize) -> (String, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push_str("\n[cut: the result was longer]");
    (text, true)
}

fn check_config(c: &ServerConfig) -> Result<(), McpError> {
    let bad = |w: &str| Err(McpError::BadProgram(w.to_string()));
    if !plain_name(&c.name) {
        return bad("the server name must be 1 to 64 letters, digits, dots, dashes or underscores");
    }
    if !c.program.is_absolute() {
        return bad("the program must be given as an absolute path");
    }
    match std::fs::metadata(&c.program) {
        Ok(m) if m.is_file() => {}
        _ => return bad("the program is not a file that exists"),
    }
    if c.args.len() > 32 || c.args.iter().any(|a| a.len() > 4096 || a.contains('\0')) {
        return bad("at most 32 arguments of up to 4096 characters each");
    }
    for (k, v) in &c.env {
        if !env_name_ok(k) || k == "PATH" || v.contains('\0') || v.len() > 8192 {
            return bad("a variable name is capitals, digits and underscores, is not PATH, and its value is plain text");
        }
    }
    if let Some(d) = &c.cwd {
        if !d.is_absolute() || !d.is_dir() {
            return bad("the working folder must be an existing absolute path");
        }
    }
    Ok(())
}

impl Server {
    pub fn start(config: ServerConfig, limits: Limits) -> Result<Server, McpError> {
        check_config(&config)?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|e| McpError::Start(e.to_string()))?;
        let stderr_buf = Arc::new(Mutex::new(Vec::new()));
        let started = rt.block_on(async {
            let mut cmd = Command::new(&config.program);
            cmd.args(&config.args)
                .env_clear()
                .env("PATH", FIXED_PATH)
                .envs(config.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            if let Some(d) = &config.cwd {
                cmd.current_dir(d);
            }
            let mut child = cmd.spawn().map_err(|e| McpError::Start(e.to_string()))?;
            let (Some(stdin), Some(stdout), Some(mut stderr)) =
                (child.stdin.take(), child.stdout.take(), child.stderr.take())
            else {
                return Err(McpError::Start(
                    "the server's pipes could not be opened".into(),
                ));
            };
            let keep = stderr_buf.clone();
            tokio::spawn(async move {
                let mut chunk = [0u8; 4096];
                while let Ok(n) = stderr.read(&mut chunk).await {
                    if n == 0 {
                        break;
                    }
                    let mut b = keep.lock().expect("lock");
                    b.extend_from_slice(&chunk[..n]);
                    if b.len() > STDERR_KEEP {
                        let cut = b.len() - STDERR_KEEP;
                        b.drain(..cut);
                    }
                }
            });
            let transport = AsyncRwTransport::<RoleClient, _, _>::new(stdout, stdin)
                .with_max_line_length(limits.max_line_bytes);
            let client = match tokio::time::timeout(limits.start_timeout, ().serve(transport)).await
            {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => return Err(McpError::Start(e.to_string())),
                Err(_) => return Err(McpError::Start("no answer to the handshake in time".into())),
            };
            Ok((child, client))
        });
        let (child, client) = started?;
        Ok(Server {
            rt,
            client: Some(client),
            child,
            stderr: stderr_buf,
            listed: None,
            limits,
            dead: false,
        })
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }

    /// The last few kilobytes the server wrote to stderr, for the owner's "why did it fail" view. Never for a model.
    pub fn stderr_tail(&self) -> String {
        clean(&String::from_utf8_lossy(&self.stderr.lock().expect("lock")))
    }

    fn die(&mut self) {
        self.dead = true;
        let _ = self.child.start_kill();
    }

    pub fn list_tools(&mut self) -> Result<Vec<ToolInfo>, McpError> {
        if self.dead {
            return Err(McpError::Dead);
        }
        let client = self.client.as_ref().ok_or(McpError::Dead)?;
        let limit = self.limits.call_timeout;
        let got = self
            .rt
            .block_on(async { tokio::time::timeout(limit, client.list_all_tools()).await });
        let tools = match got {
            Err(_) => {
                self.die();
                return Err(McpError::Timeout);
            }
            Ok(Err(e)) => return Err(self.service_error(e)),
            Ok(Ok(t)) => t,
        };
        if tools.len() > self.limits.max_tools {
            return Err(McpError::TooManyTools);
        }
        let mut out = Vec::new();
        for t in tools {
            let name = t.name.to_string();
            if !plain_name(&name) {
                continue;
            }
            let raw = t.description.as_deref().unwrap_or("").to_string();
            let schema = serde_json::to_value(&t.input_schema).unwrap_or(Value::Null);
            let pin = sha256_hex(
                json!({"name": name, "description": raw, "schema": schema})
                    .to_string()
                    .as_bytes(),
            );
            let description = clip(clean(&raw), MAX_DESCRIPTION).0;
            out.push(ToolInfo {
                name,
                description,
                schema,
                pin,
            });
        }
        self.listed = Some(out.iter().map(|t| t.name.clone()).collect());
        Ok(out)
    }

    fn service_error(&mut self, e: ServiceError) -> McpError {
        match e {
            ServiceError::McpError(data) => McpError::Failed(clean(&data.message)),
            other => {
                self.die();
                McpError::Failed(clean(&other.to_string()))
            }
        }
    }

    pub fn call(&mut self, tool: &str, args: Value) -> Result<Outcome, McpError> {
        if self.dead {
            return Err(McpError::Dead);
        }
        if !self
            .listed
            .as_ref()
            .is_some_and(|l| l.iter().any(|n| n == tool))
        {
            return Err(McpError::UnknownTool);
        }
        if args.to_string().len() > self.limits.max_args_bytes {
            return Err(McpError::Failed("the arguments are too large".into()));
        }
        let map = match args {
            Value::Object(m) => m,
            Value::Null => serde_json::Map::new(),
            _ => return Err(McpError::Failed("the arguments must be an object".into())),
        };
        let client = self.client.as_ref().ok_or(McpError::Dead)?;
        let params = CallToolRequestParams::new(tool.to_string()).with_arguments(map);
        let limit = self.limits.call_timeout;
        let got = self
            .rt
            .block_on(async { tokio::time::timeout(limit, client.call_tool_once(params)).await });
        let response = match got {
            Err(_) => {
                self.die();
                return Err(McpError::Timeout);
            }
            Ok(Err(e)) => return Err(self.service_error(e)),
            Ok(Ok(r)) => r,
        };
        let CallToolResponse::Complete(result) = response else {
            self.die();
            return Err(McpError::Failed(
                "the server asked for more than this client offers, and was stopped".into(),
            ));
        };
        let mut parts = Vec::new();
        let mut omitted = 0usize;
        for c in &result.content {
            let v = serde_json::to_value(c).unwrap_or(Value::Null);
            match (v["type"].as_str(), v["text"].as_str()) {
                (Some("text"), Some(t)) => parts.push(clean(t)),
                _ => omitted += 1,
            }
        }
        let (text, truncated) = clip(parts.join("\n"), self.limits.max_result_bytes);
        Ok(Outcome {
            text,
            is_error: result.is_error.unwrap_or(false),
            truncated,
            omitted_parts: omitted,
        })
    }

    /// Ends the server: a polite close, then a kill.
    pub fn stop(&mut self) {
        self.dead = true;
        if let Some(c) = self.client.take() {
            let _ = self
                .rt
                .block_on(async { tokio::time::timeout(Duration::from_secs(3), c.cancel()).await });
        }
        let _ = self.rt.block_on(async {
            let _ = self.child.start_kill();
            tokio::time::timeout(Duration::from_secs(3), self.child.wait()).await
        });
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _inside = self.rt.enter();
        if !self.dead || self.client.is_some() {
            self.stop();
        }
    }
}
