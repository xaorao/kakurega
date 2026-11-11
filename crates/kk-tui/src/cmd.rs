/// What `/instructions` was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PiiAction {
    Show,
    Add(String),
    Remove(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstructionsAction {
    Show,
    Set {
        project: Option<String>,
        text: String,
    },
    Clear(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpAction {
    List,
    Check(String),
    /// Allow or turn off one tool, with the pin the list showed in this session.
    /// The third is true when the owner wrote `knowingly`: they confirm a tool that may change things.
    Allow(String, String, bool),
    Off(String, String),
    /// Make a tool ask the owner before each call (the bool is `knowingly`).
    Ask(String, String, bool),
    Pending,
    Decide(String, bool),
    Remove(String),
    Add {
        name: String,
        program: String,
        args: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MethodAction {
    List,
    Show(String),
    /// Approve the text that `Show` displayed in this session.
    Approve(String),
    Revoke(String),
    Remove(String),
    /// Methods a review suggested, waiting for the owner.
    Suggestions,
    /// The methods that ship with the program.
    Bundled,
    /// Add one of them to the owner's methods, unapproved.
    Take(String),
    Keep(i64),
    Dismiss(i64),
    Add {
        name: String,
        triggers: Vec<String>,
        text: String,
    },
}

/// What `/team` was asked to do. A run id left out means the run this terminal showed last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamAction {
    List,
    Plan(String),
    Show(Option<String>),
    Start(Option<String>),
    Stop(Option<String>),
    Answer { task: usize, text: String },
    Delete(String),
}

pub fn team_id_ok(id: &str) -> bool {
    id.len() == 17
        && id.starts_with("team-")
        && id[5..]
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotesOnlyAction {
    Show,
    /// Turn "answer only from this project's notes and files" on or off for the named project, or for the conversation's own.
    Set {
        project: Option<String>,
        on: bool,
    },
}

fn plain_project(p: &str) -> bool {
    let p = p.trim();
    !p.is_empty()
        && p.chars().count() <= 80
        && p != "."
        && p != ".."
        && !p.contains(['/', '\\'])
        && !p.chars().any(char::is_control)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Status,
    Review {
        folder: String,
        recipient: String,
    },
    Reviews,
    Open(Option<String>),
    Findings(Option<String>),
    Fixes(Option<String>),
    ApplyFix {
        n: usize,
        job: Option<String>,
    },
    UndoFix {
        n: usize,
        job: Option<String>,
    },
    Release(Option<String>),
    Approve(Option<String>),
    Deliver(Option<String>),
    Memory(Option<String>),
    Remember {
        text: String,
        project: Option<String>,
    },
    Ask {
        folder: String,
        question: String,
    },
    /// A question put to the assistant that is not part of the conversation and is never saved.
    Aside(String),
    /// One question to several experts, each alone, then a lead's resolution. With no experts: list who can be asked.
    Roundtable(Option<(Vec<String>, String)>),
    Team(TeamAction),
    /// Stop a running review, coding task or team run by its id; with no id, list what is working.
    Stop(Option<String>),
    Correct {
        id: String,
        text: String,
    },
    Forget(String),
    ForgetReview(String),
    Instructions(InstructionsAction),
    NotesOnly(NotesOnlyAction),
    Method(MethodAction),
    Mcp(McpAction),
    Pii(PiiAction),
    Find(String),
    Transcript(String),
    Copy(bool),
    Export(String),
    Usage(u32),
    /// `/code FOLDER : TASK`: a local model changes a copy of the folder and its tests run in the sandbox; nothing on disk changes.
    Code {
        folder: String,
        task: String,
    },
    /// `/code show|apply|undo [ID]`: read the last (or named) coding task's patch, apply it after a yes, or put it back.
    CodeAct(&'static str, Option<String>),
    /// `/model`: the models installed on this computer; `/model NAME`: answer with that one from now on.
    Model(Option<String>),
    /// Branch the open kept conversation, keeping its first N messages (all when none is given).
    Branch(Option<usize>),
    /// `/branch pick` lists the messages of the open conversation to choose where the branch ends.
    BranchPick,
    /// Go back to before message N of the open kept conversation: a copy that stops just before it, with that message back in the box.
    Rewind(usize),
    /// `/rewind` alone: choose which of the owner's earlier messages to go back to.
    RewindPick,
    RenameChat {
        id: String,
        title: String,
    },
    Conflicts,
    Choose {
        keep: String,
        replaced: String,
    },
    BothRight {
        a: String,
        b: String,
    },
    Log,
    Clear,
    Keep,
    Stash(bool),
    Quiet,
    ClearHistory,
    Proof(Option<String>),
    Gateway,
    Search(String, Option<String>),
    Read(String),
    Web(bool),
    Sessions,
    /// `/resume ID` opens that kept conversation without the picker.
    Resume(String),
    NewChat,
    /// `/project` shows the project of this conversation, `/project NAME` sets it and `/project none` clears it.
    Project(Option<String>),
    /// `/similar` lists saved notes that say almost the same thing.
    Similar,
    /// `/dormant [DAYS]` lists current notes nobody has used for that long (default 180). Nothing is put away by listing.
    Dormant(Option<i64>),
    /// `/archived` lists the notes the owner put away.
    Archived,
    /// `/quarantine` lists notes a newer secret check kept out of every answer; `/notsecret ID` says one is not a secret.
    Held,
    NotSecret(String),
    /// `/archive ID`, `/restore ID`, `/pin ID`, `/unpin ID` and `/stilltrue ID`: one owner decision about how a note is kept.
    NoteKeeping {
        action: &'static str,
        id: String,
    },
    Verify(String),
    Quit,
}

pub const NAMES: &[&str] = &[
    "help",
    "status",
    "review",
    "reviews",
    "open",
    "findings",
    "fixes",
    "applyfix",
    "undofix",
    "release",
    "approve",
    "deliver",
    "memory",
    "ask",
    "code",
    "aside",
    "roundtable",
    "team",
    "stop",
    "remember",
    "correct",
    "forget",
    "forgetreview",
    "instructions",
    "notesonly",
    "method",
    "mcp",
    "pii",
    "find",
    "transcript",
    "copy",
    "export",
    "usage",
    "branch",
    "rename",
    "conflicts",
    "choose",
    "bothright",
    "log",
    "verify",
    "clear",
    "keep",
    "clearhistory",
    "proof",
    "gateway",
    "search",
    "read",
    "web",
    "sessions",
    "resume",
    "newchat",
    "project",
    "similar",
    "dormant",
    "archived",
    "quarantine",
    "notsecret",
    "archive",
    "restore",
    "pin",
    "unpin",
    "stilltrue",
    "rewind",
    "quiet",
    "model",
    "quit",
];

/// One line per command for the completion list.
pub fn describe(name: &str) -> &'static str {
    match name {
        "help" => "list every command",
        "status" => "is the model, sandbox and storage ready",
        "review" => "start a private review of a folder",
        "reviews" => "list reviews and their state",
        "open" => "show a review's progress",
        "findings" => "what the kernel confirmed",
        "fixes" => "proposed fixes, checked against the reproducer",
        "applyfix" => "apply a verified fix to your file",
        "undofix" => "restore the original file",
        "release" => "show exactly what would be sent",
        "approve" => "look at the exact files, then approve or refuse",
        "deliver" => "sign and deliver an approved release",
        "memory" => "list or search saved notes",
        "ask" => "ask about documents; every quote is checked",
        "code" => "have a local model change a project; you read the patch before anything changes",
        "aside" => "ask something that stays out of the conversation and is not saved",
        "roundtable" => "ask 2 to 5 experts one question, each alone; a lead says what to do",
        "team" => {
            "a lead splits a goal into tasks for experts; nothing runs until you start the plan"
        }
        "stop" => "stop a running review, coding task or team run; alone, it lists what is working",
        "remember" => "save a note",
        "correct" => "change a saved note",
        "forget" => "delete a saved note for good",
        "forgetreview" => "delete everything one review left in memory",
        "instructions" => "your standing orders to the assistant",
        "notesonly" => "make a project answer only from its own notes and attached files",
        "method" => "your saved methods: guidance that applies only after you approve its text",
        "mcp" => "outside tools: programs whose tools a coding task may call once you allow them",
        "pii" => "values you say are not personal data, left out of release notices",
        "find" => "search kept conversations by words",
        "transcript" => "search this conversation on the screen",
        "copy" => "copy the last reply (or all) to the clipboard, after asking",
        "export" => "write this conversation to a file in the exports folder",
        "usage" => "what each model route answered over some days",
        "model" => "show the models installed on this computer, or choose which one answers",
        "branch" => "start a copy of this kept conversation, optionally cut after message N",
        "rename" => "rename a kept conversation",
        "conflicts" => "notes that seem to disagree",
        "choose" => "keep one of two disagreeing notes",
        "bothright" => "two notes that look alike are both right",
        "log" => "the signed activity log",
        "verify" => "check a capsule offline",
        "clear" => "start a fresh conversation",
        "keep" => "save this conversation in the encrypted store",
        "stash" => "put the line you are typing aside (Ctrl-S does the same)",
        "rewind" => "go back to before one of your messages, with it back in the box to change",
        "quiet" => "show only the end of a review, not each step; run again for every step",
        "pop" => "bring back the most recently stashed line",
        "clearhistory" => "forget the saved command history",
        "proof" => "hardware proof of a NEAR confidential model",
        "gateway" => "hardware proof of the NEAR gateway",
        "search" => "search the web (you approve the words first)",
        "read" => "read web pages you approve, as untrusted sources",
        "web" => "list the pages in this conversation (/web clear forgets them)",
        "sessions" => "pick a kept conversation or a review",
        "resume" => "open a kept conversation by its id (without an id, the same as /sessions)",
        "newchat" => "leave this conversation and start a new one",
        "project" => "show or set the project this conversation is about",
        "similar" => "list saved notes that say almost the same thing",
        "dormant" => "list notes nobody has used for a long time (nothing is put away)",
        "archived" => "list the notes you put away",
        "quarantine" => "list notes kept back because they look like secrets",
        "notsecret" => "say a held note is not a secret, so it is offered again",
        "archive" => "put a note away; it can be brought back",
        "restore" => "bring an archived note back",
        "pin" => "keep a note out of the quiet-notes list",
        "unpin" => "let a note appear in the quiet-notes list again",
        "stilltrue" => "say a note is still true; its quiet time starts again",
        "quit" => "leave Kakurega",
        _ => "",
    }
}

/// Commands whose name starts with, then contains, what has been typed after the slash.
pub fn matches(typed: &str) -> Vec<&'static str> {
    let t = typed.trim_start_matches('/').to_ascii_lowercase();
    let mut names: Vec<&'static str> = NAMES.to_vec();
    names.sort_unstable();
    let mut out: Vec<&'static str> = names
        .iter()
        .copied()
        .filter(|n| n.starts_with(&t))
        .collect();
    out.extend(
        names
            .iter()
            .copied()
            .filter(|n| !n.starts_with(&t) && n.contains(&t)),
    );
    out
}

/// True when running the command needs something typed after its name.
pub fn takes_arguments(name: &str) -> bool {
    usage(name).contains(' ')
}

pub fn usage(name: &str) -> &'static str {
    match name {
        "review" => "/review FOLDER for RECIPIENT",
        "open" => "/open [JOB]",
        "findings" => "/findings [JOB]",
        "fixes" => "/fixes [JOB]",
        "applyfix" => "/applyfix N [JOB]",
        "undofix" => "/undofix N [JOB]",
        "release" => "/release [JOB]",
        "approve" => "/approve [JOB]",
        "deliver" => "/deliver [JOB]",
        "memory" => "/memory [WORDS]",
        "remember" => "/remember [in PROJECT:] TEXT",
        "project" => "/project [NAME | none]",
        "rewind" => "/rewind [N]   (N is the number of one of your messages; without it you choose from a list)",
        "similar" => "/similar",
        "dormant" => "/dormant [DAYS]   (30 to 3650, default 180)",
        "archived" => "/archived",
        "quarantine" => "/quarantine",
        "notsecret" => "/notsecret ID",
        "archive" => "/archive ID",
        "restore" => "/restore ID",
        "pin" => "/pin ID",
        "unpin" => "/unpin ID",
        "stilltrue" => "/stilltrue ID",
        "ask" => "/ask FOLDER : QUESTION",
        "code" => "/code FOLDER : TASK | /code show|apply|undo [ID]",
        "aside" => "/aside QUESTION",
        "roundtable" => "/roundtable EXPERT,EXPERT[,EXPERT] : QUESTION   (alone, it lists the experts)",
        "team" => "/team plan GOAL | /team start|stop|show [ID] | /team answer TASK TEXT | /team delete ID   (alone, it lists your runs)",
        "stop" => "/stop ID   (job-N, code-N or team-...; alone, it lists what is working)",
        "correct" => "/correct ID NEW TEXT",
        "forget" => "/forget ID",
        "forgetreview" => "/forgetreview JOB",
        "instructions" => "/instructions [set [in PROJECT:] TEXT | clear [PROJECT]]",
        "notesonly" => "/notesonly [on|off [PROJECT]]",
        "method" => "/method [show|approve|revoke|remove NAME | add NAME WORD,WORD|- TEXT | suggestions | keep|dismiss NUMBER | bundled | take NAME]",
        "mcp" => "/mcp [check NAME | allow|ask SERVER TOOL [knowingly] | off SERVER TOOL | pending | approve|deny ID | remove NAME | add NAME /full/path [ARGS]]",
        "pii" => "/pii [add ENTRY | remove ENTRY]",
        "find" => "/find WORDS",
        "transcript" => "/transcript WORDS",
        "copy" => "/copy [all]",
        "export" => "/export NAME",
        "usage" => "/usage [DAYS]",
        "model" => "/model [NAME]",
        "branch" => "/branch [N | pick]",
        "rename" => "/rename ID NEW TITLE",
        "resume" => "/resume [ID]",
        "conflicts" => "/conflicts",
        "choose" => "/choose KEEP_ID OVER_ID",
        "bothright" => "/bothright ID ID",
        "verify" => "/verify FILE",
        "proof" => "/proof [MODEL]",
        "search" => "/search [--day|--week|--month|--year] WORDS",
        "read" => "/read ADDRESS_OR_NUMBER ...",
        _ => "/help",
    }
}

fn job_arg(rest: &str) -> Result<Option<String>, String> {
    let r = rest.trim();
    if r.is_empty() {
        return Ok(None);
    }
    if r.len() > 64
        || !r
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("a job name looks like job-3".into());
    }
    Ok(Some(r.to_string()))
}

/// The freshness windows `/search` accepts; a test in kk-cli keeps this equal to the list the web fetcher validates.
pub const RECENCY_WINDOWS: [&str; 4] = ["day", "week", "month", "year"];

pub fn parse(line: &str) -> Result<Command, String> {
    let line = line.trim();
    let Some(body) = line.strip_prefix('/') else {
        return Err("Not a command: /help lists them.".into());
    };
    let (name, rest) = body
        .split_once(char::is_whitespace)
        .map(|(a, b)| (a, b.trim()))
        .unwrap_or((body, ""));
    let name = name.to_ascii_lowercase();
    match name.as_str() {
        "help" | "?" => Ok(Command::Help),
        "status" => Ok(Command::Status),
        "reviews" => Ok(Command::Reviews),
        "quit" | "exit" | "q" => Ok(Command::Quit),
        "log" => Ok(Command::Log),
        "clear" => Ok(Command::Clear),
        "keep" => Ok(Command::Keep),
        "stash" => Ok(Command::Stash(true)),
        "quiet" => Ok(Command::Quiet),
        "code" => {
            let mut words = rest.split_whitespace();
            match (words.next(), words.next(), words.next()) {
                (Some(act @ ("show" | "apply" | "undo")), id, None) => {
                    let act: &'static str = match act {
                        "show" => "show",
                        "apply" => "apply",
                        _ => "undo",
                    };
                    match id {
                        None => Ok(Command::CodeAct(act, None)),
                        Some(i)
                            if i.strip_prefix("code-").is_some_and(|n| {
                                !n.is_empty()
                                    && n.len() <= 18
                                    && n.chars().all(|c| c.is_ascii_digit())
                            }) =>
                        {
                            Ok(Command::CodeAct(act, Some(i.to_string())))
                        }
                        Some(_) => Err("a coding task is named like code-3".into()),
                    }
                }
                _ => match rest.split_once(" : ") {
                    Some((f, t)) if !f.trim().is_empty() && !t.trim().is_empty() => {
                        Ok(Command::Code {
                            folder: f.trim().to_string(),
                            task: t.trim().to_string(),
                        })
                    }
                    _ => Err(format!("usage: {}", usage("code"))),
                },
            }
        }
        "model" if rest.is_empty() => Ok(Command::Model(None)),
        "model" => {
            // one plain model name: the same characters the service accepts, so nothing else is ever sent
            let ok = rest.len() <= 100
                && rest
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '_' | '-' | '/'));
            if ok {
                Ok(Command::Model(Some(rest.to_string())))
            } else {
                Err(format!("usage: {}", usage("model")))
            }
        }
        "pop" => Ok(Command::Stash(false)),
        "clearhistory" => Ok(Command::ClearHistory),
        "gateway" => Ok(Command::Gateway),
        "search" => {
            let mut w = rest.trim();
            let mut recency = None;
            if let Some(flag) = w.strip_prefix("--") {
                let (name, after) = flag.split_once(char::is_whitespace).unwrap_or((flag, ""));
                if !RECENCY_WINDOWS.contains(&name) {
                    return Err(format!("usage: {}", usage("search")));
                }
                recency = Some(name.to_string());
                w = after.trim();
            }
            if w.is_empty()
                || w.starts_with("--")
                || w.chars().count() > 200
                || w.chars().any(char::is_control)
            {
                Err(format!("usage: {}", usage("search")))
            } else {
                Ok(Command::Search(w.to_string(), recency))
            }
        }
        "read" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.is_empty() || parts.len() > 5 || parts.iter().any(|p| p.len() > 2000) {
                Err(format!("usage: {}", usage("read")))
            } else {
                Ok(Command::Read(parts.join(" ")))
            }
        }
        "web" => match rest.trim() {
            "" => Ok(Command::Web(false)),
            "clear" => Ok(Command::Web(true)),
            _ => Err("usage: /web  or  /web clear".into()),
        },
        "proof" => {
            if rest.len() > 80
                || rest.contains(char::is_whitespace)
                || !rest
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-' | ':'))
            {
                Err(format!("usage: {}", usage("proof")))
            } else {
                Ok(Command::Proof((!rest.is_empty()).then(|| rest.to_string())))
            }
        }
        "sessions" => Ok(Command::Sessions),
        "resume" if rest.is_empty() => Ok(Command::Sessions),
        "resume" => Ok(Command::Resume(rest.to_string())),
        "newchat" => Ok(Command::NewChat),
        "similar" => Ok(Command::Similar),
        "dormant" => {
            if rest.is_empty() {
                Ok(Command::Dormant(None))
            } else {
                match rest.parse::<i64>() {
                    Ok(d) if (30..=3650).contains(&d) => Ok(Command::Dormant(Some(d))),
                    _ => Err(format!("usage: {}", usage("dormant"))),
                }
            }
        }
        "quarantine" if rest.is_empty() => Ok(Command::Held),
        "quarantine" => Err(format!("usage: {}", usage("quarantine"))),
        "notsecret" => {
            let plain = !rest.is_empty()
                && rest.len() <= 64
                && rest
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
            if plain {
                Ok(Command::NotSecret(rest.to_string()))
            } else {
                Err(format!("usage: {}", usage("notsecret")))
            }
        }
        "archived" if rest.is_empty() => Ok(Command::Archived),
        "archived" => Err(format!("usage: {}", usage("archived"))),
        "archive" | "restore" | "pin" | "unpin" | "stilltrue" => {
            let action = match name.as_str() {
                "archive" => "archive",
                "restore" => "restore",
                "pin" => "pin",
                "unpin" => "unpin",
                _ => "keep",
            };
            let plain = !rest.is_empty()
                && rest.len() <= 64
                && rest
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
            if plain {
                Ok(Command::NoteKeeping {
                    action,
                    id: rest.to_string(),
                })
            } else {
                Err(format!("usage: {}", usage(&name)))
            }
        }
        "project" => {
            if rest.is_empty() {
                Ok(Command::Project(None))
            } else if rest.eq_ignore_ascii_case("none") {
                Ok(Command::Project(Some(String::new())))
            } else if rest.chars().count() <= 80
                && !rest.contains(['/', '\\'])
                && !rest.chars().any(char::is_control)
                && rest.trim() == rest
            {
                Ok(Command::Project(Some(rest.to_string())))
            } else {
                Err(format!("usage: {}", usage("project")))
            }
        }
        "review" => {
            let Some((folder, recipient)) = rest.rsplit_once(" for ") else {
                return Err(format!("usage: {}", usage("review")));
            };
            let (folder, recipient) = (folder.trim(), recipient.trim());
            if folder.is_empty() || recipient.is_empty() {
                return Err(format!("usage: {}", usage("review")));
            }
            Ok(Command::Review {
                folder: folder.to_string(),
                recipient: recipient.to_string(),
            })
        }
        "open" => job_arg(rest).map(Command::Open),
        "findings" => job_arg(rest).map(Command::Findings),
        "fixes" => job_arg(rest).map(Command::Fixes),
        "undofix" => {
            let mut parts = rest.split_whitespace();
            let n = parts
                .next()
                .and_then(|x| x.parse::<usize>().ok())
                .filter(|n| *n >= 1 && *n <= 99);
            let job = job_arg(parts.next().unwrap_or(""))?;
            match (n, parts.next()) {
                (Some(n), None) => Ok(Command::UndoFix { n, job }),
                _ => Err(format!("usage: {}", usage("undofix"))),
            }
        }
        "applyfix" => {
            let mut parts = rest.split_whitespace();
            let n = parts
                .next()
                .and_then(|x| x.parse::<usize>().ok())
                .filter(|n| *n >= 1 && *n <= 99);
            let job = job_arg(parts.next().unwrap_or(""))?;
            match (n, parts.next()) {
                (Some(n), None) => Ok(Command::ApplyFix { n, job }),
                _ => Err(format!("usage: {}", usage("applyfix"))),
            }
        }
        "release" => job_arg(rest).map(Command::Release),
        "approve" => job_arg(rest).map(Command::Approve),
        "deliver" => job_arg(rest).map(Command::Deliver),
        "memory" => Ok(Command::Memory(if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        })),
        "ask" => match rest.split_once(" : ") {
            Some((f, q)) if !f.trim().is_empty() && !q.trim().is_empty() => Ok(Command::Ask {
                folder: f.trim().to_string(),
                question: q.trim().to_string(),
            }),
            _ => Err(format!("usage: {}", usage("ask"))),
        },
        "pii" => match rest.split_once(char::is_whitespace) {
            None if rest.is_empty() => Ok(Command::Pii(PiiAction::Show)),
            Some(("add", e)) if !e.trim().is_empty() => {
                Ok(Command::Pii(PiiAction::Add(e.trim().to_string())))
            }
            Some(("remove", e)) if !e.trim().is_empty() => {
                Ok(Command::Pii(PiiAction::Remove(e.trim().to_string())))
            }
            _ => Err(format!("usage: {}", usage("pii"))),
        },
        "stop" => {
            let digits =
                |t: &str| !t.is_empty() && t.len() <= 18 && t.chars().all(|c| c.is_ascii_digit());
            if rest.is_empty() {
                Ok(Command::Stop(None))
            } else if rest.strip_prefix("job-").is_some_and(digits)
                || rest.strip_prefix("code-").is_some_and(digits)
                || team_id_ok(rest)
            {
                Ok(Command::Stop(Some(rest.to_string())))
            } else {
                Err(format!("usage: {}", usage("stop")))
            }
        }
        "team" => {
            let usage_err = || Err(format!("usage: {}", usage("team")));
            let (word, tail) = match rest.split_once(char::is_whitespace) {
                Some((w, t)) => (w, t.trim()),
                None => (rest, ""),
            };
            let id = |t: &str| -> Result<Option<String>, ()> {
                if t.is_empty() {
                    Ok(None)
                } else if team_id_ok(t) {
                    Ok(Some(t.to_string()))
                } else {
                    Err(())
                }
            };
            match word {
                "" | "list" if tail.is_empty() => Ok(Command::Team(TeamAction::List)),
                "plan" if !tail.is_empty() => Ok(Command::Team(TeamAction::Plan(tail.to_string()))),
                "show" => id(tail)
                    .map(|i| Command::Team(TeamAction::Show(i)))
                    .or_else(|_| usage_err()),
                "start" => id(tail)
                    .map(|i| Command::Team(TeamAction::Start(i)))
                    .or_else(|_| usage_err()),
                "stop" => id(tail)
                    .map(|i| Command::Team(TeamAction::Stop(i)))
                    .or_else(|_| usage_err()),
                "delete" if team_id_ok(tail) => {
                    Ok(Command::Team(TeamAction::Delete(tail.to_string())))
                }
                "answer" => match tail.split_once(char::is_whitespace) {
                    Some((n, text)) if !text.trim().is_empty() => match n.parse::<usize>() {
                        Ok(task) if task > 0 => Ok(Command::Team(TeamAction::Answer {
                            task,
                            text: text.trim().to_string(),
                        })),
                        _ => usage_err(),
                    },
                    _ => usage_err(),
                },
                _ => usage_err(),
            }
        }
        "roundtable" => {
            if rest.is_empty() {
                return Ok(Command::Roundtable(None));
            }
            match rest.split_once(" : ") {
                Some((who, q)) if !q.trim().is_empty() => {
                    let ids: Vec<String> = who
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    if ids.len() < 2 || ids.iter().any(|i| i.contains(char::is_whitespace)) {
                        Err(format!("usage: {}", usage("roundtable")))
                    } else {
                        Ok(Command::Roundtable(Some((ids, q.trim().to_string()))))
                    }
                }
                _ => Err(format!("usage: {}", usage("roundtable"))),
            }
        }
        "aside" => {
            if rest.is_empty() {
                Err(format!("usage: {}", usage("aside")))
            } else {
                Ok(Command::Aside(rest.to_string()))
            }
        }
        "remember" => {
            if rest.is_empty() {
                Err(format!("usage: {}", usage("remember")))
            } else if let Some(after) = rest.strip_prefix("in ") {
                match after.split_once(':') {
                    Some((p, text))
                        if !text.trim().is_empty()
                            && !p.trim().is_empty()
                            && p.trim().chars().count() <= 80
                            && !p.contains(['/', '\\'])
                            && !p.chars().any(char::is_control) =>
                    {
                        Ok(Command::Remember {
                            text: text.trim().to_string(),
                            project: Some(p.trim().to_string()),
                        })
                    }
                    _ => Err(format!("usage: {}", usage("remember"))),
                }
            } else {
                Ok(Command::Remember {
                    text: rest.to_string(),
                    project: None,
                })
            }
        }
        "correct" => match rest.split_once(char::is_whitespace) {
            Some((id, text)) if !text.trim().is_empty() => Ok(Command::Correct {
                id: id.to_string(),
                text: text.trim().to_string(),
            }),
            _ => Err(format!("usage: {}", usage("correct"))),
        },
        "forget" => {
            if rest.is_empty() || rest.contains(char::is_whitespace) {
                Err(format!("usage: {}", usage("forget")))
            } else {
                Ok(Command::Forget(rest.to_string()))
            }
        }
        "usage" => {
            if rest.is_empty() {
                Ok(Command::Usage(30))
            } else {
                match rest.parse::<u32>() {
                    Ok(d) if (1..=366).contains(&d) => Ok(Command::Usage(d)),
                    _ => Err(format!("usage: {}", usage("usage"))),
                }
            }
        }
        "rewind" if rest.is_empty() => Ok(Command::RewindPick),
        "rewind" => match rest.parse::<usize>() {
            Ok(n) if (1..=100_000).contains(&n) => Ok(Command::Rewind(n)),
            _ => Err(format!("usage: {}", usage("rewind"))),
        },
        "branch" => {
            if rest.is_empty() {
                Ok(Command::Branch(None))
            } else if rest.eq_ignore_ascii_case("pick") {
                Ok(Command::BranchPick)
            } else {
                match rest.parse::<usize>() {
                    Ok(n) if n <= 100_000 => Ok(Command::Branch(Some(n))),
                    _ => Err(format!("usage: {}", usage("branch"))),
                }
            }
        }
        "transcript" => {
            if rest.is_empty() {
                Err(format!("usage: {}", usage("transcript")))
            } else {
                Ok(Command::Transcript(rest.to_string()))
            }
        }
        "copy" => match rest {
            "" => Ok(Command::Copy(false)),
            "all" => Ok(Command::Copy(true)),
            _ => Err(format!("usage: {}", usage("copy"))),
        },
        "export" => {
            if rest.is_empty() || rest.contains(char::is_whitespace) {
                Err(format!("usage: {}", usage("export")))
            } else {
                Ok(Command::Export(rest.to_string()))
            }
        }
        "find" => {
            if rest.is_empty() {
                Err(format!("usage: {}", usage("find")))
            } else {
                Ok(Command::Find(rest.to_string()))
            }
        }
        "rename" => match rest.split_once(char::is_whitespace) {
            Some((id, title)) if !title.trim().is_empty() && !id.contains(['/', '\\']) => {
                Ok(Command::RenameChat {
                    id: id.to_string(),
                    title: title.trim().to_string(),
                })
            }
            _ => Err(format!("usage: {}", usage("rename"))),
        },
        "conflicts" => {
            if rest.is_empty() {
                Ok(Command::Conflicts)
            } else {
                Err(format!("usage: {}", usage("conflicts")))
            }
        }
        "choose" | "bothright" => {
            let ids: Vec<&str> = rest.split_whitespace().collect();
            let plain = |i: &&str| !i.contains(['/', '\\']) && !i.chars().any(char::is_control);
            match ids.as_slice() {
                [x, y] if x != y && plain(x) && plain(y) => Ok(if name == "choose" {
                    Command::Choose {
                        keep: x.to_string(),
                        replaced: y.to_string(),
                    }
                } else {
                    Command::BothRight {
                        a: x.to_string(),
                        b: y.to_string(),
                    }
                }),
                _ => Err(format!("usage: {}", usage(&name))),
            }
        }
        "instructions" => {
            let bad = || Err(format!("usage: {}", usage("instructions")));
            if rest.is_empty() {
                return Ok(Command::Instructions(InstructionsAction::Show));
            }
            let (sub, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let tail = tail.trim();
            match sub {
                "set" if !tail.is_empty() => match tail.strip_prefix("in ") {
                    Some(after) => match after.split_once(':') {
                        Some((p, text)) if plain_project(p) && !text.trim().is_empty() => {
                            Ok(Command::Instructions(InstructionsAction::Set {
                                project: Some(p.trim().to_string()),
                                text: text.trim().to_string(),
                            }))
                        }
                        _ => bad(),
                    },
                    None => Ok(Command::Instructions(InstructionsAction::Set {
                        project: None,
                        text: tail.to_string(),
                    })),
                },
                "clear" if tail.is_empty() => {
                    Ok(Command::Instructions(InstructionsAction::Clear(None)))
                }
                "clear" if plain_project(tail) && !tail.contains(char::is_whitespace) => Ok(
                    Command::Instructions(InstructionsAction::Clear(Some(tail.to_string()))),
                ),
                _ => bad(),
            }
        }
        "mcp" => {
            let bad = || Err(format!("usage: {}", usage("mcp")));
            if rest.is_empty() {
                return Ok(Command::Mcp(McpAction::List));
            }
            let plain = |n: &str| {
                !n.is_empty()
                    && n.len() <= 64
                    && n.chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            };
            let parts: Vec<&str> = rest.split_whitespace().collect();
            match parts.as_slice() {
                ["check", n] if plain(n) => Ok(Command::Mcp(McpAction::Check((*n).into()))),
                ["remove", n] if plain(n) => Ok(Command::Mcp(McpAction::Remove((*n).into()))),
                ["allow", srv, tool] if plain(srv) && plain(tool) => Ok(Command::Mcp(
                    McpAction::Allow((*srv).into(), (*tool).into(), false),
                )),
                ["allow", srv, tool, "knowingly"] if plain(srv) && plain(tool) => Ok(Command::Mcp(
                    McpAction::Allow((*srv).into(), (*tool).into(), true),
                )),
                ["ask", srv, tool] if plain(srv) && plain(tool) => Ok(Command::Mcp(
                    McpAction::Ask((*srv).into(), (*tool).into(), false),
                )),
                ["ask", srv, tool, "knowingly"] if plain(srv) && plain(tool) => Ok(Command::Mcp(
                    McpAction::Ask((*srv).into(), (*tool).into(), true),
                )),
                ["pending"] => Ok(Command::Mcp(McpAction::Pending)),
                ["approve", id] if plain(id) => {
                    Ok(Command::Mcp(McpAction::Decide((*id).into(), true)))
                }
                ["deny", id] if plain(id) => {
                    Ok(Command::Mcp(McpAction::Decide((*id).into(), false)))
                }
                ["off", srv, tool] if plain(srv) && plain(tool) => {
                    Ok(Command::Mcp(McpAction::Off((*srv).into(), (*tool).into())))
                }
                ["add", n, program, args @ ..]
                    if plain(n) && program.starts_with('/') && args.len() <= 32 =>
                {
                    Ok(Command::Mcp(McpAction::Add {
                        name: (*n).into(),
                        program: (*program).into(),
                        args: args.iter().map(|a| a.to_string()).collect(),
                    }))
                }
                _ => bad(),
            }
        }
        "method" => {
            let bad = || Err(format!("usage: {}", usage("method")));
            if rest.is_empty() {
                return Ok(Command::Method(MethodAction::List));
            }
            let (sub, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let tail = tail.trim();
            let plain_name = |n: &str| {
                !n.is_empty()
                    && n.len() <= 64
                    && n.chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            };
            match sub {
                "show" | "approve" | "revoke" | "remove" if plain_name(tail) => {
                    let n = tail.to_string();
                    Ok(Command::Method(match sub {
                        "show" => MethodAction::Show(n),
                        "approve" => MethodAction::Approve(n),
                        "revoke" => MethodAction::Revoke(n),
                        _ => MethodAction::Remove(n),
                    }))
                }
                "suggestions" if tail.is_empty() => Ok(Command::Method(MethodAction::Suggestions)),
                "bundled" if tail.is_empty() => Ok(Command::Method(MethodAction::Bundled)),
                "take"
                    if !tail.is_empty()
                        && tail.len() <= 64
                        && tail
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) =>
                {
                    Ok(Command::Method(MethodAction::Take(tail.to_string())))
                }
                "keep" | "dismiss" => match tail.parse::<i64>() {
                    Ok(n) if n > 0 => Ok(Command::Method(if sub == "keep" {
                        MethodAction::Keep(n)
                    } else {
                        MethodAction::Dismiss(n)
                    })),
                    _ => bad(),
                },
                "add" => {
                    let mut parts = tail.splitn(3, char::is_whitespace);
                    let name = parts.next().unwrap_or("");
                    let words = parts.next().unwrap_or("");
                    let text = parts.next().unwrap_or("").trim();
                    if !plain_name(name) || words.is_empty() || text.is_empty() {
                        return bad();
                    }
                    let triggers = if words == "-" {
                        Vec::new()
                    } else {
                        words
                            .split(',')
                            .map(|w| w.trim().to_string())
                            .filter(|w| !w.is_empty())
                            .collect()
                    };
                    Ok(Command::Method(MethodAction::Add {
                        name: name.to_string(),
                        triggers,
                        text: text.to_string(),
                    }))
                }
                _ => bad(),
            }
        }
        "notesonly" => {
            let bad = || Err(format!("usage: {}", usage("notesonly")));
            if rest.is_empty() {
                return Ok(Command::NotesOnly(NotesOnlyAction::Show));
            }
            let (sub, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let tail = tail.trim();
            let on = match sub {
                "on" => true,
                "off" => false,
                _ => return bad(),
            };
            if tail.is_empty() {
                Ok(Command::NotesOnly(NotesOnlyAction::Set {
                    project: None,
                    on,
                }))
            } else if plain_project(tail) && !tail.contains(char::is_whitespace) {
                Ok(Command::NotesOnly(NotesOnlyAction::Set {
                    project: Some(tail.to_string()),
                    on,
                }))
            } else {
                bad()
            }
        }
        "forgetreview" => {
            if rest.is_empty() || rest.contains(char::is_whitespace) {
                Err(format!("usage: {}", usage("forgetreview")))
            } else {
                Ok(Command::ForgetReview(rest.to_string()))
            }
        }
        "verify" => {
            if rest.is_empty() {
                Err(format!("usage: {}", usage("verify")))
            } else {
                Ok(Command::Verify(rest.to_string()))
            }
        }
        other => {
            let near: Vec<String> = NAMES
                .iter()
                .filter(|n| n.starts_with(other) && !other.is_empty())
                .take(3)
                .map(|n| format!("/{n}"))
                .collect();
            if near.is_empty() {
                Err(format!("unknown command /{other}; /help lists them"))
            } else {
                Err(format!(
                    "unknown command /{other}; did you mean {}?",
                    near.join(" or ")
                ))
            }
        }
    }
}

pub fn complete(prefix: &str) -> Option<String> {
    let p = prefix.strip_prefix('/')?;
    if p.contains(char::is_whitespace) {
        return None;
    }
    let matches: Vec<&&str> = NAMES
        .iter()
        .filter(|n| n.starts_with(&p.to_ascii_lowercase()))
        .collect();
    match matches.as_slice() {
        [one] => Some(format!("/{one} ")),
        [] => None,
        many => {
            let first = many[0];
            let mut common = first.len();
            for m in many.iter() {
                common = common.min(
                    first
                        .chars()
                        .zip(m.chars())
                        .take_while(|(a, b)| a == b)
                        .count(),
                );
            }
            if common > p.len() {
                Some(format!("/{}", &first[..common]))
            } else {
                None
            }
        }
    }
}
