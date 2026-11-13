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
/// The first word that is neither one of this command's own options nor a service option with its value.
fn stray_word(args: &[String], own: &[&str]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if WITH_VALUE.contains(&a) || a == "--profile" {
            i += 2;
        } else if own.contains(&a) {
            i += 1;
        } else {
            return Some(a.to_string());
        }
    }
    None
}

fn refuse_stray(command: &str, args: &[String], own: &[&str], json_mode: bool) -> Option<u8> {
    let w = stray_word(args, own)?;
    Some(fail(
        json_mode,
        USAGE,
        &format!(
            "kaku {command} takes no {:?}: it takes {}. See kaku {command} --help",
            clean(&w),
            own.join(" ")
        ),
    ))
}

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
    "--route",
    "--outside-model",
    "--near-model",
    "--near-url",
    "--near-key-file",
];

fn serve_args(args: &[String]) -> Vec<String> {
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
    if let Some(code) = refuse_stray("jobs", args, &["--json"], json_mode) {
        return code;
    }
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
    if let Some(code) = refuse_stray("sessions", args, &["--json"], json_mode) {
        return code;
    }
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
    let Some(job) = positional(args, 1, &[]).into_iter().next() else {
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
    let Some(job) = positional(args, 1, &["--timeout"]).into_iter().next() else {
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
    let dir_arg = positional(args, 1, &["--for", "--timeout", "--change"])
        .into_iter()
        .next();
    let (Some(dir_arg), Some(recipient)) = (dir_arg, crate::flag(args, "--for")) else {
        return fail(
            json_mode,
            USAGE,
            "usage: kaku review DIR --for RECIPIENT [--change BASE] [--json] [--timeout SECONDS]\n  --change BASE  review only what the checked-out commit changes relative to BASE (a branch, a tag or a commit), as for a pull request",
        );
    };
    let base = crate::flag(args, "--change");
    if args.iter().any(|a| a == "--change") && base.is_none() {
        return fail(
            json_mode,
            USAGE,
            "name the base after --change: a branch, a tag or a commit",
        );
    }
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
            Some(json!({"repo": repo.to_string_lossy(), "recipient": recipient, "base": base})),
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
    let question = positional(args, 1, &["--folder"]).into_iter().next();
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
    let arg = positional(args, 1, &["--session"]).into_iter().next();
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
                // the answer is shown either way; a kept conversation the service could not add it to is a failure
                let lost = session.is_some() && v["saved"] != true;
                if json_mode {
                    println!(
                        "{}",
                        json!({"reply": reply, "cut_short": v["cut_short"] == true, "session": session, "saved": session.is_some() && !lost, "not_saved": if lost { v["not_saved"].clone() } else { Value::Null }, "route": {"class": v["route"]["class"], "simulated": v["route"]["simulated"]}, "notes_used": v["notes_used"]})
                    );
                } else {
                    println!("{}", clean(reply));
                    if v["cut_short"] == true {
                        eprintln!("this answer reached the model's length limit and stops there; ask it to continue");
                    }
                    if keep && !lost {
                        if let Some(id) = &session {
                            eprintln!(
                                "kept as {}; continue it with --session {}",
                                clean(id),
                                clean(id)
                            );
                        }
                    }
                }
                if lost {
                    let why = match v["not_saved"].as_str() {
                        Some("forgotten") => {
                            "the conversation was forgotten while the answer was written"
                        }
                        Some("error") => "the service could not write it (is the disk full?)",
                        Some("full") => "the conversation holds as many messages as it can; start a new one with --keep",
                        _ => "the service did not say why",
                    };
                    if !json_mode {
                        eprintln!("this answer was NOT saved to the conversation: {why}");
                    }
                    return FAILED;
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
    let Some(id) = positional(args, 0, &["--out"]).into_iter().next() else {
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
    positional(args, 1, with_value)
        .into_iter()
        .cloned()
        .collect()
}

/// The words of a command that are not flags or a flag's value, in order from `from`. The service's flags (`WITH_VALUE`) and
/// the command's own flags that take a value are skipped with their value wherever they stand, so `--route near` before a
/// question is never read as the question.
fn positional<'a>(args: &'a [String], from: usize, own: &[&str]) -> Vec<&'a String> {
    let mut out = Vec::new();
    let mut i = from;
    while i < args.len() {
        let a = &args[i];
        if WITH_VALUE.contains(&a.as_str()) || own.contains(&a.as_str()) {
            i += 2;
            continue;
        }
        if !a.starts_with("--") {
            out.push(a);
        }
        i += 1;
    }
    out
}

/// Where an agent that reads the project works, in the words of its start line.
fn agent_place(model: &Value) -> String {
    if model["stays_here"] == false {
        format!(
            "the agent reads the project with the model in confidential hardware at {}, which is given the files it reads",
            clean(model["host"].as_str().unwrap_or("another computer"))
        )
    } else {
        "the agent reads the project on this computer".into()
    }
}

const CODE_USAGE: &str = "usage: kaku code DIR TASK... [--json] [--timeout SECONDS]\n       kaku code show ID | apply ID --digest DIGEST | undo ID | list";

fn code_id_ok(id: &str) -> bool {
    id.strip_prefix("code-")
        .is_some_and(|n| !n.is_empty() && n.len() <= 18 && n.chars().all(|c| c.is_ascii_digit()))
}

fn code_text(v: &Value) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    macro_rules! put {
        ($($a:tt)*) => {{
            let _ = writeln!(out, $($a)*);
        }};
    }
    let id = clean(v["id"].as_str().unwrap_or(""));
    put!("{id}  {}", clean(v["state"].as_str().unwrap_or("")));
    if let Some(e) = v["error"].as_str() {
        put!("  {}", clean(e));
    }
    if let Some(e) = v["ended"].as_str() {
        put!("  {}", clean(e));
    }
    if let Some(sm) = v["summary"].as_str().filter(|s| !s.is_empty()) {
        put!("  the model says: {}", clean(sm));
    }
    if let Some(n) = v["withheld_files"].as_u64().filter(|n| *n > 0) {
        put!(
            "  {n} {} withheld from the model",
            crate::script::plural(n, "file was", "files were")
        );
    }
    match &v["tests"] {
        Value::Null => {
            put!("  tests: none were found that can run here, so nothing was checked by running it")
        }
        t => put!(
            "  tests ({}): {}",
            clean(v["test_command"].as_str().unwrap_or("")),
            if t["ran"] == false {
                "could NOT run here, so nothing is known about whether they pass (the sandbox could not start them; run `kaku doctor`)"
            } else if t["passed"] == true {
                "pass on the changed files, run by the kernel"
            } else {
                "do NOT pass on the changed files"
            }
        ),
    }
    for c in v["concerns"].as_array().into_iter().flatten() {
        put!("  ! {}", clean(c.as_str().unwrap_or("")));
    }
    for c in v["changes"].as_array().into_iter().flatten() {
        put!(
            "\n{} {}  (+{} -{})",
            clean(c["kind"].as_str().unwrap_or("")),
            clean(c["path"].as_str().unwrap_or("")),
            c["added"],
            c["removed"]
        );
        for line in c["diff"].as_str().unwrap_or("").lines() {
            put!("  {}", clean(line));
        }
    }
    if v["changes"].as_array().is_some_and(|a| a.is_empty()) && v["state"] == "ready" {
        put!("  the task changed no file");
    }
    // the digest binds an apply to the patch read above, so a patch that changed since is not applied
    if let Some(d) = v["digest"].as_str().filter(|d| {
        v["state"] == "ready"
            && v["changes"].as_array().is_some_and(|a| !a.is_empty())
            && !d.is_empty()
            && d.chars().all(|c| c.is_ascii_hexdigit())
    }) {
        put!("\ndigest of this patch: {d}\napply exactly this patch with: kaku code apply {id} --digest {d}");
    }
    out
}

fn print_code(v: &Value) {
    print!("{}", code_text(v));
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
                    Ok(v) => (
                        clean(v["id"].as_str().unwrap_or("")),
                        agent_place(&v["model"]),
                    ),
                    Err(c) => return c,
                };
                let (id, place) = id;
                say(json_mode, &json!({"event": "started", "task": id}), || {
                    format!("{id} started; {place}; nothing on disk changes until you apply it")
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
    let question = positional(args, 1, &["--experts"]).into_iter().next();
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
                if v["cut_short"] == true {
                    println!("(this answer reached the model's length limit and stops there)\n");
                }
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

/// `kaku share JOB FINDING [--out FILE] [--json]`: one finding of a delivered review, with a proof that it is part of the
/// signed release and nothing about the rest. The file is written for the owner to hand on; nothing is sent anywhere.
pub fn share(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    let out = crate::flag(args, "--out");
    let mut words: Vec<&String> = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => {}
            "--out" => {
                it.next();
            }
            other if other.starts_with("--") => break,
            _ => words.push(a),
        }
    }
    let usage = "usage: kaku share JOB FINDING [--out FILE] [--json]   (a delivered review and the number of one of its findings)";
    let (job, finding) = match words.as_slice() {
        [job, n]
            if job.len() > 4
                && job.starts_with("job-")
                && job[4..].chars().all(|c| c.is_ascii_digit()) =>
        {
            match n.parse::<u32>() {
                Ok(n) if n > 0 => (job.to_string(), n),
                _ => return fail(json_mode, USAGE, usage),
            }
        }
        _ => return fail(json_mode, USAGE, usage),
    };
    crate::tui::with_service(&serve_args(args), |dir| {
        let http = match connect(dir, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        let v = match call(
            &http,
            Method::Post,
            &format!("/api/jobs/{job}/share"),
            Some(json!({"finding": finding})),
        ) {
            Ok((200, v)) => v,
            Ok((s, v)) => {
                let (m, c) = refusal(s, &v);
                return fail(json_mode, c, &m);
            }
            Err(e) => return fail(json_mode, UNAVAILABLE, &e),
        };
        let Some(bytes) = v["share"]
            .as_str()
            .and_then(|t| kk_evidence::b64::decode(t).ok())
        else {
            return fail(
                json_mode,
                UNAVAILABLE,
                "the service answered without a share",
            );
        };
        let name = v["name"].as_str().unwrap_or("finding.kks").to_string();
        let path = std::path::PathBuf::from(out.clone().unwrap_or(name));
        // never over a file that exists
        let written = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, &bytes));
        if let Err(e) = written {
            return fail(
                json_mode,
                USAGE,
                &format!("cannot write {}: {e}", path.display()),
            );
        }
        if json_mode {
            println!(
                "{}",
                json!({"file": path, "files": v["files"], "total_files": v["total_files"], "capsule_sha256": v["capsule_sha256"], "signer": v["signer"]})
            );
        } else {
            println!("Wrote {}", path.display());
            println!("It shows: {}", clean(v["shows"].as_str().unwrap_or("")));
            println!("It hides: {}", clean(v["hides"].as_str().unwrap_or("")));
            println!(
                "Anyone can check it with: kaku verify {} --pin {}",
                path.display(),
                clean(v["signer_public"].as_str().unwrap_or("YOUR-PUBLIC-KEY"))
            );
        }
        OK
    })
}

