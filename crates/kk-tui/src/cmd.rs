#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    Status,
    Review { folder: String, recipient: String },
    Reviews,
    Open(Option<String>),
    Findings(Option<String>),
    Release(Option<String>),
    Approve(Option<String>),
    Deliver(Option<String>),
    Memory(Option<String>),
    Remember(String),
    Correct { id: String, text: String },
    Forget(String),
    Log,
    Clear,
    Verify(String),
    Quit,
}

pub const NAMES: &[&str] = &[
    "help", "status", "review", "reviews", "open", "findings", "release", "approve", "deliver",
    "memory", "remember", "correct", "forget", "log", "verify", "clear", "quit",
];

pub fn usage(name: &str) -> &'static str {
    match name {
        "review" => "/review FOLDER for RECIPIENT",
        "open" => "/open [JOB]",
        "findings" => "/findings [JOB]",
        "release" => "/release [JOB]",
        "approve" => "/approve [JOB]",
        "deliver" => "/deliver [JOB]",
        "memory" => "/memory [WORDS]",
        "remember" => "/remember TEXT",
        "correct" => "/correct ID NEW TEXT",
        "forget" => "/forget ID",
        "verify" => "/verify FILE",
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
        "release" => job_arg(rest).map(Command::Release),
        "approve" => job_arg(rest).map(Command::Approve),
        "deliver" => job_arg(rest).map(Command::Deliver),
        "memory" => Ok(Command::Memory(if rest.is_empty() {
            None
        } else {
            Some(rest.to_string())
        })),
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
