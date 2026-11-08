//! The workspace as plain lines: type a line, press Enter, read what is appended. Nothing is drawn over, nothing moves, and no
//! control sequence is ever written, so a screen reader, a braille display, a speech tool or a pipe reads it as it comes.
//!
//! It is the same program as the full-screen workspace: the same `App`, the same commands, the same approval, the same service.
//! Only the way in and out differs. Every state that the full-screen view shows in an overlay is a spoken prompt here that
//! says exactly what to type.

use crate::app::{App, Card, Event, Kind, Mode, Req};
use crate::http::Http;
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

/// One card as lines of plain text that start with what kind of message it is, so nothing depends on colour or symbols.
pub fn card_text(c: &Card) -> String {
    let label = match c.kind {
        Kind::User => "You",
        Kind::Info => "Note",
        Kind::Ok => "Done",
        Kind::Warn => "Warning",
        Kind::Error => "Error",
    };
    let lines: Vec<&str> = {
        let mut v: Vec<&str> = c.lines.iter().map(String::as_str).collect();
        while v.last().is_some_and(|l| l.trim().is_empty()) {
            v.pop();
        }
        v
    };
    match (&c.title, lines.as_slice()) {
        (Some(t), []) => format!("[{label}] {t}"),
        (Some(t), rest) => format!("[{label}] {t}\n{}", indent(rest)),
        (None, [one]) => format!("[{label}] {one}"),
        (None, rest) => format!("[{label}]\n{}", indent(rest)),
    }
}

fn indent(lines: &[&str]) -> String {
    lines
        .iter()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The question the owner is being asked, when the screen would be showing an overlay; None in the ordinary state.
pub fn mode_text(app: &App) -> Option<String> {
    match &app.mode {
        Mode::Normal => None,
        Mode::Confirm(c) => Some(format!(
            "[Question] {}\n  Type y and press Enter to say yes. Anything else says no.",
            c.prompt
        )),
        Mode::Approval(v) => {
            let want: String = v.digest.chars().take(crate::app::DIGEST_ACK_LEN).collect();
            let mut t = format!("[Approval needed] release {} ({})\n", v.job, v.aid);
            for l in &v.lines {
                t.push_str(&format!("  {l}\n"));
            }
            t.push_str(&format!(
                "  To approve, type the first {} characters of the digest, {want}, and press Enter. To refuse, type refuse and press Enter.",
                crate::app::DIGEST_ACK_LEN
            ));
            Some(t)
        }
        Mode::Picker(p) => {
            let mut t = format!("[Choose] {}\n", p.title);
            if p.loading {
                t.push_str("  Loading.");
                return Some(t);
            }
            let visible = p.visible();
            if visible.is_empty() {
                t.push_str("  Nothing matches.\n");
            }
            for (n, i) in visible.iter().take(30).enumerate() {
                let it = &p.items[*i];
                t.push_str(&format!("  {}. {} {}\n", n + 1, it.label, it.detail));
            }
            if visible.len() > 30 {
                t.push_str(&format!("  and {} more\n", visible.len() - 30));
            }
            t.push_str("  Type a number and press Enter to open it. Type words to narrow the list. Type cancel to close.");
            Some(t)
        }
    }
}

/// The error shown inside an overlay, written alone when it changes so the whole prompt is not read out again.
pub fn mode_error(app: &App) -> Option<String> {
    match &app.mode {
        Mode::Approval(v) => v.error.clone(),
        Mode::Picker(p) => p.error.clone(),
        _ => None,
    }
}

/// What a typed line does in the state the program is in: the keys it stands for, in order.
pub fn feed_line(app: &mut App, line: &str) -> Vec<Req> {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    let key = |code| {
        Event::Key(KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        })
    };
    let t = line.trim();
    let mut out = Vec::new();
    match &mut app.mode {
        Mode::Normal => {
            app.input = line.to_string();
            app.cursor = app.input.chars().count();
            out.extend(app.handle(key(KeyCode::Enter)));
        }
        Mode::Confirm(_) => {
            let yes = t.eq_ignore_ascii_case("y") || t.eq_ignore_ascii_case("yes");
            out.extend(app.handle(key(KeyCode::Char(if yes { 'y' } else { 'n' }))));
            // a prompt that ignores a stray letter would otherwise stay open
            if matches!(app.mode, Mode::Confirm(_)) && !yes {
                out.extend(app.handle(key(KeyCode::Esc)));
            }
        }
        Mode::Approval(v) => {
            if t.eq_ignore_ascii_case("refuse") {
                out.extend(app.handle(key(KeyCode::Esc)));
            } else {
                v.typed.clear();
                v.error = None;
                out.extend(app.handle(Event::Paste(t.to_string())));
                out.extend(app.handle(key(KeyCode::Enter)));
            }
        }
        Mode::Picker(p) => {
            if t.eq_ignore_ascii_case("cancel") {
                out.extend(app.handle(key(KeyCode::Esc)));
            } else if let Ok(n) = t.parse::<usize>() {
                if n >= 1 && n <= p.visible().len() {
                    p.selected = n - 1;
                    out.extend(app.handle(key(KeyCode::Enter)));
                } else {
                    p.error = Some(format!("There is no number {n} in the list."));
                }
            } else {
                p.filter = t.to_string();
                p.selected = 0;
                p.error = None;
            }
        }
    }
    out
}

/// Remembers what has been written so each thing is written once: how many cards, the last prompt, the last status line.
#[derive(Default)]
pub struct Printed {
    /// Whether the owner's own lines are written back. A terminal already shows what was typed, so they are skipped there.
    echo: bool,
    cards: usize,
    mode: Option<String>,
    error: Option<String>,
    status: String,
    waiting_noted: bool,
}

