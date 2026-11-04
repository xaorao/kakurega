use crate::cmd::{self, Command};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const DIGEST_ACK_LEN: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Req {
    pub id: u64,
    pub admin: bool,
    pub method: Method,
    pub path: String,
    pub body: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Resp {
    pub id: u64,
    pub result: Result<(u16, Value), String>,
}

#[derive(Debug, Clone)]
pub enum Event {
    Key(KeyEvent),
    Paste(String),
    Resp(Resp),
    Tick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    User,
    Info,
    Ok,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub kind: Kind,
    pub title: Option<String>,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone)]
enum Intent {
    Config,
    Readiness,
    Jobs,
    StartReview,
    Poll(String),
    Findings,
    Release,
    ApproveRequest(String),
    ApproveJob(String),
    ApprovalDetail { job: String, aid: String },
    ApproveSubmit,
    Deny,
    Deliver,
    MemoryList,
    Remember,
    Correct,
    Forget,
    Log,
    Chat,
}

#[derive(Debug, Clone)]
pub struct ApprovalView {
    pub job: String,
    pub aid: String,
    pub digest: String,
    pub lines: Vec<String>,
    pub typed: String,
    pub error: Option<String>,
    pub scroll: u16,
}

#[derive(Debug, Clone)]
pub struct ConfirmView {
    pub prompt: String,
    pub forget_id: String,
}

#[derive(Debug, Clone)]
pub enum Mode {
    Normal,
    Approval(ApprovalView),
    Confirm(ConfirmView),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRow {
    pub id: String,
    pub recipient: String,
    pub state: String,
}

pub struct App {
    pub cards: Vec<Card>,
    pub input: String,
    pub cursor: usize,
    pub history: Vec<String>,
    hist_pos: Option<usize>,
    pub scroll_back: usize,
    pub mode: Mode,
    pub route: String,
    pub ready: Option<bool>,
    pub jobs: Vec<JobRow>,
    pub roots: Vec<String>,
    pub quit: bool,
    next_id: u64,
    pending: BTreeMap<u64, Intent>,
    watching: BTreeMap<String, usize>,
    inflight_polls: BTreeSet<String>,
    pub chat_history: Vec<(String, String)>,
    chat_waiting: bool,
}

pub fn clean(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\n' | '\t' => c,
            c if c.is_control() => '\u{fffd}',
            '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
            | '\u{200b}'..='\u{200f}'
            | '\u{feff}' => '\u{fffd}',
            c => c,
        })
        .collect()
}

fn text_lines(s: &str) -> Vec<String> {
    clean(s)
        .split('\n')
        .map(|l| l.replace('\t', "    "))
        .collect()
}

fn err_text(status: u16, body: &Value) -> String {
    let m = body["error"]["message"]
        .as_str()
        .or_else(|| body["message"].as_str())
        .unwrap_or("the service refused the request");
    format!("{} ({status})", clean(m))
}

impl App {
    pub fn new() -> App {
        App {
            cards: vec![Card {
                kind: Kind::Info,
                title: Some("Kakurega".into()),
                lines: vec!["Type /help for the commands. Everything runs on this computer; nothing leaves until you approve the exact files.".into()],
            }],
            input: String::new(),
            cursor: 0,
            history: Vec::new(),
            hist_pos: None,
            scroll_back: 0,
            mode: Mode::Normal,
            route: String::new(),
            ready: None,
            jobs: Vec::new(),
            roots: Vec::new(),
            quit: false,
            next_id: 1,
            pending: BTreeMap::new(),
            watching: BTreeMap::new(),
            inflight_polls: BTreeSet::new(),
            chat_history: Vec::new(),
            chat_waiting: false,
        }
    }

    fn req(
        &mut self,
        intent: Intent,
        admin: bool,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Req {
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id, intent);
        Req {
            id,
            admin,
            method,
            path: path.to_string(),
            body,
        }
    }

    fn push(&mut self, kind: Kind, title: Option<&str>, lines: Vec<String>) {
        self.cards.push(Card {
            kind,
            title: title.map(str::to_string),
            lines,
        });
        self.scroll_back = 0;
    }

    fn say(&mut self, kind: Kind, line: impl AsRef<str>) {
        self.push(kind, None, text_lines(line.as_ref()));
    }

    pub fn start(&mut self) -> Vec<Req> {
        vec![
            self.req(Intent::Config, false, Method::Get, "/api/config", None),
            self.req(
                Intent::Readiness,
                false,
                Method::Get,
                "/api/readiness",
                None,
            ),
            self.req(Intent::Jobs, false, Method::Get, "/api/jobs", None),
        ]
    }

