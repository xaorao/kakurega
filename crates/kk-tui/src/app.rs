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
    /// The terminal reported that it gained (true) or lost (false) the owner's focus.
    Focus(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    User,
    Info,
    Ok,
    Warn,
    Error,
}

/// The title of an answer to `/aside`: shown on the screen, and left out of the conversation text that is searched, copied and exported.
pub const ASIDE_TITLE: &str = "Aside";

/// The title of the card that lists what `/transcript` found.
const SEARCH_TITLE: &str = "Search in this conversation";

/// Standard base64 with padding, the form a terminal's clipboard sequence takes.
pub fn base64_standard(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |a, (i, b)| a | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The most text put on the clipboard in one go: terminals refuse longer clipboard sequences.
pub const COPY_MAX_BYTES: usize = 74_000;

/// The terminal sequence that puts `text` on the system clipboard. Base64 contains no escape or bell, so nothing in the text
/// can end it early or start another. None for nothing, or for more than a terminal takes.
pub fn osc52(text: &str) -> Option<String> {
    if text.is_empty() || text.len() > COPY_MAX_BYTES {
        return None;
    }
    Some(format!(
        "\x1b]52;c;{}\x07",
        base64_standard(text.as_bytes())
    ))
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
    Aside,
    PiiShow,
    PiiEdit,
    HistoryClear,
    Proof,
    GatewayProof,
    Keep,
    Branch,
    /// Open the newest kept conversation of this project (else the newest of all); `true` opens a branch of it instead.
    ResumeLast(bool),
    /// Branch the named kept conversation and open the branch: the list is read first to know how many messages it has.
    ResumeFork(String),
    /// The last messages of a kept conversation, shown under its row in the picker.
    PickerPreview(String),
    /// Which reviews stopped since the owner last looked; asked once when the workspace opens.
    Away,
    /// Tell the service the list was seen (at launch, after showing it, and when leaving).
    AwaySeen,
    Sessions,
    OpenChat(String),
    ForgetChat(String),
    Readiness,
    Jobs,
    /// Folder names asked for to finish the folder being typed after /review: the line as it was and the part typed so far.
    PathComplete {
        line: String,
        head: String,
        partial: String,
    },
    StartReview,
    Poll(String),
    Findings,
    Fixes(String),
    ApplyFix(String),
    UndoFix(String),
    Release,
    ApproveRequest(String),
    ApproveJob(String),
    ApprovalDetail {
        job: String,
        aid: String,
    },
    ApproveSubmit,
    ApprovalWatch(String),
    Deny,
    Deliver,
    MemoryList,
    Similar,
    Remember,
    Ask,
    Correct,
    Forget,
    /// The counts of what forgetting a review would remove, asked for before anything is confirmed.
    ReviewFootprint(String),
    ForgetReview,
    InstructionsShow,
    InstructionsSet,
    ConflictsList,
    FindChats,
    ExportTranscript,
    UsageShow,
    RenameChat,
    ChooseNote,
    BothRight,
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
    /// Everything one review left in memory, waiting for a yes: the review's id.
    pub review: Option<String>,
    /// Two disagreeing notes waiting for a yes: the one to keep, the one it replaces.
    pub choose: Option<(String, String)>,
    /// Text waiting for a yes before it goes to the clipboard.
    pub copy: Option<String>,
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

/// The most cards kept on the screen, how many go at once when it is exceeded, and the most lines one card keeps.
pub const MAX_CARDS: usize = 2000;
pub const DROP_CARDS: usize = 250;
pub const MAX_CARD_LINES: usize = 20_000;

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
    approval_watch_inflight: bool,
    pub chat_history: Vec<(String, String)>,
    pub session: Option<String>,
    /// The project this conversation is about: its notes are offered, and no other project's. Set from the repository the
    /// terminal was started in, by `/project`, or by a kept conversation.
    pub chat_project: Option<String>,
    /// How much of the model's window the last answer's question took, for the status line; empty until an answer comes.
    context_meter: Option<String>,
    /// Long pastes shown as a short placeholder in the composer and expanded when a chat message is sent.
    pub pasted: Vec<(String, String)>,
    /// Messages typed while an answer is still arriving; each is sent after the previous answer completes.
    pub queued: Vec<String>,
    /// Lines put aside with Ctrl-S or /stash, newest last. Kept in memory only and gone when the terminal closes.
    pub stashed: Vec<String>,
    /// Set by Ctrl-G: the text to open in the owner's editor. The runtime takes it, runs the editor and calls `finish_edit`.
    pub editor_request: Option<String>,
    /// Text the owner agreed to put on the system clipboard; the screen loop writes the clipboard sequence for it.
    pub clipboard_request: Option<String>,
    /// Whether long cards are shown whole (Ctrl-O). Off by default: a card of more than 24 lines shows its first 10 and says how many more.
    pub expand_all: bool,
    /// When set, the steps of a running review are not printed; its end still is.
    pub quiet: bool,
    /// The notes the last /memory listing showed (id and the start of the text), so Tab can finish a note id.
    pub notes_seen: Vec<(String, String)>,
    /// Project names the last /memory listing showed, for Tab after /project.
    pub projects_seen: Vec<String>,
    /// The message /rewind took back, to be put in the box once the copy has opened.
    pub rewound: Option<String>,
    /// Set by Tab after /review: the folder to list, the line it was asked for, what comes before the last name, the part typed.
    path_request: Option<(String, String, String, String)>,
    /// Set when a review finished or stopped; the screen loop takes it and rings the bell once.
    pub attention: Option<crate::attention::Attention>,
    /// What the terminal last said about focus; unknown until it says.
    pub focused: Option<bool>,
    /// The latest thing that waits for the owner, shown in the window title until the owner acts.
    pub waiting: Option<crate::attention::Attention>,
    /// Set by the first Esc while an answer is arriving; the second Esc within a few seconds stops it. Counts down on ticks.
    pub esc_armed: u8,
    pub popup_sel: usize,
    popup_dismissed: Option<String>,
    history_to_save: Vec<String>,
    history_loaded: bool,
    away_pending: bool,
    /// Set once the service stopped answering, so the owner is alerted once and not on every failed request; cleared by any answer.
    service_down: bool,
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

/// A path as a query value: everything but letters, digits and `-_.~/` becomes %XX, so no folder name can add a parameter.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
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

/// What `kaku --resume` asked for: a chosen conversation (else the newest of this project), and whether to open a branch of it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resume {
    pub id: Option<String>,
    pub fork: bool,
}

/// What a kept conversation's id is made of; the id becomes part of a request path.
fn valid_chat_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
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
            approval_watch_inflight: false,
            chat_history: Vec::new(),
            session: None,
            chat_project: None,
            context_meter: None,
            pasted: Vec::new(),
            queued: Vec::new(),
            stashed: Vec::new(),
            popup_sel: 0,
            popup_dismissed: None,
            history_to_save: Vec::new(),
            history_loaded: false,
            away_pending: true,
            service_down: false,
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            live: None,
            chat_waiting: false,
            approver: None,
            editor_request: None,
            clipboard_request: None,
            expand_all: false,
            quiet: false,
            notes_seen: Vec::new(),
            projects_seen: Vec::new(),
            rewound: None,
            path_request: None,
            attention: None,
            focused: None,
            waiting: None,
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

    fn push(&mut self, kind: Kind, title: Option<&str>, mut lines: Vec<String>) {
        if lines.len() > MAX_CARD_LINES {
            let cut = lines.len() - MAX_CARD_LINES;
            lines.truncate(MAX_CARD_LINES);
            lines.push(format!("… {cut} more line(s) were not kept on the screen."));
        }
        self.cards.push(Card {
            kind,
            title: title.map(str::to_string),
            lines,
        });
        if self.cards.len() > MAX_CARDS {
            self.drop_oldest_cards();
        }
        self.scroll_back = 0;
    }

    /// A long session must not grow without limit: the oldest cards leave the screen's memory, in one step of `DROP_CARDS` so the work
    /// is not repeated for every card, and one marker card at the top says how many went. The conversation itself (what is sent, saved
    /// and exported) is held elsewhere and is not touched.
    fn drop_oldest_cards(&mut self) {
        const MARK: &str = "Earlier output was removed from this screen to save memory";
        let had_marker = self
            .cards
            .first()
            .is_some_and(|c| c.lines.first().is_some_and(|l| l.starts_with(MARK)));
        let mut gone = if had_marker {
            self.cards
                .first()
                .and_then(|c| c.lines.first())
                .and_then(|l| l.rsplit(" (").next())
                .and_then(|t| t.split(' ').next())
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(0)
        } else {
            0
        };
        let start = usize::from(had_marker);
        let n = DROP_CARDS.min(self.cards.len().saturating_sub(start));
        self.cards.drain(start..start + n);
        gone += n;
        let marker = Card {
            kind: Kind::Info,
            title: None,
            lines: vec![format!("{MARK} ({gone} card(s)). The conversation itself is unchanged; /export writes all of it.")],
        };
        if had_marker {
            self.cards[0] = marker;
        } else {
            self.cards.insert(0, marker);
        }
    }

    fn say(&mut self, kind: Kind, line: impl AsRef<str>) {
        self.push(kind, None, text_lines(line.as_ref()));
    }

    /// Opens the named kept conversation (`kaku --resume ID`), or the most recently updated one, once the screen is up.
    pub fn resume_at_start(&mut self, id: Option<&str>, fork: bool) -> Vec<Req> {
        if let Some(id) = id {
            if !fork {
                return self.open_chat_by_id(id);
            }
            let id = id.trim();
            if !valid_chat_id(id) {
                self.say(
                    Kind::Error,
                    "That is not a conversation id. /sessions lists the kept ones.",
                );
                return Vec::new();
            }
            return vec![self.req(
                Intent::ResumeFork(id.to_string()),
                false,
                Method::Get,
                "/api/sessions",
                None,
            )];
        }
        vec![self.req(
            Intent::ResumeLast(fork),
            false,
            Method::Get,
            "/api/sessions",
            None,
        )]
    }

    /// Opens a list of the owner's own messages in the open conversation to go back to (`/rewind`, or Esc twice on an empty box).
    fn rewind_picker(&mut self) -> Vec<Req> {
        if self.session.is_none() {
            self.say(
                Kind::Info,
                "Only a kept conversation can be rewound. /keep saves this one first.",
            );
            return Vec::new();
        }
        let mut p = crate::picker::Picker::new("Go back to before one of your messages");
        p.loading = false;
        for (i, (role, content)) in self.chat_history.iter().enumerate() {
            if role != "user" {
                continue;
            }
            let one_line: String = content.split_whitespace().collect::<Vec<_>>().join(" ");
            let cut: String = one_line.chars().take(90).collect();
            p.items.push(crate::picker::Item {
                id: (i + 1).to_string(),
                label: format!(
                    "{cut}{}",
                    if one_line.chars().count() > 90 {
                        "…"
                    } else {
                        ""
                    }
                ),
                detail: format!("message {}", i + 1),
                when: i as i64,
                kind: crate::picker::ItemKind::Message,
                project: None,
            });
        }
        if p.items.is_empty() {
            self.say(
                Kind::Info,
                "You have not written a message in this conversation yet.",
            );
            return Vec::new();
        }
        self.mode = Mode::Picker(p);
        Vec::new()
    }

    /// Goes back to before the owner's message number `n` (counting from 1) by branching the kept conversation there; the message
    /// is put back in the box to change. The original conversation is untouched.
    fn rewind_to(&mut self, n: usize) -> Vec<Req> {
        let Some(id) = self.session.clone() else {
            self.say(
                Kind::Info,
                "Only a kept conversation can be rewound. /keep saves this one first.",
            );
            return Vec::new();
        };
        match self.chat_history.get(n - 1) {
            Some((role, content)) if role == "user" => {
                self.rewound = Some(content.clone());
            }
            Some(_) => {
                self.say(
                            Kind::Error,
                            format!("Message {n} is not one of yours; choose the number of a message you wrote."),
                        );
                return Vec::new();
            }
            None => {
                self.say(
                    Kind::Error,
                    format!(
                        "This conversation has {} message(s); choose a number up to that.",
                        self.chat_history.len()
                    ),
                );
                return Vec::new();
            }
        }
        vec![self.req(
            Intent::Branch,
            false,
            Method::Post,
            &format!("/api/sessions/{id}/fork"),
            Some(json!({"at": n - 1})),
        )]
    }

    /// Opens a kept conversation from the list, or a branch of it (a new kept conversation with all its messages; the original is untouched).
    fn open_or_branch(&mut self, k: &Value, fork: bool) -> Vec<Req> {
        let Some(id) = k["id"].as_str().map(clean) else {
            return Vec::new();
        };
        if fork {
            let at = k["turns"].as_u64().unwrap_or(0);
            return vec![self.req(
                Intent::Branch,
                false,
                Method::Post,
                &format!("/api/sessions/{id}/fork"),
                Some(json!({"at": at})),
            )];
        }
        vec![self.req(
            Intent::OpenChat(id.clone()),
            false,
            Method::Get,
            &format!("/api/sessions/{id}"),
            None,
        )]
    }

    /// Opens one kept conversation by id. The id becomes part of a request path, so only what an id is made of is accepted.
    fn open_chat_by_id(&mut self, id: &str) -> Vec<Req> {
        let id = id.trim();
        if !valid_chat_id(id) {
            self.say(
                Kind::Error,
                "That is not a conversation id. /sessions lists the kept ones.",
            );
            return Vec::new();
        }
        vec![self.req(
            Intent::OpenChat(id.to_string()),
            false,
            Method::Get,
            &format!("/api/sessions/{id}"),
            None,
        )]
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
        if let Some((dir, line, head, partial)) = self.path_request.take() {
            out.push(self.req(
                Intent::PathComplete {
                    line,
                    head,
                    partial,
                },
                false,
                Method::Get,
                &format!("/api/fs?path={}", percent_encode(&dir)),
                None,
            ));
        }
        if std::mem::take(&mut self.away_pending) {
            out.push(self.req(Intent::Away, false, Method::Get, "/api/away", None));
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

    /// The request that marks the list of stopped reviews as seen; the event loop sends it when the owner leaves, so what stopped
    /// afterwards is what the next launch reports.
    pub fn leaving(&mut self) -> Req {
        self.req(
            Intent::AwaySeen,
            false,
            Method::Post,
            "/api/away/seen",
            Some(json!({})),
        )
    }

    /// Tab after a command that takes a review id (`/open`, `/findings`, `/fixes`, `/applyfix N`, `/undofix N`, `/release`, `/approve`,
    /// `/deliver`, `/forgetreview`): the id being typed is finished from the reviews listed. One match completes it, several fill in what
    /// they share and are listed with who each is for, none changes nothing. Returns the new line, if it changes, and a list to show.
    fn job_completion(&self) -> Option<(Option<String>, Option<Vec<String>>)> {
        const TAKES_JOB: &[&str] = &[
            "open",
            "findings",
            "fixes",
            "applyfix",
            "undofix",
            "release",
            "approve",
            "deliver",
            "forgetreview",
        ];
        let rest = self.input.strip_prefix('/')?;
        let (name, args) = rest.split_once(' ')?;
        if !TAKES_JOB.contains(&name) {
            return None;
        }
        // the id is the last word, and only if the line does not end in a space (nothing started yet is a complete start: all are offered)
        let (before, partial) = match args.rfind(' ') {
            Some(i) => (&args[..=i], &args[i + 1..]),
            None => ("", args),
        };
        // `/applyfix N` and `/undofix N` take the number first and the review second
        if matches!(name, "applyfix" | "undofix") && before.is_empty() {
            return None;
        }
        let want = partial.to_ascii_lowercase();
        let mut hits: Vec<&JobRow> = self
            .jobs
            .iter()
            .filter(|j| j.id.to_ascii_lowercase().starts_with(&want))
            .collect();
        hits.sort_by(|a, b| a.id.cmp(&b.id));
        match hits.as_slice() {
            [] => Some((None, None)),
            [one] => Some((Some(format!("/{name} {before}{} ", one.id)), None)),
            many => {
                let first = &many[0].id;
                let mut common = first.chars().count();
                for m in many {
                    common = common.min(
                        first
                            .chars()
                            .zip(m.id.chars())
                            .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                            .count(),
                    );
                }
                let done = (common > partial.chars().count()).then(|| {
                    format!(
                        "/{name} {before}{}",
                        first.chars().take(common).collect::<String>()
                    )
                });
                let list = many
                    .iter()
                    .take(12)
                    .map(|j| {
                        format!(
                            "{}  {}  for {}",
                            j.id,
                            j.state.replace('_', " "),
                            j.recipient
                        )
                    })
                    .collect();
                Some((done, Some(list)))
            }
        }
    }

    /// Tab after `/project` (a project name the last /memory listing showed, or `none`) and after `for` in `/review FOLDER for WHO` (a
    /// recipient an earlier review in this window already used). The line is left alone when nothing matches.
    fn name_completion(&self) -> Option<(Option<String>, Option<Vec<String>>)> {
        let rest = self.input.strip_prefix('/')?;
        let (name, args) = rest.split_once(' ')?;
        let (head, partial, candidates): (String, &str, Vec<String>) = match name {
            "project" if !args.contains(' ') => {
                let mut c = self.projects_seen.clone();
                c.push("none".into());
                (format!("/{name} "), args, c)
            }
            "review" => {
                let at = args.rfind(' ').map_or(0, |i| i + 1);
                let (before, partial) = args.split_at(at);
                if !before.trim_end().ends_with(" for") && before.trim() != "for" {
                    return None;
                }
                let mut c: Vec<String> = self
                    .jobs
                    .iter()
                    .map(|j| j.recipient.clone())
                    .filter(|r| !r.is_empty())
                    .collect();
                c.sort();
                c.dedup();
                (format!("/{name} {before}"), partial, c)
            }
            _ => return None,
        };
        let want = partial.to_ascii_lowercase();
        let hits: Vec<&String> = candidates
            .iter()
            .filter(|c| c.to_ascii_lowercase().starts_with(&want))
            .collect();
        match hits.as_slice() {
            [] => Some((None, None)),
            [one] => Some((Some(format!("{head}{one}")), None)),
            many => {
                let first = many[0];
                let mut common = first.chars().count();
                for m in many {
                    common = common.min(
                        first
                            .chars()
                            .zip(m.chars())
                            .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                            .count(),
                    );
                }
                let done = (common > partial.chars().count())
                    .then(|| format!("{head}{}", first.chars().take(common).collect::<String>()));
                Some((
                    done,
                    Some(many.iter().take(12).map(|s| s.to_string()).collect()),
                ))
            }
        }
    }

    /// Tab after `/correct` or `/forget`: the note id being typed is finished from the notes the last /memory listing showed. The id is the
    /// first word only; what follows `/correct ID` is the new text and is never completed.
    fn note_completion(&self) -> Option<(Option<String>, Option<Vec<String>>)> {
        let rest = self.input.strip_prefix('/')?;
        let (name, partial) = rest.split_once(' ')?;
        if !matches!(name, "correct" | "forget") || partial.contains(' ') {
            return None;
        }
        let want = partial.to_ascii_lowercase();
        let mut hits: Vec<&(String, String)> = self
            .notes_seen
            .iter()
            .filter(|(id, _)| id.to_ascii_lowercase().starts_with(&want))
            .collect();
        hits.sort();
        match hits.as_slice() {
            [] => Some((None, None)),
            [one] => Some((Some(format!("/{name} {} ", one.0)), None)),
            many => {
                let first = &many[0].0;
                let mut common = first.chars().count();
                for m in many {
                    common = common.min(
                        first
                            .chars()
                            .zip(m.0.chars())
                            .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                            .count(),
                    );
                }
                let done = (common > partial.chars().count())
                    .then(|| format!("/{name} {}", first.chars().take(common).collect::<String>()));
                let list = many
                    .iter()
                    .take(12)
                    .map(|(id, t)| format!("{id}  {t}"))
                    .collect();
                Some((done, Some(list)))
            }
        }
    }

    /// What to ask the service when Tab is pressed while the folder after /review is being typed: the folder to list, the line
    /// as it is, the text that stays in front of the last name, and the part of the name typed so far. The listing is always
    /// the service's own view of its allowed folders, never this terminal's working directory.
    fn folder_request(&self) -> Option<(String, String, String, String)> {
        let rest = self.input.strip_prefix("/review ")?;
        if rest.contains(" for ") || rest.trim_start() != rest {
            return None;
        }
        let root = self.roots.first()?.trim_end_matches('/').to_string();
        let (head, partial) = match rest.rfind('/') {
            Some(i) => (&rest[..=i], &rest[i + 1..]),
            None => ("", rest),
        };
        let dir = if head.starts_with('/') {
            head.to_string()
        } else {
            format!("{root}/{}", head.trim_start_matches("./"))
        };
        Some((
            dir,
            self.input.clone(),
            format!("/review {head}"),
            partial.to_string(),
        ))
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

    /// The card an answer is still being written into, if any. Its text changes until the answer ends.
    pub fn live_card(&self) -> Option<usize> {
        self.live.as_ref().map(|(i, _)| *i)
    }

    pub fn status_line(&self) -> String {
        let ready = match self.ready {
            Some(true) => "ready",
            Some(false) => "setup needed (/status)",
            None => "checking",
        };
        let running = self.watching.len();
        format!(
            "{} · {} · {} review(s){}{}{}",
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
            },
            match &self.chat_project {
                Some(p) => format!(" · project {}", clean(p)),
                None => String::new(),
            },
            match &self.context_meter {
                Some(m) => format!(" · {m}"),
                None => String::new(),
            }
        )
    }

    pub fn handle(&mut self, ev: Event) -> Vec<Req> {
        match ev {
            Event::Key(k) => {
                self.waiting = None;
                self.on_key(k)
            }
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
            Event::Focus(on) => {
                self.focused = Some(on);
                if on {
                    self.waiting = None;
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
                // an approval view must not stay open for something already decided in the desktop window or another terminal
                if let Mode::Approval(v) = &self.mode {
                    let deciding = self
                        .pending
                        .values()
                        .any(|i| matches!(i, Intent::ApproveSubmit | Intent::Deny));
                    if !self.approval_watch_inflight && !deciding {
                        self.approval_watch_inflight = true;
                        let aid = v.aid.clone();
                        out.push(self.req(
                            Intent::ApprovalWatch(aid),
                            true,
                            Method::Get,
                            "/admin/approvals",
                            None,
                        ));
                    }
                }
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
            KeyCode::Tab => p.toggle_scope(),
            KeyCode::Left => p.preview = None,
            KeyCode::Right => {
                let Some(it) = p.current().cloned() else {
                    return Vec::new();
                };
                if it.kind != ItemKind::Chat {
                    return Vec::new();
                }
                if p.preview.as_ref().is_some_and(|(id, _)| *id == it.id) {
                    p.preview = None;
                    return Vec::new();
                }
                p.preview = None;
                return vec![self.req(
                    Intent::PickerPreview(it.id.clone()),
                    false,
                    Method::Get,
                    &format!("/api/sessions/{}", it.id),
                    None,
                )];
            }
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
                Delete::NotAllowed => {
                    p.error = Some(
                        if p.current().is_some_and(|it| it.kind == ItemKind::Message) {
                            "Your messages are not forgotten here; Enter goes back to one."
                        } else {
                            "Reviews are not removed here."
                        }
                        .into(),
                    )
                }
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
                    ItemKind::Message => match it.id.parse::<usize>() {
                        Ok(n) => self.rewind_to(n),
                        Err(_) => Vec::new(),
                    },
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

    /// Takes back what the editor returned. Control characters are removed like in any paste; a failure or an empty
    /// result leaves the composer as it was.
    pub fn finish_edit(&mut self, result: Result<String, String>) {
        match result {
            Ok(text) => {
                let text = clean(&text.replace("\r\n", "\n"))
                    .trim_end_matches('\n')
                    .to_string();
                // an emptied file is a cancel, as with `git commit`: the draft is kept
                if text.trim().is_empty() {
                    self.say(
                        Kind::Info,
                        "the editor returned nothing, so your message is unchanged",
                    );
                    return;
                }
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
    fn history_may_replace(&self) -> bool {
        self.input.is_empty()
            || self
                .hist_pos
                .is_some_and(|p| self.history.get(p).is_some_and(|h| *h == self.input))
    }

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
                        // a release past its deadline cannot be approved: the service refuses it, and this does not even ask
                        if v.statement["expires"]
                            .as_i64()
                            .is_some_and(|e| now_secs() >= e)
                        {
                            v.error = Some("This release has expired. Nothing was approved. Refuse it with Esc and run the review again.".into());
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
                let review = c.review.clone();
                let choose = c.choose.clone();
                let copy = c.copy.clone();
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
                    KeyCode::Char('y') | KeyCode::Char('Y') if !ctrl && copy.is_some() => {
                        self.mode = Mode::Normal;
                        self.clipboard_request = copy;
                        self.say(Kind::Ok, "Copied.");
                        Vec::new()
                    }
                    KeyCode::Char('y') | KeyCode::Char('Y') if !ctrl && choose.is_some() => {
                        self.mode = Mode::Normal;
                        let (keep, replaced) = choose.expect("checked");
                        vec![self.req(
                            Intent::ChooseNote,
                            false,
                            Method::Post,
                            "/api/memories/conflicts/keep",
                            Some(json!({"keep": keep, "replaced": replaced})),
                        )]
                    }
                    KeyCode::Char('y') | KeyCode::Char('Y') if !ctrl && review.is_some() => {
                        self.mode = Mode::Normal;
                        let job = review.expect("checked");
                        vec![self.req(
                            Intent::ForgetReview,
                            false,
                            Method::Post,
                            &format!("/api/memories/sources/{job}/forget"),
                            Some(json!({"reason": "you asked to forget this review in the terminal"})),
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
            // history only replaces an empty composer or a line it put there itself: Up must never throw away a draft
            KeyCode::Up if !self.history_may_replace() => {}
            KeyCode::Down if !self.history_may_replace() => {}
            KeyCode::Up => {
                if !self.history.is_empty() {
                    // an emptied composer starts from the newest entry again, wherever the last walk stopped
                    if self.input.is_empty() {
                        self.hist_pos = None;
                    }
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
                } else if let Some(c) = self
                    .job_completion()
                    .or_else(|| self.note_completion())
                    .or_else(|| self.name_completion())
                {
                    // a review id after a command that takes one: finished from the reviews this screen already knows
                    if let Some(done) = c.0 {
                        self.input = done;
                        self.cursor = self.input.chars().count();
                    }
                    if let Some(list) = c.1 {
                        self.push(Kind::Info, Some("Matches"), list);
                    }
                } else {
                    self.path_request = self.folder_request();
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
                } else if self.input.is_empty() {
                    // Esc twice on an empty box: choose a message to go back to
                    if self.esc_armed > 0 {
                        self.esc_armed = 0;
                        return self.rewind_picker();
                    }
                    self.esc_armed = ESC_ARM_TICKS;
                } else {
                    self.input.clear();
                    self.cursor = 0;
                }
            }
            KeyCode::Char('s') if ctrl => {
                self.stash_input();
            }
            KeyCode::Char('r') if ctrl => self.pop_stash(),
            KeyCode::Char('o') if ctrl => {
                self.expand_all = !self.expand_all;
                self.scroll_back = 0;
            }
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
                        "Ctrl-O                         show long results whole, or fold them again".into(),
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
                self.context_meter = None;
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
            Command::Project(arg) => {
                match arg {
                    None => {
                        let line = match &self.chat_project {
                            Some(p) => format!("This conversation is about the project \"{}\": notes saved for it are offered, and no other project's. /project none clears it.", clean(p)),
                            None => "This conversation has no project, so only notes that apply everywhere are offered. /project NAME sets one.".to_string(),
                        };
                        self.say(Kind::Info, line);
                    }
                    Some(name) if name.is_empty() => {
                        self.chat_project = None;
                        self.say(
                            Kind::Ok,
                            "No project: only notes that apply everywhere are offered.",
                        );
                    }
                    Some(name) => {
                        self.say(Kind::Ok, format!("Project set to \"{}\": its notes are offered in this conversation.", clean(&name)));
                        self.chat_project = Some(name);
                    }
                }
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
            Command::Search(words, recency) => {
                let mut body = json!({"query": words});
                if let Some(w) = recency {
                    body["recency"] = json!(w);
                }
                vec![self.req(
                    Intent::SearchPropose,
                    false,
                    Method::Post,
                    "/api/search/propose",
                    Some(body),
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
            Command::Quiet => {
                self.quiet = !self.quiet;
                self.say(
                    Kind::Info,
                    if self.quiet {
                        "Quiet: a review shows only how it ended. /quiet again shows every step."
                    } else {
                        "Every step of a review is shown. /quiet shows only how it ended."
                    },
                );
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
                    Some(match &self.chat_project {
                        Some(p) => json!({"title": "", "project": p}),
                        None => json!({"title": ""}),
                    }),
                )]
            }
            Command::Branch(at) => {
                let Some(id) = self.session.clone() else {
                    self.say(
                        Kind::Info,
                        "Only a kept conversation can be branched. /keep saves this one first.",
                    );
                    return Vec::new();
                };
                let total = self.chat_history.len();
                let at = at.unwrap_or(total);
                if at > total {
                    self.say(
                        Kind::Error,
                        format!(
                            "This conversation has {total} message(s); choose a number up to that."
                        ),
                    );
                    return Vec::new();
                }
                vec![self.req(
                    Intent::Branch,
                    false,
                    Method::Post,
                    &format!("/api/sessions/{id}/fork"),
                    Some(json!({"at": at})),
                )]
            }
            Command::Rewind(n) => self.rewind_to(n),
            Command::RewindPick => self.rewind_picker(),
            Command::Resume(id) => self.open_chat_by_id(&id),
            Command::Sessions => {
                let mut p = crate::picker::Picker::new("Conversations and reviews");
                p.scope_to(self.chat_project.clone());
                for j in &self.jobs {
                    p.items.push(crate::picker::Item {
                        id: j.id.clone(),
                        label: format!("Review for {}", j.recipient),
                        detail: j.state.clone(),
                        when: 0,
                        kind: crate::picker::ItemKind::Review,
                        project: None,
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
                    review: None,
                    choose: None,
                    copy: None,
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
                    review: None,
                    choose: None,
                    copy: None,
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
            Command::Similar => {
                vec![self.req(
                    Intent::Similar,
                    false,
                    Method::Get,
                    "/api/memories/similar",
                    None,
                )]
            }
            Command::Ask { folder, question } => {
                if self.chat_waiting {
                    self.say(Kind::Warn, "Still waiting for the last answer.");
                    return Vec::new();
                }
                self.chat_waiting = true;
                self.esc_armed = 0;
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
            Command::Usage(days) => {
                let to = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs() as i64)
                    + 1;
                let from = to - i64::from(days) * 86_400;
                vec![self.req(
                    Intent::UsageShow,
                    false,
                    Method::Get,
                    &format!("/api/usage?from={from}&to={to}"),
                    None,
                )]
            }
            Command::Transcript(words) => {
                let (total, hits) = self.search_transcript(&words);
                if total == 0 {
                    self.say(Kind::Info, "No line has all of those words.");
                } else {
                    let mut lines = hits;
                    lines.insert(0, format!("{total} lines hold all of those words (the first 30 are shown); each word is marked [[like this]]."));
                    self.push(Kind::Info, Some(SEARCH_TITLE), lines);
                }
                Vec::new()
            }
            Command::Copy(all) => {
                let text = if all {
                    Some(self.transcript_text())
                } else {
                    self.cards
                        .iter()
                        .rev()
                        .find(|c| c.title.as_deref() == Some("Assistant"))
                        .map(|c| c.lines.join("\n"))
                };
                match text {
                    None => self.say(Kind::Info, "There is no reply to copy yet."),
                    Some(t) if osc52(&t).is_none() => self.say(Kind::Error, "That is too large for the clipboard; /export NAME writes it to a file instead."),
                    Some(t) => {
                        self.mode = Mode::Confirm(ConfirmView {
                            prompt: format!("Copy {} characters to the system clipboard? Other programs can read the clipboard. [y/N]", t.chars().count()),
                            forget_id: String::new(),
                            review: None,
                            choose: None,
                            copy: Some(t),
                            apply: None,
                            undo: false,
                            web: None,
                        });
                    }
                }
                Vec::new()
            }
            Command::Export(name) => {
                let text = self.transcript_text();
                vec![self.req(
                    Intent::ExportTranscript,
                    false,
                    Method::Post,
                    "/api/exports/transcript",
                    Some(json!({"name": name, "text": text})),
                )]
            }
            Command::Find(words) => {
                let q: String = words
                    .bytes()
                    .map(|b| {
                        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' {
                            (b as char).to_string()
                        } else {
                            format!("%{b:02X}")
                        }
                    })
                    .collect();
                vec![self.req(
                    Intent::FindChats,
                    false,
                    Method::Get,
                    &format!("/api/sessions/search?q={q}"),
                    None,
                )]
            }
            Command::RenameChat { id, title } => vec![self.req(
                Intent::RenameChat,
                false,
                Method::Post,
                &format!("/api/sessions/{id}/rename"),
                Some(json!({"title": title})),
            )],
            Command::Conflicts => vec![self.req(
                Intent::ConflictsList,
                false,
                Method::Get,
                "/api/memories/conflicts",
                None,
            )],
            Command::Choose { keep, replaced } => {
                self.mode = Mode::Confirm(ConfirmView {
                    prompt: format!("Keep {keep} and make {replaced} its earlier version? Nothing is deleted. [y/N]"),
                    forget_id: String::new(),
                    review: None,
                    choose: Some((keep, replaced)), copy: None,
                    apply: None,
                    undo: false,
                    web: None,
                });
                Vec::new()
            }
            Command::BothRight { a, b } => vec![self.req(
                Intent::BothRight,
                false,
                Method::Post,
                "/api/memories/conflicts/both",
                Some(json!({"a": a, "b": b})),
            )],
            Command::Instructions(action) => {
                use crate::cmd::InstructionsAction as A;
                match action {
                    A::Show => vec![self.req(
                        Intent::InstructionsShow,
                        false,
                        Method::Get,
                        "/api/instructions",
                        None,
                    )],
                    A::Set { project, text } => {
                        let body = match project {
                            Some(p) => json!({"project": p, "text": text}),
                            None => json!({"text": text}),
                        };
                        vec![self.req(
                            Intent::InstructionsSet,
                            false,
                            Method::Post,
                            "/api/instructions",
                            Some(body),
                        )]
                    }
                    A::Clear(project) => {
                        let body = match project {
                            Some(p) => json!({"project": p, "text": ""}),
                            None => json!({"text": ""}),
                        };
                        vec![self.req(
                            Intent::InstructionsSet,
                            false,
                            Method::Post,
                            "/api/instructions",
                            Some(body),
                        )]
                    }
                }
            }
            Command::ForgetReview(job) => vec![self.req(
                Intent::ReviewFootprint(job.clone()),
                false,
                Method::Get,
                &format!("/api/memories/sources/{job}"),
                None,
            )],
            Command::Forget(id) => {
                self.mode = Mode::Confirm(ConfirmView { prompt: format!("Forget {id} for good? The text is deleted and will not be saved again. [y/N]"), forget_id: id, review: None, choose: None, copy: None, apply: None, undo: false, web: None });
                Vec::new()
            }
            Command::Pii(a) => {
                use crate::cmd::PiiAction;
                match a {
                    PiiAction::Show => {
                        vec![self.req(Intent::PiiShow, false, Method::Get, "/api/pii-allow", None)]
                    }
                    PiiAction::Add(e) => vec![self.req(
                        Intent::PiiEdit,
                        false,
                        Method::Post,
                        "/api/pii-allow",
                        Some(json!({"add": e})),
                    )],
                    PiiAction::Remove(e) => vec![self.req(
                        Intent::PiiEdit,
                        false,
                        Method::Post,
                        "/api/pii-allow",
                        Some(json!({"remove": e})),
                    )],
                }
            }
            Command::Aside(text) => {
                if self.chat_waiting {
                    self.say(
                        Kind::Warn,
                        "An answer is still arriving. Ask the aside when it is finished.",
                    );
                    return Vec::new();
                }
                if text.chars().count() > 4000 {
                    self.say(
                        Kind::Warn,
                        "That question is over 4000 characters; shorten it.",
                    );
                    return Vec::new();
                }
                let start = self.chat_history.len().saturating_sub(18);
                let history: Vec<Value> = self.chat_history[start..]
                    .iter()
                    .map(|(r, c)| json!({"role": r, "content": c}))
                    .collect();
                // no session id is sent, so the service has nothing to append the exchange to
                let mut body = json!({"message": text, "history": history});
                if let Some(p) = &self.chat_project {
                    body["project"] = json!(p);
                }
                vec![self.req(Intent::Aside, false, Method::Post, "/api/chat", Some(body))]
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
        self.esc_armed = 0;
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
                    // an empty name tells a kept conversation to forget its project
                    body["project"] = json!(self.chat_project.clone().unwrap_or_default());
                } else if let Some(p) = &self.chat_project {
                    body["project"] = json!(p);
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

    /// The conversation on the screen as plain text: what the owner typed after "You:", then each answer or message under its
    /// title. The commands typed to read or write it are not part of it.
    pub fn transcript_text(&self) -> String {
        let mut blocks: Vec<String> = Vec::new();
        for c in &self.cards {
            match (&c.kind, &c.title) {
                (Kind::User, _) => {
                    let line = c.lines.join("\n");
                    if line.trim_start().starts_with('/') {
                        continue;
                    }
                    blocks.push(format!("You: {line}"));
                }
                // what a search found is about the conversation, not part of it
                (_, Some(t)) if t == SEARCH_TITLE || t == ASIDE_TITLE => {}
                (_, Some(t)) => blocks.push(format!("{t}:\n{}", c.lines.join("\n"))),
                (_, None) => blocks.push(c.lines.join("\n")),
            }
        }
        blocks.join("\n\n") + "\n"
    }

    /// Lines of the transcript that hold every one of `words` (any case), with each word's first place marked [[like this]].
    fn search_transcript(&self, words: &str) -> (usize, Vec<String>) {
        let words: Vec<String> = words
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .take(6)
            .collect();
        let mut hits = Vec::new();
        let mut total = 0;
        for (n, line) in self.transcript_text().lines().enumerate() {
            if line.trim().is_empty() || line.trim_start().to_lowercase().starts_with("you: /") {
                continue;
            }
            let low = line.to_lowercase();
            if words.is_empty() || !words.iter().all(|w| low.contains(w.as_str())) {
                continue;
            }
            total += 1;
            if hits.len() < 30 {
                // mark the first place each word appears, working on characters so lower-casing never shifts the text
                let chars: Vec<char> = line.chars().collect();
                let low_chars: Vec<char> = low.chars().collect();
                let mut marks: Vec<(usize, usize)> = Vec::new();
                for w in &words {
                    let wc: Vec<char> = w.chars().collect();
                    if let Some(at) =
                        (0..low_chars.len().saturating_sub(wc.len().saturating_sub(1)))
                            .find(|i| low_chars[*i..].starts_with(&wc))
                    {
                        marks.push((at, at + wc.len()));
                    }
                }
                marks.sort();
                marks.dedup();
                let mut out = String::new();
                let mut at = 0;
                for (a, b) in marks {
                    if a < at || b > chars.len() {
                        continue;
                    }
                    out.extend(&chars[at..a]);
                    out.push_str("[[");
                    out.extend(&chars[a..b]);
                    out.push_str("]]");
                    at = b;
                }
                out.extend(&chars[at..]);
                hits.push(format!("{:>5}: {}", n + 1, clean(&out)));
            }
        }
        (total, hits)
    }

    fn scrub_last_remember(&mut self) {
        self.scrub_last(
            "/remember",
            "/remember [text hidden: it looked like a secret and was not stored]",
        );
    }

    /// Replaces the last typed command that starts with `prefix` in the transcript, and drops it from the command history.
    fn scrub_last(&mut self, prefix: &str, replacement: &str) {
        if let Some(c) = self.cards.iter_mut().rev().find(|c| {
            c.kind == Kind::User
                && c.lines
                    .first()
                    .is_some_and(|l| l.trim_start().to_ascii_lowercase().starts_with(prefix))
        }) {
            c.lines = vec![replacement.to_string()];
        }
        if let Some(i) = self
            .history
            .iter()
            .rposition(|h| h.trim_start().to_ascii_lowercase().starts_with(prefix))
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
                if matches!(intent, Intent::ApprovalWatch(_)) {
                    // a background look; the next tick tries again and other requests report an unreachable service
                    self.approval_watch_inflight = false;
                    return Vec::new();
                }
                self.say(
                    Kind::Error,
                    format!("Could not reach the service: {}", clean(&e)),
                );
                if !std::mem::replace(&mut self.service_down, true) {
                    self.attention = Some(crate::attention::Attention::ServiceDown);
                }
                return Vec::new();
            }
        };
        // any answer at all means the service is there again
        self.service_down = false;
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
                                        if c["ok"] == true {
                                            "[ok]  "
                                        } else if c["required"] == false {
                                            "[optional]"
                                        } else {
                                            "[todo]"
                                        },
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
            Intent::PathComplete {
                line,
                head,
                partial,
            } => {
                if !ok || self.input != line {
                    return Vec::new();
                }
                let want = partial.to_ascii_lowercase();
                let mut names: Vec<String> = body["dirs"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|d| d["name"].as_str())
                            .filter(|n| {
                                !n.is_empty()
                                    && clean(n) == *n
                                    && !n.chars().any(char::is_whitespace)
                                    && n.to_ascii_lowercase().starts_with(&want)
                            })
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();
                names.sort();
                match names.as_slice() {
                    [] => {}
                    [one] => {
                        self.input = format!("{head}{one}/");
                        self.cursor = self.input.chars().count();
                    }
                    many => {
                        let first = &many[0];
                        let mut common = first.chars().count();
                        for m in many {
                            common = common.min(
                                first
                                    .chars()
                                    .zip(m.chars())
                                    .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
                                    .count(),
                            );
                        }
                        if common > partial.chars().count() {
                            let stem: String = first.chars().take(common).collect();
                            self.input = format!("{head}{stem}");
                            self.cursor = self.input.chars().count();
                        }
                        let shown: Vec<String> =
                            many.iter().take(12).map(|n| format!("{n}/")).collect();
                        let more = many.len().saturating_sub(12);
                        let mut lines = vec![shown.join("  ")];
                        if more > 0 {
                            lines.push(format!("and {more} more"));
                        }
                        self.push(Kind::Info, Some("Folders"), lines);
                    }
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
                        if stage == "queued" || stage == "finished" || self.quiet {
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
                    let what = if state == "ready" {
                        crate::attention::Attention::ReviewReady
                    } else {
                        crate::attention::Attention::ReviewStopped
                    };
                    self.attention = Some(what);
                    self.waiting = Some(what);
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
                    if !f["reproducer"].as_str().unwrap_or("").is_empty() {
                        // a service that does not say reads as python, as stored reviews from before the field did
                        let language = match f["language"].as_str() {
                            None | Some("python") => "Python".to_string(),
                            Some("javascript") => "JavaScript".to_string(),
                            Some(_) => "unknown language".to_string(),
                        };
                        lines.push(format!("Checked with {language}"));
                    }
                    lines.extend(text_lines(f["explanation"].as_str().unwrap_or("")));
                    let found: Vec<String> = f["pattern_matches"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .take(5)
                        .map(|m| {
                            format!(
                                "  line {}: {}",
                                m["line"],
                                clean(m["what"].as_str().unwrap_or(""))
                            )
                        })
                        .collect();
                    if !found.is_empty() {
                        lines.push("Pattern rules also matched in this file (code of that shape; not a reproduction):".into());
                        lines.extend(found);
                    }
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
                let scan = &body["pattern_scan"];
                match scan["status"].as_str() {
                    Some("ran") => self.say(
                        Kind::Info,
                        format!(
                            "Pattern rules ran over {} files and matched {} place(s). A match is not a reproduction, and no match does not mean the code is safe.",
                            scan["files"], scan["matches"]
                        ),
                    ),
                    Some("failed") => self.say(
                        Kind::Warn,
                        format!(
                            "Pattern rules could not run: {}",
                            clean(scan["reason"].as_str().unwrap_or("unknown reason"))
                        ),
                    ),
                    _ => {}
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
            Intent::ApprovalWatch(aid) => {
                self.approval_watch_inflight = false;
                if !ok {
                    return Vec::new();
                }
                let waiting = body["approvals"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|x| x["id"].as_str() == Some(aid.as_str())));
                // an answer to our own approve or refuse is on its way: it will say what happened
                let deciding = self
                    .pending
                    .values()
                    .any(|i| matches!(i, Intent::ApproveSubmit | Intent::Deny));
                let still_open =
                    !deciding && matches!(&self.mode, Mode::Approval(v) if v.aid == aid);
                if still_open && !waiting {
                    self.mode = Mode::Normal;
                    self.say(
                        Kind::Warn,
                        "Approval closed: it was decided somewhere else (the desktop window or another terminal) or has expired, so there is nothing to approve here. /reviews shows where the review stands.",
                    );
                }
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
                self.notes_seen = list
                    .iter()
                    .take(200)
                    .filter_map(|m| {
                        Some((
                            clean(m["id"].as_str()?),
                            clean(m["text"].as_str().unwrap_or(""))
                                .chars()
                                .take(40)
                                .collect(),
                        ))
                    })
                    .collect();
                let mut projects: Vec<String> = list
                    .iter()
                    .filter_map(|m| m["project"].as_str().map(clean))
                    .filter(|p| !p.is_empty())
                    .collect();
                projects.sort();
                projects.dedup();
                projects.truncate(100);
                self.projects_seen = projects;
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
            Intent::Similar => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let groups = body["groups"].as_array().cloned().unwrap_or_default();
                let scanned = body["scanned"].as_u64().unwrap_or(0);
                let mut lines: Vec<String> = Vec::new();
                for g in &groups {
                    let notes = g.as_array().cloned().unwrap_or_default();
                    lines.push(format!("{} notes say almost the same:", notes.len()));
                    for m in &notes {
                        lines.push(format!(
                            "  {}{}  {}",
                            clean(m["id"].as_str().unwrap_or("?")),
                            m["project"]
                                .as_str()
                                .map(|p| format!(" [project {}]", clean(p)))
                                .unwrap_or_default(),
                            clean(m["text"].as_str().unwrap_or(""))
                        ));
                    }
                }
                if groups.is_empty() {
                    lines.push(format!(
                        "No notes say almost the same thing ({scanned} notes compared)."
                    ));
                } else {
                    lines.push(format!(
                        "{scanned} notes compared. Nothing was merged or forgotten. /forget ID drops a copy you do not want."
                    ));
                }
                if body["complete"] == false {
                    lines.push(format!(
                        "There are more notes than that: only the newest {scanned} were compared."
                    ));
                }
                self.push(Kind::Info, Some("Similar notes"), lines);
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
                    let state = f["state"].as_str().unwrap_or("");
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
                    let diff: Vec<String> = f["diff"]
                        .as_str()
                        .unwrap_or("")
                        .lines()
                        .map(clean)
                        .collect();
                    lines.extend(crate::diff::number(&diff).into_iter().take(60));
                    lines.push(String::new());
                    lines.push(if state == "attention" {
                        format!(
                            "Needs your attention: these files are not as Kakurega left them: {}. Make each one match its original or its fixed text again, or remove whatever stops it being written, then run /fixes again. Nothing is changed until then.",
                            clean(f["attention"].as_str().unwrap_or(""))
                        )
                    } else if state == "applying" {
                        "A change to your files is being made right now. Run /fixes again in a moment.".to_string()
                    } else if f["applied"] == true {
                        "Already applied.".to_string()
                    } else if verified {
                        format!("The reproducer stops reproducing. Your project's tests were not run. /applyfix {n} to apply.")
                    } else {
                        format!("NOT accepted: {}", clean(&f["status"].as_str().unwrap_or("").chars().take(160).collect::<String>()))
                    });
                    self.push(
                        if verified && state != "attention" {
                            Kind::Ok
                        } else {
                            Kind::Warn
                        },
                        Some(&format!("Fix {n}")),
                        lines,
                    );
                }
                Vec::new()
            }
            Intent::UndoFix(file) => {
                self.say(
                    if ok { Kind::Ok } else { Kind::Error },
                    if ok && body["kept_later_edits"] == true {
                        format!("Restored {file}. Your later edits to it were kept.")
                    } else if ok {
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
            Intent::UsageShow => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let routes = body["routes"].as_array().cloned().unwrap_or_default();
                if routes.is_empty() {
                    self.say(Kind::Info, "No replies in these days.");
                    return Vec::new();
                }
                let mut lines: Vec<String> = Vec::new();
                for r in &routes {
                    let star = if r["measured_turns"].as_i64() < r["turns"].as_i64() {
                        "*"
                    } else {
                        ""
                    };
                    lines.push(format!(
                        "{}{}  {} replies, {}{star} tokens read, {} written, {} ms a reply, {} tokens a second",
                        clean(r["route"].as_str().unwrap_or("?")),
                        if r["simulated"] == true { " (test model)" } else { "" },
                        r["turns"].as_i64().unwrap_or(0),
                        r["prompt_tokens"].as_i64().unwrap_or(0),
                        r["reply_tokens"].as_i64().unwrap_or(0),
                        r["avg_ms"].as_i64().unwrap_or(0),
                        r["reply_tokens_per_second"].as_f64().unwrap_or(0.0)
                    ));
                }
                lines.push(format!(
                    "Total  {} replies, {} tokens read, {} written. * part of this is an estimate; no prices are known, so no cost is shown.",
                    body["total"]["turns"].as_i64().unwrap_or(0),
                    body["total"]["prompt_tokens"].as_i64().unwrap_or(0),
                    body["total"]["reply_tokens"].as_i64().unwrap_or(0)
                ));
                self.push(Kind::Info, Some("Model usage"), lines);
                Vec::new()
            }
            Intent::ExportTranscript => {
                if ok {
                    self.say(
                        Kind::Ok,
                        format!("Written to {}", clean(body["path"].as_str().unwrap_or("?"))),
                    );
                } else {
                    self.say(Kind::Error, err_text(status, &body));
                }
                Vec::new()
            }
            Intent::FindChats => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let results = body["results"].as_array().cloned().unwrap_or_default();
                if results.is_empty() {
                    self.say(Kind::Info, "No kept conversation has all of those words.");
                    return Vec::new();
                }
                let mut lines: Vec<String> = Vec::new();
                for r in &results {
                    lines.push(format!(
                        "{}  {}",
                        clean(r["id"].as_str().unwrap_or("?")),
                        clean(r["title"].as_str().unwrap_or(""))
                    ));
                    for sn in r["snippets"].as_array().cloned().unwrap_or_default() {
                        lines.push(format!(
                            "    {}: {}",
                            clean(sn["role"].as_str().unwrap_or("?")),
                            clean(sn["text"].as_str().unwrap_or(""))
                        ));
                    }
                }
                lines.push("/resume ID opens one; /rename ID TITLE renames it.".into());
                self.push(Kind::Info, Some("Matching conversations"), lines);
                Vec::new()
            }
            Intent::RenameChat => {
                self.say(
                    if ok { Kind::Ok } else { Kind::Error },
                    if ok {
                        "Renamed.".to_string()
                    } else {
                        err_text(status, &body)
                    },
                );
                Vec::new()
            }
            Intent::ConflictsList => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let pairs = body["pairs"].as_array().cloned().unwrap_or_default();
                if pairs.is_empty() {
                    self.say(Kind::Info, "No notes disagree.");
                    return Vec::new();
                }
                let mut lines: Vec<String> = Vec::new();
                for p in &pairs {
                    let why = match (p["why"].as_str(), p["subject"].as_str()) {
                        (Some("same_subject"), Some(s)) => format!("both about \"{}\"", clean(s)),
                        _ => "almost the same words, a different claim".to_string(),
                    };
                    lines.push(format!("Disagree ({why}):"));
                    for side in ["a", "b"] {
                        lines.push(format!(
                            "  {}  {}",
                            clean(p[side]["id"].as_str().unwrap_or("?")),
                            clean(p[side]["text"].as_str().unwrap_or(""))
                        ));
                    }
                }
                lines.push("/choose KEEP_ID OVER_ID keeps one (the other becomes its earlier version); /bothright ID ID keeps both.".into());
                self.push(Kind::Info, Some("Notes that disagree"), lines);
                Vec::new()
            }
            Intent::ChooseNote => {
                self.say(
                    if ok { Kind::Ok } else { Kind::Error },
                    if ok {
                        "Kept. The other note is now an earlier version.".to_string()
                    } else {
                        err_text(status, &body)
                    },
                );
                Vec::new()
            }
            Intent::BothRight => {
                self.say(
                    if ok { Kind::Ok } else { Kind::Error },
                    if ok {
                        "Both notes stay. They will not be listed together again.".to_string()
                    } else {
                        err_text(status, &body)
                    },
                );
                Vec::new()
            }
            Intent::InstructionsShow => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let mut lines: Vec<String> = Vec::new();
                if let Some(g) = body["global"]["text"].as_str() {
                    lines.push("For every conversation:".into());
                    lines.extend(g.lines().map(|l| format!("  {}", clean(l))));
                }
                for p in body["projects"].as_array().cloned().unwrap_or_default() {
                    lines.push(format!(
                        "For the project {}:",
                        clean(p["project"].as_str().unwrap_or("?"))
                    ));
                    lines.extend(
                        p["text"]
                            .as_str()
                            .unwrap_or("")
                            .lines()
                            .map(|l| format!("  {}", clean(l))),
                    );
                }
                if lines.is_empty() {
                    self.say(Kind::Info, "No standing instructions. /instructions set TEXT writes one; /instructions set in PROJECT: TEXT keeps it for one project.");
                } else {
                    self.push(Kind::Info, Some("Your standing instructions"), lines);
                }
                Vec::new()
            }
            Intent::InstructionsSet => {
                if ok {
                    self.say(
                        Kind::Ok,
                        if body["removed"] == true {
                            "Removed."
                        } else if body["saved"] == true {
                            "Saved."
                        } else {
                            "Nothing was set there."
                        },
                    );
                } else {
                    if body["error"]["message"]
                        .as_str()
                        .is_some_and(|m| m.contains("looks like a secret"))
                    {
                        self.scrub_last("/instructions", "/instructions [text hidden: it looked like a secret and was not saved]");
                    }
                    self.say(Kind::Error, err_text(status, &body));
                }
                Vec::new()
            }
            Intent::ReviewFootprint(job) => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let notes = body["notes"].as_u64().unwrap_or(0);
                let waiting = body["suggestions"].as_u64().unwrap_or(0);
                if notes + waiting == 0 {
                    self.say(Kind::Info, format!("Review {job} left nothing in memory."));
                    return Vec::new();
                }
                self.mode = Mode::Confirm(ConfirmView {
                    prompt: format!("Forget {notes} notes and {waiting} waiting suggestions that review {job} left? The text is deleted and will not be saved again. [y/N]"),
                    forget_id: String::new(),
                    review: Some(job), choose: None, copy: None,
                    apply: None,
                    undo: false,
                    web: None,
                });
                Vec::new()
            }
            Intent::ForgetReview => {
                self.say(
                    if ok { Kind::Ok } else { Kind::Error },
                    if ok {
                        format!(
                            "Forgot {} notes and {} suggestions. The text is deleted and will not be saved again.",
                            body["notes"].as_u64().unwrap_or(0),
                            body["suggestions"].as_u64().unwrap_or(0)
                        )
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
                    if let Some(w) = body["recency"].as_str() {
                        lines.push(format!("Only results from the last {}.", clean(w)));
                    }
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
                    review: None,
                    choose: None,
                    copy: None,
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
                    if let Some(why) = body["empty_because"].as_str() {
                        lines.push(clean(why));
                    }
                    let failed: Vec<String> = body["engines_failed"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|e| e.as_str())
                        .map(clean)
                        .collect();
                    if !failed.is_empty() {
                        lines.push(format!(
                            "Engines that did not answer: {}",
                            failed.join(", ")
                        ));
                    }
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
            Intent::PiiShow => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let list: Vec<String> = body["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|e| format!("  {}", clean(e["entry"].as_str().unwrap_or(""))))
                    .collect();
                let mut lines = if list.is_empty() {
                    vec!["Nothing is on your list: every value that looks like personal data is counted in release notices.".to_string()]
                } else {
                    let mut v = vec![format!(
                        "{} value(s) you said are not personal data:",
                        list.len()
                    )];
                    v.extend(list);
                    v
                };
                lines.push("Add one with /pii add ENTRY (an address, @domain.com, or a pattern with *); take one off with /pii remove ENTRY. This changes only the notice before a release; nothing is sent or withheld differently.".into());
                self.push(Kind::Info, Some("Personal data allowlist"), lines);
                Vec::new()
            }
            Intent::PiiEdit => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                } else if body["changed"] == true {
                    self.say(
                        Kind::Ok,
                        format!("Done. The list now has {} entr(ies).", body["entries"]),
                    );
                } else {
                    self.say(
                        Kind::Info,
                        "No change: that entry was already there, or was not on the list.",
                    );
                }
                Vec::new()
            }
            Intent::Aside => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let mut lines = text_lines(body["reply"].as_str().unwrap_or(""));
                lines.push(String::new());
                lines.push("Aside: not part of this conversation and not saved.".into());
                self.push(Kind::Info, Some(ASIDE_TITLE), lines);
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
                if let Some(p) = body["project"].as_str() {
                    foot[0].push_str(&format!(" · project {}", clean(p)));
                }
                if let Some(estimated) = body["usage"]["context_tokens_estimate"].as_u64() {
                    let u = &body["usage"];
                    // the model's own count when it gave one, otherwise the estimate; the window only if the service sent it
                    let counted = u["estimate"] == false
                        && u["context_tokens"].as_u64().is_some_and(|n| n > 0);
                    let n = u["context_tokens"].as_u64().unwrap_or(estimated);
                    self.context_meter =
                        Some(context_meter(n, u["context_window"].as_u64(), counted));
                    let how = if counted {
                        ", counted by the model"
                    } else {
                        ""
                    };
                    match u["context_window"].as_u64().filter(|w| *w > 0) {
                        Some(max) => {
                            let pct = (n.saturating_mul(100) / max).min(999);
                            foot.push(if counted {
                                format!("{n} of {max} tokens of context used ({pct}%{how})")
                            } else {
                                format!("about {n} of {max} tokens of context used ({pct}%)")
                            });
                            if pct >= 80 {
                                foot.push("This conversation is close to the model's limit. Start a new conversation soon.".into());
                            }
                        }
                        None if counted => {
                            foot.push(format!("{n} tokens of context sent (counted by the model)"))
                        }
                        None => foot.push(format!("about {n} tokens of context sent")),
                    }
                    let summarised = body["usage"]["history_turns_summarized"]
                        .as_u64()
                        .unwrap_or(0);
                    if summarised > 0 {
                        foot.push(if body["usage"]["compacted_now"] == true {
                            format!("The oldest {summarised} messages were just summarised to make room. You can still open them in full.")
                        } else {
                            format!("The oldest {summarised} messages are replaced by a summary. You can still open them in full.")
                        });
                    }
                    if let Some(text) = body["usage"]["summary_text"]
                        .as_str()
                        .filter(|_| summarised > 0)
                    {
                        foot.push(format!(
                            "Summary sent in their place: {}",
                            clean(text).chars().take(600).collect::<String>()
                        ));
                    }
                    if !body["usage"]["compaction_failed"].is_null() {
                        foot.push("The oldest messages could not be summarised, so they were left out of this answer.".into());
                    }
                    let dropped = body["usage"]["history_turns_dropped"].as_u64().unwrap_or(0);
                    if dropped > 0 {
                        foot.push(format!("{dropped} older message(s) were not sent because the conversation is long"));
                    }
                }
                if self.session.is_some() {
                    foot.push(if body["saved"] == true {
                        "saved in the encrypted store".into()
                    } else {
                        match body["not_saved"].as_str() {
                            Some("forgotten") => {
                                "not saved: this conversation was forgotten".to_string()
                            }
                            Some("error") => {
                                "not saved: the encrypted store could not be written".to_string()
                            }
                            _ => "not saved (this conversation is full)".to_string(),
                        }
                    });
                }
                for n in body["notes"].as_array().into_iter().flatten() {
                    foot.push(format!(
                        "note: {} ({})",
                        clean(n["text"].as_str().unwrap_or(""))
                            .chars()
                            .take(120)
                            .collect::<String>(),
                        clean(n["why"].as_str().unwrap_or(""))
                    ));
                }
                if body["notes_withheld"].as_u64().unwrap_or(0) > 0 {
                    foot.push(format!("{} note(s) were not given to the model because they look like instructions.", body["notes_withheld"]));
                }
                if body["notes_left_out"].as_u64().unwrap_or(0) > 0 {
                    foot.push(format!(
                        "{} matching note(s) left out: no room in the model's window",
                        body["notes_left_out"]
                    ));
                }
                let pages = |k: &str| body[k].as_u64().unwrap_or(0);
                if pages("web_sources_used") > 0 {
                    foot.push(format!("{} web page(s) used", pages("web_sources_used")));
                }
                if pages("web_sources_trimmed") > 0 {
                    foot.push(format!(
                        "{} page(s) cut short to fit the model's window",
                        pages("web_sources_trimmed")
                    ));
                }
                if pages("web_sources_left_out") > 0 {
                    foot.push(format!(
                        "{} page(s) left out: no room in the model's window",
                        pages("web_sources_left_out")
                    ));
                }
                // the answer is a card of its own so it can be shown as formatted text; what the answer cost and used follows as a plain card
                let lines = text_lines(&reply);
                match live.and_then(|(i, _)| self.cards.get_mut(i)) {
                    Some(c) => {
                        c.lines = lines;
                        self.scroll_back = 0;
                    }
                    None => self.push(Kind::Info, Some("Assistant"), lines),
                }
                self.push(Kind::Info, None, foot);
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
            Intent::ResumeLast(fork) => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let list = body["sessions"].as_array().cloned().unwrap_or_default();
                // the newest by last change, not the first listed: the list puts pinned conversations first
                let newest = |only: Option<&str>| {
                    list.iter()
                        .filter(|k| k["id"].as_str().is_some())
                        .filter(|k| only.is_none_or(|p| k["project"].as_str() == Some(p)))
                        .max_by_key(|k| k["updated"].as_i64().unwrap_or(0))
                        .cloned()
                };
                let here = self.chat_project.clone();
                let pick = match here.as_deref().and_then(|p| newest(Some(p))) {
                    Some(k) => Some(k),
                    None => {
                        let any = newest(None);
                        if let (Some(k), Some(p)) = (&any, here.as_deref()) {
                            let from = k["project"].as_str().map(clean);
                            self.say(
                                Kind::Info,
                                format!(
                                    "No kept conversation for project {} yet: opening the newest one{}.",
                                    clean(p),
                                    from.map_or(String::new(), |f| format!(", from project {f}"))
                                ),
                            );
                        }
                        any
                    }
                };
                match pick {
                    Some(k) => self.open_or_branch(&k, fork),
                    None => {
                        self.say(
                            Kind::Info,
                            "There is no kept conversation to resume yet. /keep saves one.",
                        );
                        Vec::new()
                    }
                }
            }
            Intent::AwaySeen => Vec::new(),
            Intent::Away => {
                if !ok {
                    return Vec::new();
                }
                let finished = body["finished"].as_array().cloned().unwrap_or_default();
                if !finished.is_empty() {
                    let mut lines: Vec<String> = finished
                        .iter()
                        .map(|j| {
                            let outcome = match j["state"].as_str().unwrap_or("") {
                                "ready" => "finished, ready for your approval".to_string(),
                                "nothing_to_release" => "finished, nothing to release".to_string(),
                                "failed" => format!(
                                    "stopped: {}",
                                    clean(j["note"].as_str().unwrap_or("it did not finish"))
                                ),
                                other => clean(other),
                            };
                            format!(
                                "{:<8} {:<20} {}",
                                clean(j["id"].as_str().unwrap_or("?")),
                                clean(j["recipient"].as_str().unwrap_or("?")),
                                outcome
                            )
                        })
                        .collect();
                    lines.push("/open ID shows one.".into());
                    self.push(Kind::Info, Some("Stopped while you were away"), lines);
                }
                // told once: the next launch reports only what stops from now on
                vec![self.req(
                    Intent::AwaySeen,
                    false,
                    Method::Post,
                    "/api/away/seen",
                    Some(json!({})),
                )]
            }
            Intent::PickerPreview(id) => {
                if let Mode::Picker(p) = &mut self.mode {
                    // an answer that arrives after the owner moved on belongs to a row that is no longer chosen
                    if p.current().is_some_and(|it| it.id == id) {
                        if ok {
                            let turns = body["turns"].as_array().cloned().unwrap_or_default();
                            let mut lines: Vec<String> = turns
                                .iter()
                                .filter_map(|t| {
                                    let who = match t["role"].as_str()? {
                                        "user" => "you",
                                        "assistant" => "assistant",
                                        _ => return None,
                                    };
                                    let text = clean(t["content"].as_str().unwrap_or(""));
                                    let text: String =
                                        text.split_whitespace().collect::<Vec<_>>().join(" ");
                                    let cut: String = text.chars().take(100).collect();
                                    Some(format!(
                                        "{who}: {cut}{}",
                                        if text.chars().count() > 100 {
                                            "…"
                                        } else {
                                            ""
                                        }
                                    ))
                                })
                                .collect();
                            let skip = lines.len().saturating_sub(4);
                            lines.drain(..skip);
                            if lines.is_empty() {
                                lines.push("(no messages)".into());
                            }
                            p.preview = Some((id, lines));
                        } else {
                            p.error = Some(format!(
                                "Could not read that conversation. {}",
                                err_text(status, &body)
                            ));
                        }
                    }
                }
                Vec::new()
            }
            Intent::ResumeFork(id) => {
                if !ok {
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                }
                let found = body["sessions"]
                    .as_array()
                    .and_then(|l| l.iter().find(|k| k["id"].as_str() == Some(id.as_str())))
                    .cloned();
                match found {
                    Some(k) => self.open_or_branch(&k, true),
                    None => {
                        self.say(
                            Kind::Error,
                            "There is no such kept conversation. /sessions lists the kept ones.",
                        );
                        Vec::new()
                    }
                }
            }
            Intent::Branch => {
                let Some(new_id) = ok.then(|| body["id"].as_str().map(clean)).flatten() else {
                    self.rewound = None;
                    self.say(Kind::Error, err_text(status, &body));
                    return Vec::new();
                };
                // the copy is opened like any kept conversation; the original is untouched and stays in /sessions
                vec![self.req(
                    Intent::OpenChat(new_id.clone()),
                    false,
                    Method::Get,
                    &format!("/api/sessions/{new_id}"),
                    None,
                )]
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
                                detail: format!(
                                    "{} message(s){}",
                                    k["turns"].as_u64().unwrap_or(0),
                                    k["project"].as_str().map_or(String::new(), |p| format!(
                                        " · project {}",
                                        clean(p)
                                    ))
                                ),
                                when: k["updated"].as_i64().unwrap_or(0),
                                kind: crate::picker::ItemKind::Chat,
                                project: k["project"].as_str().map(clean),
                            });
                        }
                        p.settle_scope();
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
                self.context_meter = None;
                self.cards.clear();
                self.live = None;
                self.chat_project = body["project"].as_str().map(clean);
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
                if let Some(text) = self.rewound.take() {
                    self.input = text;
                    self.cursor = self.input.chars().count();
                    self.say(
                        Kind::Ok,
                        "Rewound to a copy that stops before that message, with it back in the box. The original conversation is unchanged.",
                    );
                } else {
                    self.say(
                        Kind::Ok,
                        "Opened the kept conversation. New messages are saved to it.",
                    );
                }
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
                    self.context_meter = None;
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

/// Seconds since 1970 on this computer, which is also the computer the service runs on.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// "2 h 5 min", "4 min 12 s", "40 s": the two largest units that are not zero.
pub fn span_text(secs: i64) -> String {
    let s = secs.max(0);
    if s < 60 {
        return format!("{s} s");
    }
    let (h, m, r) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        if m > 0 {
            format!("{h} h {m} min")
        } else {
            format!("{h} h")
        }
    } else if r > 0 {
        format!("{m} min {r} s")
    } else {
        format!("{m} min")
    }
}

/// What to say about how long a release can still be approved. Nothing is said as valid unless the deadline is known.
pub fn expiry_line(expires: Option<i64>, now: i64) -> Option<String> {
    let expires = expires?;
    let left = expires - now;
    Some(if left <= 0 {
        "This release has expired. Nothing can be approved or sent from it. Run the review again to make a new one.".to_string()
    } else if left <= 600 {
        format!("Only {} left to approve this release.", span_text(left))
    } else {
        format!(
            "This release can be approved for another {}.",
            span_text(left)
        )
    })
}

pub fn release_lines(d: &Value) -> Vec<String> {
    release_lines_at(d, now_secs())
}

pub fn release_lines_at(d: &Value, now: i64) -> Vec<String> {
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
    if let Some(l) = expiry_line(d["expires"].as_i64(), now) {
        lines.push(l);
    }
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
    if let Some(pd) = d["personal_data"].as_array().filter(|a| !a.is_empty()) {
        lines.push(String::new());
        lines.push("Personal data noticed in these files (counts only; this blocks nothing, check you mean to send it):".into());
        for x in pd {
            lines.push(format!(
                "  {}: {}",
                clean(x["file"].as_str().unwrap_or("")),
                clean(x["found"].as_str().unwrap_or(""))
            ));
        }
    }
    lines.push(String::new());
    lines.push("This release does not establish:".into());
    for n in d["not_established"].as_array().into_iter().flatten() {
        lines.push(format!("  - {}", clean(n.as_str().unwrap_or(""))));
    }
    lines
}

/// The short form of the context use for the status line: `context 12% (2.0k of 16.4k)` with a known window, `context about
/// 2.0k` without one, a tilde in front of the figure when it is the program's own estimate.
pub fn context_meter(tokens: u64, window: Option<u64>, counted: bool) -> String {
    let k = |n: u64| {
        if n < 1000 {
            n.to_string()
        } else {
            format!("{:.1}k", n as f64 / 1000.0)
        }
    };
    let tilde = if counted { "" } else { "~" };
    match window.filter(|w| *w > 0) {
        Some(w) => format!(
            "context {}% ({tilde}{} of {})",
            (tokens.saturating_mul(100) / w).min(999),
            k(tokens),
            k(w)
        ),
        None => format!("context {tilde}{}", k(tokens)),
    }
}
