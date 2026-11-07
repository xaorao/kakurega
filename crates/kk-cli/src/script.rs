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
        let started = Instant::now();
        let mut shown = 0usize;
        loop {
            match call(&http, Method::Get, &format!("/api/jobs/{id}"), None) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