impl Printed {
    /// Starts with nothing written. `echo` says whether the owner's own lines are written back (not when a terminal shows them).
    pub fn new(echo: bool) -> Printed {
        Printed {
            echo,
            ..Printed::default()
        }
    }
}

/// The new text since the last call, in the order it happened. Empty when nothing is new.
pub fn news(app: &mut App, seen: &mut Printed) -> String {
    let mut out = String::new();
    let limit = app.live_card().unwrap_or(app.cards.len());
    for c in app.cards.get(seen.cards..limit).unwrap_or(&[]) {
        if c.kind == Kind::User && !seen.echo {
            continue;
        }
        out.push_str(&card_text(c));
        out.push('\n');
    }
    seen.cards = seen.cards.max(limit);
    if app.chat_waiting {
        if !seen.waiting_noted {
            out.push_str(
                "[Note] The assistant is answering. The answer appears when it is finished.\n",
            );
            seen.waiting_noted = true;
        }
    } else {
        seen.waiting_noted = false;
    }
    if let Some(a) = app.attention.take() {
        out.push_str(&format!("[Notice] {}.\n", capital(a.phrase())));
    }
    let status = app.status_line();
    if status != seen.status {
        out.push_str(&format!("[Status] {status}\n"));
        seen.status = status;
    }
    let mode = mode_text(app);
    if mode != seen.mode {
        if let Some(m) = &mode {
            out.push_str(m);
            out.push('\n');
        }
        seen.mode = mode;
    }
    let error = mode_error(app);
    if error != seen.error {
        if let Some(e) = &error {
            out.push_str(&format!("[Error] {e}\n"));
        }
        seen.error = error;
    }
    if let Some(_text) = app.clipboard_request.take() {
        out.push_str("[Note] Copying to the clipboard is not available in this mode. Use /export NAME to write it to a file.\n");
    }
    if app.editor_request.take().is_some() {
        out.push_str(
            "[Note] The editor is not available in this mode. Type the message on one line.\n",
        );
    }
    out
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

enum Msg {
    Ev(Event),
    Line(String),
    End,
}

pub fn run(data_dir: &Path, resume: Option<crate::app::Resume>) -> Result<(), String> {
    run_with(data_dir, resume, crate::settings::Settings::from_env())
}

pub fn run_with(
    data_dir: &Path,
    resume: Option<crate::app::Resume>,
    settings: crate::settings::Settings,
) -> Result<(), String> {
    let http = Http::connect(data_dir)?;
    let mut app = App::new();
    app.apply_settings(&settings);
    app.chat_project = std::env::current_dir()
        .ok()
        .and_then(|d| crate::project::detect(&d));
    app.approver = kk_evidence::approval::load_approver(data_dir)
        .ok()
        .flatten();
    let (req_tx, req_rx) = mpsc::channel::<Req>();
    let (tx, rx) = mpsc::channel::<Msg>();
    {
        let tx = tx.clone();
        let cancel = app.cancel.clone();
        std::thread::spawn(move || {
            while let Ok(req) = req_rx.recv() {
                let resp = if req.stream {
                    let t = tx.clone();
                    let id = req.id;
                    http.run_stream(&req, &cancel, &mut |piece| {
                        let _ = t.send(Msg::Ev(Event::Delta {
                            id,
                            text: piece.to_string(),
                        }));
                    })
                } else {
                    http.run(&req)
                };
                if tx.send(Msg::Ev(Event::Resp(resp))).is_err() {
                    break;
                }
            }
        });
    }
    {
        let tx = tx.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(700));
            if tx.send(Msg::Ev(Event::Tick)).is_err() {
                break;
            }
        });
    }
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                match line {
                    Ok(l) => {
                        if tx.send(Msg::Line(l)).is_err() {
                            return;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = tx.send(Msg::End);
        });
    }
    let mut out = std::io::stdout();
    let mut seen = Printed::new(!std::io::IsTerminal::is_terminal(&std::io::stdin()));
    let _ = writeln!(
        out,
        "Kakurega, plain-line mode. Type a line and press Enter. Type /help for the commands and /quit to leave."
    );
    let send = |reqs: Vec<Req>| {
        for r in reqs {
            let _ = req_tx.send(r);
        }
    };
    send(app.start());
    if let Some(r) = &resume {
        send(app.resume_at_start(r.id.as_deref(), r.fork));
    }
    send(app.drain_background());
    loop {
        let msg = rx.recv_timeout(Duration::from_millis(100));
        let mut ended = false;
        match msg {
            Ok(Msg::Ev(ev)) => send(app.handle(ev)),
            Ok(Msg::Line(l)) => send(feed_line(&mut app, &l)),
            Ok(Msg::End) => ended = true,
            Err(_) => {}
        }
        send(app.drain_background());
        let text = news(&mut app, &mut seen);
        if !text.is_empty() {
            let _ = out.write_all(text.as_bytes());
            let _ = out.flush();
        }
        if app.quit || ended {
            // say the list of stopped reviews was seen, and give the answer a moment
            let last = app.leaving();
            let id = last.id;
            if req_tx.send(last).is_ok() {
                let until = std::time::Instant::now() + Duration::from_millis(1500);
                while let Some(left) = until.checked_duration_since(std::time::Instant::now()) {
                    match rx.recv_timeout(left) {
                        Ok(Msg::Ev(Event::Resp(r))) if r.id == id => break,
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }
            let _ = writeln!(out, "Goodbye.");
            return Ok(());
        }
    }
}
