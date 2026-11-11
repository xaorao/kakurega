//! Commands for scripts and CI: start a review, list reviews, read findings, send one message. They are clients of the same local
//! service as the terminal workspace and the web page, print plain lines or one JSON object per line (`--json`), and end with an
//! exit code a script can act on. None of them can approve a release: `kaku approve` is the only way, and a script that wants one
//! gets the instruction to run it, not a bypass. There is no flag that skips an approval, and none is planned.
//!
//! Exit codes: 0 done (a review that is ready ends at `awaiting_approval`), 2 the command was used wrongly, 3 the review finished
//! with nothing that can be released, 4 the review failed, 5 the service could not be reached or the model did not answer,
//! 6 the service refused the request.

use kk_tui::app::{clean, Method, Req};
use kk_tui::http::Http;
use serde_json::{json, Value};
use std::io::{IsTerminal, Read};
use std::time::{Duration, Instant};

pub const OK: u8 = 0;
pub const USAGE: u8 = 2;
pub const NOTHING: u8 = 3;
pub const FAILED: u8 = 4;
pub const UNAVAILABLE: u8 = 5;
pub const REFUSED: u8 = 6;

fn call(
    http: &Http,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Result<(u16, Value), String> {
    http.run(&Req {
        id: 0,
        admin: false,
        method,
        path: path.to_string(),
        body,
        stream: false,
    })
    .result
}

/// The flags that mean something to the service this command may have to start (where the data lives, which folders and model it
/// uses), and nothing else: the command's own words are not the service's.
fn serve_args(args: &[String]) -> Vec<String> {
    const WITH_VALUE: &[&str] = &[
        "--data-dir",
        "--root",
        "--scripted",
        "--ui-dir",
        "--model",
        "--ollama",
        "--context",
        "--transport",
        "--vm",
        "--worker-bin",
        "--launcher",
    ];
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if WITH_VALUE.contains(&args[i].as_str()) {
            if let Some(v) = args.get(i + 1) {
                out.push(args[i].clone());
                out.push(v.clone());
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    out
}

/// What to do about an answer that is not 2xx: its plain message and the exit code it deserves.
fn refusal(status: u16, body: &Value) -> (String, u8) {
    let msg = body["error"]["message"]
        .as_str()
        .map(clean)
        .unwrap_or_else(|| format!("the service answered {status}"));
    (msg, if status >= 500 { UNAVAILABLE } else { REFUSED })
}

fn say(json_mode: bool, v: &Value, human: impl FnOnce() -> String) {
    if json_mode {
        println!("{v}");
    } else {
        println!("{}", human());
    }
}

fn fail(json_mode: bool, code: u8, msg: &str) -> u8 {
    if json_mode {
        println!(
            "{}",
            json!({"event": "error", "code": code, "message": msg})
        );
    } else {
        eprintln!("{msg}");
    }
    code
}

fn connect(dir: &std::path::Path, json_mode: bool) -> Result<Http, u8> {
    Http::connect(dir).map_err(|e| {
        fail(
            json_mode,
            UNAVAILABLE,
            &format!("the service could not be reached: {e}"),
        )
    })
}

pub fn jobs(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    crate::tui::with_service(&serve_args(args), |dir| {
        let http = match connect(dir, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        match call(&http, Method::Get, "/api/jobs", None) {
            Ok((200, v)) => {
                let list = v["jobs"].as_array().cloned().unwrap_or_default();
                if json_mode {
                    println!(
                        "{}",
                        json!({"jobs": list.iter().map(|j| json!({"id": j["id"], "state": j["state"], "recipient": j["recipient"], "created": j["created"]})).collect::<Vec<_>>()})
                    );
                } else if list.is_empty() {
                    println!("no reviews yet");
                } else {
                    for j in &list {
                        println!(
                            "{}  {}  for {}",
                            clean(j["id"].as_str().unwrap_or("?")),
                            clean(j["state"].as_str().unwrap_or("?")),
                            clean(j["recipient"].as_str().unwrap_or("?"))
                        );
                    }
                }
                OK
            }
            Ok((s, v)) => {
                let (m, c) = refusal(s, &v);
                fail(json_mode, c, &m)
            }
            Err(e) => fail(json_mode, UNAVAILABLE, &e),
        }
    })
}

fn session_id_ok(id: &str) -> bool {
    id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && !id.is_empty()
}

/// The conversations kept in the encrypted store: id, title, when it last changed and how many messages it holds.
pub fn sessions(args: &[String]) -> u8 {
    if args.first().map(String::as_str) == Some("export") {
        return session_export(&args[1..]);
    }
    let json_mode = args.iter().any(|a| a == "--json");
    crate::tui::with_service(&serve_args(args), |dir| {
        let http = match connect(dir, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        match call(&http, Method::Get, "/api/sessions", None) {
            Ok((200, v)) => {
                let list = v["sessions"].as_array().cloned().unwrap_or_default();
                if json_mode {
                    println!(
                        "{}",
                        json!({"sessions": list.iter().map(|x| json!({"id": x["id"], "title": x["title"], "updated": x["updated"], "messages": x["turns"], "project": x["project"]})).collect::<Vec<_>>()})
                    );
                } else if list.is_empty() {
                    println!("no kept conversations");
                } else {
                    for x in &list {
                        println!(
                            "{}  {}  {}",
                            clean(x["id"].as_str().unwrap_or("?")),
                            x["turns"],
                            clean(x["title"].as_str().unwrap_or(""))
                        );
                    }
                }
                OK
            }
            Ok((s, v)) => {
                let (m, c) = refusal(s, &v);
                fail(json_mode, c, &m)
            }
            Err(e) => fail(json_mode, UNAVAILABLE, &e),
        }
    })
}

pub fn findings(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    let Some(job) = args.iter().skip(1).find(|a| {
        !a.starts_with("--") && Some((*a).as_str()) != crate::flag(args, "--data-dir").as_deref()
    }) else {
        return fail(json_mode, USAGE, "usage: kaku findings JOB [--json]");
    };
    if !job.starts_with("job-") || job.len() > 24 || !job[4..].chars().all(|c| c.is_ascii_digit()) {
        return fail(json_mode, USAGE, "a review is named like job-3");
    }
    let job = job.clone();
    crate::tui::with_service(&serve_args(args), |dir| {
        let http = match connect(dir, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        match call(
            &http,
            Method::Get,
            &format!("/api/jobs/{job}/findings"),
            None,
        ) {
            Ok((200, v)) => {
                let list = v["findings"].as_array().cloned().unwrap_or_default();
                if json_mode {
                    println!(
                        "{}",
                        json!({"job": job, "findings": list.iter().map(|f| json!({"title": f["title"], "file": f["file"], "line": f["line"], "status": f["status"], "language": f["language"], "pattern_matches": f["pattern_matches"]})).collect::<Vec<_>>(), "pattern_scan": v["pattern_scan"]["status"]})
                    );
                } else if list.is_empty() {
                    println!("{job}: the agent reported no findings. That is not proof the project is free of defects.");
                } else {
                    for f in &list {
                        println!(
                            "{job}  {}  {}:{}  {}",
                            clean(f["status"].as_str().unwrap_or("?")),
                            clean(f["file"].as_str().unwrap_or("?")),
                            f["line"],
                            clean(f["title"].as_str().unwrap_or(""))
                        );
                    }
                }
                OK
            }
            Ok((s, v)) => {
                let (m, c) = refusal(s, &v);
                fail(json_mode, c, &m)
            }
            Err(e) => fail(json_mode, UNAVAILABLE, &e),
        }
    })
}

/// Prints each step of a review once, and ends the way `kaku review` ends: 0 at `awaiting_approval`, 3 when nothing can be released, 4 when it
/// failed, 5 when the time given runs out or the service cannot be reached, 6 when the service refuses.
fn follow_job(http: &Http, id: &str, json_mode: bool, timeout: u64) -> u8 {
    let started = Instant::now();
    let mut shown = 0usize;
    loop {
        match call(http, Method::Get, &format!("/api/jobs/{id}"), None) {
            Ok((200, v)) => {
                let events = v["events"].as_array().cloned().unwrap_or_default();
                for e in events.iter().skip(shown) {
                    let (stage, detail) = (
                        clean(e["stage"].as_str().unwrap_or("")),
                        clean(e["detail"].as_str().unwrap_or("")),
                    );
                    say(
                        json_mode,
                        &json!({"event": "stage", "job": id, "stage": stage, "detail": detail}),
                        || format!("{id}  {stage}  {detail}"),
                    );
                }
                shown = events.len();
                match v["state"].as_str().unwrap_or("running") {
                    "running" => {}
                    "ready" => {
                        say(
                            json_mode,
                            &json!({"event": "awaiting_approval", "job": id, "next": "kaku approve", "note": "a script cannot approve; a person runs kaku approve and reads the exact files first"}),
                            || {
                                format!("{id} is ready. Nothing has been sent. Run `kaku approve` to read exactly what would be sent and decide.")
                            },
                        );
                        return OK;
                    }
                    "nothing_to_release" => {
                        say(
                            json_mode,
                            &json!({"event": "finished", "job": id, "state": "nothing_to_release"}),
                            || {
                                format!("{id} finished: the kernel confirmed no finding, so there is nothing that can be sent")
                            },
                        );
                        return NOTHING;
                    }
                    other => {
                        let note = clean(v["note"].as_str().unwrap_or(""));
                        say(
                            json_mode,
                            &json!({"event": "finished", "job": id, "state": other, "note": note}),
                            || format!("{id} stopped: {note}"),
                        );
                        return FAILED;
                    }
                }
            }
            Ok((s, v)) => {
                let (m, c) = refusal(s, &v);
                return fail(json_mode, c, &m);
            }
            Err(e) => return fail(json_mode, UNAVAILABLE, &e),
        }
        if started.elapsed() > Duration::from_secs(timeout) {
            return fail(json_mode, UNAVAILABLE, &format!("{id} did not finish within {timeout} seconds; it is still running, check it with kaku jobs"));
        }
        std::thread::sleep(Duration::from_millis(700));
    }
}

/// `kaku follow JOB`: watch a review that is already running (or finished) until it ends.
pub fn follow(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    let Some(job) = args.iter().skip(1).find(|a| {
        !a.starts_with("--")
            && Some((*a).as_str()) != crate::flag(args, "--data-dir").as_deref()
            && Some((*a).as_str()) != crate::flag(args, "--timeout").as_deref()
    }) else {
        return fail(
            json_mode,
            USAGE,
            "usage: kaku follow JOB [--json] [--timeout SECONDS]",
        );
    };
    if !job.starts_with("job-") || job.len() > 24 || !job[4..].chars().all(|c| c.is_ascii_digit()) {
        return fail(json_mode, USAGE, "a review is named like job-3");
    }
    let timeout = crate::flag(args, "--timeout")
        .and_then(|t| t.parse::<u64>().ok())
        .unwrap_or(3600)
        .clamp(10, 86_400);
    let job = job.clone();
    crate::tui::with_service(&serve_args(args), |dir| {
        let http = match connect(dir, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        follow_job(&http, &job, json_mode, timeout)
    })
}

pub fn review(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    let dir_arg = args.iter().skip(1).find(|a| {
        !a.starts_with("--")
            && Some((*a).as_str()) != crate::flag(args, "--for").as_deref()
            && Some((*a).as_str()) != crate::flag(args, "--data-dir").as_deref()
            && Some((*a).as_str()) != crate::flag(args, "--timeout").as_deref()
    });
    let (Some(dir_arg), Some(recipient)) = (dir_arg, crate::flag(args, "--for")) else {
        return fail(
            json_mode,
            USAGE,
            "usage: kaku review DIR --for RECIPIENT [--json] [--timeout SECONDS]",
        );
    };
    let timeout = crate::flag(args, "--timeout")
        .and_then(|t| t.parse::<u64>().ok())
        .unwrap_or(3600)
        .clamp(10, 86_400);
    let Ok(repo) = std::path::Path::new(dir_arg).canonicalize() else {
        return fail(json_mode, USAGE, "that folder does not exist");
    };
    crate::tui::with_service(&serve_args(args), |data| {
        let http = match connect(data, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        let id = match call(
            &http,
            Method::Post,
            "/api/jobs",
            Some(json!({"repo": repo.to_string_lossy(), "recipient": recipient})),
        ) {
            Ok((202, v)) => v["id"].as_str().unwrap_or("").to_string(),
            Ok((s, v)) => {
                let (m, c) = refusal(s, &v);
                return fail(json_mode, c, &m);
            }
            Err(e) => return fail(json_mode, UNAVAILABLE, &e),
        };
        say(json_mode, &json!({"event": "started", "job": id}), || {
            format!("review {id} started")
        });
        follow_job(&http, &id, json_mode, timeout)
    })
}

/// What is printed when the model's reply could not be checked: nothing of the raw reply when it was an empty answer, and
/// otherwise the reply, marked as unchecked.
fn unchecked_lines(v: &Value) -> Vec<String> {
    if v["no_answer"] == true {
        return vec!["The model gave no answer, even when asked again with fewer passages. Try a bigger model (choose one in Settings or with /model in the terminal), or ask about one document at a time.".to_string()];
    }
    vec![
        "The model did not give a checkable answer. Unchecked text follows; trust none of it:"
            .to_string(),
        clean(v["unchecked_text"].as_str().unwrap_or("")),
    ]
}

/// `kaku ask QUESTION --folder DIR [--json]`: a question about the documents in a folder, answered only from them. Every quote the model
/// gives is checked against the file it names. Exit 0 when the answer is checkable and every quote was found; 3 when it is not checkable or
/// any quote was not found or named a file the model was not shown (a script must not rely on it); the other codes are as for the rest.
pub fn ask(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    let folder = crate::flag(args, "--folder");
    let question = args.iter().skip(1).find(|a| {
        !a.starts_with("--")
            && Some((*a).as_str()) != folder.as_deref()
            && Some((*a).as_str()) != crate::flag(args, "--data-dir").as_deref()
    });
    let (Some(question), Some(folder)) = (question, folder) else {
        return fail(
            json_mode,
            USAGE,
            "usage: kaku ask QUESTION --folder DIR [--json]",
        );
    };
    if question.trim().is_empty() || question.len() > 2000 {
        return fail(json_mode, USAGE, "the question is 1 to 2000 characters");
    }
    let Ok(dir) = std::path::Path::new(&folder).canonicalize() else {
        return fail(json_mode, USAGE, "that folder does not exist");
    };
    let question = question.clone();
    crate::tui::with_service(&serve_args(args), |data| {
        let http = match connect(data, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        match call(
            &http,
            Method::Post,
            "/api/ask",
            Some(json!({"folder": dir.to_string_lossy(), "question": question})),
        ) {
            Ok((200, v)) => {
                let checkable = v["checkable"] == true;
                let cites = v["citations"].as_array().cloned().unwrap_or_default();
                let all_found = cites.iter().all(|c| c["verdict"] == "verified");
                if json_mode {
                    println!(
                        "{}",
                        json!({
                            "checkable": checkable,
                            "answer": if checkable { v["answer"].clone() } else { Value::Null },
                            "unchecked_text": if checkable || v["no_answer"] == true { Value::Null } else { v["unchecked_text"].clone() },
                            "no_answer": v["no_answer"] == true,
                            "citations": cites.iter().map(|c| json!({"file": c["file"], "quote": c["quote"], "verdict": c["verdict"], "line": c["line"]})).collect::<Vec<_>>(),
                            "files_read": v["files_used"].as_array().map_or(0, |a| a.len()),
                            "files_withheld": v["files_withheld"].as_array().map_or(0, |a| a.len()),
                            "files_too_large": v["files_skipped"].as_array().map_or(0, |a| a.len()),
                            "injection_suspected": v["injection_suspects"].as_array().is_some_and(|a| !a.is_empty()),
                        })
                    );
                } else if checkable {
                    println!("{}", clean(v["answer"].as_str().unwrap_or("")));
                    for c in &cites {
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
                        println!(
                            "{}: \"{}\" ({mark})",
                            clean(c["file"].as_str().unwrap_or("?")),
                            clean(c["quote"].as_str().unwrap_or(""))
                        );
                    }
                } else {
                    for line in unchecked_lines(&v) {
                        println!("{line}");
                    }
                }
                if checkable && all_found {
                    OK
                } else {
                    NOTHING
                }
            }
            Ok((s, v)) => {
                let (m, c) = refusal(s, &v);
                fail(json_mode, c, &m)
            }
            Err(e) => fail(json_mode, UNAVAILABLE, &e),
        }
    })
}

/// One message to the private assistant, printed with its answer. The message is the argument, or standard input when the argument is
/// `-` or there is none and input is piped. Nothing is kept: no conversation is saved by this command.
pub fn chat(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    let keep = args.iter().any(|a| a == "--keep");
    let session = crate::flag(args, "--session");
    if let Some(id) = &session {
        if !session_id_ok(id) {
            return fail(
                json_mode,
                USAGE,
                "a conversation is named like chat-1a2b3c4d5e; `kaku sessions` lists them",
            );
        }
        if keep {
            return fail(
                json_mode,
                USAGE,
                "--keep starts a new kept conversation; --session continues one: use one of them",
            );
        }
    }
    let arg = args.iter().skip(1).find(|a| {
        (!a.starts_with("--") || a.as_str() == "-")
            && Some((*a).as_str()) != crate::flag(args, "--data-dir").as_deref()
            && Some((*a).as_str()) != session.as_deref()
    });
    let message = match arg.map(String::as_str) {
        Some("-") | None if !std::io::stdin().is_terminal() => {
            let mut buf = String::new();
            let _ = std::io::stdin().take(10_000).read_to_string(&mut buf);
            buf.trim().to_string()
        }
        Some(m) if m != "-" => m.to_string(),
        _ => return fail(
            json_mode,
            USAGE,
            "usage: kaku chat MESSAGE [--session ID | --keep] [--json]   (or pipe the message in)",
        ),
    };
    if message.is_empty() {
        return fail(json_mode, USAGE, "the message is empty");
    }
    crate::tui::with_service(&serve_args(args), |dir| {
        let http = match connect(dir, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        let mut session = session.clone();
        if keep {
            match call(
                &http,
                Method::Post,
                "/api/sessions",
                Some(json!({"title": ""})),
            ) {
                Ok((201, v)) => session = v["id"].as_str().map(str::to_string),
                Ok((s, v)) => {
                    let (m, c) = refusal(s, &v);
                    return fail(json_mode, c, &m);
                }
                Err(e) => return fail(json_mode, UNAVAILABLE, &e),
            }
        }
        let mut body = json!({"message": message});
        if let Some(id) = &session {
            body["session"] = json!(id);
        }
        match call(&http, Method::Post, "/api/chat", Some(body)) {
            Ok((200, v)) => {
                let reply = v["reply"].as_str().unwrap_or("");
                if json_mode {
                    println!(
                        "{}",
                        json!({"reply": reply, "session": session, "route": {"class": v["route"]["class"], "simulated": v["route"]["simulated"]}, "notes_used": v["notes_used"]})
                    );
                } else {
                    println!("{}", clean(reply));
                    if keep {
                        if let Some(id) = &session {
                            eprintln!(
                                "kept as {}; continue it with --session {}",
                                clean(id),
                                clean(id)
                            );
                        }
                    }
                }
                OK
            }
            Ok((s, v)) => {
                let (m, c) = refusal(s, &v);
                fail(json_mode, c, &m)
            }
            Err(e) => fail(json_mode, UNAVAILABLE, &e),
        }
    })
}

/// A kept conversation as Markdown: who spoke, then what was said. Every line from the service is cleaned of control characters.
pub fn conversation_markdown(id: &str, turns: &[Value]) -> String {
    let mut out = format!("# Conversation {id}\n");
    for t in turns {
        let who = match t["role"].as_str() {
            Some("user") => "You",
            Some("assistant") => "Assistant",
            _ => continue,
        };
        out.push_str(&format!("\n## {who}\n\n"));
        for line in t["content"].as_str().unwrap_or("").lines() {
            out.push_str(&clean(line));
            out.push('\n');
        }
    }
    out
}

/// `kaku sessions export ID [--out FILE]`: the conversation as Markdown on standard output, or in a new file (never over an existing one;
/// readable by the owner only).
fn session_export(args: &[String]) -> u8 {
    let out_path = crate::flag(args, "--out");
    let Some(id) = args.iter().find(|a| {
        !a.starts_with("--")
            && Some((*a).as_str()) != crate::flag(args, "--data-dir").as_deref()
            && Some((*a).as_str()) != out_path.as_deref()
    }) else {
        return fail(false, USAGE, "usage: kaku sessions export ID [--out FILE]");
    };
    if !session_id_ok(id) {
        return fail(
            false,
            USAGE,
            "a conversation is named like chat-1a2b3c4d5e; `kaku sessions` lists them",
        );
    }
    let id = id.clone();
    crate::tui::with_service(&serve_args(args), |dir| {
        let http = match connect(dir, false) {
            Ok(h) => h,
            Err(c) => return c,
        };
        let turns = match call(&http, Method::Get, &format!("/api/sessions/{id}"), None) {
            Ok((200, v)) => v["turns"].as_array().cloned().unwrap_or_default(),
            Ok((s, v)) => {
                let (m, c) = refusal(s, &v);
                return fail(false, c, &m);
            }
            Err(e) => return fail(false, UNAVAILABLE, &e),
        };
        let text = conversation_markdown(&clean(&id), &turns);
        match &out_path {
            None => {
                print!("{text}");
                OK
            }
            Some(path) => {
                use std::os::unix::fs::OpenOptionsExt;
                match std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path)
                {
                    Ok(mut f) => match std::io::Write::write_all(&mut f, text.as_bytes()) {
                        Ok(()) => {
                            eprintln!("wrote {} bytes to {}", text.len(), clean(path));
                            OK
                        }
                        Err(e) => fail(false, FAILED, &format!("could not write the file: {e}")),
                    },
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => fail(
                        false,
                        REFUSED,
                        "that file already exists; choose a new name",
                    ),
                    Err(e) => fail(false, FAILED, &format!("could not create the file: {e}")),
                }
            }
        }
    })
}

/// The words of a command that are not flags or flag values, after the command name.
fn plain_words(args: &[String], with_value: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if with_value.contains(&a.as_str()) {
            i += 2;
            continue;
        }
        if !a.starts_with("--") {
            out.push(a.clone());
        }
        i += 1;
    }
    out
}

const CODE_USAGE: &str = "usage: kaku code DIR TASK... [--json] [--timeout SECONDS]\n       kaku code show ID | apply ID --digest DIGEST | undo ID | list";

fn code_id_ok(id: &str) -> bool {
    id.strip_prefix("code-")
        .is_some_and(|n| !n.is_empty() && n.len() <= 18 && n.chars().all(|c| c.is_ascii_digit()))
}

fn print_code(v: &Value) {
    let id = clean(v["id"].as_str().unwrap_or(""));
    println!("{id}  {}", clean(v["state"].as_str().unwrap_or("")));
    if let Some(e) = v["error"].as_str() {
        println!("  {}", clean(e));
    }
    if let Some(e) = v["ended"].as_str() {
        println!("  {}", clean(e));
    }
    if let Some(sm) = v["summary"].as_str().filter(|s| !s.is_empty()) {
        println!("  the model says: {}", clean(sm));
    }
    if let Some(n) = v["withheld_files"].as_u64().filter(|n| *n > 0) {
        println!(
            "  {n} {} withheld from the model",
            crate::script::plural(n, "file was", "files were")
        );
    }
    match &v["tests"] {
        Value::Null => println!(
            "  tests: none were found that can run here, so nothing was checked by running it"
        ),
        t => println!(
            "  tests ({}): {}",
            clean(v["test_command"].as_str().unwrap_or("")),
            if t["passed"] == true {
                "pass on the changed files, run by the kernel"
            } else {
                "do NOT pass on the changed files"
            }
        ),
    }
    for c in v["concerns"].as_array().into_iter().flatten() {
        println!("  ! {}", clean(c.as_str().unwrap_or("")));
    }
    for c in v["changes"].as_array().into_iter().flatten() {
        println!(
            "\n{} {}  (+{} -{})",
            clean(c["kind"].as_str().unwrap_or("")),
            clean(c["path"].as_str().unwrap_or("")),
            c["added"],
            c["removed"]
        );
        for line in c["diff"].as_str().unwrap_or("").lines() {
            println!("  {}", clean(line));
        }
    }
    if v["changes"].as_array().is_some_and(|a| a.is_empty()) && v["state"] == "ready" {
        println!("  the task changed no file");
    }
}

fn plural(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_string()
    } else {
        many.to_string()
    }
}

fn confirm(question: &str) -> bool {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return false;
    }
    print!("{question} [y/N] ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).is_ok() && matches!(line.trim(), "y" | "Y" | "yes")
}

/// `kaku code`: a model changes the project in memory, the tests run in the sandbox, and the patch is shown. Nothing on disk
/// changes until a person applies it with the digest that was shown; in a terminal that is one question after the patch.
pub fn code(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    let words = plain_words(
        args,
        &[
            "--data-dir",
            "--timeout",
            "--digest",
            "--model",
            "--ollama",
            "--root",
            "--scripted",
            "--ui-dir",
            "--context",
            "--transport",
            "--vm",
            "--worker-bin",
            "--launcher",
        ],
    );
    let Some(first) = words.first().cloned() else {
        return fail(json_mode, USAGE, CODE_USAGE);
    };
    let timeout = crate::flag(args, "--timeout")
        .and_then(|t| t.parse::<u64>().ok())
        .unwrap_or(3600)
        .clamp(10, 86_400);
    crate::tui::with_service(&serve_args(args), |data| {
        let http = match connect(data, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        let get = |id: &str| call(&http, Method::Get, &format!("/api/code/{id}"), None);
        let answer = |r: Result<(u16, Value), String>, ok: u16| -> Result<Value, u8> {
            match r {
                Ok((s, v)) if s == ok => Ok(v),
                Ok((s, v)) => {
                    let (m, c) = refusal(s, &v);
                    Err(fail(json_mode, c, &m))
                }
                Err(e) => Err(fail(json_mode, UNAVAILABLE, &e)),
            }
        };
        match first.as_str() {
            "list" => match answer(call(&http, Method::Get, "/api/code", None), 200) {
                Ok(v) => {
                    for t in v["tasks"].as_array().into_iter().flatten() {
                        say(json_mode, t, || {
                            format!(
                                "{}  {}  {}",
                                clean(t["id"].as_str().unwrap_or("")),
                                clean(t["state"].as_str().unwrap_or("")),
                                clean(t["task"].as_str().unwrap_or(""))
                            )
                        });
                    }
                    OK
                }
                Err(c) => c,
            },
            "show" | "apply" | "undo" => {
                let Some(id) = words.get(1).filter(|i| code_id_ok(i)) else {
                    return fail(json_mode, USAGE, "a coding task is named like code-3");
                };
                if first == "show" {
                    return match answer(get(id), 200) {
                        Ok(v) => {
                            if json_mode {
                                println!("{v}")
                            } else {
                                print_code(&v)
                            }
                            OK
                        }
                        Err(c) => c,
                    };
                }
                if first == "undo" {
                    return match answer(
                        call(
                            &http,
                            Method::Post,
                            &format!("/api/code/{id}/undo"),
                            Some(json!({})),
                        ),
                        200,
                    ) {
                        Ok(v) => {
                            say(json_mode, &v, || {
                                format!("{id}: every file is back as it was before the patch")
                            });
                            OK
                        }
                        Err(c) => c,
                    };
                }
                let Some(digest) = crate::flag(args, "--digest") else {
                    return fail(
                        json_mode,
                        USAGE,
                        "apply needs --digest, the digest printed under the patch you read",
                    );
                };
                match answer(
                    call(
                        &http,
                        Method::Post,
                        &format!("/api/code/{id}/apply"),
                        Some(json!({"digest": digest})),
                    ),
                    200,
                ) {
                    Ok(v) => {
                        say(json_mode, &v, || {
                            format!(
                                "{id}: applied to {} {}. Undo with: kaku code undo {id}",
                                v["files"],
                                plural(v["files"].as_u64().unwrap_or(0), "file", "files")
                            )
                        });
                        OK
                    }
                    Err(c) => c,
                }
            }
            _ => {
                let Ok(repo) = std::path::Path::new(&first).canonicalize() else {
                    return fail(
                        json_mode,
                        USAGE,
                        &format!("that folder does not exist\n{CODE_USAGE}"),
                    );
                };
                let task = words[1..].join(" ");
                if task.trim().is_empty() {
                    return fail(json_mode, USAGE, CODE_USAGE);
                }
                let id = match answer(
                    call(
                        &http,
                        Method::Post,
                        "/api/code",
                        Some(json!({"folder": repo.to_string_lossy(), "task": task})),
                    ),
                    202,
                ) {
                    Ok(v) => clean(v["id"].as_str().unwrap_or("")),
                    Err(c) => return c,
                };
                say(json_mode, &json!({"event": "started", "task": id}), || {
                    format!("{id} started; nothing on disk changes until you apply it")
                });
                let started = Instant::now();
                let mut shown = 0usize;
                let v = loop {
                    let v = match answer(get(&id), 200) {
                        Ok(v) => v,
                        Err(c) => return c,
                    };
                    let progress = v["progress"].as_array().cloned().unwrap_or_default();
                    for p in progress.iter().skip(shown) {
                        let line = clean(p.as_str().unwrap_or(""));
                        say(
                            json_mode,
                            &json!({"event": "progress", "task": id, "detail": line}),
                            || format!("{id}  {line}"),
                        );
                    }
                    shown = progress.len();
                    if v["state"] != "running" {
                        break v;
                    }
                    if started.elapsed() > Duration::from_secs(timeout) {
                        return fail(json_mode, UNAVAILABLE, &format!("{id} did not finish within {timeout} seconds; it is still running, see it with kaku code show {id}"));
                    }
                    std::thread::sleep(Duration::from_millis(700));
                };
                if json_mode {
                    println!("{}", json!({"event": "finished", "task": v}));
                } else {
                    print_code(&v);
                }
                if v["state"] != "ready" {
                    return FAILED;
                }
                if v["can_apply"] != true {
                    if !json_mode {
                        println!("\nThis patch cannot be applied.");
                    }
                    return NOTHING;
                }
                let digest = clean(v["digest"].as_str().unwrap_or(""));
                if !json_mode && confirm(&format!("\nApply these changes to {}?", repo.display())) {
                    return match answer(
                        call(
                            &http,
                            Method::Post,
                            &format!("/api/code/{id}/apply"),
                            Some(json!({"digest": digest})),
                        ),
                        200,
                    ) {
                        Ok(_) => {
                            println!("Applied. Undo with: kaku code undo {id}");
                            OK
                        }
                        Err(c) => c,
                    };
                }
                say(
                    json_mode,
                    &json!({"event": "awaiting_apply", "task": id, "digest": digest, "next": format!("kaku code apply {id} --digest {digest}")}),
                    || {
                        format!("\nNothing was changed. To apply exactly this patch: kaku code apply {id} --digest {digest}")
                    },
                );
                OK
            }
        }
    })
}

/// `kaku roundtable QUESTION --experts ID,ID[,ID...] [--json]`, or `kaku roundtable --list`: one question to several
/// experts, each alone, then a lead's resolution. In words by default; with `--json`, one event per line as the service sends it.
pub fn roundtable(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    let list = args.iter().any(|a| a == "--list");
    let experts = crate::flag(args, "--experts");
    let question = args.iter().skip(1).find(|a| {
        !a.starts_with("--")
            && Some((*a).as_str()) != crate::flag(args, "--data-dir").as_deref()
            && Some((*a).as_str()) != experts.as_deref()
    });
    let usage = "usage: kaku roundtable QUESTION --experts ID,ID[,ID...] [--json]   (kaku roundtable --list names the experts)";
    if !list && (question.is_none() || experts.is_none()) {
        return fail(json_mode, USAGE, usage);
    }
    let ids: Vec<String> = experts
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let question = question.cloned().unwrap_or_default();
    crate::tui::with_service(&serve_args(args), |dir| {
        let http = match connect(dir, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        if list {
            return match call(&http, Method::Get, "/api/experts", None) {
                Ok((200, v)) => {
                    if json_mode {
                        println!("{v}");
                    } else {
                        for e in v["experts"].as_array().into_iter().flatten() {
                            println!(
                                "{:22} {} ({}): {}",
                                clean(e["id"].as_str().unwrap_or("")),
                                clean(e["name"].as_str().unwrap_or("")),
                                clean(e["group"].as_str().unwrap_or("")),
                                clean(e["summary"].as_str().unwrap_or(""))
                            );
                        }
                    }
                    OK
                }
                Ok((s, v)) => {
                    let (m, c) = refusal(s, &v);
                    fail(json_mode, c, &m)
                }
                Err(e) => fail(json_mode, UNAVAILABLE, &e),
            };
        }
        let req = Req {
            id: 0,
            admin: false,
            method: Method::Post,
            path: "/api/chat/roundtable".into(),
            body: Some(json!({"message": question, "experts": ids})),
            stream: true,
        };
        let never = std::sync::atomic::AtomicBool::new(false);
        let mut answered = 0usize;
        let mut resolved = false;
        // an answer names its expert by id; the name arrives when the expert is first asked
        let mut names: std::collections::BTreeMap<String, String> = Default::default();
        let resp = http.run_events(&req, &never, &mut |v| {
            if json_mode {
                // pieces of an answer are for a screen; a program gets each whole answer once
                if v.get("delta").is_none() {
                    println!("{v}");
                }
                return;
            }
            let id = v["expert"].as_str().unwrap_or("").to_string();
            if let Some(n) = v["name"].as_str() {
                names.insert(id.clone(), n.to_string());
            }
            let name = clean(names.get(&id).map(String::as_str).unwrap_or(&id));
            if let Some(a) = v["answer"].as_str() {
                answered += 1;
                println!("== {name} ==\n{}\n", clean(a));
            } else if let Some(why) = v["failed"].as_str() {
                println!("== {name} ==\n(no answer: {})\n", clean(why));
            } else if v.get("resolution").is_some() {
                let r = &v["resolution"];
                println!("== Resolution ==");
                if r.is_null() {
                    println!(
                        "(none: {})",
                        clean(v["reason"].as_str().unwrap_or("no reason given"))
                    );
                } else if r["structured"] == true {
                    resolved = true;
                    for (label, key) in [
                        ("Decision", "decision"),
                        ("Agreed", "agreed"),
                        ("Disputed", "disputed"),
                        ("Open questions", "open_questions"),
                    ] {
                        match r[key].as_str().filter(|t| !t.is_empty()) {
                            Some(t) => println!("{label}: {}", clean(t)),
                            None => println!("{label}: (the lead wrote nothing here)"),
                        }
                    }
                } else {
                    resolved = true;
                    println!("{}", clean(r["text"].as_str().unwrap_or("")));
                }
            }
        });
        match resp.result {
            Ok((200, v)) => {
                if json_mode {
                    println!(
                        "{}",
                        json!({"done": v["done"] == true, "unresolved": v["unresolved"] == true})
                    );
                }
                // answers without a resolution are still worth having, but a script should be able to tell
                if resolved || json_mode && v["unresolved"] != true {
                    OK
                } else if answered > 0 {
                    3
                } else {
                    UNAVAILABLE
                }
            }
            Ok((502, v)) => fail(
                json_mode,
                UNAVAILABLE,
                &clean(
                    v["error"]["message"]
                        .as_str()
                        .unwrap_or("the roundtable could not be run"),
                ),
            ),
            Ok((s, v)) => {
                let (m, c) = refusal(s, &v);
                fail(json_mode, c, &m)
            }
            Err(e) => fail(json_mode, UNAVAILABLE, &e),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ex_busy_the_limit_on_reviews_running_at_once_is_a_refusal_with_the_services_own_words_not_a_crash(
    ) {
        let msg =
            "4 reviews are already running; wait for one to finish, then start this one again";
        let (m, code) = refusal(429, &json!({"error": {"code": "BUSY", "message": msg}}));
        assert_eq!((m.as_str(), code), (msg, REFUSED));
        // a service error is still told apart from a refusal
        assert_eq!(refusal(503, &json!({})).1, UNAVAILABLE);
        // control characters in a message are removed before they reach the terminal
        assert!(
            !refusal(429, &json!({"error": {"message": "wait\u{1b}[2J"}}))
                .0
                .contains('\u{1b}')
        );
    }

    #[test]
    fn ex_noanswer_an_empty_model_reply_is_said_in_words_and_never_shown_raw() {
        let none = unchecked_lines(
            &json!({"no_answer": true, "unchecked_text": "{\"answer\": \"\", \"citations\": []}"}),
        );
        assert_eq!(none.len(), 1);
        assert!(none[0].starts_with("The model gave no answer"), "{none:?}");
        assert!(!none.join("\n").contains("citations"), "{none:?}");
        let other = unchecked_lines(
            &json!({"no_answer": false, "unchecked_text": "free text \u{1b}[2J here"}),
        );
        assert_eq!(other.len(), 2);
        assert!(
            other[0].contains("did not give a checkable answer")
                && other[1].contains("free text")
                && !other[1].contains('\u{1b}'),
            "{other:?}"
        );
    }

    #[test]
    fn ex01_a_conversation_becomes_markdown_with_control_characters_removed() {
        let turns = vec![
            json!({"role": "user", "content": "first\nsecond"}),
            json!({"role": "assistant", "content": "ok \u{1b}[2J done"}),
            json!({"role": "system", "content": "hidden"}),
            json!({"role": "user"}),
        ];
        let md = conversation_markdown("chat-1", &turns);
        assert!(
            md.starts_with("# Conversation chat-1\n\n## You\n\nfirst\nsecond\n\n## Assistant\n\n")
        );
        assert!(
            !md.contains("hidden") && !md.chars().any(|c| c.is_control() && c != '\n'),
            "{md:?}"
        );
        assert_eq!(md.matches("## You").count(), 2);
    }
}
