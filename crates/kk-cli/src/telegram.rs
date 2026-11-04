use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const MAX_MESSAGE: usize = 1500;

/// Read-only on purpose: the bridge has no way to change anything on the service.
pub trait Admin {
    fn get(&mut self, path: &str) -> Result<Value, String>;
}

pub const FLOOD_LIMIT: usize = 5;
pub const FLOOD_WINDOW: Duration = Duration::from_secs(10);

/// The commands the phone's `/` menu offers; exactly the ones the bridge answers.
pub const MENU: &[(&str, &str)] = &[
    ("status", "Counts of reviews, memories and log entries"),
    ("reviews", "Recent reviews and their states"),
    ("help", "What this bot does and does not do"),
];

pub struct Bridge {
    pub bound_chat: Option<i64>,
    pairing_code: Option<(String, Instant, u8)>,
    seen_states: BTreeMap<String, String>,
    seen_approvals: BTreeMap<String, bool>,
    primed: bool,
    /// When the bound chat's recent commands were handled, newest last, to bound the load a hijacked chat can cause.
    recent: std::collections::VecDeque<Instant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Out {
    pub chat: i64,
    pub text: String,
}

fn plain(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(40)
        .collect()
}

fn state_word(s: &str) -> &'static str {
    match s {
        "running" => "running",
        "ready" => "finished, with a release waiting for your approval",
        "nothing_to_release" => "finished, nothing confirmed so nothing can be sent",
        "failed" => "stopped with an error",
        _ => "in an unknown state",
    }
}

impl Bridge {
    pub fn new(bound_chat: Option<i64>) -> Bridge {
        Bridge {
            bound_chat,
            pairing_code: None,
            seen_states: BTreeMap::new(),
            seen_approvals: BTreeMap::new(),
            primed: false,
            recent: std::collections::VecDeque::new(),
        }
    }

    #[cfg(test)]
    pub fn set_pairing_code(&mut self, code: &str) {
        self.set_pairing_code_aged(code, Duration::ZERO);
    }

    /// `age` is how long ago the code was issued, so a code read from disk keeps its real five-minute window.
    pub fn set_pairing_code_aged(&mut self, code: &str, age: Duration) {
        if self
            .pairing_code
            .as_ref()
            .is_some_and(|(c, _, _)| c == code)
        {
            return;
        }
        let issued = Instant::now().checked_sub(age).unwrap_or_else(Instant::now);
        self.pairing_code = Some((code.to_string(), issued, 0));
    }

    pub fn handle_update(
        &mut self,
        admin: &mut dyn Admin,
        update: &Value,
    ) -> (Vec<Out>, Option<i64>) {
        let Some(msg) = update.get("message") else {
            return (vec![], None);
        };
        let (Some(chat), Some(text)) = (msg["chat"]["id"].as_i64(), msg["text"].as_str()) else {
            return (vec![], None);
        };
        if msg["chat"]["type"].as_str() != Some("private") {
            return (vec![], None);
        }
        let word = text.split_whitespace().next().unwrap_or("");
        let word = word.split('@').next().unwrap_or("").to_ascii_lowercase();
        match self.bound_chat {
            None => {
                if word == "/pair" {
                    let given = text.split_whitespace().nth(1).unwrap_or("").to_string();
                    let Some((code, issued, attempts)) = self.pairing_code.clone() else {
                        return (vec![], None);
                    };
                    let fresh = issued.elapsed() < Duration::from_secs(300);
                    if fresh && !given.is_empty() && code == given {
                        self.pairing_code = None;
                        self.bound_chat = Some(chat);
                        return (vec![Out { chat, text: "Paired. I will tell you when a review finishes or an approval is waiting. I never send file contents. Commands: /status /reviews /help.".into() }], Some(chat));
                    }
                    // While a pairing window is open the owner needs to know why it did not work. Five wrong tries close it.
                    if !fresh || attempts + 1 >= 5 {
                        self.pairing_code = None;
                    } else {
                        self.pairing_code = Some((code, issued, attempts + 1));
                    }
                    return (vec![Out { chat, text: "That pairing code is wrong or has expired. Run `kaku telegram pair` again for a new one.".into() }], None);
                }
                (vec![], None)
            }
            Some(bound) if bound != chat => (vec![], None),
            Some(_) => {
                // at most five commands in any ten seconds; beyond that the bridge stays silent and asks nothing of the service
                let now = Instant::now();
                while self
                    .recent
                    .front()
                    .is_some_and(|t| now.duration_since(*t) > FLOOD_WINDOW)
                {
                    self.recent.pop_front();
                }
                if self.recent.len() >= FLOOD_LIMIT {
                    return (vec![], None);
                }
                self.recent.push_back(now);
                let reply = match word.as_str() {
                    "/status" => match admin.get("/admin/overview") {
                        Ok(v) => {
                            let r = &v["reviews"];
                            let m = v["memories_by_level"].as_object().map(|o| o.values().filter_map(Value::as_u64).sum::<u64>()).unwrap_or(0);
                            format!("Setup: {}. Reviews: {} running, {} ready, {} nothing to release, {} failed. Memories saved: {}. Log entries: {}.", if v["ready"] == true { "ready" } else { "needs attention on your computer" }, r["running"].as_u64().unwrap_or(0), r["ready"].as_u64().unwrap_or(0), r["nothing_to_release"].as_u64().unwrap_or(0), r["failed"].as_u64().unwrap_or(0), m, v["log"]["entries"].as_u64().unwrap_or(0))
                        }
                        Err(_) => "Kakurega is not reachable on your computer right now.".to_string(),
                    },
                    "/reviews" => match admin.get("/admin/jobs") {
                        Ok(v) => {
                            let lines: Vec<String> = v["jobs"].as_array().map(|a| a.iter().take(15).map(|j| format!("{} {}", plain(j["id"].as_str().unwrap_or("?")), plain(j["state"].as_str().unwrap_or("?")).replace('_', " "))).collect()).unwrap_or_default();
                            if lines.is_empty() { "No reviews yet.".to_string() } else { lines.join("\n") }
                        }
                        Err(_) => "Kakurega is not reachable on your computer right now.".to_string(),
                    },
                    "/help" | "/start" => "I tell you when a review finishes or an approval is waiting. /status shows counts, /reviews lists ids and states. I never send file contents, and approvals are only done on your computer.".to_string(),
                    _ => "I only understand /status, /reviews and /help. I cannot approve or send anything.".to_string(),
                };
                (
                    vec![Out {
                        chat,
                        text: reply.chars().take(MAX_MESSAGE).collect(),
                    }],
                    None,
                )
            }
        }
    }