    pub fn status_line(&self) -> String {
        let ready = match self.ready {
            Some(true) => "ready",
            Some(false) => "setup needed (/status)",
            None => "checking",
        };
        let running = self.watching.len();
        format!(
            "{} · {} · {} review(s){}",
            if self.route.is_empty() {
                "connecting".to_string()
            } else {
                self.route.clone()
            },
            ready,
            self.jobs.len(),
            if running > 0 {
                format!(" · {running} running")
            } else {
                String::new()
            }
        )
    }

    pub fn handle(&mut self, ev: Event) -> Vec<Req> {
        match ev {
            Event::Key(k) => self.on_key(k),
            Event::Paste(t) => {
                if let Mode::Approval(v) = &mut self.mode {
                    let t: String = t.chars().filter(|c| c.is_ascii_hexdigit()).collect();
                    v.typed.push_str(&t);
                    v.typed.truncate(DIGEST_ACK_LEN);
                } else if matches!(self.mode, Mode::Normal) {
                    for c in clean(&t).chars() {
                        self.insert(if c == '\n' || c == '\t' { ' ' } else { c });
                    }
                }
                Vec::new()
            }
            Event::Resp(r) => self.on_resp(r),
            Event::Tick => {
                let ids: Vec<String> = self
                    .watching
                    .keys()
                    .filter(|id| !self.inflight_polls.contains(*id))
                    .cloned()
                    .collect();
                let mut out = Vec::new();
                for id in ids {
                    self.inflight_polls.insert(id.clone());
                    out.push(self.req(
                        Intent::Poll(id.clone()),
                        false,
                        Method::Get,
                        &format!("/api/jobs/{id}"),
                        None,
                    ));
                }
                out
            }
        }
    }