/// Text for the terminal, line by line: control characters are dropped from each line and the line breaks are kept.
fn clean_lines(text: &str) -> String {
    text.lines().map(clean).collect::<Vec<_>>().join("\n")
}

/// `kaku notes FILE [--title TEXT] [--out FILE] [--json]`: notes from a meeting transcript, each point kept only when the
/// words it quotes are in the transcript. The text goes to the answering model of the running service and nowhere else.
pub fn notes(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    let out = crate::flag(args, "--out");
    let title = crate::flag(args, "--title");
    let usage = "usage: kaku notes FILE [--title TEXT] [--out FILE] [--json]   (a transcript as a text file; - reads it from standard input)";
    let Some(file) = args
        .get(1)
        .filter(|a| !a.starts_with("--") || a.as_str() == "-")
    else {
        return fail(json_mode, USAGE, usage);
    };
    let text = if file == "-" {
        let mut t = String::new();
        match std::io::Read::read_to_string(&mut std::io::stdin(), &mut t) {
            Ok(_) => t,
            Err(e) => {
                return fail(
                    json_mode,
                    USAGE,
                    &format!("cannot read standard input: {e}"),
                )
            }
        }
    } else {
        match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) => {
                return fail(
                    json_mode,
                    USAGE,
                    &format!("cannot read {file} as text: {e}"),
                )
            }
        }
    };
    crate::tui::with_service(&serve_args(args), |dir| {
        let http = match connect(dir, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        let mut body = json!({"text": text});
        if let Some(t) = &title {
            body["title"] = json!(t);
        }
        let v = match http.post_slow("/api/meeting-notes", &body) {
            Ok((200, v)) => v,
            Ok((s, v)) => {
                let (m, c) = refusal(s, &v);
                return fail(json_mode, c, &m);
            }
            Err(e) => return fail(json_mode, UNAVAILABLE, &e),
        };
        let markdown = v["markdown"].as_str().unwrap_or("").to_string();
        if let Some(path) = &out {
            // never over a file that exists
            let written = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .and_then(|mut f| std::io::Write::write_all(&mut f, markdown.as_bytes()));
            if let Err(e) = written {
                return fail(json_mode, USAGE, &format!("cannot write {path}: {e}"));
            }
        }
        if json_mode {
            println!("{v}");
            return OK;
        }
        if out.is_none() {
            println!("{}", clean_lines(&markdown));
        } else {
            println!("Wrote {}", out.as_deref().unwrap_or(""));
        }
        let n = |k: &str| v[k].as_u64().unwrap_or(0);
        let count = |k: &str| v[k].as_array().map_or(0, Vec::len);
        eprintln!(
            "{} checked against the transcript, {} not backed by it.",
            count("checked"),
            count("unchecked")
        );
        if v["route"]["stays_here"].as_bool() == Some(true) {
            eprintln!("The model that read this runs on this computer.");
        } else {
            eprintln!(
                "The model that read this is at {}, not on this computer.",
                clean(v["route"]["host"].as_str().unwrap_or("another computer"))
            );
        }
        if n("swapped") > 0 {
            match n("swapped") {
                1 => eprintln!("1 name or personal value went as a placeholder and was put back."),
                k => eprintln!(
                    "{k} names and personal values went as placeholders and were put back."
                ),
            }
        }
        if n("withheld_lines") > 0 {
            match n("withheld_lines") {
                1 => eprintln!("1 line that holds a secret was not sent."),
                k => eprintln!("{k} lines that hold a secret were not sent."),
            }
        }
        if n("unreadable_parts") > 0 {
            eprintln!(
                "{} of {} parts came back in a form that could not be read, so their points are missing.",
                n("unreadable_parts"),
                n("parts")
            );
        }
        OK
    })
}