    pub fn poll_notifications(&mut self, admin: &mut dyn Admin) -> Vec<Out> {
        let Some(chat) = self.bound_chat else {
            return vec![];
        };
        let Ok(v) = admin.get("/admin/jobs") else {
            return vec![];
        };
        let mut out = Vec::new();
        for j in v["jobs"].as_array().cloned().unwrap_or_default() {
            let id = plain(j["id"].as_str().unwrap_or(""));
            let state = plain(j["state"].as_str().unwrap_or("")).to_string();
            if id.is_empty() {
                continue;
            }
            let prev = self.seen_states.insert(id.clone(), state.clone());
            if self.primed && prev.as_deref() != Some(state.as_str()) && state != "running" {
                out.push(Out {
                    chat,
                    text: format!(
                        "Review {id} {}. {} confirmed finding(s). Decide on your computer.",
                        state_word(&state),
                        j["findings_confirmed"].as_u64().unwrap_or(0)
                    ),
                });
            }
            let waiting = j["approval"].as_str().is_some() && j["delivered"].is_null();
            let was = self.seen_approvals.insert(id.clone(), waiting);
            if self.primed && waiting && was != Some(true) {
                out.push(Out { chat, text: format!("An approval for {id} is waiting on your computer. Nothing is sent until you approve there.") });
            }
        }
        self.primed = true;
        out
    }
}

pub fn scrub(text: &str, token: &str) -> String {
    if token.is_empty() {
        text.to_string()
    } else {
        text.replace(token, "<token>")
    }
}

/// Problems with a token file that make it unsafe to keep a bot token in. Empty when the file is fine.
pub fn token_file_problems(path: &Path) -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;
    let mut out = Vec::new();
    match std::fs::symlink_metadata(path) {
        Err(_) => out.push("the token file does not exist".to_string()),
        Ok(m) if m.file_type().is_symlink() => {
            out.push("the token file is a symbolic link; use the real file".to_string())
        }
        Ok(m) if !m.is_file() => out.push("the token file is not a regular file".to_string()),
        Ok(m) if m.permissions().mode() & 0o077 != 0 => out.push(format!(
            "the token file is readable by other users (mode {:o}); run chmod 600 on it",
            m.permissions().mode() & 0o777
        )),
        Ok(_) => {}
    }
    out
}

pub fn read_token(file: Option<&Path>) -> Result<String, String> {
    if let Ok(t) = std::env::var("TELEGRAM_BOT_TOKEN") {
        if !t.is_empty() {
            return Ok(t);
        }
    }
    let path = file
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("KAKU_TELEGRAM_ENV").map(PathBuf::from))
        .ok_or("set TELEGRAM_BOT_TOKEN or pass --token-file FILE")?;
    if std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err("the token file is a symbolic link; use the real file".to_string());
    }
    let text =
        std::fs::read_to_string(&path).map_err(|_| "cannot read the token file".to_string())?;
    text.lines()
        .find_map(|l| l.strip_prefix("TELEGRAM_BOT_TOKEN="))
        .map(|t| t.trim().trim_matches('"').to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "the token file has no TELEGRAM_BOT_TOKEN".to_string())
}

struct HttpAdmin(crate::approve::Admin);

impl Admin for HttpAdmin {
    fn get(&mut self, path: &str) -> Result<Value, String> {
        match self.0.get(path) {
            Ok((200, v)) => Ok(v),
            Ok((c, _)) => Err(format!("status {c}")),
            Err(e) => Err(e),
        }
    }
}

fn bound_path(dir: &Path) -> PathBuf {
    dir.join("telegram.json")
}

fn load_bound(dir: &Path) -> Option<i64> {
    std::fs::read_to_string(bound_path(dir))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["chat"].as_i64())
}