    fn insert(&mut self, c: char) {
        let at = self
            .input
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.input.len());
        self.input.insert(at, c);
        self.cursor += 1;
    }

    fn on_key(&mut self, k: KeyEvent) -> Vec<Req> {
        if k.kind == KeyEventKind::Release {
            return Vec::new();
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match &mut self.mode {
            Mode::Approval(v) => {
                match k.code {
                    KeyCode::Esc => return self.refuse(),
                    KeyCode::Char('c') if ctrl => return self.refuse(),
                    KeyCode::Enter => {
                        let want: String = v.digest.chars().take(DIGEST_ACK_LEN).collect();
                        if v.typed.to_ascii_lowercase() != want {
                            v.error = Some(format!("That is not the first {DIGEST_ACK_LEN} characters of the digest. Nothing was approved."));
                            return Vec::new();
                        }
                        let (aid, ack) = (v.aid.clone(), want);
                        return vec![self.req(
                            Intent::ApproveSubmit,
                            true,
                            Method::Post,
                            &format!("/admin/approvals/{aid}/approve"),
                            Some(json!({"ack": ack})),
                        )];
                    }
                    KeyCode::Backspace => {
                        v.typed.pop();
                        v.error = None;
                    }
                    KeyCode::PageDown | KeyCode::Down => v.scroll = v.scroll.saturating_add(3),
                    KeyCode::PageUp | KeyCode::Up => v.scroll = v.scroll.saturating_sub(3),
                    KeyCode::Char(c)
                        if !ctrl && c.is_ascii_hexdigit() && v.typed.len() < DIGEST_ACK_LEN =>
                    {
                        v.typed.push(c.to_ascii_lowercase());
                        v.error = None;
                    }
                    _ => {}
                }
                Vec::new()
            }
            Mode::Confirm(c) => {
                let id = c.forget_id.clone();
                match k.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') if !ctrl => {
                        self.mode = Mode::Normal;
                        vec![self.req(
                            Intent::Forget,
                            false,
                            Method::Post,
                            &format!("/api/memories/{id}/forget"),
                            Some(json!({"reason": "you asked to forget it in the terminal"})),
                        )]
                    }
                    _ => {
                        self.mode = Mode::Normal;
                        self.say(Kind::Info, "Cancelled. Nothing was forgotten.");
                        Vec::new()
                    }
                }
            }
            Mode::Normal => self.on_key_normal(k),
        }
    }

    fn refuse(&mut self) -> Vec<Req> {
        let Mode::Approval(v) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return Vec::new();
        };
        vec![self.req(
            Intent::Deny,
            true,
            Method::Post,
            &format!("/admin/approvals/{}/deny", v.aid),
            Some(json!({})),
        )]
    }

    fn on_key_normal(&mut self, k: KeyEvent) -> Vec<Req> {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let n = self.input.chars().count();
        match k.code {
            KeyCode::Char('c') | KeyCode::Char('d') if ctrl => {
                if self.input.is_empty() {
                    self.quit = true;
                } else {
                    self.input.clear();
                    self.cursor = 0;
                }
            }
            KeyCode::Char('u') if ctrl => {
                let at = self
                    .input
                    .char_indices()
                    .nth(self.cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(self.input.len());
                self.input.drain(..at);
                self.cursor = 0;
            }
            KeyCode::Char('k') if ctrl => {
                let at = self
                    .input
                    .char_indices()
                    .nth(self.cursor)
                    .map(|(i, _)| i)
                    .unwrap_or(self.input.len());
                self.input.truncate(at);
            }
            KeyCode::Char('w') if ctrl => {
                let chars: Vec<char> = self.input.chars().collect();
                let mut i = self.cursor;
                while i > 0 && chars[i - 1] == ' ' {
                    i -= 1;
                }
                while i > 0 && chars[i - 1] != ' ' {
                    i -= 1;
                }
                let kept: String = chars[..i]
                    .iter()
                    .chain(chars[self.cursor..].iter())
                    .collect();
                self.input = kept;
                self.cursor = i;
            }
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = n,
            KeyCode::Char(c) if !ctrl && !k.modifiers.contains(KeyModifiers::ALT) => self.insert(c),
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    let at = self
                        .input
                        .char_indices()
                        .nth(self.cursor - 1)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.input.remove(at);
                    self.cursor -= 1;
                }
            }
            KeyCode::Delete => {
                if self.cursor < n {
                    let at = self
                        .input
                        .char_indices()
                        .nth(self.cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.input.remove(at);
                }
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(n),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = n,
            KeyCode::Up => {
                if !self.history.is_empty() {
                    let p = match self.hist_pos {
                        None => self.history.len() - 1,
                        Some(p) => p.saturating_sub(1),
                    };
                    self.hist_pos = Some(p);
                    self.input = self.history[p].clone();
                    self.cursor = self.input.chars().count();
                }
            }
            KeyCode::Down => {
                if let Some(p) = self.hist_pos {
                    if p + 1 < self.history.len() {
                        self.hist_pos = Some(p + 1);
                        self.input = self.history[p + 1].clone();
                    } else {
                        self.hist_pos = None;
                        self.input.clear();
                    }
                    self.cursor = self.input.chars().count();
                }
            }
            KeyCode::PageUp => self.scroll_back = self.scroll_back.saturating_add(8),
            KeyCode::PageDown => self.scroll_back = self.scroll_back.saturating_sub(8),
            KeyCode::Tab => {
                if let Some(c) = cmd::complete(&self.input) {
                    self.input = c;
                    self.cursor = self.input.chars().count();
                }
            }
            KeyCode::Esc => {
                self.input.clear();
                self.cursor = 0;
            }
            KeyCode::Enter => {
                let line = std::mem::take(&mut self.input);
                self.cursor = 0;
                self.hist_pos = None;
                if line.trim().is_empty() {
                    return Vec::new();
                }
                if self.history.last().map(String::as_str) != Some(line.as_str()) {
                    self.history.push(line.clone());
                    if self.history.len() > 200 {
                        self.history.remove(0);
                    }
                }
                self.say(Kind::User, clean(&line));
                return self.run_line(&line);
            }
            _ => {}
        }
        Vec::new()
    }

    fn latest(&self, want: &[&str]) -> Option<String> {
        self.jobs
            .iter()
            .rev()
            .find(|j| want.is_empty() || want.contains(&j.state.as_str()))
            .map(|j| j.id.clone())
    }

    fn job_or_latest(
        &mut self,
        given: Option<String>,
        want: &[&str],
        what: &str,
    ) -> Option<String> {
        match given.or_else(|| self.latest(want)) {
            Some(j) => Some(j),
            None => {
                self.say(Kind::Warn, format!("There is no review to {what} yet. Start one with /review FOLDER for RECIPIENT."));
                None
            }
        }
    }

    pub fn run_line(&mut self, line: &str) -> Vec<Req> {
        if !line.trim_start().starts_with('/') {
            return self.chat(line.trim());
        }
        let cmd = match cmd::parse(line) {
            Ok(c) => c,
            Err(m) => {
                self.say(Kind::Warn, m);
                return Vec::new();
            }
        };
        match cmd {
            Command::Help => {
                self.push(
                    Kind::Info,
                    Some("Commands"),
                    vec![
                        "/review FOLDER for RECIPIENT   start a private review".into(),
                        "/reviews                       list reviews".into(),
                        "/open [JOB]                    show a review's progress".into(),
                        "/findings [JOB]  /release [JOB]   what the kernel confirmed, and what would be sent".into(),
                        "/approve [JOB]                 look at the exact files, then approve or refuse".into(),
                        "/deliver [JOB]                 sign and deliver an approved release".into(),
                        "/memory [WORDS]  /remember TEXT  /correct ID TEXT  /forget ID".into(),
                        "/log                           the signed activity log".into(),
                        "/verify FILE                   check a capsule offline".into(),
                        "/clear                         forget this conversation (it is never saved)".into(),
                        "/status  /help  /quit          Tab completes, Up/Down recall, PageUp/PageDown scroll".into(),
                        "Anything else you type is a message to the private assistant on this computer.".into(),
                    ],
                );
                Vec::new()
            }
            Command::Quit => {
                self.quit = true;
                Vec::new()
            }
            Command::Clear => {
                self.chat_history.clear();
                self.say(Kind::Info, "The conversation was cleared. Conversations are never saved.");
                Vec::new()
            }
            Command::Status => vec![self.req(Intent::Readiness, false, Method::Get, "/api/readiness", None)],
            Command::Reviews => vec![self.req(Intent::Jobs, false, Method::Get, "/api/jobs", None)],
            Command::Review { folder, recipient } => {
                let path = if folder.starts_with('/') {
                    folder
                } else {
                    match self.roots.first() {
                        Some(r) => format!("{}/{}", r.trim_end_matches('/'), folder.trim_start_matches("./")),
                        None => folder,
                    }
                };
                vec![self.req(Intent::StartReview, false, Method::Post, "/api/jobs", Some(json!({"repo": path, "recipient": recipient})))]
            }
            Command::Open(j) => {
                let Some(j) = self.job_or_latest(j, &[], "open") else { return Vec::new() };
                vec![self.req(Intent::Poll(j.clone()), false, Method::Get, &format!("/api/jobs/{j}"), None)]
            }
            Command::Findings(j) => {
                let Some(j) = self.job_or_latest(j, &["ready", "nothing_to_release"], "show findings for") else { return Vec::new() };
                vec![self.req(Intent::Findings, false, Method::Get, &format!("/api/jobs/{j}/findings"), None)]
            }
            Command::Release(j) => {
                let Some(j) = self.job_or_latest(j, &["ready"], "show a release for") else { return Vec::new() };
                vec![self.req(Intent::Release, false, Method::Get, &format!("/api/jobs/{j}/release"), None)]
            }
            Command::Approve(j) => {
                let Some(j) = self.job_or_latest(j, &["ready"], "approve") else { return Vec::new() };
                vec![self.req(Intent::ApproveRequest(j.clone()), false, Method::Post, &format!("/api/jobs/{j}/approval"), Some(json!({})))]
            }
            Command::Deliver(j) => {
                let Some(j) = self.job_or_latest(j, &["ready"], "deliver") else { return Vec::new() };
                vec![self.req(Intent::Deliver, false, Method::Post, &format!("/api/jobs/{j}/deliver"), Some(json!({})))]
            }
            Command::Memory(q) => {
                let path = match q {
                    Some(q) => format!("/api/memories?q={}", q.chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect::<String>().replace(' ', "%20")),
                    None => "/api/memories".to_string(),
                };
                vec![self.req(Intent::MemoryList, false, Method::Get, &path, None)]
            }
            Command::Remember(t) => vec![self.req(Intent::Remember, false, Method::Post, "/api/memories", Some(json!({"text": t, "level": "internal", "reason": "you added it in the terminal"})))],
            Command::Correct { id, text } => vec![self.req(Intent::Correct, false, Method::Post, &format!("/api/memories/{id}/correct"), Some(json!({"text": text, "reason": "you corrected it in the terminal"})))],
            Command::Forget(id) => {
                self.mode = Mode::Confirm(ConfirmView { prompt: format!("Forget {id} for good? The text is deleted and will not be saved again. [y/N]"), forget_id: id });
                Vec::new()
            }
            Command::Log => vec![self.req(Intent::Log, false, Method::Get, "/api/log", None)],
            Command::Verify(path) => {
                self.verify_file(&path);
                Vec::new()
            }
        }
    }

    fn chat(&mut self, message: &str) -> Vec<Req> {
        if self.chat_waiting {
            self.say(Kind::Warn, "Still waiting for the last answer.");
            return Vec::new();
        }
        let start = self.chat_history.len().saturating_sub(18);
        let history: Vec<Value> = self.chat_history[start..]
            .iter()
            .map(|(r, c)| json!({"role": r, "content": c}))
            .collect();
        self.chat_history.push(("user".into(), message.to_string()));
        self.chat_waiting = true;
        vec![self.req(
            Intent::Chat,
            false,
            Method::Post,
            "/api/chat",
            Some(json!({"message": message, "history": history})),
        )]
    }

    fn scrub_last_remember(&mut self) {
        if let Some(c) = self.cards.iter_mut().rev().find(|c| {
            c.kind == Kind::User
                && c.lines
                    .first()
                    .is_some_and(|l| l.trim_start().to_ascii_lowercase().starts_with("/remember"))
        }) {
            c.lines =
                vec!["/remember [text hidden: it looked like a secret and was not stored]".into()];
        }
        if let Some(i) = self
            .history
            .iter()
            .rposition(|h| h.trim_start().to_ascii_lowercase().starts_with("/remember"))
        {
            self.history.remove(i);
        }
    }

    fn verify_file(&mut self, path: &str) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.say(Kind::Error, format!("Cannot read {}: {e}", clean(path)));
                return;
            }
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let r = kk_evidence::capsule::verify(
            &bytes,
            &kk_evidence::capsule::VerifyOptions {
                pinned_keys: None,
                now,
                expected_recipient: None,
            },
        );
        if r.valid {
            let mut lines = vec![
                format!(
                    "Recipient: {}",
                    clean(r.recipient.as_deref().unwrap_or("?"))
                ),
                "Established:".into(),
            ];
            lines.extend(r.established.iter().map(|e| format!("  + {}", clean(e))));
            lines.push("Not established:".into());
            lines.extend(
                r.not_established
                    .iter()
                    .map(|e| format!("  - {}", clean(e))),
            );
            lines.push(
                "No signing key was pinned, so the key comes from the capsule itself.".into(),
            );
            self.push(Kind::Ok, Some("VALID capsule"), lines);
        } else {
            self.push(
                Kind::Error,
                Some("INVALID capsule"),
                vec![format!("Reason: {}", r.reason.unwrap_or("unknown"))],
            );
        }
    }

    fn on_resp(&mut self, r: Resp) -> Vec<Req> {
        let Some(intent) = self.pending.remove(&r.id) else {
            return Vec::new();
        };
        let (status, body) = match r.result {
            Ok(x) => x,
            Err(e) => {
                if let Intent::Poll(id) = &intent {
                    self.inflight_polls.remove(id);
                }
                self.say(
                    Kind::Error,
                    format!("Could not reach the service: {}", clean(&e)),
                );
                return Vec::new();
            }
        };
        let ok = status < 300;
        match intent {
            Intent::Config => {
                if ok {
                    let model = body["route"]["model"]
                        .as_str()
                        .or_else(|| body["route"]["id"].as_str())
                        .unwrap_or("model");
                    let simulated = body["route"]["simulated"] == true;
                    self.route = format!(
                        "private · {}{}",
                        clean(model),
                        if simulated { " (test)" } else { "" }
                    );
                    self.roots = body["roots"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default();
                } else {
                    self.say(Kind::Error, err_text(status, &body));
                }
                Vec::new()
            }
            Intent::Readiness => {
                if ok {
                    self.ready = Some(body["ready"] == true);
                    let checks: Vec<String> = body["checks"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .flat_map(|c| {
                                    let mut v = vec![format!(
                                        "{} {}: {}",
                                        if c["ok"] == true { "[ok]  " } else { "[todo]" },
                                        clean(c["label"].as_str().unwrap_or("?")),
                                        clean(c["detail"].as_str().unwrap_or(""))
                                    )];
                                    if let Some(f) = c["fix"].as_str().filter(|f| !f.is_empty()) {
                                        v.push(format!("       fix: {}", clean(f)));
                                    }
                                    v
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let kind = if body["ready"] == true {
                        Kind::Ok
                    } else {
                        Kind::Warn
                    };
                    self.push(
                        kind,
                        Some(if body["ready"] == true {
                            "Ready to review"
                        } else {
                            "Finish setup first"
                        }),
                        checks,
                    );
                }
                Vec::new()
            }
            Intent::Jobs => {
                if ok {
                    self.jobs = body["jobs"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|j| JobRow {
                                    id: clean(j["id"].as_str().unwrap_or("?")),
                                    recipient: clean(j["recipient"].as_str().unwrap_or("?")),
                                    state: clean(j["state"].as_str().unwrap_or("?")),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    self.jobs.sort_by_key(|j| {
                        j.id.trim_start_matches("job-").parse::<u64>().unwrap_or(0)
                    });
                    if self.cards.len() > 1 || !self.jobs.is_empty() {
                        let lines: Vec<String> = self
                            .jobs
                            .iter()
                            .map(|j| {
                                format!(
                                    "{:<8} {:<20} {}",
                                    j.id,
                                    j.recipient,
                                    j.state.replace('_', " ")
                                )
                            })
                            .collect();
                        if lines.is_empty() {
                            self.say(Kind::Info, "No reviews yet.");
                        } else {
                            self.push(Kind::Info, Some("Reviews"), lines);
                        }
                    }
                    let running: Vec<String> = self
                        .jobs
                        .iter()
                        .filter(|j| j.state == "running")
                        .map(|j| j.id.clone())
                        .collect();
                    for id in running {
                        self.watching.entry(id).or_insert(0);
                    }
                } else {
                    self.say(Kind::Error, err_text(status, &body));
                }
                Vec::new()
            }
            Intent::StartReview => {
                if ok {
                    let id = clean(body["id"].as_str().unwrap_or("?"));
                    self.say(
                        Kind::Ok,
                        format!(
                            "Review {id} started. Progress appears here; you can keep working."
                        ),
                    );
                    self.watching.insert(id.clone(), 0);
                    self.jobs.push(JobRow {
                        id,
                        recipient: String::new(),
                        state: "running".into(),
                    });
                } else {
                    self.say(Kind::Error, err_text(status, &body));
                }
                Vec::new()
            }
            Intent::Poll(id) => {
                self.inflight_polls.remove(&id);
                if !ok {
                    self.watching.remove(&id);
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let seen = self.watching.get(&id).copied();
                let events = body["events"].as_array().cloned().unwrap_or_default();
                let from = seen.unwrap_or(0);
                if seen.is_some() {
                    for e in events.iter().skip(from) {
                        let stage = clean(e["stage"].as_str().unwrap_or(""));
                        if stage == "queued" || stage == "finished" {
                            continue;
                        }
                        self.say(
                            Kind::Info,
                            format!(
                                "{id}  {stage}  {}",
                                clean(e["detail"].as_str().unwrap_or(""))
                            ),
                        );
                    }
                    self.watching.insert(id.clone(), events.len());
                }
                let state = clean(body["state"].as_str().unwrap_or("?"));
                if let Some(row) = self.jobs.iter_mut().find(|j| j.id == id) {
                    row.state = state.clone();
                    row.recipient = clean(body["recipient"].as_str().unwrap_or(&row.recipient));
                }
                if seen.is_none() {
                    let lines = vec![
                        format!("State: {}", state.replace('_', " ")),
                        format!(
                            "Recipient: {}",
                            clean(body["recipient"].as_str().unwrap_or("?"))
                        ),
                        format!("Events: {}", events.len()),
                    ];
                    self.push(Kind::Info, Some(&id), lines);
                } else if state != "running" {
                    self.watching.remove(&id);
                    let next = match state.as_str() {
                        "ready" => "Next: /findings, /release, then /approve.".to_string(),
                        "nothing_to_release" => "The kernel confirmed no finding, so nothing can be sent. /findings shows why.".to_string(),
                        _ => format!("It stopped: {}", clean(body["note"].as_str().unwrap_or(""))),
                    };
                    self.push(
                        if state == "ready" {
                            Kind::Ok
                        } else {
                            Kind::Warn
                        },
                        Some(&format!("{id} finished: {}", state.replace('_', " "))),
                        vec![next],
                    );
                }
                Vec::new()
            }
            Intent::Findings => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let list = body["findings"].as_array().cloned().unwrap_or_default();
                if list.is_empty() {
                    self.push(Kind::Info, Some("Findings"), vec!["The agent reported no findings. That is not proof the project is free of defects.".into()]);
                }
                for f in list {
                    let status = f["status"].as_str().unwrap_or("");
                    let label = if status == "reproduced" {
                        "CONFIRMED by the kernel"
                    } else if status == "not_reproduced" {
                        "NOT reproduced"
                    } else {
                        "not confirmed"
                    };
                    let mut lines = vec![
                        format!("{}:{}", clean(f["file"].as_str().unwrap_or("?")), f["line"]),
                        label.to_string(),
                    ];
                    lines.extend(text_lines(f["explanation"].as_str().unwrap_or("")));
                    self.push(
                        if status == "reproduced" {
                            Kind::Ok
                        } else {
                            Kind::Warn
                        },
                        Some(&clean(f["title"].as_str().unwrap_or("finding"))),
                        lines,
                    );
                }
                Vec::new()
            }
            Intent::Release => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                self.push(Kind::Info, Some("Release"), release_lines(&body));
                Vec::new()
            }
            Intent::ApproveRequest(job) => {
                if status == 202 {
                    let aid = clean(body["approval"].as_str().unwrap_or(""));
                    return vec![self.req(
                        Intent::ApprovalDetail {
                            job,
                            aid: aid.clone(),
                        },
                        true,
                        Method::Get,
                        &format!("/admin/approvals/{aid}"),
                        None,
                    )];
                }
                if body["error"]["code"] == "ALREADY_REQUESTED" {
                    return vec![self.req(
                        Intent::ApproveJob(job.clone()),
                        false,
                        Method::Get,
                        &format!("/api/jobs/{job}"),
                        None,
                    )];
                }
                if body["error"]["code"] == "DISCLOSURE_BLOCKED" {
                    let mut lines = vec![clean(body["error"]["message"].as_str().unwrap_or(""))];
                    lines.extend(
                        body["disclosure"]["not_checked"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str())
                                    .map(|x| format!("Not checked: {}", clean(x)))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default(),
                    );
                    self.push(Kind::Error, Some("Blocked: too much of this project would have been shown to this recipient"), lines);
                    return Vec::new();
                }
                self.say(Kind::Error, err_text(status, &body));
                Vec::new()
            }
            Intent::ApproveJob(job) => match body["approval"].as_str() {
                Some(aid) if ok => {
                    let aid = clean(aid);
                    vec![self.req(
                        Intent::ApprovalDetail {
                            job,
                            aid: aid.clone(),
                        },
                        true,
                        Method::Get,
                        &format!("/admin/approvals/{aid}"),
                        None,
                    )]
                }
                _ => {
                    self.say(Kind::Error, "No approval is waiting for that review.");
                    Vec::new()
                }
            },
            Intent::ApprovalDetail { job, aid } => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let digest = clean(body["digest"].as_str().unwrap_or(""));
                if digest.len() < DIGEST_ACK_LEN {
                    self.say(
                        Kind::Error,
                        "The service did not return a digest to approve.",
                    );
                    return Vec::new();
                }
                let mut lines = release_lines(&body);
                lines.push(String::new());
                lines
                    .push("These exact files will be signed and handed over, nothing else.".into());
                self.mode = Mode::Approval(ApprovalView {
                    job,
                    aid,
                    digest,
                    lines,
                    typed: String::new(),
                    error: None,
                    scroll: 0,
                });
                Vec::new()
            }
            Intent::ApproveSubmit => {
                if ok {
                    self.say(
                        Kind::Ok,
                        "Approved. The files are fixed. Run /deliver to sign and send the capsule.",
                    );
                    self.mode = Mode::Normal;
                } else if let Mode::Approval(v) = &mut self.mode {
                    v.error = Some(err_text(status, &body));
                }
                Vec::new()
            }
            Intent::Deny => {
                self.say(
                    Kind::Warn,
                    if ok {
                        "Refused. Nothing was approved and nothing was sent."
                    } else {
                        "The refusal could not be recorded; nothing was approved."
                    },
                );
                Vec::new()
            }
            Intent::Deliver => {
                if ok {
                    self.push(
                        Kind::Ok,
                        Some("Delivered"),
                        vec![
                            format!("Capsule {}", clean(body["name"].as_str().unwrap_or("?"))),
                            format!("Digest {}", clean(body["digest"].as_str().unwrap_or("?"))),
                            "Check it anywhere with: kaku verify FILE, or /verify FILE".into(),
                        ],
                    );
                } else {
                    self.say(Kind::Error, err_text(status, &body));
                }
                Vec::new()
            }
            Intent::MemoryList => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let list = body["memories"].as_array().cloned().unwrap_or_default();
                if list.is_empty() {
                    self.say(
                        Kind::Info,
                        "Nothing remembered yet. /remember TEXT adds something.",
                    );
                }
                let lines: Vec<String> = list
                    .iter()
                    .flat_map(|m| {
                        let mut v = vec![format!(
                            "{}  [{}]{}  {}",
                            clean(m["id"].as_str().unwrap_or("?")),
                            clean(m["level"].as_str().unwrap_or("?")),
                            if m["untrusted"] == true {
                                " untrusted"
                            } else {
                                ""
                            },
                            clean(m["text"].as_str().unwrap_or(""))
                        )];
                        v.push(format!(
                            "    why: {}",
                            clean(m["why"].as_str().unwrap_or(""))
                        ));
                        v
                    })
                    .collect();
                if !lines.is_empty() {
                    self.push(Kind::Info, Some("Memory"), lines);
                }
                Vec::new()
            }
            Intent::Remember => {
                if ok {
                    self.say(
                        Kind::Ok,
                        if body["added"] == true {
                            "Remembered."
                        } else {
                            "Already remembered."
                        },
                    );
                } else {
                    if body["error"]["message"]
                        .as_str()
                        .is_some_and(|m| m.contains("secrets are never stored"))
                    {
                        self.scrub_last_remember();
                    }
                    self.say(Kind::Error, err_text(status, &body));
                }
                Vec::new()
            }
            Intent::Correct => {
                self.say(
                    if ok { Kind::Ok } else { Kind::Error },
                    if ok {
                        "Corrected. The old wording stays in history.".to_string()
                    } else {
                        err_text(status, &body)
                    },
                );
                Vec::new()
            }
            Intent::Forget => {
                self.say(
                    if ok { Kind::Ok } else { Kind::Error },
                    if ok {
                        "Forgotten. The text is deleted and will not be saved again.".to_string()
                    } else {
                        err_text(status, &body)
                    },
                );
                Vec::new()
            }
            Intent::Chat => {
                self.chat_waiting = false;
                if !ok {
                    self.chat_history.pop();
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let reply = body["reply"].as_str().unwrap_or("").to_string();
                self.chat_history.push(("assistant".into(), reply.clone()));
                let mut foot = vec![format!(
                    "{} · {} note(s) used",
                    if body["route"]["simulated"] == true {
                        "test model"
                    } else {
                        "private, on this computer"
                    },
                    body["notes_used"]
                )];
                if body["notes_withheld"].as_u64().unwrap_or(0) > 0 {
                    foot.push(format!("{} note(s) were not given to the model because they look like instructions.", body["notes_withheld"]));
                }
                let mut lines = text_lines(&reply);
                lines.push(String::new());
                lines.extend(foot);
                self.push(Kind::Info, Some("Assistant"), lines);
                Vec::new()
            }
            Intent::Log => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let mut lines = vec![
                    format!(
                        "{} entries; latest fingerprint {}",
                        body["count"],
                        clean(body["head"].as_str().unwrap_or("-"))
                    ),
                    format!(
                        "Signing key {}",
                        clean(body["public_key"].as_str().unwrap_or("?"))
                    ),
                ];
                for e in body["recent"]
                    .as_array()
                    .map(|a| a.iter().take(10).collect::<Vec<_>>())
                    .unwrap_or_default()
                {
                    lines.push(format!(
                        "#{:<4} {:<28} {}",
                        e["seq"],
                        clean(e["kind"].as_str().unwrap_or("?")),
                        clean(e["subject"].as_str().unwrap_or(""))
                    ));
                }
                lines.push("Verify a saved copy anywhere: kaku log verify FILE --pin KEY".into());
                self.push(Kind::Info, Some("Activity log"), lines);
                Vec::new()
            }
        }
    }
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

pub fn release_lines(d: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    if d["available"] == false {
        lines.push(format!(
            "Nothing to release: {}",
            clean(d["reason"].as_str().unwrap_or("no finding was confirmed"))
        ));
        return lines;
    }
    lines.push(format!(
        "Recipient: {}",
        clean(d["recipient"].as_str().unwrap_or("?"))
    ));
    lines.push(format!(
        "Digest:    {}",
        clean(d["digest"].as_str().unwrap_or("?"))
    ));
    lines.push(String::new());
    lines.push("Files that would be sent:".into());
    for f in d["files"].as_array().into_iter().flatten() {
        lines.push(format!(
            "  {:<18} {:>7} bytes  sha256 {}",
            clean(f["name"].as_str().unwrap_or("?")),
            f["bytes"],
            clean(f["sha256"].as_str().unwrap_or(""))
                .chars()
                .take(16)
                .collect::<String>()
        ));
    }
    for f in d["files"].as_array().into_iter().flatten() {
        if let Some(t) = f["text"].as_str() {
            let n = f["name"].as_str().unwrap_or("");
            if n.starts_with("finding-") || n.starts_with("repro-") {
                lines.push(String::new());
                lines.push(format!("----- {} -----", clean(n)));
                lines.extend(text_lines(t.trim_end()));
            }
        }
    }
    if let Some(dis) = d.get("disclosure").filter(|v| v.is_object()) {
        lines.push(String::new());
        lines.push(format!(
            "Cumulative disclosure: {} of {} project files would have been shown to this recipient ({} earlier release(s)).",
            dis["files_after"], dis["project_files"], dis["releases_before"]
        ));
        for r in dis["reasons"].as_array().into_iter().flatten() {
            lines.push(format!("  [!] {}", clean(r.as_str().unwrap_or(""))));
        }
        let nc: Vec<String> = dis["not_checked"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(clean)).collect())
            .unwrap_or_default();
        if !nc.is_empty() {
            lines.push(format!("  Not checked: {}", nc.join(" ")));
        }
    }
    lines.push(String::new());
    lines.push("Withheld and never released:".into());
    let removed: Vec<String> = d["removed"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    format!(
                        "  {} ({})",
                        clean(r["what"].as_str().unwrap_or("")),
                        clean(r["why"].as_str().unwrap_or(""))
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if removed.is_empty() {
        lines.push("  (none)".into());
    }
    lines.extend(removed);
    lines.push(String::new());
    lines.push("This release does not establish:".into());
    for n in d["not_established"].as_array().into_iter().flatten() {
        lines.push(format!("  - {}", clean(n.as_str().unwrap_or(""))));
    }
    lines
}