/// `kaku stop ID [--json]`: stops a running review (job-N), coding task (code-N), project question (question-N) or team run (team-...). Nothing a stopped
/// review or coding task found is offered afterwards.
pub fn stop(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    let id = args.iter().skip(1).find(|a| !a.starts_with("--"));
    let path = match id.map(String::as_str) {
        Some(i) if i.starts_with("job-") && i[4..].chars().all(|c| c.is_ascii_digit()) && i.len() > 4 => format!("/api/jobs/{i}/stop"),
        Some(i) if i.starts_with("code-") && i[5..].chars().all(|c| c.is_ascii_digit()) && i.len() > 5 => format!("/api/code/{i}/stop"),
        Some(i) if i.starts_with("question-") && i[9..].chars().all(|c| c.is_ascii_digit()) && i.len() > 9 => format!("/api/questions/{i}/stop"),
        Some(i) if i.len() == 17 && i.starts_with("team-") && i[5..].chars().all(|c| c.is_ascii_hexdigit()) => format!("/api/team/{i}/stop"),
        _ => return fail(json_mode, USAGE, "usage: kaku stop ID   (a review job-N, a coding task code-N, a question about a project question-N or a team run team-...; kaku work lists what is running)"),
    };
    crate::tui::with_service(&serve_args(args), |dir| {
        let http = match connect(dir, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        match call(&http, Method::Post, &path, Some(json!({}))) {
            Ok((200, v)) => {
                say(json_mode, &v, || {
                    if v["stopping"] == false {
                        "Stopped.".into()
                    } else if v["at_once"] == false {
                        "It will stop at the end of the step in progress. Nothing it found will be offered.".into()
                    } else {
                        "Stopping now.".into()
                    }
                });
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

/// `kaku work [--json]`: everything that is working now (reviews, coding tasks, team runs), with a mark on what has shown
/// no progress for a while. Exit 0 when nothing has gone quiet, 3 when something has.
pub fn work(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    // this command is given the whole line, its own name first
    if let Some(code) = refuse_stray("work", &args[1..], &["--json"], json_mode) {
        return code;
    }
    crate::tui::with_service(&serve_args(args), |dir| {
        let http = match connect(dir, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        match call(&http, Method::Get, "/api/work", None) {
            Ok((200, v)) => {
                say(json_mode, &v, || {
                    let rows: Vec<String> = v["items"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|i| {
                            let mins = (i["quiet_for"].as_i64().unwrap_or(0) / 60).max(1);
                            format!(
                                "{:6}  {:17}  {}  {}{}",
                                clean(i["kind"].as_str().unwrap_or("")),
                                clean(i["id"].as_str().unwrap_or("")),
                                if i["quiet"] == true {
                                    format!("QUIET for {mins} min")
                                } else {
                                    format!("progress {mins} min ago")
                                },
                                clean(i["title"].as_str().unwrap_or("")),
                                if i["can_stop"] == true {
                                    ""
                                } else {
                                    "  (ends by itself)"
                                }
                            )
                        })
                        .collect();
                    if rows.is_empty() {
                        "Nothing is working right now.".into()
                    } else {
                        rows.join("\n")
                    }
                });
                if v["quiet"].as_u64().unwrap_or(0) > 0 {
                    3
                } else {
                    OK
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

fn team_words(v: &Value) -> String {
    let mut out = format!(
        "{}  {}\n{}\n",
        clean(v["id"].as_str().unwrap_or("")),
        clean(v["status"].as_str().unwrap_or("")),
        clean(v["goal"].as_str().unwrap_or(""))
    );
    for t in v["tasks"].as_array().into_iter().flatten() {
        let after: Vec<String> = t["after"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n.as_u64())
            .map(|n| n.to_string())
            .collect();
        out.push_str(&format!(
            "\n{}. [{}] {}{}: {}\n",
            t["n"],
            clean(t["state"].as_str().unwrap_or("")),
            clean(t["expert_name"].as_str().unwrap_or("")),
            if after.is_empty() {
                String::new()
            } else {
                format!(" (after {})", after.join(", "))
            },
            clean(t["task"].as_str().unwrap_or(""))
        ));
        if let Some(q) = t["question"].as_str().filter(|q| !q.is_empty()) {
            out.push_str(&format!("   asks you: {}\n", clean(q)));
        }
        if let Some(n) = t["note"].as_str().filter(|n| !n.is_empty()) {
            out.push_str(&format!("   why: {}\n", clean(n)));
        }
        if let Some(r) = t["result"].as_str().filter(|r| !r.is_empty()) {
            for line in r.lines() {
                out.push_str(&format!("   {}\n", clean(line)));
            }
        }
    }
    if let Some(f) = v["final"].as_str() {
        out.push_str(&format!("\nResult:\n{}\n", clean(f)));
    } else if let Some(n) = v["final_note"].as_str().filter(|n| !n.is_empty()) {
        out.push_str(&format!("\nNo result from the lead: {}\n", clean(n)));
    }
    out
}

/// `kaku team plan GOAL [--experts ID,ID]`, `kaku team list`, `kaku team show ID`, `kaku team start ID [--wait]`,
/// `kaku team stop ID`, `kaku team answer ID TASK TEXT`, `kaku team delete ID`: a lead's plan of tasks for experts.
/// A plan is never started by making it. `--wait` follows a run until it ends or needs the owner (exit 3).
pub fn team(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    let wait = args.iter().any(|a| a == "--wait");
    let experts = crate::flag(args, "--experts");
    // the command's own words come first; from the first other option on, the rest belongs to the service (its values are not words)
    let mut words: Vec<&String> = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" | "--wait" => {}
            "--experts" => {
                it.next();
            }
            other if other.starts_with("--") => break,
            _ => words.push(a),
        }
    }
    let usage = "usage: kaku team plan GOAL [--experts ID,ID] | list | show ID | start ID [--wait] | stop ID | answer ID TASK TEXT | delete ID   (add --json for one object)";
    let id_ok = |id: &str| {
        id.len() == 17 && id.starts_with("team-") && id[5..].chars().all(|c| c.is_ascii_hexdigit())
    };
    let (method, path, body): (Method, String, Option<Value>) = match words.as_slice() {
        [w, goal] if w.as_str() == "plan" && !goal.trim().is_empty() => {
            let mut b = json!({"goal": goal});
            if let Some(e) = &experts {
                b["experts"] = json!(e
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>());
            }
            (Method::Post, "/api/team".into(), Some(b))
        }
        [w] if w.as_str() == "list" => (Method::Get, "/api/team".into(), None),
        [w, id] if w.as_str() == "show" && id_ok(id) => {
            (Method::Get, format!("/api/team/{id}"), None)
        }
        [w, id] if matches!(w.as_str(), "start" | "stop" | "delete") && id_ok(id) => {
            (Method::Post, format!("/api/team/{id}/{w}"), Some(json!({})))
        }
        [w, id, task, text]
            if w.as_str() == "answer" && id_ok(id) && task.parse::<usize>().is_ok() =>
        {
            (
                Method::Post,
                format!("/api/team/{id}/answer"),
                Some(json!({"task": task.parse::<usize>().unwrap_or(0), "answer": text})),
            )
        }
        _ => return fail(json_mode, USAGE, usage),
    };
    let what = words[0].clone();
    crate::tui::with_service(&serve_args(args), |dir| {
        let http = match connect(dir, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        let mut v = match call(&http, method, &path, body.clone()) {
            Ok((s, v)) if s < 300 => v,
            Ok((s, v)) => {
                let (m, c) = refusal(s, &v);
                return fail(json_mode, c, &m);
            }
            Err(e) => return fail(json_mode, UNAVAILABLE, &e),
        };
        if what == "list" {
            say(json_mode, &v, || {
                let rows: Vec<String> = v["runs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|r| {
                        format!(
                            "{}  {:11}  {} of {} done  {}",
                            clean(r["id"].as_str().unwrap_or("")),
                            clean(r["status"].as_str().unwrap_or("")),
                            r["done"],
                            r["tasks"],
                            clean(r["goal"].as_str().unwrap_or(""))
                        )
                    })
                    .collect();
                if rows.is_empty() {
                    "No team runs yet.".into()
                } else {
                    rows.join("\n")
                }
            });
            return OK;
        }
        if matches!(what.as_str(), "stop" | "delete") {
            say(json_mode, &v, || {
                if what == "delete" {
                    "Deleted.".into()
                } else if v["stopping"] == true {
                    "Stopping: the answer being written is cut off and no further task is asked."
                        .into()
                } else {
                    "Stopped.".into()
                }
            });
            return OK;
        }
        if wait && matches!(what.as_str(), "start" | "answer") {
            let id = v["id"].as_str().unwrap_or("").to_string();
            let started = Instant::now();
            while (v["status"] == "running" || v["working"] == true)
                && started.elapsed() < Duration::from_secs(3600)
            {
                std::thread::sleep(Duration::from_millis(500));
                match call(&http, Method::Get, &format!("/api/team/{id}"), None) {
                    Ok((200, next)) => v = next,
                    Ok((s, e)) => {
                        let (m, c) = refusal(s, &e);
                        return fail(json_mode, c, &m);
                    }
                    Err(e) => return fail(json_mode, UNAVAILABLE, &e),
                }
            }
        }
        say(json_mode, &v, || {
            let mut text = team_words(&v);
            if v["status"] == "planned" {
                text.push_str(&format!(
                    "\nNothing has run. Start it with: kaku team start {} --wait\n",
                    clean(v["id"].as_str().unwrap_or(""))
                ));
            }
            text.trim_end().to_string()
        });
        // a run that waits for the owner is not a failure, but a script must be able to tell
        if v["status"] == "waiting" {
            3
        } else {
            OK
        }
    })
}

const EXPLORE_USAGE: &str = "usage: kaku explore DIR QUESTION... [--depth quick|normal|thorough] [--json] [--timeout SECONDS]\n       kaku explore show ID | list | forget ID";

fn question_id_ok(id: &str) -> bool {
    id.strip_prefix("question-")
        .is_some_and(|n| !n.is_empty() && n.len() <= 18 && n.chars().all(|c| c.is_ascii_digit()))
}

/// A question and its answer as text: the agent's words, each line it rests on with what the file holds there (a line not
/// in the file is shown as the agent's words), the scripts run with what they printed.
fn print_question(v: &Value) {
    println!(
        "{}  {}",
        clean(v["id"].as_str().unwrap_or("")),
        clean(v["question"].as_str().unwrap_or(""))
    );
    if v["state"] == "failed" {
        println!("No answer: {}", clean(v["failure"].as_str().unwrap_or("")));
        return;
    }
    let a = &v["answer"];
    match a["ending"].as_str().unwrap_or("") {
        "answered" => {}
        "unchecked" => println!("The agent answered without naming the lines its answer rests on, so nothing in it was checked."),
        "turn_limit" => println!("The agent used all its steps without answering. Ask again more narrowly, or with --depth thorough."),
        "stuck" => println!("The agent repeated the same step and was stopped."),
        "stopped" => println!("Stopped."),
        _ => println!("The model stopped answering part way."),
    }
    if let Some(t) = a["text"].as_str().filter(|t| !t.is_empty()) {
        println!(
            "\nThe agent's answer:\n  {}",
            clean(t).replace('\n', "\n  ")
        );
    }
    let places = a["places"].as_array().cloned().unwrap_or_default();
    if places.is_empty() {
        println!("\nIt names no line it rests on, so nothing in it was checked.");
    } else {
        println!(
            "\n{} of the {} {} it rests on {} found:",
            a["checked"],
            places.len(),
            plural(places.len() as u64, "line", "lines"),
            if places.len() == 1 { "was" } else { "were" }
        );
        println!(
            "(A line found shows the answer rests on real code, not that the answer is right.)"
        );
        for p in &places {
            let file = clean(p["file"].as_str().unwrap_or("?"));
            match p["found"]["state"].as_str().unwrap_or("") {
                s @ ("at_line" | "moved") => {
                    let at = if s == "moved" {
                        &p["found"]["line"]
                    } else {
                        &p["line"]
                    };
                    println!("  {file}:{at}  found");
                    for l in p["text"].as_str().unwrap_or("").lines() {
                        println!("      {}", clean(l));
                    }
                }
                other => println!(
                    "  {file}:{}  {}; the agent wrote: {}",
                    p["line"],
                    match other {
                        "not_found" => "NOT in the file",
                        "too_short" => "too short to check",
                        "withheld" => "a withheld file the agent could not read",
                        _ => "no such file",
                    },
                    clean(p["quote"].as_str().unwrap_or(""))
                ),
            }
        }
    }
    if let Some(nf) = a["not_found"].as_str().filter(|t| !t.is_empty()) {
        println!("\nWhat it could not find: {}", clean(nf));
    }
    for (i, k) in a["checks"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let run = &k["run"];
        println!(
            "\nCheck {} ({}), run in the sealed sandbox: {}. Script:",
            i + 1,
            clean(k["language"].as_str().unwrap_or("")),
            if run["ran"] != true {
                "could not be run"
            } else if run["timed_out"] == true {
                "stopped after 30 seconds"
            } else if run["exit_ok"] == true {
                "ended without an error"
            } else {
                "ended with an error"
            }
        );
        for l in k["script"].as_str().unwrap_or("").lines() {
            println!("      {}", clean(l));
        }
        println!("  It printed:");
        for l in run["output"].as_str().unwrap_or("").lines() {
            println!("      {}", clean(l));
        }
    }
    let read = a["files_read"].as_array().map_or(0, Vec::len) as u64;
    let withheld = a["withheld"].as_u64().unwrap_or(0);
    println!(
        "\n{read} {} read, {} {}.{}",
        plural(read, "file", "files"),
        a["turns"],
        plural(a["turns"].as_u64().unwrap_or(0), "step", "steps"),
        if withheld > 0 {
            format!(
                " {withheld} {} withheld from the agent.",
                plural(withheld, "file was", "files were")
            )
        } else {
            String::new()
        }
    );
    let names: Vec<String> = a["withheld_files"]
        .as_array()
        .map(|v| v.iter().filter_map(|x| x.as_str().map(clean)).collect())
        .unwrap_or_default();
    if !names.is_empty() {
        println!(
            "Withheld (their name or content says they hold secrets or private data): {}",
            names.join(", ")
        );
    }
}

/// `kaku explore DIR QUESTION`: an agent reads the project (with the model here or in confidential hardware) and answers; every line its answer rests on is
/// checked against the file. Exit 0 when every line was found, 3 when the answer rests on a line that was not, or on none.
pub fn explore(args: &[String]) -> u8 {
    let json_mode = args.iter().any(|a| a == "--json");
    let words = plain_words(
        args,
        &[
            "--data-dir",
            "--timeout",
            "--depth",
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
        return fail(json_mode, USAGE, EXPLORE_USAGE);
    };
    let depth = crate::flag(args, "--depth").unwrap_or_else(|| "normal".into());
    if !matches!(depth.as_str(), "quick" | "normal" | "thorough") {
        return fail(
            json_mode,
            USAGE,
            &format!(
                "--depth is quick, normal or thorough, not {:?}",
                clean(&depth)
            ),
        );
    }
    let timeout = crate::flag(args, "--timeout")
        .and_then(|t| t.parse::<u64>().ok())
        .unwrap_or(3600)
        .clamp(10, 86_400);
    crate::tui::with_service(&serve_args(args), |data| {
        let http = match connect(data, json_mode) {
            Ok(h) => h,
            Err(c) => return c,
        };
        let get = |id: &str| call(&http, Method::Get, &format!("/api/questions/{id}"), None);
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
        let verdict = |v: &Value| -> u8 {
            let a = &v["answer"];
            if v["state"] != "answered" {
                FAILED
            } else if a["places"].as_array().is_none_or(Vec::is_empty) || a["unchecked"] != 0 {
                NOTHING
            } else {
                OK
            }
        };
        match first.as_str() {
            "list" => match answer(call(&http, Method::Get, "/api/questions", None), 200) {
                Ok(v) => {
                    let list = v["questions"].as_array().cloned().unwrap_or_default();
                    if json_mode {
                        println!("{}", json!({"questions": list}));
                    } else if list.is_empty() {
                        println!("no questions yet");
                    } else {
                        for q in &list {
                            println!(
                                "{}  {}  {}",
                                clean(q["id"].as_str().unwrap_or("")),
                                clean(q["state"].as_str().unwrap_or("")),
                                clean(q["question"].as_str().unwrap_or(""))
                            );
                        }
                    }
                    OK
                }
                Err(c) => c,
            },
            "show" | "forget" => {
                let Some(id) = words.get(1).filter(|i| question_id_ok(i)) else {
                    return fail(json_mode, USAGE, "a question is named like question-3");
                };
                if first == "forget" {
                    return match answer(
                        call(
                            &http,
                            Method::Post,
                            &format!("/api/questions/{id}/forget"),
                            Some(json!({})),
                        ),
                        200,
                    ) {
                        Ok(v) => {
                            say(json_mode, &v, || format!("{id} is forgotten"));
                            OK
                        }
                        Err(c) => c,
                    };
                }
                match answer(get(id), 200) {
                    Ok(v) => {
                        if json_mode {
                            println!("{v}")
                        } else {
                            print_question(&v)
                        }
                        verdict(&v)
                    }
                    Err(c) => c,
                }
            }
            _ => {
                let Ok(folder) = std::path::Path::new(&first).canonicalize() else {
                    return fail(
                        json_mode,
                        USAGE,
                        &format!("that folder does not exist\n{EXPLORE_USAGE}"),
                    );
                };
                let question = words[1..].join(" ");
                if question.trim().is_empty() {
                    return fail(json_mode, USAGE, EXPLORE_USAGE);
                }
                let id = match answer(
                    call(
                        &http,
                        Method::Post,
                        "/api/questions",
                        Some(
                            json!({"folder": folder.to_string_lossy(), "question": question, "depth": depth}),
                        ),
                    ),
                    202,
                ) {
                    Ok(v) => (
                        (
                            clean(v["id"].as_str().unwrap_or("")),
                            agent_place(&v["model"]),
                        ),
                        v["forgotten"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|x| x.as_str().map(clean))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default(),
                    ),
                    Err(c) => return c,
                };
                let ((id, place), forgotten) = id;
                say(
                    json_mode,
                    &json!({"event": "started", "question": id, "forgotten": forgotten}),
                    || {
                        let mut line = format!("{id} started; {place}, and nothing is changed");
                        if !forgotten.is_empty() {
                            line.push_str(&format!(
                                "\n100 questions are kept: {} {} forgotten to make room",
                                forgotten.join(", "),
                                if forgotten.len() == 1 { "was" } else { "were" }
                            ));
                        }
                        line
                    },
                );
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
                            &json!({"event": "progress", "question": id, "detail": line}),
                            || format!("{id}  {line}"),
                        );
                    }
                    shown = progress.len();
                    if v["state"] != "running" {
                        break v;
                    }
                    if started.elapsed() > Duration::from_secs(timeout) {
                        return fail(json_mode, UNAVAILABLE, &format!("{id} was not answered within {timeout} seconds; it is still running, see it with kaku explore show {id}"));
                    }
                    std::thread::sleep(Duration::from_millis(700));
                };
                if json_mode {
                    println!("{}", json!({"event": "finished", "question": v}));
                } else {
                    print_question(&v);
                }
                verdict(&v)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ex02_the_service_flags_and_their_values_are_never_read_as_a_commands_input_wherever_they_stand(
    ) {
        let a = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let near = [
            "--route",
            "near",
            "--near-model",
            "Qwen/Qwen3.8-27B",
            "--near-key-file",
            "/home/me/.secrets/near-ai.env",
        ];
        // before, between and after the words: only the words are left
        let mut explore = vec!["explore"];
        explore.extend(near);
        explore.extend(["proj", "Where", "--depth", "quick", "is", "logout?"]);
        explore.extend(["--data-dir", "/d"]);
        assert_eq!(
            plain_words(&a(&explore), &["--depth"]),
            a(&["proj", "Where", "is", "logout?"])
        );
        let mut chat = vec!["chat"];
        chat.extend(near);
        chat.push("hello there");
        assert_eq!(
            positional(&a(&chat), 1, &["--session"]),
            vec![&"hello there".to_string()]
        );
        let mut review = vec!["review", "--route", "near", "--for", "Acme", "proj"];
        review.extend(["--outside-model", "m", "--near-url", "https://x"]);
        assert_eq!(
            positional(&a(&review), 1, &["--for", "--timeout"]),
            vec![&"proj".to_string()]
        );
        // a flag the command does not take a value for leaves the next word a word
        assert_eq!(
            positional(&a(&["chat", "--keep", "hi"]), 1, &["--session"]),
            vec![&"hi".to_string()]
        );
    }

    #[test]
    fn ex03_the_start_line_says_where_the_agents_model_is() {
        assert_eq!(
            agent_place(&json!({"stays_here": true})),
            "the agent reads the project on this computer"
        );
        let away = agent_place(
            &json!({"stays_here": false, "class": "verified_tee", "host": "cloud-api.near.ai\u{1b}[2J"}),
        );
        assert!(
            away.contains("confidential hardware at cloud-api.near.ai")
                && away.contains("given the files it reads")
                && !away.contains('\u{1b}'),
            "{away}"
        );
    }

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

#[cfg(test)]
mod code_text_tests {
    use serde_json::json;

    #[test]
    fn the_digest_and_the_command_that_applies_it_are_under_a_ready_patch_only() {
        let ready = json!({"id": "code-3", "state": "ready", "tests": null, "digest": "ab12", "changes": [{"kind": "modified", "path": "a.py", "added": 1, "removed": 0, "diff": "+x"}]});
        let t = super::code_text(&ready);
        assert!(
            t.contains("digest of this patch: ab12")
                && t.contains("kaku code apply code-3 --digest ab12"),
            "{t}"
        );
        for v in [
            json!({"id": "code-3", "state": "running", "tests": null, "digest": "ab12", "changes": [{"path": "a.py"}]}),
            json!({"id": "code-3", "state": "ready", "tests": null, "digest": "ab12", "changes": []}),
            json!({"id": "code-3", "state": "ready", "tests": null, "digest": "ab 12; rm", "changes": [{"path": "a.py"}]}),
        ] {
            assert!(!super::code_text(&v).contains("--digest"), "{v}");
        }
    }
}