pub fn run(args: &[String]) -> u8 {
    let dir = crate::data_dir(args);
    let admin = match crate::approve::connect(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let mut admin = HttpAdmin(admin);
    if args.first().map(String::as_str) == Some("pair") {
        let code: String = {
            let mut b = [0u8; 6];
            getrandom_fill(&mut b);
            b.iter().map(|x| format!("{x:02x}")).collect()
        };
        let _ = std::fs::remove_file(bound_path(&dir));
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let path = dir.join("telegram.pairing");
            let _ = std::fs::remove_file(&path);
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(mut f) => {
                    let _ = f.write_all(code.as_bytes());
                }
                Err(e) => {
                    eprintln!("could not save the pairing code: {e}");
                    return 1;
                }
            }
        }
        println!("Send this to your bot in a private chat within 5 minutes:\n  /pair {code}\nThen run `kaku telegram` to start the bridge. Any earlier pairing was removed.");
        return 0;
    }
    if args.first().map(String::as_str) == Some("status") {
        return match std::fs::read_to_string(dir.join("telegram-status.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        {
            Some(v) => {
                let text = describe_status(&v, unix_now());
                println!("{text}");
                u8::from(text.starts_with("The bridge is not running"))
            }
            None => {
                println!(
                    "The bridge has not run yet (no status file). Start it with: kaku telegram"
                );
                1
            }
        };
    }
    if args.first().map(String::as_str) == Some("doctor") {
        return doctor(args, &dir);
    }
    let token = match read_token(
        crate::flag(args, "--token-file")
            .map(PathBuf::from)
            .as_deref(),
    ) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let api = crate::flag(args, "--api-base").unwrap_or_else(|| "https://api.telegram.org".into());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(40)))
        .build()
        .into();
    let mut bridge = Bridge::new(load_bound(&dir));
    let reload_code = |bridge: &mut Bridge| {
        let path = dir.join("telegram.pairing");
        if let Ok(code) = std::fs::read_to_string(&path) {
            let age = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .unwrap_or(Duration::from_secs(301));
            bridge.set_pairing_code_aged(code.trim(), age);
        }
    };
    reload_code(&mut bridge);
    // the phone's `/` menu lists exactly the commands the bridge answers
    let menu = json!({"commands": MENU.iter().map(|(c, d)| json!({"command": c, "description": d})).collect::<Vec<_>>()});
    if let Err(e) = agent
        .post(&format!("{api}/bot{token}/setMyCommands"))
        .send_json(&menu)
    {
        eprintln!(
            "could not set the command menu: {}",
            scrub(&e.to_string(), &token)
        );
    }
    println!("Telegram bridge running. It sends counts and ids only, never file contents. Ctrl-C stops it.");
    let mut offset: i64 = 0;
    let mut outbox = Outbox::default();
    let mut last_poll = Instant::now() - Duration::from_secs(10);
    let mut status = Status::default();
    let mut status_written = Instant::now() - Duration::from_secs(60);
    loop {
        reload_code(&mut bridge);
        // A re-pair or an unpair removes the binding file: stop notifying the old chat at once.
        if bridge.bound_chat.is_some() && !bound_path(&dir).exists() {
            bridge.bound_chat = None;
            outbox = Outbox::default();
            eprintln!("the chat binding was removed: no further messages will be sent");
        }
        outbox.flush(Instant::now(), &mut |o| {
            let r = send_once(&agent, &api, &token, o);
            match &r {
                SendOutcome::Delivered => status.last_send_ok = Some(unix_now()),
                SendOutcome::RetryAfter(_) => {
                    status.last_error = "Telegram asked the bridge to slow down".into()
                }
                SendOutcome::Retry => {
                    status.last_error = "a message could not be delivered yet".into()
                }
                SendOutcome::Drop(_) => {
                    status.last_error = "a message was refused by Telegram".into()
                }
            }
            r
        });
        let url = format!("{api}/bot{token}/getUpdates?timeout=2&offset={offset}&allowed_updates=%5B%22message%22%5D");
        match agent.get(&url).call() {
            Ok(mut r) if r.status().as_u16() == 200 => {
                status.last_poll_ok = Some(unix_now());
                if status.last_error == "Telegram is not reachable" {
                    status.last_error.clear();
                }
                let v: Value = r.body_mut().read_json().unwrap_or(Value::Null);
                for u in v["result"].as_array().cloned().unwrap_or_default() {
                    offset = offset.max(u["update_id"].as_i64().unwrap_or(0) + 1);
                    let (outs, bound) = bridge.handle_update(&mut admin, &u);
                    if let Some(chat) = bound {
                        let _ = std::fs::write(bound_path(&dir), json!({"chat": chat}).to_string());
                        let _ = std::fs::remove_file(dir.join("telegram.pairing"));
                    }
                    for o in outs {
                        outbox.push(o);
                    }
                }
            }
            Ok(r) if r.status().as_u16() == 401 => {
                eprintln!("Telegram rejected the bot token (401). Check TELEGRAM_BOT_TOKEN; the bridge is stopping.");
                return 1;
            }
            Ok(r) if r.status().as_u16() == 409 => {
                eprintln!("Telegram reports a conflict (409): another bridge is polling this bot, or a webhook is set for it. Stop the other one; trying again in 10 seconds.");
                std::thread::sleep(Duration::from_secs(10));
            }
            Ok(r) => {
                eprintln!("telegram answered {}", r.status());
                std::thread::sleep(Duration::from_secs(3));
            }
            Err(e) => {
                status.last_error = "Telegram is not reachable".into();
                eprintln!(
                    "telegram is not reachable: {}",
                    scrub(&e.to_string(), &token)
                );
                std::thread::sleep(Duration::from_secs(3));
            }
        }
        if status_written.elapsed() >= Duration::from_secs(30) {
            status_written = Instant::now();
            status.updated = unix_now();
            status.paired = bridge.bound_chat.is_some();
            status.queued = outbox.depth();
            write_status(&dir, &status);
        }
        if last_poll.elapsed() >= Duration::from_secs(2) {
            last_poll = Instant::now();
            for o in bridge.poll_notifications(&mut admin) {
                outbox.push(o);
            }
        }
    }
}

