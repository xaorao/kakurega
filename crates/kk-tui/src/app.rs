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
    /// The response is a stream of JSON lines (`delta` pieces, then a final object) rather than one document.
    pub stream: bool,
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
    /// A piece of a streamed answer for request `id`.
    Delta {
        id: u64,
        text: String,
    },
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
    HistoryLoad,
    HistorySave,
    HistoryClear,
    Proof,
    GatewayProof,
    Keep,
    Sessions,
    OpenChat(String),
    ForgetChat(String),
    Readiness,
    Jobs,
    StartReview,
    Poll(String),
    Findings,
    Fixes(String),
    ApplyFix(String),
    UndoFix(String),
    Release,
    ApproveRequest(String),
    ApproveJob(String),
    ApprovalDetail { job: String, aid: String },
    ApproveSubmit,
    Deny,
    Deliver,
    MemoryList,
    Remember,
    Ask,
    Correct,
    Forget,
    Log,
    Chat,
    SearchPropose,
    SearchRun,
    ReadPropose,
    ReadFetch,
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
    pub statement: Value,
    pub requires_proof: bool,
}

/// Ticks (about 0.7 s each) the first Esc stays armed: roughly four seconds.
pub const ESC_ARM_TICKS: u8 = 6;

#[derive(Debug, Clone)]
pub struct ConfirmView {
    pub prompt: String,
    pub forget_id: String,
    pub apply: Option<(String, usize, String)>,
    pub undo: bool,
    /// A web request waiting for a yes: ("search" or "read", request id, digest of exactly what was shown).
    pub web: Option<(String, String, String)>,
}

#[derive(Debug, Clone)]
pub enum Mode {
    Normal,
    Picker(crate::picker::Picker),
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
    pub session: Option<String>,
    /// Long pastes shown as a short placeholder in the composer and expanded when a chat message is sent.
    pub pasted: Vec<(String, String)>,
    /// Messages typed while an answer is still arriving; each is sent after the previous answer completes.
    pub queued: Vec<String>,
    /// Lines put aside with Ctrl-S or /stash, newest last. Kept in memory only and gone when the terminal closes.
    pub stashed: Vec<String>,
    /// Set by Ctrl-G: the text to open in the owner's editor. The runtime takes it, runs the editor and calls `finish_edit`.
    pub editor_request: Option<String>,
    /// Set by the first Esc while an answer is arriving; the second Esc within a few seconds stops it. Counts down on ticks.
    pub esc_armed: u8,
    pub popup_sel: usize,
    popup_dismissed: Option<String>,
    history_to_save: Vec<String>,
    history_loaded: bool,
    /// Set by Esc while an answer is streaming; the network thread then closes the connection.
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    live: Option<(usize, String)>,
    pub chat_waiting: bool,
    pub approver: Option<kk_evidence::approval::ApproverKey>,
    pub fixes_seen: Vec<(String, usize, String, bool)>,
    /// Addresses suggested by the last search, numbered from 1 for /read.
    pub search_hits: Vec<String>,
    /// Pages the owner approved in this conversation: (page id, address). They go to the model as untrusted sources.
    pub web_pages: Vec<(String, String)>,
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
            session: None,
            pasted: Vec::new(),
            queued: Vec::new(),
            stashed: Vec::new(),
            popup_sel: 0,
            popup_dismissed: None,
            history_to_save: Vec::new(),
            history_loaded: false,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            live: None,
            chat_waiting: false,
            approver: None,
            editor_request: None,
            esc_armed: 0,
            fixes_seen: Vec::new(),
            search_hits: Vec::new(),
            web_pages: Vec::new(),
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
            stream: false,
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

    /// Requests that run in the background and are never shown: loading the saved command history once, and saving each
    /// line the owner types. The event loop calls this after every event.
    pub fn drain_background(&mut self) -> Vec<Req> {
        let mut out = Vec::new();
        if !self.history_loaded {
            self.history_loaded = true;
            out.push(self.req(
                Intent::HistoryLoad,
                false,
                Method::Get,
                "/api/history",
                None,
            ));
        }
        for line in std::mem::take(&mut self.history_to_save) {
            out.push(self.req(
                Intent::HistorySave,
                false,
                Method::Post,
                "/api/history",
                Some(json!({"text": line})),
            ));
        }
        out
    }

