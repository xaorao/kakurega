use crate::app::{App, Event, Req};
use crate::http::Http;
use crate::ui::{draw, Theme};
use crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste, Event as CtEvent};
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
        LeaveAlternateScreen,
        crossterm::cursor::Show
    );
}

pub fn run(data_dir: &Path) -> Result<(), String> {
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
    execute!(stdout(), EnterAlternateScreen, EnableBracketedPaste).map_err(|e| e.to_string())?;
    let result = event_loop(http, data_dir);
    restore();
    result
}

fn event_loop(http: Http, data_dir: &Path) -> Result<(), String> {
    let mut term = Terminal::new(CrosstermBackend::new(stdout())).map_err(|e| e.to_string())?;
    let theme = Theme::from_env();
    let mut app = App::new();
    app.approver = kk_evidence::approval::load_approver(data_dir)
        .ok()
        .flatten();
    let (req_tx, req_rx) = mpsc::channel::<Req>();
    let (ev_tx, ev_rx) = mpsc::channel::<Event>();
    {
        let ev_tx = ev_tx.clone();
        std::thread::spawn(move || {
            while let Ok(req) = req_rx.recv() {
                let resp = http.run(&req);
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
    loop {
        term.draw(|f| draw(f, &app, &theme))
            .map_err(|e| e.to_string())?;
        if app.quit {
            return Ok(());
        }
        if event::poll(Duration::from_millis(60)).map_err(|e| e.to_string())? {
            let ev = match event::read().map_err(|e| e.to_string())? {
                CtEvent::Key(k) => Some(Event::Key(k)),
                CtEvent::Paste(t) => Some(Event::Paste(t)),
                _ => None,
            };
            if let Some(ev) = ev {
                for r in app.handle(ev) {
                    let _ = req_tx.send(r);
                }
            }
        }
        while let Ok(ev) = ev_rx.try_recv() {
            for r in app.handle(ev) {
                let _ = req_tx.send(r);
            }
        }
    }
}