/// What the running bridge knows about itself, written to `telegram-status.json` (mode 0600) every thirty seconds. It holds
/// no message text, no chat id and no token: only whether it is alive and what went wrong in plain classes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    pub updated: u64,
    pub paired: bool,
    pub last_poll_ok: Option<u64>,
    pub last_send_ok: Option<u64>,
    pub queued: usize,
    pub last_error: String,
}

pub fn status_json(s: &Status) -> Value {
    json!({"updated": s.updated, "paired": s.paired, "last_poll_ok": s.last_poll_ok, "last_send_ok": s.last_send_ok, "queued": s.queued, "last_error": s.last_error})
}

/// Plain words for the status command: alive, stale, or not running.
pub fn describe_status(v: &Value, now: u64) -> String {
    let updated = v["updated"].as_u64().unwrap_or(0);
    let age = now.saturating_sub(updated);
    if updated == 0 || age > 90 {
        return format!("The bridge is not running (its last sign of life was {age} seconds ago). Start it with: kaku telegram");
    }
    let ago = |k: &str| {
        v[k].as_u64().map_or("never".to_string(), |t| {
            format!("{} s ago", now.saturating_sub(t))
        })
    };
    format!(
        "The bridge is running. Chat paired: {}. Last poll ok: {}. Last message delivered: {}. Waiting to send: {}.{}",
        if v["paired"] == true { "yes" } else { "no" },
        ago("last_poll_ok"),
        ago("last_send_ok"),
        v["queued"].as_u64().unwrap_or(0),
        match v["last_error"].as_str().unwrap_or("") {
            "" => String::new(),
            e => format!(" Last problem: {}.", e.chars().filter(|c| !c.is_control()).take(80).collect::<String>()),
        }
    )
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn write_status(dir: &Path, s: &Status) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = dir.join("telegram-status.json.tmp");
    let _ = std::fs::remove_file(&tmp);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
    {
        if f.write_all(status_json(s).to_string().as_bytes()).is_ok() {
            let _ = std::fs::rename(&tmp, dir.join("telegram-status.json"));
        }
    }
}

/// `kaku telegram doctor`: checks the setup without sending anything to any chat.
fn doctor(args: &[String], dir: &Path) -> u8 {
    let mut bad = 0u8;
    let mut say = |ok: bool, text: String| {
        println!("{} {text}", if ok { "ok   " } else { "FIX  " });
        if !ok {
            bad = 1;
        }
    };
    let file = crate::flag(args, "--token-file")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("KAKU_TELEGRAM_ENV").map(PathBuf::from));
    match &file {
        Some(f) => {
            let p = token_file_problems(f);
            say(p.is_empty(), if p.is_empty() { "token file is a regular file readable only by you".into() } else { p.join("; ") });
        }
        None => say(std::env::var("TELEGRAM_BOT_TOKEN").is_ok_and(|t| !t.is_empty()), "token comes from TELEGRAM_BOT_TOKEN (a token file with mode 600 is safer: it does not appear in the process environment)".into()),
    }
    let token = read_token(file.as_deref());
    say(
        token.is_ok(),
        token
            .clone()
            .err()
            .unwrap_or_else(|| "a token was found".into()),
    );
    if let Ok(token) = token {
        let api =
            crate::flag(args, "--api-base").unwrap_or_else(|| "https://api.telegram.org".into());
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .into();
        match agent.get(&format!("{api}/bot{token}/getMe")).call() {
            Ok(mut r) if r.status().as_u16() == 200 => {
                let v: Value = r.body_mut().read_json().unwrap_or(Value::Null);
                say(
                    v["ok"] == true,
                    format!(
                        "Telegram accepts the token (bot @{})",
                        plain(v["result"]["username"].as_str().unwrap_or("?"))
                    ),
                );
            }
            Ok(r) => say(
                false,
                format!(
                    "Telegram answered {} to getMe: the token is wrong or revoked",
                    r.status()
                ),
            ),
            Err(e) => say(
                false,
                format!(
                    "Telegram is not reachable: {}",
                    scrub(&e.to_string(), &token)
                ),
            ),
        }
        match agent
            .get(&format!("{api}/bot{token}/getWebhookInfo"))
            .call()
        {
            Ok(mut r) if r.status().as_u16() == 200 => {
                let v: Value = r.body_mut().read_json().unwrap_or(Value::Null);
                let url = v["result"]["url"].as_str().unwrap_or("");
                say(
                    url.is_empty(),
                    if url.is_empty() {
                        "no webhook is set, so polling works".into()
                    } else {
                        "a webhook is set for this bot; polling will conflict with it (the bridge calls deleteWebhook only if you ask)".into()
                    },
                );
            }
            _ => say(false, "could not read the webhook state".into()),
        }
    }
    say(
        bound_path(dir).exists(),
        if bound_path(dir).exists() {
            "a chat is paired".into()
        } else {
            "no chat is paired yet: run `kaku telegram pair`".into()
        },
    );
    say(
        !dir.join("telegram.pairing").exists() || !bound_path(dir).exists(),
        "no stale pairing code".into(),
    );
    bad
}

