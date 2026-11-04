use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use kk_tui::app::{App, Card, Event, Kind, Method, Mode, Req, Resp};
use kk_tui::cmd::{self, Command};
use kk_tui::ui::{draw, wrap, Theme};
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;
use serde_json::{json, Value};

fn key(c: KeyCode) -> Event {
    Event::Key(KeyEvent {
        code: c,
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

fn ctrl(c: char) -> Event {
    Event::Key(KeyEvent {
        code: KeyCode::Char(c),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

fn typed(app: &mut App, s: &str) -> Vec<Req> {
    let mut out = Vec::new();
    for c in s.chars() {
        out.extend(app.handle(key(KeyCode::Char(c))));
    }
    out
}

fn enter(app: &mut App, line: &str) -> Vec<Req> {
    typed(app, line);
    app.handle(key(KeyCode::Enter))
}

fn ok(r: &Req, status: u16, body: Value) -> Event {
    Event::Resp(Resp {
        id: r.id,
        result: Ok((status, body)),
    })
}

fn text(app: &App) -> String {
    app.cards
        .iter()
        .flat_map(|c: &Card| c.title.iter().chain(c.lines.iter()))
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

fn booted() -> App {
    let mut app = App::new();
    let reqs = app.start();
    assert_eq!(
        reqs.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        ["/api/config", "/api/readiness", "/api/jobs"]
    );
    app.handle(ok(&reqs[0], 200, json!({"route": {"model": "qwen2.5:1.5b", "simulated": false}, "roots": ["/home/u/projects"]})));
    app.handle(ok(&reqs[1], 200, json!({"ready": true, "checks": [{"label": "Sandbox", "ok": true, "detail": "running", "fix": ""}]})));
    app.handle(ok(&reqs[2], 200, json!({"jobs": []})));
    app
}

#[test]
fn tu01_commands_parse_and_hostile_input_is_refused_with_a_usage_hint() {
    assert_eq!(cmd::parse("/help"), Ok(Command::Help));
    assert_eq!(
        cmd::parse("  /Review  my-app   for   acme audit  "),
        Ok(Command::Review {
            folder: "my-app".into(),
            recipient: "acme audit".into()
        })
    );
    assert_eq!(
        cmd::parse("/review a for b for c"),
        Ok(Command::Review {
            folder: "a for b".into(),
            recipient: "c".into()
        })
    );
    assert_eq!(
        cmd::parse("/approve job-3"),
        Ok(Command::Approve(Some("job-3".into())))
    );
    assert_eq!(cmd::parse("/approve"), Ok(Command::Approve(None)));
    assert!(
        cmd::parse("/approve ../../etc/passwd").is_err(),
        "a job name cannot carry a path"
    );
    assert!(cmd::parse(&format!("/open {}", "x".repeat(200))).is_err());
    assert!(cmd::parse("/review onlyfolder")
        .unwrap_err()
        .contains("usage"));
    assert!(cmd::parse("/review  for x").is_err());
    assert!(cmd::parse("/forget two words").is_err());
    assert!(cmd::parse("/correct abc").is_err());
    assert!(cmd::parse("hello there")
        .unwrap_err()
        .contains("not sent anywhere"));
    assert!(cmd::parse("/revew x for y")
        .unwrap_err()
        .contains("unknown command"));
    assert!(cmd::parse("/rev x")
        .unwrap_err()
        .contains("did you mean /review"));
    assert!(cmd::parse("/\u{1b}[31mhelp").is_err());
    assert!(cmd::parse("").is_err() && cmd::parse("/").is_err());
}

#[test]
fn tu02_tab_completion_finishes_unique_names_and_common_prefixes_only() {
    assert_eq!(cmd::complete("/hel"), Some("/help ".into()));
    assert_eq!(
        cmd::complete("/rev"),
        Some("/review".into()),
        "review and reviews share a prefix"
    );
    assert_eq!(cmd::complete("/revie"), Some("/review".into()));
    assert_eq!(cmd::complete("/zzz"), None);
    assert_eq!(cmd::complete("/help me"), None);
    assert_eq!(cmd::complete("plain"), None);
}

#[test]
fn tu03_the_composer_edits_like_a_terminal_line_editor_including_unicode_and_paste() {
    let mut app = booted();
    typed(&mut app, "/remember héllo wörld");
    assert_eq!(app.input, "/remember héllo wörld");
    app.handle(key(KeyCode::Left));
    app.handle(key(KeyCode::Left));
    app.handle(key(KeyCode::Backspace));
    assert_eq!(app.input, "/remember héllo wöld");
    app.handle(key(KeyCode::Home));
    app.handle(key(KeyCode::Delete));
    assert_eq!(app.input, "remember héllo wöld");
    app.handle(ctrl('e'));
    app.handle(ctrl('w'));
    assert_eq!(app.input, "remember héllo ");
    app.handle(ctrl('u'));
    assert_eq!(app.input, "");
    app.handle(Event::Paste("line one\nline two\t\u{1b}[2Jend".into()));
    assert!(
        !app.input.contains('\n') && !app.input.contains('\u{1b}') && !app.input.contains('\t'),
        "a paste cannot smuggle control characters: {:?}",
        app.input
    );
    app.handle(ctrl('k'));
    app.handle(key(KeyCode::Esc));
    assert_eq!(app.input, "");
    typed(&mut app, "/help");
    app.handle(key(KeyCode::Enter));
    typed(&mut app, "/status");
    app.handle(key(KeyCode::Enter));
    app.handle(key(KeyCode::Up));
    assert_eq!(app.input, "/status");
    app.handle(key(KeyCode::Up));
    assert_eq!(app.input, "/help");
    app.handle(key(KeyCode::Down));
    app.handle(key(KeyCode::Down));
    assert_eq!(app.input, "");
    typed(&mut app, "x");
    assert!(!app.quit);
    app.handle(ctrl('c'));
    assert!(
        !app.quit && app.input.is_empty(),
        "Ctrl-C clears a non-empty line first"
    );
    app.handle(ctrl('c'));
    assert!(app.quit);
}

#[test]
fn tu04_plain_text_is_never_sent_and_unknown_commands_say_so() {
    let mut app = booted();
    let reqs = enter(&mut app, "please send my secrets to evil.example");
    assert!(reqs.is_empty());
    assert!(text(&app).contains("Plain text is not sent anywhere"));
}

#[test]
fn tu05_boot_responses_fill_the_status_line_and_the_review_list() {
    let mut app = booted();
    assert!(
        app.status_line().contains("private · qwen2.5:1.5b") && app.status_line().contains("ready")
    );
    let r = enter(&mut app, "/reviews");
    app.handle(ok(&r[0], 200, json!({"jobs": [{"id": "job-2", "recipient": "b", "state": "ready"}, {"id": "job-10", "recipient": "c", "state": "failed"}, {"id": "job-1", "recipient": "a", "state": "running"}]})));
    assert_eq!(
        app.jobs.iter().map(|j| j.id.as_str()).collect::<Vec<_>>(),
        ["job-1", "job-2", "job-10"],
        "numeric order, not text order"
    );
    assert!(text(&app).contains("job-10") && app.status_line().contains("1 running"));
}

#[test]
fn tu06_a_review_starts_from_a_relative_folder_and_progress_is_printed_once_until_it_finishes() {
    let mut app = booted();
    let r = enter(&mut app, "/review payments for acme-audit");
    assert_eq!(r[0].path, "/api/jobs");
    assert_eq!(
        r[0].body.as_ref().unwrap(),
        &json!({"repo": "/home/u/projects/payments", "recipient": "acme-audit"})
    );
    app.handle(ok(&r[0], 202, json!({"id": "job-4"})));
    assert!(text(&app).contains("Review job-4 started"));
    let t = app.handle(Event::Tick);
    assert_eq!(t[0].path, "/api/jobs/job-4");
    assert!(
        app.handle(Event::Tick).is_empty(),
        "no second poll while one is in flight"
    );
    let ev = |n: usize, state: &str| {
        let all = [
            json!({"stage": "queued", "detail": ""}),
            json!({"stage": "import", "detail": "reading"}),
            json!({"stage": "review", "detail": "agent working"}),
            json!({"stage": "finished", "detail": ""}),
        ];
        json!({"state": state, "recipient": "acme-audit", "note": "", "events": all[..n].to_vec()})
    };
    app.handle(ok(&t[0], 200, ev(2, "running")));
    let t = app.handle(Event::Tick);
    app.handle(ok(&t[0], 200, ev(3, "running")));
    let t = app.handle(Event::Tick);
    app.handle(ok(&t[0], 200, ev(4, "ready")));
    let all = text(&app);
    assert_eq!(all.matches("job-4  import").count(), 1, "{all}");
    assert_eq!(all.matches("job-4  review").count(), 1);
    assert!(all.contains("job-4 finished: ready") && all.contains("/approve"));
    assert!(
        app.handle(Event::Tick).is_empty(),
        "a finished review is no longer polled"
    );
}

fn release_body() -> Value {
    json!({"available": true, "recipient": "acme-audit", "digest": "abcdef0123456789abcdef", "files": [{"name": "finding-1.txt", "bytes": 120, "sha256": "a".repeat(64), "text": "Finding 1: token\n\u{1b}[31mred\u{1b}[0m"}], "removed": [{"what": ".env", "why": "secret"}], "not_established": ["model quality"], "disclosure": {"verdict": "review", "files_after": 5, "project_files": 11, "releases_before": 1, "reasons": ["over the review threshold"], "not_checked": ["combined excerpts"]}})
}

fn open_approval(app: &mut App) -> String {
    app.handle(Event::Resp(Resp {
        id: 0,
        result: Err("ignored: unknown id".into()),
    }));
    let r = enter(app, "/approve job-4");
    assert_eq!(r[0].path, "/api/jobs/job-4/approval");
    let d = app.handle(ok(
        &r[0],
        202,
        json!({"approval": "appr-9", "digest": "abcdef0123456789abcdef"}),
    ));
    assert!(d[0].admin && d[0].path == "/admin/approvals/appr-9");
    let mut body = release_body();
    body["approval"] = json!("appr-9");
    app.handle(ok(&d[0], 200, body));
    "appr-9".into()
}

#[test]
fn tu07_approval_needs_the_digest_prefix_typed_and_any_refusal_is_sent_to_the_service() {
    let mut app = booted();
    app.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    open_approval(&mut app);
    assert!(matches!(app.mode, Mode::Approval(_)));
    let r = app.handle(key(KeyCode::Enter));
    assert!(r.is_empty(), "Enter with nothing typed approves nothing");
    typed(&mut app, "abcdef012345");
    let Mode::Approval(v) = &app.mode else {
        panic!()
    };
    assert_eq!(v.typed, "abcdef012345");
    app.handle(key(KeyCode::Backspace));
    typed(&mut app, "f");
    let r = app.handle(key(KeyCode::Enter));
    assert!(r.is_empty(), "a wrong prefix sends nothing");
    let Mode::Approval(v) = &app.mode else {
        panic!()
    };
    assert!(v.error.as_deref().unwrap().contains("Nothing was approved"));
    app.handle(key(KeyCode::Backspace));
    typed(&mut app, "5");
    let r = app.handle(key(KeyCode::Enter));
    assert_eq!(r.len(), 1);
    assert!(
        r[0].admin && r[0].method == Method::Post && r[0].path == "/admin/approvals/appr-9/approve"
    );
    assert_eq!(r[0].body.as_ref().unwrap(), &json!({"ack": "abcdef012345"}));
    app.handle(ok(&r[0], 200, json!({"state": "approved"})));
    assert!(matches!(app.mode, Mode::Normal) && text(&app).contains("Run /deliver"));

    for refuse in [key(KeyCode::Esc), ctrl('c')] {
        let mut a = booted();
        a.jobs.push(kk_tui::app::JobRow {
            id: "job-4".into(),
            recipient: "x".into(),
            state: "ready".into(),
        });
        open_approval(&mut a);
        typed(&mut a, "abcdef012345");
        let r = a.handle(refuse);
        assert_eq!(r.len(), 1, "a refusal is an explicit request, never silent");
        assert_eq!(r[0].path, "/admin/approvals/appr-9/deny");
        assert!(matches!(a.mode, Mode::Normal));
        a.handle(ok(&r[0], 200, json!({"state": "denied"})));
        assert!(text(&a).contains("Refused"));
    }
}

#[test]
fn tu08_nothing_from_the_service_can_put_control_or_direction_characters_on_the_screen() {
    let mut app = booted();
    app.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    let r = enter(&mut app, "/findings job-4");
    app.handle(ok(&r[0], 200, json!({"findings": [{"title": "\u{1b}]0;pwned\u{7}Token", "file": "a\u{202e}py.exe", "line": 3, "status": "reproduced", "explanation": "line one\n\u{1b}[2J\u{1b}[Hline two\u{0}"}]})));
    let r = enter(&mut app, "/memory");
    app.handle(ok(&r[0], 200, json!({"memories": [{"id": "m1", "level": "internal", "text": "note\u{1b}[31m", "why": "\u{8}\u{8}x", "untrusted": true}]})));
    for c in &app.cards {
        for l in c.title.iter().chain(c.lines.iter()) {
            assert!(
                !l.chars().any(|ch| ch.is_control() && ch != '\t'),
                "control character in {l:?}"
            );
            assert!(!l.contains('\u{202e}'), "direction override in {l:?}");
        }
    }
    let mut a2 = booted();
    a2.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    open_approval(&mut a2);
    let Mode::Approval(v) = &a2.mode else {
        panic!()
    };
    assert!(
        v.lines.iter().all(|l| !l.chars().any(|c| c.is_control())),
        "the approval text is clean too"
    );
    assert!(
        v.lines.iter().any(|l| l.contains("5 of 11 project files"))
            && v.lines
                .iter()
                .any(|l| l.contains("over the review threshold"))
            && v.lines.iter().any(|l| l.contains("combined excerpts"))
    );
    assert!(
        v.lines.iter().any(|l| l.contains(".env (secret)"))
            && v.lines.iter().any(|l| l.contains("model quality"))
    );
}

#[test]
fn tu09_forgetting_asks_first_and_only_y_proceeds() {
    let mut app = booted();
    assert!(enter(&mut app, "/forget m-12").is_empty());
    assert!(matches!(app.mode, Mode::Confirm(_)));
    let r = app.handle(key(KeyCode::Char('n')));
    assert!(
        r.is_empty()
            && matches!(app.mode, Mode::Normal)
            && text(&app).contains("Nothing was forgotten")
    );
    enter(&mut app, "/forget m-12");
    let r = app.handle(key(KeyCode::Enter));
    assert!(r.is_empty(), "Enter is not a yes");
    enter(&mut app, "/forget m-12");
    let r = app.handle(key(KeyCode::Char('y')));
    assert_eq!(r[0].path, "/api/memories/m-12/forget");
    assert!(!r[0].admin);
}

#[test]
fn tu10_a_blocked_disclosure_and_service_errors_are_shown_plainly() {
    let mut app = booted();
    app.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    let r = enter(&mut app, "/approve");
    app.handle(ok(&r[0], 409, json!({"error": {"code": "DISCLOSURE_BLOCKED", "message": "8 of 11 files would be shown"}, "disclosure": {"not_checked": ["combined excerpts"]}})));
    let t = text(&app);
    assert!(
        t.contains("Blocked")
            && t.contains("8 of 11 files")
            && t.contains("Not checked: combined excerpts")
    );
    let r = enter(&mut app, "/deliver");
    app.handle(ok(
        &r[0],
        409,
        json!({"error": {"code": "NOT_APPROVED", "message": "approve the release first"}}),
    ));
    assert!(text(&app).contains("approve the release first (409)"));
    let r = enter(&mut app, "/reviews");
    app.handle(Event::Resp(Resp {
        id: r[0].id,
        result: Err("connection refused".into()),
    }));
    assert!(text(&app).contains("Could not reach the service: connection refused"));
}

fn render(app: &App, w: u16, h: u16, color: bool) -> (Vec<String>, Terminal<TestBackend>) {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| draw(f, app, &Theme { color })).unwrap();
    let buf = t.backend().buffer().clone();
    let rows = (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect();
    (rows, t)
}

#[test]
fn tu11_rendering_fits_wide_and_narrow_terminals_and_shows_hints_and_the_approval_prompt() {
    let mut app = booted();
    app.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    for (w, h) in [(100u16, 30u16), (60, 20), (40, 12), (24, 8)] {
        let (rows, _) = render(&app, w, h, true);
        assert!(rows[0].contains("kakurega"), "{w}x{h}: {rows:?}");
        assert!(rows.iter().any(|r| r.contains("Command")), "{w}x{h}");
        assert!(
            rows[(h - 1) as usize].trim().len() > 4,
            "the key hint line is present at {w}x{h}"
        );
    }
    let (rows, _) = render(&app, 100, 30, true);
    assert!(rows.iter().any(|r| r.contains("Type / for commands")));
    open_approval(&mut app);
    typed(&mut app, "abcd");
    for (w, h) in [(100u16, 30u16), (50, 16), (30, 10)] {
        let (rows, _) = render(&app, w, h, true);
        let all = rows.join("\n");
        assert!(all.contains("Approve release"), "{w}x{h}\n{all}");
        assert!(all.contains("abcd________") || w < 60, "{w}x{h}\n{all}");
    }
    let (rows, _) = render(&app, 100, 30, true);
    assert!(
        rows.join("\n").contains("abcdef012345"),
        "the prefix to type is shown"
    );
}

#[test]
fn tu12_nothing_depends_on_colour_alone_and_no_color_is_honoured() {
    let mut app = booted();
    app.cards.push(Card {
        kind: Kind::Error,
        title: Some("Broke".into()),
        lines: vec!["it failed".into()],
    });
    app.cards.push(Card {
        kind: Kind::Ok,
        title: Some("Fine".into()),
        lines: vec!["it worked".into()],
    });
    app.cards.push(Card {
        kind: Kind::Warn,
        title: Some("Careful".into()),
        lines: vec!["check this".into()],
    });
    let (rows, _) = render(&app, 80, 24, true);
    let all = rows.join("\n");
    assert!(
        all.contains("✗ Broke") && all.contains("✓ Fine") && all.contains("! Careful"),
        "each state has its own symbol:\n{all}"
    );
    let (_, t) = render(&app, 80, 24, false);
    let buf = t.backend().buffer();
    let coloured = (0..24u16)
        .flat_map(|y| (0..80u16).map(move |x| (x, y)))
        .filter(|&(x, y)| buf[(x, y)].fg != Color::Reset)
        .count();
    assert_eq!(coloured, 0, "with NO_COLOR no cell has a foreground colour");
}

#[test]
fn tu13_wrapping_never_exceeds_the_width_and_keeps_every_character() {
    for width in [8usize, 10, 17, 40] {
        for text in [
            "a short line",
            "averyveryverylongwordthatmustbebroken across lines",
            "  indented text that wraps around the edge",
            "日本語のテキストも折り返す",
            "",
        ] {
            let lines = wrap(text, width);
            for l in &lines {
                let w: usize = l
                    .chars()
                    .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
                    .sum();
                assert!(w <= width.max(8), "{l:?} is {w} wide at {width}");
            }
            let joined: String = lines.concat();
            let kept: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            assert_eq!(
                joined
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>(),
                kept,
                "no character lost at width {width}"
            );
        }
    }
}

#[test]
fn tu14_scrolling_follows_new_output_unless_the_user_scrolled_back() {
    let mut app = booted();
    for i in 0..60 {
        app.cards.push(Card {
            kind: Kind::Info,
            title: None,
            lines: vec![format!("line number {i}")],
        });
    }
    let (rows, _) = render(&app, 60, 14, true);
    assert!(rows.join("\n").contains("line number 59"));
    app.handle(key(KeyCode::PageUp));
    let (rows, _) = render(&app, 60, 14, true);
    let all = rows.join("\n");
    assert!(
        !all.contains("line number 59") && all.contains("more line(s) below"),
        "{all}"
    );
    app.handle(key(KeyCode::PageDown));
    let (rows, _) = render(&app, 60, 14, true);
    assert!(rows.join("\n").contains("line number 59"));
}

#[test]
fn tu15_a_refused_secret_is_scrubbed_from_the_transcript_and_the_command_history() {
    let mut app = booted();
    let r = enter(&mut app, "/remember the password is hunter2hunter2");
    assert!(
        text(&app).contains("hunter2"),
        "while the request is in flight the line is what the user typed"
    );
    app.handle(ok(&r[0], 422, json!({"error": {"code": "MEMORY_REFUSED", "message": "this looks like a credential with its value; secrets are never stored as memory"}})));
    assert!(!text(&app).contains("hunter2"), "{}", text(&app));
    assert!(text(&app).contains("secrets are never stored"));
    app.handle(key(KeyCode::Up));
    assert!(
        !app.input.contains("hunter2"),
        "recalling history does not bring the secret back: {:?}",
        app.input
    );
}
