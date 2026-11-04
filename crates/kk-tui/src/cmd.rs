#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Status,
    Review { folder: String, recipient: String },
    Reviews,
    Open(Option<String>),
    Findings(Option<String>),
    Fixes(Option<String>),
    ApplyFix { n: usize, job: Option<String> },
    UndoFix { n: usize, job: Option<String> },
    Release(Option<String>),
    Approve(Option<String>),
    Deliver(Option<String>),
    Memory(Option<String>),
    Remember(String),
    Ask { folder: String, question: String },
    Correct { id: String, text: String },
    Forget(String),
    Log,
    Clear,
    Keep,
    ClearHistory,
    Proof(Option<String>),
    Gateway,
    Sessions,
    NewChat,
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
    "remember",
    "correct",
    "forget",
    "log",
    "verify",
    "clear",
    "keep",
    "clearhistory",
    "proof",
    "gateway",
    "sessions",
    "resume",
    "newchat",
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
        "remember" => "save a note",
        "correct" => "change a saved note",
        "forget" => "delete a saved note for good",
        "log" => "the signed activity log",
        "verify" => "check a capsule offline",
        "clear" => "start a fresh conversation",
        "keep" => "save this conversation in the encrypted store",
        "clearhistory" => "forget the saved command history",
        "proof" => "hardware proof of a NEAR confidential model",
        "gateway" => "hardware proof of the NEAR gateway",
        "sessions" => "pick a kept conversation or a review",
        "resume" => "same as /sessions",
        "newchat" => "leave this conversation and start a new one",
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
        "remember" => "/remember TEXT",
        "ask" => "/ask FOLDER : QUESTION",
        "correct" => "/correct ID NEW TEXT",
        "forget" => "/forget ID",
        "verify" => "/verify FILE",
        "proof" => "/proof [MODEL]",
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
        "clearhistory" => Ok(Command::ClearHistory),
        "gateway" => Ok(Command::Gateway),
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
        "sessions" | "resume" => Ok(Command::Sessions),
        "newchat" => Ok(Command::NewChat),
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
        "remember" => {
            if rest.is_empty() {
                Err(format!("usage: {}", usage("remember")))
            } else {
                Ok(Command::Remember(rest.to_string()))
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
