use crate::app::{App, Event, Req};
use crate::http::Http;
use crate::ui::{draw, Theme};
use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
    Event as CtEvent,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{stdout, IsTerminal};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(
        stdout(),
        DisableBracketedPaste,
        DisableFocusChange,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    );
}

pub fn run(data_dir: &Path, resume: Option<crate::app::Resume>) -> Result<(), String> {
    run_with(data_dir, resume, crate::settings::Settings::from_env())
}

pub fn run_with(
    data_dir: &Path,
    resume: Option<crate::app::Resume>,
    settings: crate::settings::Settings,
) -> Result<(), String> {
    if !stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return Err("the full-screen workspace needs a terminal; use `kaku --help` for the scripting commands".into());
    }
    let http = Http::connect(data_dir)?;
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        prev(info);
    }));
    enable_raw_mode().map_err(|e| e.to_string())?;
    execute!(
        stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableFocusChange
    )
    .map_err(|e| e.to_string())?;
    let result = event_loop(http, data_dir, resume, settings);
    restore();
    result
}

fn event_loop(
    http: Http,
    data_dir: &Path,
    resume: Option<crate::app::Resume>,
    settings: crate::settings::Settings,
) -> Result<(), String> {
    let mut term = Terminal::new(CrosstermBackend::new(stdout())).map_err(|e| e.to_string())?;
    let theme = Theme {
        color: settings.color,
        palette: settings.theme,
    };
    let mut app = App::new();
    app.apply_settings(&settings);
    app.chat_project = std::env::current_dir()
        .ok()
        .and_then(|d| crate::project::detect(&d));
    app.approver = kk_evidence::approval::load_approver(data_dir)
        .ok()
        .flatten();
    let (req_tx, req_rx) = mpsc::channel::<Req>();
    let (ev_tx, ev_rx) = mpsc::channel::<Event>();
    {
        let ev_tx = ev_tx.clone();
        let cancel = app.cancel.clone();
        std::thread::spawn(move || {
            let mut http = http;
            while let Ok(req) = req_rx.recv() {
                let send = |http: &Http, req: &Req| {
                    if req.stream && req.path == "/api/chat/roundtable" {
                        let t = ev_tx.clone();
                        let id = req.id;
                        http.run_events(req, &cancel, &mut |v| {
                            let _ = t.send(Event::Table {
                                id,
                                line: v.clone(),
                            });
                        })
                    } else if req.stream {
                        let tx = ev_tx.clone();
                        let id = req.id;
                        http.run_stream(req, &cancel, &mut |t| {
                            let _ = tx.send(Event::Delta {
                                id,
                                text: t.to_string(),
                            });
                        })
                    } else {
                        http.run(req)
                    }
                };
                let mut resp = send(&http, &req);
                // the service was stopped and started again: connect to the new one and ask once more
                if Http::lost(&resp) && http.reconnect().is_ok() {
                    resp = send(&http, &req);
                }
                if ev_tx.send(Event::Resp(resp)).is_err() {
                    break;
                }
            }
        });
    }
    {
        let ev_tx = ev_tx.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(700));
            if ev_tx.send(Event::Tick).is_err() {
                break;
            }
        });
    }
    for r in app.start() {
        let _ = req_tx.send(r);
    }
    if let Some(r) = &resume {
        for r in app.resume_at_start(r.id.as_deref(), r.fork) {
            let _ = req_tx.send(r);
        }
    }
    for r in app.drain_background() {
        let _ = req_tx.send(r);
    }
    let notify = settings.notify.clone();
    let term_info = crate::attention::TermInfo::from_env();
    let mut shown_title: Option<Option<crate::attention::Attention>> = None;
    let mut dirty = true;
    loop {
        if shown_title != Some(app.waiting) {
            shown_title = Some(app.waiting);
            if let Some(t) = crate::attention::title(notify.as_deref(), app.waiting) {
                use std::io::Write;
                let mut out = stdout();
                let _ = out.write_all(t.as_bytes());
                let _ = out.flush();
            }
        }
        if let Some(what) = app.attention.take() {
            if let Some(b) =
                crate::attention::alert(notify.as_deref(), app.focused, &term_info, what)
            {
                use std::io::Write;
                let mut out = stdout();
                let _ = out.write_all(b.as_bytes());
                let _ = out.flush();
            }
        }
        if dirty {
            term.draw(|f| draw(f, &app, &theme))
                .map_err(|e| e.to_string())?;
            dirty = false;
        }
        if app.quit {
            // say the list was seen, and give the answer a moment, so what stops from now on is what the next launch reports
            let last = app.leaving();
            let id = last.id;
            if req_tx.send(last).is_ok() {
                let until = std::time::Instant::now() + Duration::from_millis(1500);
                while let Some(left) = until.checked_duration_since(std::time::Instant::now()) {
                    match ev_rx.recv_timeout(left) {
                        Ok(Event::Resp(r)) if r.id == id => break,
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
            }
            return Ok(());
        }
        if event::poll(Duration::from_millis(60)).map_err(|e| e.to_string())? {
            let ev = match event::read().map_err(|e| e.to_string())? {
                CtEvent::Key(k) => Some(Event::Key(k)),
                CtEvent::Paste(t) => Some(Event::Paste(t)),
                CtEvent::FocusGained => Some(Event::Focus(true)),
                CtEvent::FocusLost => Some(Event::Focus(false)),
                CtEvent::Resize(..) => {
                    dirty = true;
                    None
                }
                _ => None,
            };
            if let Some(ev) = ev {
                dirty = true;
                for r in app.handle(ev) {
                    let _ = req_tx.send(r);
                }
                for r in app.drain_background() {
                    let _ = req_tx.send(r);
                }
                if let Some(text) = app.clipboard_request.take() {
                    // the owner said yes: write the terminal's clipboard sequence for exactly this text, once
                    if let Some(seq) = crate::app::osc52(&text) {
                        use std::io::Write;
                        let mut out = stdout();
                        let _ = out.write_all(seq.as_bytes());
                        let _ = out.flush();
                    }
                }
                if let Some(text) = app.editor_request.take() {
                    // hand the screen to the editor, then take it back exactly as it was
                    restore();
                    let command = crate::editor::chosen(
                        std::env::var("VISUAL").ok(),
                        std::env::var("EDITOR").ok(),
                    );
                    let result = crate::editor::edit_with(&command, &text);
                    enable_raw_mode().map_err(|e| e.to_string())?;
                    execute!(
                        stdout(),
                        EnterAlternateScreen,
                        EnableBracketedPaste,
                        EnableFocusChange
                    )
                    .map_err(|e| e.to_string())?;
                    term.clear().map_err(|e| e.to_string())?;
                    dirty = true;
                    app.finish_edit(result);
                }
            }
        }
        while let Ok(ev) = ev_rx.try_recv() {
            dirty = true;
            for r in app.handle(ev) {
                let _ = req_tx.send(r);
            }
        }
    }
}