    /// The command names to offer while the first word of the line is being typed.
    pub fn completions(&self) -> Vec<&'static str> {
        if !matches!(self.mode, Mode::Normal)
            || !self.input.starts_with('/')
            || self.input.contains(char::is_whitespace)
            || self.popup_dismissed.as_deref() == Some(self.input.as_str())
        {
            return Vec::new();
        }
        let list = cmd::matches(&self.input);
        // a command typed out in full needs no help, and Up/Down then keep walking the history
        if list.len() == 1 && list[0] == self.input[1..].to_ascii_lowercase() {
            return Vec::new();
        }
        list
    }

    fn popup_key(&mut self, k: KeyEvent) -> Option<Vec<Req>> {
        let list = self.completions();
        if list.is_empty() || k.modifiers.contains(KeyModifiers::CONTROL) {
            return None;
        }
        let n = list.len();
        let sel = self.popup_sel.min(n - 1);
        match k.code {
            KeyCode::Down => {
                self.popup_sel = (sel + 1) % n;
                Some(Vec::new())
            }
            KeyCode::Up => {
                self.popup_sel = (sel + n - 1) % n;
                Some(Vec::new())
            }
            KeyCode::Esc => {
                self.popup_dismissed = Some(self.input.clone());
                Some(Vec::new())
            }
            KeyCode::Tab => {
                self.input = format!("/{} ", list[sel]);
                self.cursor = self.input.chars().count();
                self.popup_sel = 0;
                Some(Vec::new())
            }
            KeyCode::Enter
                if cmd::NAMES.contains(&self.input[1..].to_ascii_lowercase().as_str()) =>
            {
                None
            }
            KeyCode::Enter => {
                let name = list[sel];
                self.popup_sel = 0;
                if cmd::takes_arguments(name) {
                    self.input = format!("/{name} ");
                    self.cursor = self.input.chars().count();
                    Some(Vec::new())
                } else {
                    self.input = format!("/{name}");
                    self.cursor = self.input.chars().count();
                    None
                }
            }
            _ => None,
        }
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
                    self.paste(&t);
                }
                Vec::new()
            }
            Event::Resp(r) => self.on_resp(r),
            Event::Delta { id, text } => {
                if matches!(self.pending.get(&id), Some(Intent::Chat)) {
                    let text = clean(&text);
                    match &mut self.live {
                        Some((i, acc)) => {
                            acc.push_str(&text);
                            let lines = text_lines(acc);
                            if let Some(c) = self.cards.get_mut(*i) {
                                c.lines = lines;
                            }
                        }
                        None => {
                            self.push(Kind::Info, Some("Assistant"), text_lines(&text));
                            self.live = Some((self.cards.len() - 1, text));
                        }
                    }
                    self.scroll_back = 0;
                }
                Vec::new()
            }
            Event::Tick => {
                self.esc_armed = self.esc_armed.saturating_sub(1);
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

    fn picker_key(&mut self, k: KeyEvent) -> Vec<Req> {
        use crate::picker::{Delete, ItemKind};
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let Mode::Picker(p) = &mut self.mode else {
            return Vec::new();
        };
        match k.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Char('c') if ctrl => self.mode = Mode::Normal,
            KeyCode::Down => p.move_by(1),
            KeyCode::Up => p.move_by(-1),
            KeyCode::PageDown => p.move_by(8),
            KeyCode::PageUp => p.move_by(-8),
            KeyCode::Backspace => p.backspace(),
            KeyCode::Char('d') if ctrl => match p.delete_press() {
                Delete::Armed(_) => {
                    p.error =
                        Some("Press Ctrl-D again to forget this conversation for good.".into())
                }
                Delete::Confirmed(id) => {
                    p.error = None;
                    return vec![self.req(
                        Intent::ForgetChat(id.clone()),
                        false,
                        Method::Post,
                        &format!("/api/sessions/{id}/forget"),
                        Some(json!({})),
                    )];
                }
                Delete::NotAllowed => p.error = Some("Reviews are not removed here.".into()),
                Delete::Nothing => {}
            },
            KeyCode::Enter => {
                let Some(it) = p.current().cloned() else {
                    return Vec::new();
                };
                self.mode = Mode::Normal;
                return match it.kind {
                    ItemKind::Chat => vec![self.req(
                        Intent::OpenChat(it.id.clone()),
                        false,
                        Method::Get,
                        &format!("/api/sessions/{}", it.id),
                        None,
                    )],
                    ItemKind::Review => vec![self.req(
                        Intent::Poll(it.id.clone()),
                        false,
                        Method::Get,
                        &format!("/api/jobs/{}", it.id),
                        None,
                    )],
                };
            }
            KeyCode::Char(c) if !ctrl => p.type_char(c),
            _ => {}
        }
        Vec::new()
    }

    /// A paste never runs anything. Into a command line it is flattened to one line; into a message, a long paste
    /// becomes a short placeholder that is expanded only when the message is sent as chat.
    fn paste(&mut self, raw: &str) {
        let text = clean(raw).replace("\r\n", "\n").replace('\r', "\n");
        let text = text.trim_matches('\n');
        if self.input.trim_start().starts_with('/') {
            for c in text.chars() {
                self.insert(if c == '\n' || c == '\t' { ' ' } else { c });
            }
            return;
        }
        let lines = text.matches('\n').count() + 1;
        if lines >= 3 || text.chars().count() > 150 {
            let base = format!("[Pasted ~{lines} lines]");
            let mut label = base.clone();
            let mut k = 2;
            while self.pasted.iter().any(|(l, _)| *l == label) {
                label = format!("[Pasted ~{lines} lines #{k}]");
                k += 1;
            }
            self.pasted.push((label.clone(), text.to_string()));
            if self.pasted.len() > 20 {
                self.pasted.remove(0);
            }
            for c in label.chars() {
                self.insert(c);
            }
            return;
        }
        for c in text.chars() {
            self.insert(if c == '\t' { ' ' } else { c });
        }
    }

    /// Takes back what the editor returned. Control characters are removed like in any paste; a failure or an unchanged
    /// result leaves the composer as it was.
    pub fn finish_edit(&mut self, result: Result<String, String>) {
        match result {
            Ok(text) => {
                let text = clean(&text.replace("\r\n", "\n"))
                    .trim_end_matches('\n')
                    .to_string();
                self.input = text;
                self.cursor = self.input.chars().count();
                self.pasted.clear();
            }
            Err(e) => self.say(Kind::Warn, e),
        }
    }

    /// Replaces each placeholder still present in a chat message by what was pasted.
    pub fn expand_pasted(&self, line: &str) -> String {
        let mut out = line.to_string();
        for (label, text) in &self.pasted {
            if let Some(at) = out.find(label.as_str()) {
                out.replace_range(at..at + label.len(), text);
            }
        }
        out
    }

    /// Moves the cursor one line up or down inside a multi-line message. False when there is no such line, so the
    /// key falls through to history.
    fn move_line(&mut self, delta: i64) -> bool {
        if !self.input.contains('\n') {
            return false;
        }
        let chars: Vec<char> = self.input.chars().collect();
        let before = &chars[..self.cursor.min(chars.len())];
        let line_start = before.iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1);
        let col = self.cursor - line_start;
        if delta < 0 {
            if line_start == 0 {
                return false;
            }
            let prev_end = line_start - 1;
            let prev_start = chars[..prev_end]
                .iter()
                .rposition(|c| *c == '\n')
                .map_or(0, |i| i + 1);
            self.cursor = prev_start + col.min(prev_end - prev_start);
        } else {
            let Some(nl) = chars[self.cursor.min(chars.len())..]
                .iter()
                .position(|c| *c == '\n')
            else {
                return false;
            };
            let next_start = self.cursor + nl + 1;
            let next_end = chars[next_start..]
                .iter()
                .position(|c| *c == '\n')
                .map_or(chars.len(), |i| next_start + i);
            self.cursor = next_start + col.min(next_end - next_start);
        }
        true
    }

    /// Puts the composer's text aside (at most ten) and clears it. Placeholders for folded pastes stay valid.
    fn stash_input(&mut self) -> bool {
        if self.input.trim().is_empty() {
            return false;
        }
        if self.stashed.len() >= 10 {
            self.stashed.remove(0);
        }
        self.stashed.push(std::mem::take(&mut self.input));
        self.cursor = 0;
        self.say(
            Kind::Info,
            format!(
                "Stashed ({} aside). /pop or Ctrl-R brings the latest back.",
                self.stashed.len()
            ),
        );
        true
    }

    fn pop_stash(&mut self) {
        match self.stashed.pop() {
            None => self.say(Kind::Info, "Nothing is stashed."),
            Some(text) => {
                // what was typed since goes back on the stash rather than being lost
                if !self.input.trim().is_empty() {
                    self.stashed.push(std::mem::take(&mut self.input));
                }
                self.input = text;
                self.cursor = self.input.chars().count();
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
        if matches!(self.mode, Mode::Picker(_)) {
            return self.picker_key(k);
        }
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
                        let mut body = json!({"ack": ack});
                        if v.requires_proof {
                            let st = &v.statement;
                            let statement = kk_evidence::approval::Statement {
                                approval: st["approval"].as_str().unwrap_or("").into(),
                                job: st["job"].as_str().unwrap_or("").into(),
                                digest: st["digest"].as_str().unwrap_or("").into(),
                                recipient: st["recipient"].as_str().unwrap_or("").into(),
                                policy: st["policy"].as_str().unwrap_or("").into(),
                                expires: st["expires"].as_i64().unwrap_or(0),
                                nonce: st["nonce"].as_str().unwrap_or("").into(),
                            };
                            match &self.approver {
                                Some(signer) => match signer.sign_statement(&statement) {
                                    Ok(p) => body["proof"] = json!(p),
                                    Err(e) => {
                                        v.error = Some(format!("The approval was not signed ({e}). Nothing was approved."));
                                        return Vec::new();
                                    }
                                },
                                None => {
                                    v.error = Some("This service requires a signed approval and this terminal has no approver key. Run `kaku approver enroll` first. Nothing was approved.".into());
                                    return Vec::new();
                                }
                            }
                        }
                        return vec![self.req(
                            Intent::ApproveSubmit,
                            true,
                            Method::Post,
                            &format!("/admin/approvals/{aid}/approve"),
                            Some(body),
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
                let apply = c.apply.clone();
                let undo = c.undo;
                let web = c.web.clone();
                match k.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') if !ctrl && web.is_some() => {
                        self.mode = Mode::Normal;
                        let (kind, rid, digest) = web.expect("checked");
                        let (intent, path) = if kind == "search" {
                            (Intent::SearchRun, format!("/api/search/{rid}/run"))
                        } else {
                            (Intent::ReadFetch, format!("/api/research/{rid}/fetch"))
                        };
                        vec![self.req(
                            intent,
                            false,
                            Method::Post,
                            &path,
                            Some(json!({"approve": true, "digest": digest})),
                        )]
                    }
                    _ if web.is_some() => {
                        self.mode = Mode::Normal;
                        self.say(Kind::Info, "Cancelled. Nothing was sent.");
                        Vec::new()
                    }
                    KeyCode::Char('y') | KeyCode::Char('Y') if !ctrl && apply.is_some() => {
                        self.mode = Mode::Normal;
                        let (job, n, file) = apply.expect("checked");
                        let what = if undo { "undo" } else { "apply" };
                        vec![self.req(
                            if undo {
                                Intent::UndoFix(file.clone())
                            } else {
                                Intent::ApplyFix(file.clone())
                            },
                            false,
                            Method::Post,
                            &format!("/api/jobs/{job}/fixes/{}/{what}", n - 1),
                            Some(json!({"file": file})),
                        )]
                    }
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
                        self.say(
                            Kind::Info,
                            if apply.is_some() {
                                "Cancelled. Your file was not changed."
                            } else {
                                "Cancelled. Nothing was forgotten."
                            },
                        );
                        Vec::new()
                    }
                }
            }
            Mode::Normal => self.on_key_normal(k),
            Mode::Picker(_) => Vec::new(),
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
        if let Some(r) = self.popup_key(k) {
            return r;
        }
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
            KeyCode::Up if self.move_line(-1) => {}
            KeyCode::Down if self.move_line(1) => {}
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
                if self.chat_waiting {
                    // one stray key must not throw away a long answer: the first Esc arms, the second stops
                    if self.esc_armed > 0 {
                        self.esc_armed = 0;
                        self.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                    } else {
                        self.esc_armed = ESC_ARM_TICKS;
                    }
                } else {
                    self.input.clear();
                    self.cursor = 0;
                }
            }
            KeyCode::Char('s') if ctrl => {
                self.stash_input();
            }
            KeyCode::Char('r') if ctrl => self.pop_stash(),
            KeyCode::Char('g') if ctrl => {
                self.editor_request = Some(self.expand_pasted(&self.input));
            }
            KeyCode::Char('j') if ctrl => self.insert('\n'),
            KeyCode::Enter
                if k.modifiers.contains(KeyModifiers::SHIFT)
                    || k.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.insert('\n')
            }
            KeyCode::Enter => {
                if !self.input.trim_start().starts_with('/') && !self.input.trim().is_empty() {
                    let size = self.expand_pasted(&self.input).chars().count();
                    if size > 4000 {
                        self.say(Kind::Warn, format!("That message is {size} characters; the limit is 4000. Shorten it or send it in parts. Nothing was sent."));
                        return Vec::new();
                    }
                }
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
                self.history_to_save.push(line.clone());
                let line = if line.trim_start().starts_with('/') {
                    line
                } else {
                    self.expand_pasted(&line)
                };
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
                        "/fixes [JOB]  /applyfix N  /undofix N   proposed fixes, checked against the reproducer".into(),
                        "/approve [JOB]                 look at the exact files, then approve or refuse".into(),
                        "/deliver [JOB]                 sign and deliver an approved release".into(),
                        "/memory [WORDS]  /remember TEXT  /correct ID TEXT  /forget ID".into(),
                        "/ask FOLDER : QUESTION         ask about documents; every quote is checked".into(),
                        "/proof [MODEL]  /gateway       check a NEAR confidential model's or the gateway's hardware proof".into(),
                        "/log                           the signed activity log".into(),
                        "/verify FILE                   check a capsule offline".into(),
                        "/stash  /pop                   put the line you are typing aside; bring it back (Ctrl-S, Ctrl-R)".into(),
                        "Ctrl-G                         write the message in your editor ($VISUAL or $EDITOR)".into(),
                        "/keep                          save this conversation in the encrypted store (otherwise it is never saved)".into(),
                        "/sessions                      pick a kept conversation or a review (type to filter, Ctrl-D twice forgets a chat)".into(),
                        "/newchat  /clear               leave this conversation and start a fresh one".into(),
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
            Command::Clear | Command::NewChat => {
                self.chat_history.clear();
                self.web_pages.clear();
                self.search_hits.clear();
                self.queued.clear();
                let was_kept = self.session.take().is_some();
                self.say(
                    Kind::Info,
                    if was_kept {
                        "Started a new conversation. The kept one stays saved; /sessions opens it again."
                    } else {
                        "The conversation was cleared. It was not saved; /keep saves a conversation."
                    },
                );
                Vec::new()
            }
            Command::Proof(model) => {
                let model = model.unwrap_or_else(|| "Qwen/Qwen3.8-27B".to_string());
                self.say(Kind::Info, format!("Checking the hardware proof of {model}. Only the model name is sent, to NEAR AI Cloud and to Intel and NVIDIA's verification services."));
                vec![self.req(
                    Intent::Proof,
                    false,
                    Method::Post,
                    "/api/route/attest",
                    Some(json!({"model": model})),
                )]
            }
            Command::Gateway => {
                self.say(Kind::Info, "Checking the proof of NEAR AI Cloud's gateway (front door). Nothing of yours is sent.");
                vec![self.req(
                    Intent::GatewayProof,
                    false,
                    Method::Post,
                    "/api/route/gateway",
                    Some(json!({})),
                )]
            }
            Command::Search(words) => {
                vec![self.req(
                    Intent::SearchPropose,
                    false,
                    Method::Post,
                    "/api/search/propose",
                    Some(json!({"query": words})),
                )]
            }
            Command::Read(arg) => {
                let mut urls = Vec::new();
                for p in arg.split_whitespace() {
                    match p.parse::<usize>() {
                        Ok(n) => {
                            match self.search_hits.get(n.wrapping_sub(1)) {
                                Some(u) => urls.push(u.clone()),
                                None => {
                                    self.say(Kind::Warn, format!("There is no search result number {n}. Run /search first."));
                                    return Vec::new();
                                }
                            }
                        }
                        Err(_) => urls.push(p.to_string()),
                    }
                }
                vec![self.req(
                    Intent::ReadPropose,
                    false,
                    Method::Post,
                    "/api/research/propose",
                    Some(json!({"urls": urls})),
                )]
            }
            Command::Web(clear) => {
                if clear {
                    self.web_pages.clear();
                    self.say(Kind::Info, "Forgot the web pages in this conversation.");
                } else if self.web_pages.is_empty() {
                    self.say(Kind::Info, "No web pages in this conversation. /search finds addresses; /read adds pages you approve.");
                } else {
                    let lines: Vec<String> =
                        self.web_pages.iter().map(|(_, u)| u.clone()).collect();
                    self.push(
                        Kind::Info,
                        Some("Web pages in this conversation (untrusted)"),
                        lines,
                    );
                }
                Vec::new()
            }
            Command::ClearHistory => {
                self.history.clear();
                vec![self.req(
                    Intent::HistoryClear,
                    false,
                    Method::Post,
                    "/api/history/clear",
                    Some(json!({})),
                )]
            }
            Command::Stash(true) => {
                if !self.stash_input() {
                    self.say(
                        Kind::Info,
                        "Type something first; /stash puts the line you are typing aside.",
                    );
                }
                Vec::new()
            }
            Command::Stash(false) => {
                self.pop_stash();
                Vec::new()
            }
            Command::Keep => {
                if self.session.is_some() {
                    self.say(Kind::Info, "This conversation is already kept.");
                    return Vec::new();
                }
                vec![self.req(
                    Intent::Keep,
                    false,
                    Method::Post,
                    "/api/sessions",
                    Some(json!({"title": ""})),
                )]
            }
            Command::Sessions => {
                let mut p = crate::picker::Picker::new("Conversations and reviews");
                for j in &self.jobs {
                    p.items.push(crate::picker::Item {
                        id: j.id.clone(),
                        label: format!("Review for {}", j.recipient),
                        detail: j.state.clone(),
                        when: 0,
                        kind: crate::picker::ItemKind::Review,
                    });
                }
                self.mode = Mode::Picker(p);
                vec![self.req(Intent::Sessions, false, Method::Get, "/api/sessions", None)]
            }
            Command::Status => vec![self.req(
                Intent::Readiness,
                false,
                Method::Get,
                "/api/readiness",
                None,
            )],
            Command::Reviews => vec![self.req(Intent::Jobs, false, Method::Get, "/api/jobs", None)],
            Command::Review { folder, recipient } => {
                let path = if folder.starts_with('/') {
                    folder
                } else {
                    match self.roots.first() {
                        Some(r) => format!(
                            "{}/{}",
                            r.trim_end_matches('/'),
                            folder.trim_start_matches("./")
                        ),
                        None => folder,
                    }
                };
                vec![self.req(
                    Intent::StartReview,
                    false,
                    Method::Post,
                    "/api/jobs",
                    Some(json!({"repo": path, "recipient": recipient})),
                )]
            }
            Command::Open(j) => {
                let Some(j) = self.job_or_latest(j, &[], "open") else {
                    return Vec::new();
                };
                vec![self.req(
                    Intent::Poll(j.clone()),
                    false,
                    Method::Get,
                    &format!("/api/jobs/{j}"),
                    None,
                )]
            }
            Command::Findings(j) => {
                let Some(j) =
                    self.job_or_latest(j, &["ready", "nothing_to_release"], "show findings for")
                else {
                    return Vec::new();
                };
                vec![self.req(
                    Intent::Findings,
                    false,
                    Method::Get,
                    &format!("/api/jobs/{j}/findings"),
                    None,
                )]
            }
            Command::Fixes(j) => {
                let Some(j) =
                    self.job_or_latest(j, &["ready", "nothing_to_release"], "show fixes for")
                else {
                    return Vec::new();
                };
                vec![self.req(
                    Intent::Fixes(j.clone()),
                    false,
                    Method::Get,
                    &format!("/api/jobs/{j}/fixes"),
                    None,
                )]
            }
            Command::ApplyFix { n, job } => {
                let Some(j) =
                    self.job_or_latest(job, &["ready", "nothing_to_release"], "apply a fix for")
                else {
                    return Vec::new();
                };
                let Some((_, _, file, verified)) = self
                    .fixes_seen
                    .iter()
                    .find(|(fj, fnum, _, _)| *fj == j && *fnum == n)
                    .cloned()
                else {
                    self.say(
                        Kind::Warn,
                        "Run /fixes first and read the change; then /applyfix N.",
                    );
                    return Vec::new();
                };
                if !verified {
                    self.say(Kind::Warn, "That fix was not accepted by the reproducer check, so it cannot be applied.");
                    return Vec::new();
                }
                self.mode = Mode::Confirm(ConfirmView {
                    prompt: format!(
                        "Change {file} on this computer now? This edits your own file. [y/N]"
                    ),
                    forget_id: String::new(),
                    apply: Some((j, n, file)),
                    undo: false,
                    web: None,
                });
                Vec::new()
            }
            Command::UndoFix { n, job } => {
                let Some(j) =
                    self.job_or_latest(job, &["ready", "nothing_to_release"], "undo a fix for")
                else {
                    return Vec::new();
                };
                let Some((_, _, file, _)) = self
                    .fixes_seen
                    .iter()
                    .find(|(fj, fnum, _, _)| *fj == j && *fnum == n)
                    .cloned()
                else {
                    self.say(Kind::Warn, "Run /fixes first; then /undofix N.");
                    return Vec::new();
                };
                self.mode = Mode::Confirm(ConfirmView {
                    prompt: format!("Restore the original {file} on this computer now? [y/N]"),
                    forget_id: String::new(),
                    apply: Some((j, n, file)),
                    undo: true,
                    web: None,
                });
                Vec::new()
            }
            Command::Release(j) => {
                let Some(j) = self.job_or_latest(j, &["ready"], "show a release for") else {
                    return Vec::new();
                };
                vec![self.req(
                    Intent::Release,
                    false,
                    Method::Get,
                    &format!("/api/jobs/{j}/release"),
                    None,
                )]
            }
            Command::Approve(j) => {
                let Some(j) = self.job_or_latest(j, &["ready"], "approve") else {
                    return Vec::new();
                };
                vec![self.req(
                    Intent::ApproveRequest(j.clone()),
                    false,
                    Method::Post,
                    &format!("/api/jobs/{j}/approval"),
                    Some(json!({})),
                )]
            }
            Command::Deliver(j) => {
                let Some(j) = self.job_or_latest(j, &["ready"], "deliver") else {
                    return Vec::new();
                };
                vec![self.req(
                    Intent::Deliver,
                    false,
                    Method::Post,
                    &format!("/api/jobs/{j}/deliver"),
                    Some(json!({})),
                )]
            }
            Command::Memory(q) => {
                let path = match q {
                    Some(q) => format!(
                        "/api/memories?q={}",
                        q.chars()
                            .filter(|c| c.is_alphanumeric() || *c == ' ')
                            .collect::<String>()
                            .replace(' ', "%20")
                    ),
                    None => "/api/memories".to_string(),
                };
                vec![self.req(Intent::MemoryList, false, Method::Get, &path, None)]
            }
            Command::Ask { folder, question } => {
                if self.chat_waiting {
                    self.say(Kind::Warn, "Still waiting for the last answer.");
                    return Vec::new();
                }
                self.chat_waiting = true;
                vec![self.req(
                    Intent::Ask,
                    false,
                    Method::Post,
                    "/api/ask",
                    Some(json!({"folder": folder, "question": question})),
                )]
            }
            Command::Remember { text, project } => {
                let mut body = json!({"text": text, "level": "internal", "reason": "you added it in the terminal"});
                if let Some(p) = project {
                    body["project"] = json!(p);
                }
                vec![self.req(
                    Intent::Remember,
                    false,
                    Method::Post,
                    "/api/memories",
                    Some(body),
                )]
            }
            Command::Correct { id, text } => vec![self.req(
                Intent::Correct,
                false,
                Method::Post,
                &format!("/api/memories/{id}/correct"),
                Some(json!({"text": text, "reason": "you corrected it in the terminal"})),
            )],
            Command::Forget(id) => {
                self.mode = Mode::Confirm(ConfirmView { prompt: format!("Forget {id} for good? The text is deleted and will not be saved again. [y/N]"), forget_id: id, apply: None, undo: false, web: None });
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
            if self.queued.len() >= 5 {
                self.say(Kind::Warn, "Five messages are already waiting. Wait for an answer, or press Esc to stop the current one.");
            } else {
                self.queued.push(message.to_string());
                self.say(Kind::Info, format!("Queued ({} waiting). It is sent when the current answer is finished. Esc stops the current answer and drops the queue.", self.queued.len()));
            }
            return Vec::new();
        }
        let start = self.chat_history.len().saturating_sub(18);
        let history: Vec<Value> = self.chat_history[start..]
            .iter()
            .map(|(r, c)| json!({"role": r, "content": c}))
            .collect();
        self.chat_history.push(("user".into(), message.to_string()));
        self.chat_waiting = true;
        self.live = None;
        self.cancel
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let mut req = self.req(
            Intent::Chat,
            false,
            Method::Post,
            "/api/chat/stream",
            Some({
                let mut body = json!({"message": message, "history": history});
                if let Some(id) = &self.session {
                    body["session"] = json!(id);
                }
                if !self.web_pages.is_empty() {
                    body["sources"] = json!(self
                        .web_pages
                        .iter()
                        .map(|(p, _)| p.clone())
                        .collect::<Vec<_>>());
                }
                body
            }),
        );
        req.stream = true;
        vec![req]
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
                    statement: body["statement"].clone(),
                    requires_proof: body["requires_proof"] == true,
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
                        "Nothing remembered yet. /remember TEXT adds something; /remember in PROJECT: TEXT keeps it for one project.",
                    );
                }
                let lines: Vec<String> = list
                    .iter()
                    .flat_map(|m| {
                        let mut v = vec![format!(
                            "{}  [{}]{}  {}",
                            clean(m["id"].as_str().unwrap_or("?")),
                            clean(m["level"].as_str().unwrap_or("?")),
                            format!(
                                "{}{}",
                                if m["untrusted"] == true {
                                    " untrusted"
                                } else {
                                    ""
                                },
                                m["project"]
                                    .as_str()
                                    .map(|p| format!(" project {}", clean(p)))
                                    .unwrap_or_default()
                            ),
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
            Intent::Fixes(job) => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let list = body["fixes"].as_array().cloned().unwrap_or_default();
                self.fixes_seen.retain(|(j, _, _, _)| *j != job);
                if list.is_empty() {
                    self.push(
                        Kind::Info,
                        Some("Fixes"),
                        vec!["The agent proposed no fixes.".into()],
                    );
                }
                for (i, f) in list.iter().enumerate() {
                    let n = i + 1;
                    let file = clean(f["file"].as_str().unwrap_or("?"));
                    let shown = match f["files"].as_array() {
                        Some(all) if all.len() > 1 => all
                            .iter()
                            .filter_map(|x| x.as_str())
                            .map(clean)
                            .collect::<Vec<_>>()
                            .join(", "),
                        _ => file.clone(),
                    };
                    let verified = f["verified"] == true;
                    self.fixes_seen.push((
                        job.clone(),
                        n,
                        file.clone(),
                        verified && f["applied"] != true,
                    ));
                    let mut lines = vec![format!(
                        "{shown}: {}",
                        clean(f["title"].as_str().unwrap_or(""))
                    )];
                    lines.extend(text_lines(&clean(f["explanation"].as_str().unwrap_or(""))));
                    lines.push(String::new());
                    for l in f["diff"].as_str().unwrap_or("").lines().take(60) {
                        lines.push(clean(l));
                    }
                    lines.push(String::new());
                    lines.push(if f["applied"] == true {
                        "Already applied.".to_string()
                    } else if verified {
                        format!("The reproducer stops reproducing. Your project's tests were not run. /applyfix {n} to apply.")
                    } else {
                        format!("NOT accepted: {}", clean(&f["status"].as_str().unwrap_or("").chars().take(160).collect::<String>()))
                    });
                    self.push(
                        if verified { Kind::Ok } else { Kind::Warn },
                        Some(&format!("Fix {n}")),
                        lines,
                    );
                }
                Vec::new()
            }
            Intent::UndoFix(file) => {
                self.say(
                    if ok { Kind::Ok } else { Kind::Error },
                    if ok {
                        format!("Restored {file}.")
                    } else {
                        err_text(status, &body)
                    },
                );
                Vec::new()
            }
            Intent::ApplyFix(file) => {
                self.say(
                    if ok { Kind::Ok } else { Kind::Error },
                    if ok {
                        format!("Applied to {file}.")
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
            Intent::Ask => {
                self.chat_waiting = false;
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let mut lines = Vec::new();
                if body["checkable"] == true {
                    lines.extend(text_lines(body["answer"].as_str().unwrap_or("")));
                    lines.push(String::new());
                    for c in body["citations"].as_array().into_iter().flatten() {
                        let mark = match c["verdict"].as_str() {
                            Some("verified") => format!("verified, line {}", c["line"]),
                            Some("not_found") => {
                                "NOT FOUND in that file: do not rely on it".to_string()
                            }
                            Some("unknown_file") => {
                                "NOT A FILE the model was shown: do not rely on it".to_string()
                            }
                            _ => "could not be checked".to_string(),
                        };
                        lines.push(format!(
                            "{}: \"{}\" ({mark})",
                            clean(c["file"].as_str().unwrap_or("?")),
                            clean(c["quote"].as_str().unwrap_or(""))
                        ));
                    }
                } else {
                    lines.push("The model did not give a checkable answer. Unchecked text follows; trust none of it:".into());
                    lines.extend(text_lines(body["unchecked_text"].as_str().unwrap_or("")));
                }
                lines.push(String::new());
                lines.push(format!(
                    "{} file(s) read, {} withheld, {} too large",
                    body["files_used"].as_array().map_or(0, |a| a.len()),
                    body["files_withheld"].as_array().map_or(0, |a| a.len()),
                    body["files_skipped"].as_array().map_or(0, |a| a.len())
                ));
                if body["injection_suspects"]
                    .as_array()
                    .is_some_and(|a| !a.is_empty())
                {
                    lines.push("Some documents contain text that tries to give the model orders; they were treated as data.".into());
                }
                self.push(Kind::Info, Some("Documents"), lines);
                Vec::new()
            }
            Intent::SearchPropose | Intent::ReadPropose => {
                let search = matches!(intent, Intent::SearchPropose);
                if !ok {
                    let mut lines = text_lines(&err_text(status, &body));
                    if let Some(items) = body["items"].as_array() {
                        for i in items {
                            lines.push(format!(
                                "{} {}",
                                clean(i["url"].as_str().unwrap_or("?")),
                                clean(i["reason"].as_str().unwrap_or(""))
                            ));
                        }
                    }
                    self.push(
                        Kind::Warn,
                        Some(if search { "Search" } else { "Read" }),
                        lines,
                    );
                    return Vec::new();
                }
                let (Some(id), Some(digest)) = (body["id"].as_str(), body["digest"].as_str())
                else {
                    self.say(
                        Kind::Error,
                        "The service gave an unexpected answer; nothing was sent.",
                    );
                    return Vec::new();
                };
                let mut lines = Vec::new();
                if search {
                    lines.push(format!(
                        "These words would be sent to {}:",
                        clean(body["will_send_to"].as_str().unwrap_or("?"))
                    ));
                    lines.push(format!("  {}", clean(body["query"].as_str().unwrap_or(""))));
                } else {
                    lines.push("These exact addresses would be requested:".into());
                    for i in body["items"].as_array().into_iter().flatten() {
                        let u = clean(i["url"].as_str().unwrap_or("?"));
                        if i["allowed"] == true {
                            lines.push(format!("  {u}"));
                        } else {
                            lines.push(format!(
                                "  {u}  (not allowed: {})",
                                clean(i["reason"].as_str().unwrap_or(""))
                            ));
                        }
                    }
                }
                lines.push(clean(body["stays_here"].as_str().unwrap_or("")));
                self.push(
                    Kind::Info,
                    Some(if search {
                        "Search (not sent yet)"
                    } else {
                        "Read (not requested yet)"
                    }),
                    lines,
                );
                self.mode = Mode::Confirm(ConfirmView {
                    prompt: if search {
                        "Send exactly these words to that site? [y/N]".into()
                    } else {
                        "Request exactly these addresses? [y/N]".into()
                    },
                    forget_id: String::new(),
                    apply: None,
                    undo: false,
                    web: Some((
                        if search { "search" } else { "read" }.into(),
                        id.to_string(),
                        digest.to_string(),
                    )),
                });
                Vec::new()
            }
            Intent::SearchRun => {
                if !ok {
                    self.push(
                        Kind::Error,
                        Some("Search"),
                        text_lines(&err_text(status, &body)),
                    );
                    return Vec::new();
                }
                self.search_hits.clear();
                let mut lines = vec!["Suggestions from the search site (untrusted). Nothing has been requested from them.".to_string()];
                for h in body["results"].as_array().into_iter().flatten() {
                    let url = h["url"].as_str().unwrap_or("").to_string();
                    if url.is_empty() {
                        continue;
                    }
                    self.search_hits.push(url.clone());
                    let n = self.search_hits.len();
                    if h["withheld"] == true {
                        lines.push(format!(
                            "{n}. {}  (text withheld: reads like instructions)",
                            clean(&url)
                        ));
                    } else {
                        lines.push(format!(
                            "{n}. {}  {}",
                            clean(&url),
                            clean(h["title"].as_str().unwrap_or(""))
                        ));
                        let sn = clean(h["snippet"].as_str().unwrap_or(""));
                        if !sn.is_empty() {
                            lines.push(format!("     {sn}"));
                        }
                    }
                }
                if self.search_hits.is_empty() {
                    lines.push("No usable results.".into());
                } else {
                    lines.push(
                        "Read some with /read 1 2 (you will see the addresses and approve first)."
                            .into(),
                    );
                }
                self.push(Kind::Info, Some("Search results"), lines);
                Vec::new()
            }
            Intent::ReadFetch => {
                if !ok {
                    self.push(
                        Kind::Error,
                        Some("Read"),
                        text_lines(&err_text(status, &body)),
                    );
                    return Vec::new();
                }
                let mut lines = Vec::new();
                for p in body["pages"].as_array().into_iter().flatten() {
                    let url = clean(p["final_url"].as_str().or(p["url"].as_str()).unwrap_or("?"));
                    if let Some(e) = p["error"].as_str() {
                        lines.push(format!("{url}  could not be read: {}", clean(e)));
                    } else if p["looks_like_instructions"] == true {
                        lines.push(format!(
                            "{url}  not given to the model: it reads like instructions"
                        ));
                    } else if let Some(id) = p["page_id"].as_str() {
                        if self.web_pages.len() < 5 {
                            self.web_pages.push((id.to_string(), url.clone()));
                            lines.push(format!(
                                "{url}  added ({})",
                                clean(p["title"].as_str().unwrap_or(""))
                            ));
                        } else {
                            lines.push(format!(
                                "{url}  not added: five pages is the limit; /web clear first"
                            ));
                        }
                    }
                }
                lines.push(
                    "Pages are untrusted data for the model. Your next messages can use them."
                        .into(),
                );
                self.push(Kind::Info, Some("Read"), lines);
                Vec::new()
            }
            Intent::Chat => {
                self.chat_waiting = false;
                let live = self.live.take();
                if !ok || body["stopped"] == true {
                    self.chat_history.pop();
                    let dropped = std::mem::take(&mut self.queued).len();
                    if dropped > 0 {
                        self.say(Kind::Warn, format!("{dropped} queued message(s) were not sent because the answer did not finish. Type them again."));
                    }
                    if let Some((i, partial)) = live {
                        if let Some(c) = self.cards.get_mut(i) {
                            c.lines = text_lines(&partial);
                            c.lines.push(String::new());
                            c.lines.push(
                                "Stopped. This partial answer is not kept in the conversation."
                                    .into(),
                            );
                        }
                    }
                    if !ok {
                        self.say(Kind::Error, err_text(status, &body));
                    } else if self
                        .cards
                        .last()
                        .is_none_or(|c| c.title.as_deref() != Some("Assistant"))
                    {
                        self.say(Kind::Info, "Stopped.");
                    }
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
                if let Some(n) = body["usage"]["context_tokens_estimate"].as_u64() {
                    foot.push(format!("about {n} tokens of context sent"));
                    let dropped = body["usage"]["history_turns_dropped"].as_u64().unwrap_or(0);
                    if dropped > 0 {
                        foot.push(format!("{dropped} older message(s) were not sent because the conversation is long"));
                    }
                }
                if self.session.is_some() {
                    foot.push(if body["saved"] == true {
                        "saved in the encrypted store".into()
                    } else {
                        "not saved (this conversation is full)".to_string()
                    });
                }
                if body["notes_withheld"].as_u64().unwrap_or(0) > 0 {
                    foot.push(format!("{} note(s) were not given to the model because they look like instructions.", body["notes_withheld"]));
                }
                let mut lines = text_lines(&reply);
                lines.push(String::new());
                lines.extend(foot);
                match live.and_then(|(i, _)| self.cards.get_mut(i)) {
                    Some(c) => {
                        c.lines = lines;
                        self.scroll_back = 0;
                    }
                    None => self.push(Kind::Info, Some("Assistant"), lines),
                }
                if self.queued.is_empty() {
                    Vec::new()
                } else {
                    let next = self.queued.remove(0);
                    self.chat(&next)
                }
            }
            Intent::Proof | Intent::GatewayProof => {
                let gateway = matches!(intent, Intent::GatewayProof);
                if !ok {
                    self.push(
                        Kind::Error,
                        Some("Not verified"),
                        text_lines(&err_text(status, &body)),
                    );
                    return Vec::new();
                }
                let mut lines = Vec::new();
                let deployments: Vec<Value> = if gateway {
                    vec![body["gateway"].clone()]
                } else {
                    body["deployments"].as_array().cloned().unwrap_or_default()
                };
                for d in &deployments {
                    lines.push(format!(
                        "Intel patch level: {}",
                        clean(d["tcb"].as_str().unwrap_or("?"))
                    ));
                    if gateway {
                        lines.push(if d["tls_bound"] == true {
                            "This connection's encryption ends inside the verified software".into()
                        } else {
                            "Where this connection's encryption ends was not checked".to_string()
                        });
                    } else {
                        lines.push(if d["gpu_verified"] == true {
                            "NVIDIA GPU evidence verified".into()
                        } else {
                            "GPU evidence not verified".to_string()
                        });
                    }
                    lines.push(format!(
                        "Answer-signing key: {}",
                        clean(d["signer"].as_str().unwrap_or("?"))
                    ));
                    lines.push(format!(
                        "Fresh challenge: {}",
                        clean(d["nonce"].as_str().unwrap_or("?"))
                    ));
                }
                lines.push(String::new());
                lines.push("What this shows:".into());
                for m in body["means"].as_array().into_iter().flatten() {
                    lines.push(format!("  · {}", clean(m.as_str().unwrap_or(""))));
                }
                lines.push("What this does not show:".into());
                for m in body["does_not_mean"].as_array().into_iter().flatten() {
                    lines.push(format!("  · {}", clean(m.as_str().unwrap_or(""))));
                }
                self.push(
                    Kind::Ok,
                    Some(if gateway {
                        "Gateway verified"
                    } else {
                        "Model verified"
                    }),
                    lines,
                );
                Vec::new()
            }
            Intent::HistoryLoad => {
                if ok {
                    let saved: Vec<String> = body["lines"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|l| l.as_str().map(clean))
                        .collect();
                    let typed_now = std::mem::take(&mut self.history);
                    self.history = saved;
                    for l in typed_now {
                        if self.history.last() != Some(&l) {
                            self.history.push(l);
                        }
                    }
                }
                Vec::new()
            }
            Intent::HistorySave => Vec::new(),
            Intent::HistoryClear => {
                self.say(
                    if ok { Kind::Ok } else { Kind::Error },
                    if ok {
                        "Command history cleared.".to_string()
                    } else {
                        err_text(status, &body)
                    },
                );
                Vec::new()
            }
            Intent::Keep => {
                if ok {
                    self.session = body["id"].as_str().map(clean);
                    self.say(Kind::Ok, "Kept. From now on this conversation is saved in the encrypted store on this computer. /sessions opens kept ones; /newchat leaves it.");
                } else {
                    self.say(Kind::Error, err_text(status, &body));
                }
                Vec::new()
            }
            Intent::Sessions => {
                if let Mode::Picker(p) = &mut self.mode {
                    p.loading = false;
                    if ok {
                        for k in body["sessions"].as_array().into_iter().flatten() {
                            let title = clean(k["title"].as_str().unwrap_or(""));
                            p.items.push(crate::picker::Item {
                                id: clean(k["id"].as_str().unwrap_or("?")),
                                label: if title.is_empty() {
                                    "Untitled conversation".into()
                                } else {
                                    title
                                },
                                detail: format!("{} message(s)", k["turns"].as_u64().unwrap_or(0)),
                                when: k["updated"].as_i64().unwrap_or(0),
                                kind: crate::picker::ItemKind::Chat,
                            });
                        }
                    } else {
                        p.error = Some(format!(
                            "Could not load kept conversations. {}",
                            err_text(status, &body)
                        ));
                    }
                }
                Vec::new()
            }
            Intent::OpenChat(id) => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                self.chat_history.clear();
                self.cards.clear();
                self.live = None;
                for t in body["turns"].as_array().into_iter().flatten() {
                    let (role, content) = (
                        t["role"].as_str().unwrap_or(""),
                        clean(t["content"].as_str().unwrap_or("")),
                    );
                    if role != "user" && role != "assistant" {
                        continue;
                    }
                    self.chat_history.push((role.to_string(), content.clone()));
                    let (kind, title) = if role == "user" {
                        (Kind::User, None)
                    } else {
                        (Kind::Info, Some("Assistant"))
                    };
                    self.push(kind, title, text_lines(&content));
                }
                self.session = Some(id);
                self.say(
                    Kind::Ok,
                    "Opened the kept conversation. New messages are saved to it.",
                );
                Vec::new()
            }
            Intent::ForgetChat(id) => {
                if let Mode::Picker(p) = &mut self.mode {
                    if ok {
                        p.remove(&id);
                        p.error = None;
                    } else {
                        p.error = Some(err_text(status, &body));
                    }
                }
                if ok && self.session.as_deref() == Some(id.as_str()) {
                    self.session = None;
                    self.chat_history.clear();
                }
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