fn getrandom_fill(b: &mut [u8]) {
    let mut f = std::fs::File::open("/dev/urandom").expect("urandom");
    std::io::Read::read_exact(&mut f, b).expect("random bytes");
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendOutcome {
    Delivered,
    /// Telegram asked us to wait (flood control) before trying again.
    RetryAfter(Duration),
    /// A temporary failure: network error or a 5xx answer.
    Retry,
    /// The message will never be accepted (blocked bot, unknown chat, bad request): do not loop on it.
    Drop(String),
}

/// What an answer from `sendMessage` means. Status errors are not raised by the HTTP client, so they must be read here.
pub fn classify_send(status: u16, body: &str) -> SendOutcome {
    match status {
        200 => SendOutcome::Delivered,
        429 => {
            let wait = serde_json::from_str::<Value>(body)
                .ok()
                .and_then(|v| v["parameters"]["retry_after"].as_u64())
                .unwrap_or(5)
                .clamp(1, 60);
            SendOutcome::RetryAfter(Duration::from_secs(wait))
        }
        500..=599 => SendOutcome::Retry,
        s => SendOutcome::Drop(format!("Telegram refused the message ({s})")),
    }
}

fn send_once(agent: &ureq::Agent, api: &str, token: &str, o: &Out) -> SendOutcome {
    let url = format!("{api}/bot{token}/sendMessage");
    match agent
        .post(&url)
        .send_json(json!({"chat_id": o.chat, "text": o.text, "disable_web_page_preview": true}))
    {
        Ok(mut r) => {
            let status = r.status().as_u16();
            let body = r.body_mut().read_to_string().unwrap_or_default();
            classify_send(status, &body)
        }
        Err(e) => {
            eprintln!(
                "could not reach Telegram to send a message: {}",
                scrub(&e.to_string(), token)
            );
            SendOutcome::Retry
        }
    }
}

/// Messages that could not be delivered yet. Bounded, oldest dropped first, and a message is tried at most ten times.
#[derive(Default)]
pub struct Outbox {
    items: std::collections::VecDeque<(Out, u32, Instant)>,
}

impl Outbox {
    pub fn depth(&self) -> usize {
        self.items.len()
    }

    pub const MAX: usize = 50;
    pub const MAX_TRIES: u32 = 10;

    pub fn push(&mut self, o: Out) {
        if self.items.len() >= Self::MAX {
            self.items.pop_front();
        }
        self.items.push_back((o, 0, Instant::now()));
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Tries every message that is due, in order. A flood-control answer pauses everything behind it.
    pub fn flush(&mut self, now: Instant, send: &mut dyn FnMut(&Out) -> SendOutcome) {
        let mut keep = std::collections::VecDeque::new();
        let mut paused = false;
        while let Some((o, tries, due)) = self.items.pop_front() {
            if paused || due > now {
                keep.push_back((o, tries, due));
                continue;
            }
            match send(&o) {
                SendOutcome::Delivered => {}
                SendOutcome::Drop(m) => eprintln!("dropping a message: {m}"),
                SendOutcome::RetryAfter(wait) => {
                    paused = true;
                    keep.push_back((o, tries, now + wait));
                }
                SendOutcome::Retry => {
                    if tries + 1 < Self::MAX_TRIES {
                        let backoff = Duration::from_secs(1u64 << (tries + 1).min(6));
                        keep.push_back((o, tries + 1, now + backoff));
                    } else {
                        eprintln!("giving up on a message after {} tries", Self::MAX_TRIES);
                    }
                }
            }
        }
        self.items = keep;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        jobs: Value,
        overview: Value,
        down: bool,
        asked: Vec<String>,
    }

    impl Admin for Fake {
        fn get(&mut self, path: &str) -> Result<Value, String> {
            self.asked.push(path.to_string());
            if self.down {
                return Err("down".into());
            }
            match path {
                "/admin/jobs" => Ok(self.jobs.clone()),
                "/admin/overview" => Ok(self.overview.clone()),
                _ => Err("no".into()),
            }
        }
    }

    fn fake() -> Fake {
        Fake {
            jobs: json!({"jobs": [{"id": "job-1", "state": "running", "recipient": "Acme Secret Corp", "approval": null, "delivered": null, "findings_confirmed": 0}]}),
            overview: json!({"ready": true, "reviews": {"running": 1, "ready": 2}, "memories_by_level": {"internal": 3}, "log": {"entries": 9}}),
            down: false,
            asked: vec![],
        }
    }

    fn msg(chat: i64, text: &str) -> Value {
        json!({"update_id": 1, "message": {"chat": {"id": chat, "type": "private"}, "text": text}})
    }

    #[test]
    fn tg01_pairing_binds_one_private_chat_with_a_one_time_code_and_strangers_get_silence() {
        let mut f = fake();
        let mut b = Bridge::new(None);
        assert!(
            b.handle_update(&mut f, &msg(111, "/pair 1234"))
                .0
                .is_empty(),
            "no code issued yet"
        );
        b.set_pairing_code("abcd1234");
        assert!(
            b.handle_update(&mut f, &msg(111, "/status")).0.is_empty(),
            "unpaired chats get no answer"
        );
        let wrong = b.handle_update(&mut f, &msg(111, "/pair wrong")).0;
        assert!(
            wrong.len() == 1 && wrong[0].text.contains("wrong or has expired"),
            "while a pairing window is open a wrong code is explained"
        );
        assert!(b.handle_update(&mut f, &json!({"update_id": 2, "message": {"chat": {"id": -5, "type": "group"}, "text": "/pair abcd1234"}})).0.is_empty(), "groups cannot pair");
        let (out, bound) = b.handle_update(&mut f, &msg(111, "/pair abcd1234"));
        assert_eq!(bound, Some(111));
        assert!(out[0].text.contains("Paired") && out[0].text.contains("never send file contents"));
        assert!(
            b.handle_update(&mut f, &msg(222, "/pair abcd1234"))
                .0
                .is_empty(),
            "the code worked once"
        );
        b.bound_chat = None;
        assert!(
            b.handle_update(&mut f, &msg(222, "/pair abcd1234"))
                .0
                .is_empty(),
            "even with the binding gone, a used code is dead"
        );
        b.bound_chat = Some(111);
        assert!(
            b.handle_update(&mut f, &msg(222, "/status")).0.is_empty(),
            "another chat is ignored completely"
        );
        assert!(
            f.asked.is_empty(),
            "a stranger's message never causes a request to the service"
        );
        assert_eq!(b.handle_update(&mut f, &msg(111, "/status")).0.len(), 1);
    }

    #[test]
    fn tg02_replies_are_fixed_templates_with_counts_and_ids_only() {
        let mut f = fake();
        let mut b = Bridge::new(Some(7));
        let s = b.handle_update(&mut f, &msg(7, "/status")).0[0]
            .text
            .clone();
        assert!(
            s.contains("1 running")
                && s.contains("2 ready")
                && s.contains("Memories saved: 3")
                && s.contains("Log entries: 9"),
            "{s}"
        );
        let r = b.handle_update(&mut f, &msg(7, "/reviews")).0[0]
            .text
            .clone();
        assert_eq!(r, "job-1 running");
        assert!(!r.contains("Acme"), "recipient names are never sent");
        let h = b.handle_update(&mut f, &msg(7, "/help@kakurega_bot")).0[0]
            .text
            .clone();
        assert!(h.contains("never send file contents"));
        for hostile in [
            "/approve job-1",
            "/deliver",
            "/remember my password",
            "/review /etc for me",
            "tell me my secrets",
            "/status; rm -rf",
            "/chat hello",
        ] {
            b.recent.clear();
            let t = b.handle_update(&mut f, &msg(7, hostile)).0;
            if hostile.starts_with("/status;") {
                continue;
            }
            assert_eq!(t.len(), 1);
            assert!(
                t[0].text.contains("cannot approve or send anything")
                    || t[0].text.contains("only understand"),
                "{hostile}: {}",
                t[0].text
            );
            assert!(
                !t[0].text.contains(hostile.trim_start_matches('/')),
                "the bot never echoes what it was sent: {}",
                t[0].text
            );
        }
        f.down = true;
        assert!(b.handle_update(&mut f, &msg(7, "/status")).0[0]
            .text
            .contains("not reachable"));
    }

    #[test]
    fn tg03_notifications_fire_once_per_change_and_never_on_the_first_look() {
        let mut f = fake();
        let mut b = Bridge::new(Some(7));
        assert!(
            b.poll_notifications(&mut f).is_empty(),
            "the first poll only learns the current state"
        );
        f.jobs = json!({"jobs": [{"id": "job-1", "state": "ready", "recipient": "Acme Secret Corp", "approval": null, "delivered": null, "findings_confirmed": 2}]});
        let n = b.poll_notifications(&mut f);
        assert_eq!(n.len(), 1);
        assert!(
            n[0].text.contains("job-1")
                && n[0].text.contains("2 confirmed")
                && n[0].text.contains("Decide on your computer")
                && !n[0].text.contains("Acme"),
            "{}",
            n[0].text
        );
        assert!(b.poll_notifications(&mut f).is_empty(), "no repeat");
        f.jobs = json!({"jobs": [{"id": "job-1", "state": "ready", "approval": "appr-1", "delivered": null, "findings_confirmed": 2}]});
        let n = b.poll_notifications(&mut f);
        assert_eq!(n.len(), 1);
        assert!(
            n[0].text
                .contains("approval for job-1 is waiting on your computer")
                && n[0]
                    .text
                    .contains("Nothing is sent until you approve there")
        );
        assert!(b.poll_notifications(&mut f).is_empty());
        f.jobs = json!({"jobs": [{"id": "job-1", "state": "ready", "approval": "appr-1", "delivered": {"name": "x"}, "findings_confirmed": 2}, {"id": "job-2\u{1b}[2J<b>", "state": "failed", "approval": null, "delivered": null}]});
        let n = b.poll_notifications(&mut f);
        assert_eq!(
            n.len(),
            1,
            "a delivered release is not an approval waiting, a new failure is one message"
        );
        assert!(
            !n[0].text.contains('\u{1b}') && !n[0].text.contains('<'),
            "ids are reduced to safe characters: {}",
            n[0].text
        );
        let mut late = Bridge::new(Some(7));
        f.jobs = json!({"jobs": [{"id": "job-9", "state": "ready", "approval": "appr-9", "delivered": null, "findings_confirmed": 1}]});
        assert!(
            late.poll_notifications(&mut f).is_empty(),
            "a bridge that starts after things happened does not replay old news"
        );
        let mut unpaired = Bridge::new(None);
        assert!(
            unpaired.poll_notifications(&mut f).is_empty(),
            "an unpaired bridge sends nothing anywhere"
        );
    }

    #[test]
    fn tg04_the_token_never_appears_in_error_text_and_files_are_read_safely() {
        assert_eq!(
            scrub(
                "error for https://api.telegram.org/botSECRET123/getUpdates",
                "SECRET123"
            ),
            "error for https://api.telegram.org/bot<token>/getUpdates"
        );
        assert_eq!(scrub("nothing", ""), "nothing");
        let dir = std::env::temp_dir().join(format!("kk-tg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.env");
        std::fs::write(&ok, "OTHER=1\nTELEGRAM_BOT_TOKEN=\"abc:123\"\n").unwrap();
        std::env::remove_var("TELEGRAM_BOT_TOKEN");
        assert_eq!(read_token(Some(&ok)).unwrap(), "abc:123");
        std::fs::write(dir.join("empty.env"), "TELEGRAM_BOT_TOKEN=\n").unwrap();
        assert!(read_token(Some(&dir.join("empty.env"))).is_err());
        let missing = read_token(Some(&dir.join("nope.env"))).unwrap_err();
        assert!(
            !missing.contains("nope.env"),
            "errors do not echo paths or contents"
        );
    }

    #[test]
    fn tg05_long_replies_are_cut_and_the_bridge_cannot_write() {
        let mut f = fake();
        f.jobs = json!({"jobs": (0..40).map(|i| json!({"id": format!("job-{i}"), "state": "ready"})).collect::<Vec<_>>()});
        let mut b = Bridge::new(Some(7));
        let r = b.handle_update(&mut f, &msg(7, "/reviews")).0[0]
            .text
            .clone();
        assert!(
            r.chars().count() <= MAX_MESSAGE && r.lines().count() <= 15,
            "{}",
            r.lines().count()
        );
        assert!(
            f.asked
                .iter()
                .all(|p| p.starts_with("/admin/jobs") || p.starts_with("/admin/overview")),
            "{:?}",
            f.asked
        );
    }

    #[test]
    fn tg16_pairing_codes_expire_close_after_five_wrong_tries_and_a_newer_code_replaces_an_older_one(
    ) {
        let mut f = fake();
        let mut b = Bridge::new(None);
        b.set_pairing_code_aged("oldcode1", Duration::from_secs(301));
        let r = b.handle_update(&mut f, &msg(111, "/pair oldcode1")).0;
        assert!(
            r.len() == 1 && r[0].text.contains("expired") && b.bound_chat.is_none(),
            "an expired code never pairs, even when typed right"
        );
        assert!(
            b.handle_update(&mut f, &msg(111, "/pair oldcode1"))
                .0
                .is_empty(),
            "and the dead window is silent again"
        );
        b.set_pairing_code("newcode22");
        for i in 0..4 {
            let r = b
                .handle_update(&mut f, &msg(111, &format!("/pair guess{i}")))
                .0;
            assert_eq!(r.len(), 1);
        }
        let r = b.handle_update(&mut f, &msg(111, "/pair guess4")).0;
        assert_eq!(r.len(), 1);
        assert!(
            b.handle_update(&mut f, &msg(111, "/pair newcode22"))
                .0
                .is_empty(),
            "after five wrong tries even the right code is dead"
        );
        assert!(b.bound_chat.is_none());
        b.set_pairing_code("first111");
        b.set_pairing_code("second22");
        assert!(
            b.handle_update(&mut f, &msg(111, "/pair first111"))
                .1
                .is_none(),
            "the older code is replaced"
        );
        b.set_pairing_code("second22");
        assert_eq!(
            b.handle_update(&mut f, &msg(111, "/pair second22")).1,
            Some(111)
        );
        b.set_pairing_code("same3333");
        b.set_pairing_code_aged("same3333", Duration::from_secs(400));
        assert!(
            b.handle_update(&mut f, &msg(222, "/pair same3333"))
                .1
                .is_none()
                || b.bound_chat == Some(111)
        );
    }

    #[test]
    fn tg17_send_answers_are_read_not_ignored_and_undelivered_messages_wait_in_a_bounded_outbox() {
        assert_eq!(classify_send(200, "{}"), SendOutcome::Delivered);
        assert_eq!(
            classify_send(429, r#"{"ok":false,"parameters":{"retry_after":7}}"#),
            SendOutcome::RetryAfter(Duration::from_secs(7))
        );
        assert_eq!(
            classify_send(429, "junk"),
            SendOutcome::RetryAfter(Duration::from_secs(5))
        );
        assert_eq!(
            classify_send(429, r#"{"parameters":{"retry_after":99999}}"#),
            SendOutcome::RetryAfter(Duration::from_secs(60)),
            "a huge wait is capped"
        );
        for s in [500, 502, 503] {
            assert_eq!(classify_send(s, ""), SendOutcome::Retry);
        }
        for s in [400, 401, 403, 404] {
            assert!(matches!(classify_send(s, ""), SendOutcome::Drop(_)), "{s}");
        }
        let out = |t: &str| Out {
            chat: 1,
            text: t.into(),
        };
        let t0 = Instant::now() + Duration::from_secs(1);
        let mut box_ = Outbox::default();
        box_.push(out("a"));
        box_.push(out("b"));
        let mut sent: Vec<String> = Vec::new();
        // a temporary failure keeps the message and does not lose its order
        box_.flush(t0, &mut |o| {
            if o.text == "a" {
                SendOutcome::Retry
            } else {
                sent.push(o.text.clone());
                SendOutcome::Delivered
            }
        });
        assert_eq!((box_.len(), sent.clone()), (1, vec!["b".to_string()]));
        box_.flush(t0, &mut |_| {
            panic!("not due yet: back-off must hold the message back")
        });
        let mut again = Vec::new();
        box_.flush(t0 + Duration::from_secs(5), &mut |o| {
            again.push(o.text.clone());
            SendOutcome::Delivered
        });
        assert_eq!((again, box_.len()), (vec!["a".to_string()], 0));
        // flood control pauses everything behind it; a refusal drops only that message
        let mut b2 = Outbox::default();
        for t in ["x", "y", "z"] {
            b2.push(out(t));
        }
        let mut seen = Vec::new();
        b2.flush(t0, &mut |o| {
            seen.push(o.text.clone());
            if o.text == "x" {
                SendOutcome::RetryAfter(Duration::from_secs(10))
            } else {
                SendOutcome::Delivered
            }
        });
        assert_eq!(
            (seen, b2.len()),
            (vec!["x".to_string()], 3),
            "nothing behind a flood-control answer is sent"
        );
        let mut b3 = Outbox::default();
        b3.push(out("bad"));
        b3.push(out("ok"));
        let mut got = Vec::new();
        b3.flush(t0, &mut |o| {
            got.push(o.text.clone());
            if o.text == "bad" {
                SendOutcome::Drop("blocked".into())
            } else {
                SendOutcome::Delivered
            }
        });
        assert_eq!(
            (got, b3.len()),
            (vec!["bad".to_string(), "ok".to_string()], 0)
        );
        // bounds
        let mut b4 = Outbox::default();
        for i in 0..80 {
            b4.push(out(&format!("m{i}")));
        }
        assert_eq!(b4.len(), Outbox::MAX);
        let mut tries = 0;
        let mut now = t0;
        while !b4.is_empty() && tries < 40 {
            now += Duration::from_secs(120);
            b4.flush(now, &mut |_| SendOutcome::Retry);
            tries += 1;
        }
        assert!(
            b4.is_empty(),
            "a message is given up on after ten failed tries"
        );
    }

    #[test]
    fn tg20_a_flood_from_the_bound_chat_reaches_the_service_at_most_five_times_in_ten_seconds() {
        let mut b = Bridge::new(Some(7));
        let mut f = fake();
        let mut answered = 0;
        for _ in 0..50 {
            let (outs, _) = b.handle_update(&mut f, &msg(7, "/status"));
            answered += outs.len();
        }
        assert_eq!(answered, FLOOD_LIMIT);
        assert_eq!(
            f.asked.len(),
            FLOOD_LIMIT,
            "the admin API was asked at most five times"
        );
        // after the window the chat is served again
        b.recent
            .iter_mut()
            .for_each(|t| *t = Instant::now() - FLOOD_WINDOW - Duration::from_secs(1));
        let (outs, _) = b.handle_update(&mut f, &msg(7, "/status"));
        assert_eq!(outs.len(), 1);
        // strangers never count against the bound chat
        let mut c = Bridge::new(Some(7));
        for _ in 0..20 {
            c.handle_update(&mut f, &msg(99, "/status"));
        }
        assert!(c.handle_update(&mut f, &msg(7, "/status")).0.len() == 1);
    }

    #[test]
    fn tg21_the_menu_lists_exactly_the_commands_the_bridge_answers() {
        let mut b = Bridge::new(Some(7));
        let mut f = fake();
        for (c, d) in MENU {
            assert!(!d.is_empty() && c.chars().all(|x| x.is_ascii_lowercase()));
            b.recent.clear();
            let (outs, _) = b.handle_update(&mut f, &msg(7, &format!("/{c}")));
            assert_eq!(outs.len(), 1, "/{c} is answered");
            assert!(
                !outs[0].text.contains("cannot"),
                "/{c} is a real command, not the refusal"
            );
        }
        assert_eq!(MENU.len(), 3);
    }

    #[test]
    fn tg22_token_files_must_be_real_regular_files_readable_only_by_the_owner() {
        use std::os::unix::fs::PermissionsExt;
        let d = std::env::temp_dir().join(format!("kk-tgdoctor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let ok = d.join("good.env");
        std::fs::write(&ok, "TELEGRAM_BOT_TOKEN=1:abc\n").unwrap();
        std::fs::set_permissions(&ok, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(token_file_problems(&ok).is_empty());
        std::fs::set_permissions(&ok, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(token_file_problems(&ok)[0].contains("readable by other users"));
        let link = d.join("link.env");
        std::os::unix::fs::symlink(&ok, &link).unwrap();
        assert!(token_file_problems(&link)[0].contains("symbolic link"));
        assert!(
            read_token(Some(&link))
                .unwrap_err()
                .contains("symbolic link"),
            "a linked token file is not read"
        );
        assert!(token_file_problems(&d)[0].contains("not a regular file"));
        assert!(token_file_problems(&d.join("missing"))[0].contains("does not exist"));
    }

    #[test]
    fn tg23_the_status_says_alive_stale_or_never_and_holds_no_chat_text_or_token() {
        let s = Status {
            updated: 1000,
            paired: true,
            last_poll_ok: Some(998),
            last_send_ok: None,
            queued: 2,
            last_error: "Telegram is not reachable".into(),
        };
        let v = status_json(&s);
        let text = describe_status(&v, 1010);
        assert!(
            text.starts_with("The bridge is running.")
                && text.contains("Last poll ok: 12 s ago")
                && text.contains("Last message delivered: never")
                && text.contains("Waiting to send: 2")
                && text.contains("Telegram is not reachable"),
            "{text}"
        );
        assert!(
            describe_status(&v, 1200).starts_with("The bridge is not running"),
            "a status older than 90 seconds means it stopped"
        );
        assert!(describe_status(&json!({}), 5).starts_with("The bridge is not running"));
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "last_error",
                "last_poll_ok",
                "last_send_ok",
                "paired",
                "queued",
                "updated"
            ],
            "nothing else is ever written"
        );
        let mut o = Outbox::default();
        assert_eq!(o.depth(), 0);
        o.push(Out {
            chat: 1,
            text: "x".into(),
        });
        assert_eq!(o.depth(), 1);
    }
}
