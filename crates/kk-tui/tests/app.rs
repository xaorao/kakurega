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
    assert!(
        cmd::parse("hello there").is_err(),
        "plain text is not a command"
    );
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
        !app.input.contains('\u{1b}') && !app.input.contains('\t') && app.input.contains('\n'),
        "a short paste into a message keeps its lines but cannot smuggle control characters: {:?}",
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
fn tu04_plain_text_is_a_chat_message_sent_with_bounded_history_and_never_as_a_command() {
    let mut app = booted();
    let r = enter(&mut app, "please summarise my notes");
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].path, "/api/chat/stream");
    assert!(!r[0].admin);
    assert_eq!(
        r[0].body.as_ref().unwrap(),
        &json!({"message": "please summarise my notes", "history": []})
    );
    assert!(
        enter(&mut app, "second message while waiting").is_empty(),
        "one question at a time: the second one is queued"
    );
    let auto = app.handle(ok(&r[0], 200, json!({"reply": "Here is a summary.\u{1b}[31m", "route": {"simulated": false}, "notes_used": 2, "notes_withheld": 1})));
    assert_eq!(
        auto[0].body.as_ref().unwrap()["message"],
        "second message while waiting"
    );
    app.handle(ok(
        &auto[0],
        200,
        json!({"reply": "second answer", "route": {"simulated": false}, "notes_used": 0}),
    ));
    let t = text(&app);
    assert!(
        t.contains("Assistant")
            && t.contains("Here is a summary.")
            && t.contains("2 notes used")
            && t.contains("1 note was not given to the model"),
        "{t}"
    );
    assert!(!t.contains('\u{1b}'));
    let r2 = enter(&mut app, "and the next one");
    let sent = r2[0].body.as_ref().unwrap();
    assert_eq!(
        sent["history"].as_array().unwrap().len(),
        4,
        "the earlier turns are sent as context"
    );
    app.handle(ok(
        &r2[0],
        500,
        json!({"error": {"message": "the local model did not answer"}}),
    ));
    assert!(text(&app).contains("the local model did not answer"));
    assert_eq!(
        app.chat_history.len(),
        4,
        "a failed turn is not kept in the history"
    );
    enter(&mut app, "/clear");
    assert!(app.chat_history.is_empty());
    for i in 0..30 {
        let r = enter(&mut app, &format!("turn {i}"));
        app.handle(ok(&r[0], 200, json!({"reply": format!("answer {i}"), "route": {}, "notes_used": 0, "notes_withheld": 0})));
    }
    let r = enter(&mut app, "last");
    assert!(
        r[0].body.as_ref().unwrap()["history"]
            .as_array()
            .unwrap()
            .len()
            <= 18,
        "history sent to the service stays within its limit"
    );
    assert!(
        enter(&mut app, "/nonsense").is_empty(),
        "a mistyped command is never sent to the model"
    );
    assert!(text(&app).contains("unknown command /nonsense"));
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
    t.draw(|f| {
        draw(
            f,
            app,
            &Theme {
                color,
                palette: kk_tui::ui::Palette::Auto,
            },
        )
    })
    .unwrap();
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
        !all.contains("line number 59") && all.contains("more lines below"),
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

#[test]
fn tu16_when_the_service_requires_a_signed_approval_the_terminal_signs_the_exact_statement_or_refuses_to_send(
) {
    use kk_evidence::approval::{verify, Statement};
    use kk_evidence::keys::Signer;
    let statement = json!({"approval": "appr-9", "job": "job-4", "digest": "abcdef0123456789abcdef", "recipient": "acme-audit", "policy": "policy-1", "expires": 4_102_444_800_i64, "nonce": "n-123"});
    let open = |app: &mut App| {
        app.jobs.push(kk_tui::app::JobRow {
            id: "job-4".into(),
            recipient: "x".into(),
            state: "ready".into(),
        });
        let r = enter(app, "/approve job-4");
        let d = app.handle(ok(
            &r[0],
            202,
            json!({"approval": "appr-9", "digest": "abcdef0123456789abcdef"}),
        ));
        let mut body = release_body();
        body["approval"] = json!("appr-9");
        body["statement"] = statement.clone();
        body["requires_proof"] = json!(true);
        app.handle(ok(&d[0], 200, body));
    };
    // no key: nothing is sent
    let mut a = booted();
    open(&mut a);
    typed(&mut a, "abcdef012345");
    assert!(
        a.handle(key(KeyCode::Enter)).is_empty(),
        "without an approver key the request is never made"
    );
    let Mode::Approval(v) = &a.mode else { panic!() };
    assert!(v.error.as_deref().unwrap().contains("kaku approver enroll"));
    // with a key: the request carries a proof over exactly this statement
    let signer = Signer::from_seed([31; 32]);
    let mut b = booted();
    b.approver = Some(kk_evidence::approval::ApproverKey::Software(
        Signer::from_seed([31; 32]),
    ));
    open(&mut b);
    typed(&mut b, "abcdef012345");
    let r = b.handle(key(KeyCode::Enter));
    assert_eq!(r.len(), 1);
    let body = r[0].body.as_ref().unwrap();
    assert_eq!(body["ack"], "abcdef012345");
    let mut trusted = kk_evidence::approval::Approvers::default();
    trusted.ed.add(&signer.public()).unwrap();
    let expected = Statement {
        approval: "appr-9".into(),
        job: "job-4".into(),
        digest: "abcdef0123456789abcdef".into(),
        recipient: "acme-audit".into(),
        policy: "policy-1".into(),
        expires: 4_102_444_800_i64,
        nonce: "n-123".into(),
    };
    assert_eq!(
        verify(body["proof"].as_str().unwrap(), &trusted, &expected),
        Ok(signer.key_id().as_str().to_string())
    );
    // a service that does not require proof gets no proof field
    let mut c = booted();
    c.approver = Some(kk_evidence::approval::ApproverKey::Software(
        Signer::from_seed([31; 32]),
    ));
    c.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    let r0 = enter(&mut c, "/approve job-4");
    let d0 = c.handle(ok(
        &r0[0],
        202,
        json!({"approval": "appr-9", "digest": "abcdef0123456789abcdef"}),
    ));
    let mut legacy = release_body();
    legacy["approval"] = json!("appr-9");
    c.handle(ok(&d0[0], 200, legacy));
    typed(&mut c, "abcdef012345");
    let r = c.handle(key(KeyCode::Enter));
    assert!(r[0].body.as_ref().unwrap().get("proof").is_none());
}

#[test]
fn tu17_ask_sends_folder_and_question_and_shows_every_citation_verdict() {
    let mut a = booted();
    assert!(a.handle(key(KeyCode::Enter)).is_empty());
    for bad in ["/ask", "/ask docs", "/ask : why", "/ask docs : "] {
        let r = enter(&mut a, bad);
        assert!(r.is_empty(), "{bad} must not send anything");
    }
    let r = enter(&mut a, "/ask /Users/me/docs : What is the notice period?");
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].path, "/api/ask");
    assert_eq!(
        r[0].body.as_ref().unwrap(),
        &json!({"folder": "/Users/me/docs", "question": "What is the notice period?"})
    );
    assert!(
        enter(&mut a, "/ask /x : again").is_empty(),
        "one question at a time"
    );
    a.handle(ok(&r[0], 200, json!({"checkable": true, "answer": "Thirty days.", "citations": [
        {"file": "contract.txt", "quote": "thirty (30) days", "verdict": "verified", "line": 12},
        {"file": "contract.txt", "quote": "invented words", "verdict": "not_found", "line": null},
        {"file": "other.txt", "quote": "x", "verdict": "unknown_file", "line": null}],
        "files_used": ["contract.txt"], "files_withheld": [".env"], "files_skipped": [], "injection_suspects": ["notes.txt"]})));
    let text = a
        .cards
        .iter()
        .flat_map(|c| c.lines.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("verified, line 12")
            && text.contains("NOT FOUND")
            && text.contains("NOT A FILE")
    );
    assert!(text.contains("1 withheld") && text.contains("orders"));
    let r2 = enter(&mut a, "/ask /x : second");
    a.handle(ok(&r2[0], 200, json!({"checkable": false, "unchecked_text": "free text", "files_used": [], "files_withheld": [], "files_skipped": [], "injection_suspects": []})));
    let text = a
        .cards
        .iter()
        .flat_map(|c| c.lines.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("trust none of it") && text.contains("free text"));
}

#[test]
fn tu18_fixes_are_listed_with_their_limit_and_applied_only_after_listing_then_y() {
    let mut a = booted();
    a.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    for bad in [
        "/applyfix",
        "/applyfix 0",
        "/applyfix x",
        "/applyfix 1 2 3",
        "/applyfix 100",
    ] {
        assert!(enter(&mut a, bad).is_empty(), "{bad}");
    }
    assert!(
        enter(&mut a, "/applyfix 1").is_empty(),
        "applying before listing is refused"
    );
    assert!(matches!(a.mode, Mode::Normal));
    let r = enter(&mut a, "/fixes");
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].path, "/api/jobs/job-4/fixes");
    a.handle(ok(&r[0], 200, json!({"fixes": [
        {"index": 0, "title": "t", "file": "payments/auth/session.py", "explanation": "remove the entry", "status": "fixes_the_reproducer", "verified": true, "diff": "--- a/x\n+++ b/x\n-old\n+new\n", "applied": false},
        {"index": 1, "title": "t", "file": "payments/auth/session.py", "explanation": "bad", "status": "not_checked: breaks the login check", "verified": false, "diff": "-a\n+b\n", "applied": false}]})));
    let text = a
        .cards
        .iter()
        .flat_map(|c| c.lines.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("+new")
            && text.contains("tests were not run")
            && text.contains("/applyfix 1")
            && text.contains("NOT accepted")
    );
    assert!(
        enter(&mut a, "/applyfix 2").is_empty(),
        "an unaccepted fix cannot be applied"
    );
    assert!(matches!(a.mode, Mode::Normal));
    assert!(enter(&mut a, "/applyfix 1").is_empty());
    let Mode::Confirm(c) = &a.mode else {
        panic!("a confirmation is required")
    };
    assert!(c.prompt.contains("edits your own file"));
    assert!(a.handle(key(KeyCode::Char('n'))).is_empty());
    assert!(matches!(a.mode, Mode::Normal), "anything but y cancels");
    enter(&mut a, "/applyfix 1");
    let r = a.handle(key(KeyCode::Char('y')));
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].path, "/api/jobs/job-4/fixes/0/apply");
    assert_eq!(
        r[0].body.as_ref().unwrap(),
        &json!({"file": "payments/auth/session.py"})
    );
    a.handle(ok(
        &r[0],
        200,
        json!({"applied": true, "file": "payments/auth/session.py"}),
    ));
    let text = a
        .cards
        .iter()
        .flat_map(|c| c.lines.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Applied to payments/auth/session.py."));
}

#[test]
fn tu19_undofix_needs_a_listing_first_a_y_and_posts_to_the_undo_route() {
    let mut a = booted();
    a.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    for bad in ["/undofix", "/undofix 0", "/undofix x", "/undofix 1 2 3"] {
        assert!(enter(&mut a, bad).is_empty(), "{bad}");
    }
    assert!(enter(&mut a, "/undofix 1").is_empty());
    assert!(
        matches!(a.mode, Mode::Normal),
        "undo before listing is refused"
    );
    let r = enter(&mut a, "/fixes");
    a.handle(ok(&r[0], 200, json!({"fixes": [{"index": 0, "title": "t", "file": "payments/auth/session.py", "explanation": "e", "status": "fixes_the_reproducer", "verified": true, "diff": "-a\n+b\n", "applied": true}]})));
    assert!(enter(&mut a, "/undofix 1").is_empty());
    let Mode::Confirm(c) = &a.mode else {
        panic!("a confirmation is required")
    };
    assert!(c.prompt.contains("Restore the original"));
    assert!(a.handle(key(KeyCode::Char('n'))).is_empty());
    enter(&mut a, "/undofix 1");
    let r = a.handle(key(KeyCode::Char('y')));
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].path, "/api/jobs/job-4/fixes/0/undo");
    a.handle(ok(
        &r[0],
        200,
        json!({"undone": true, "file": "payments/auth/session.py"}),
    ));
    let text = a
        .cards
        .iter()
        .flat_map(|c| c.lines.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Restored payments/auth/session.py."));
}

fn picker_rows(app: &App) -> Vec<String> {
    let Mode::Picker(p) = &app.mode else {
        panic!("the picker is not open")
    };
    p.visible().iter().map(|i| p.items[*i].id.clone()).collect()
}

#[test]
fn tu20_keep_sends_the_session_with_every_message_and_new_chat_leaves_it() {
    let mut a = booted();
    let r = enter(&mut a, "/keep");
    assert_eq!(
        (r[0].method, r[0].path.as_str()),
        (Method::Post, "/api/sessions")
    );
    a.handle(ok(&r[0], 201, json!({"id": "chat-aa11"})));
    assert_eq!(a.session.as_deref(), Some("chat-aa11"));
    assert!(
        enter(&mut a, "/keep").is_empty(),
        "a second keep does nothing"
    );
    let r = enter(&mut a, "hello there");
    assert_eq!(r[0].body.as_ref().unwrap()["session"], "chat-aa11");
    a.handle(ok(
        &r[0],
        200,
        json!({"reply": "hi", "route": {"simulated": false}, "notes_used": 0, "saved": true}),
    ));
    assert!(a.session.is_some());
    enter(&mut a, "/newchat");
    assert!(a.session.is_none() && a.chat_history.is_empty());
    let r = enter(&mut a, "unkept message");
    assert!(
        r[0].body.as_ref().unwrap().get("session").is_none(),
        "an unkept chat sends no session"
    );
}

#[test]
fn tu21_the_picker_lists_chats_and_reviews_filters_opens_and_forgets_with_a_second_press() {
    let mut a = booted();
    a.jobs.push(kk_tui::app::JobRow {
        id: "job-1".into(),
        recipient: "acme".into(),
        state: "ready".into(),
    });
    let r = enter(&mut a, "/sessions");
    assert_eq!(
        r[0].path, "/api/sessions?archived=1",
        "the list asks for the archived ones too, and shows them in their own view"
    );
    assert!(matches!(a.mode, Mode::Picker(_)));
    a.handle(ok(&r[0], 200, json!({"sessions": [
        {"id": "chat-1", "title": "Invoice questions", "created": 100, "updated": 100, "turns": 4},
        {"id": "chat-2", "title": "Garden notes", "created": 200, "updated": 200, "turns": 2}
    ]})));
    assert_eq!(
        picker_rows(&a),
        ["chat-2", "chat-1", "job-1"],
        "newest chats first, reviews after"
    );
    typed(&mut a, "invoice");
    assert_eq!(picker_rows(&a), ["chat-1"]);
    for _ in 0..7 {
        a.handle(key(KeyCode::Backspace));
    }
    // delete: first press only arms
    a.handle(key(KeyCode::Down));
    assert!(a.handle(ctrl('d')).is_empty());
    let Mode::Picker(p) = &a.mode else { panic!() };
    assert_eq!(p.armed.as_deref(), Some("chat-1"));
    let r = a.handle(ctrl('d'));
    assert_eq!(r[0].path, "/api/sessions/chat-1/forget");
    a.handle(ok(&r[0], 200, json!({"forgotten": true})));
    assert_eq!(picker_rows(&a), ["chat-2", "job-1"]);
    // a failed forget stays in the list with its reason
    a.handle(key(KeyCode::Up));
    let r = {
        a.handle(ctrl('d'));
        a.handle(ctrl('d'))
    };
    a.handle(ok(
        &r[0],
        500,
        json!({"error": {"code": "STORE", "message": "disk trouble"}}),
    ));
    let Mode::Picker(p) = &a.mode else {
        panic!("the picker stays open on failure")
    };
    assert!(
        p.error.as_deref().unwrap_or("").contains("disk trouble"),
        "{:?}",
        p.error
    );
    assert_eq!(picker_rows(&a), ["chat-2", "job-1"]);
    // opening a chat restores both sides and continues in the same session
    let r = a.handle(key(KeyCode::Enter));
    assert_eq!(r[0].path, "/api/sessions/chat-2");
    assert!(matches!(a.mode, Mode::Normal));
    a.handle(ok(&r[0], 200, json!({"id": "chat-2", "turns": [{"role": "user", "content": "first question"}, {"role": "assistant", "content": "first answer"}]})));
    assert_eq!(a.session.as_deref(), Some("chat-2"));
    assert!(text(&a).contains("first question") && text(&a).contains("first answer"));
    assert_eq!(a.chat_history.len(), 2);
}

#[test]
fn tu22_reviews_open_from_the_picker_are_never_deleted_there_and_failures_do_not_close_it() {
    let mut a = booted();
    a.jobs.push(kk_tui::app::JobRow {
        id: "job-7".into(),
        recipient: "acme".into(),
        state: "ready".into(),
    });
    let r = enter(&mut a, "/resume");
    a.handle(ok(
        &r[0],
        503,
        json!({"error": {"code": "DOWN", "message": "service is restarting"}}),
    ));
    let Mode::Picker(p) = &a.mode else {
        panic!("a load failure keeps the picker open")
    };
    assert!(p.error.as_deref().unwrap().contains("Could not load"));
    assert_eq!(
        picker_rows(&a),
        ["job-7"],
        "reviews already known are still offered"
    );
    assert!(a.handle(ctrl('d')).is_empty());
    let Mode::Picker(p) = &a.mode else { panic!() };
    assert!(p
        .error
        .as_deref()
        .unwrap()
        .contains("Reviews are not removed"));
    let r = a.handle(key(KeyCode::Enter));
    assert_eq!(r[0].path, "/api/jobs/job-7");
    let mut b = booted();
    enter(&mut b, "/sessions");
    b.handle(key(KeyCode::Esc));
    assert!(matches!(b.mode, Mode::Normal), "Esc closes");
}

#[test]
fn tu23_the_picker_renders_in_small_and_large_terminals_without_losing_the_selection() {
    let mut a = booted();
    let r = enter(&mut a, "/sessions");
    let sessions: Vec<Value> = (0..40).map(|i| json!({"id": format!("chat-{i:02}"), "title": format!("Conversation number {i}"), "created": i, "updated": 1_700_000_000 + i, "turns": 2})).collect();
    a.handle(ok(&r[0], 200, json!({ "sessions": sessions })));
    for (w, h) in [(80u16, 24u16), (40, 12), (120, 40)] {
        for _ in 0..5 {
            a.handle(key(KeyCode::Down));
        }
        let (lines, _) = render(&a, w, h, true);
        let screen = lines.join("\n");
        assert!(
            screen.contains("Conversations and reviews") && screen.contains("Filter:"),
            "{w}x{h}\n{screen}"
        );
        assert!(screen.contains("Conversation number"), "{w}x{h}");
    }
    // the selected row stays on screen after moving far down
    a.handle(key(KeyCode::PageDown));
    a.handle(key(KeyCode::PageDown));
    let (lines, _) = render(&a, 80, 12, true);
    let Mode::Picker(p) = &a.mode else { panic!() };
    let want = p.current().unwrap().label.clone();
    assert!(
        lines.join("\n").contains(&want),
        "the selected item {want} must be visible"
    );
}

fn delta(app: &mut App, id: u64, t: &str) {
    app.handle(Event::Delta {
        id,
        text: t.to_string(),
    });
}

#[test]
fn tu24_a_streamed_answer_grows_in_one_card_is_replaced_by_the_final_text_and_can_be_stopped_with_esc(
) {
    let mut a = booted();
    let r = enter(&mut a, "tell me about invoices");
    assert!(r[0].stream && r[0].path == "/api/chat/stream");
    let id = r[0].id;
    let before = a.cards.len();
    delta(&mut a, id, "Invoices ");
    delta(&mut a, id, "are paid ");
    delta(&mut a, id, "monthly.");
    assert_eq!(a.cards.len(), before + 1, "all pieces land in one card");
    let live = a.cards.last().unwrap();
    assert_eq!(
        (live.title.as_deref(), live.lines.join("")),
        (Some("Assistant"), "Invoices are paid monthly.".to_string())
    );
    // the final object replaces the live text with the full reply and the footer
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "Invoices are paid monthly.", "route": {"simulated": true}, "notes_used": 1, "saved": false})));
    assert_eq!(
        a.cards.len(),
        before + 2,
        "the answer is still one card, and what it used follows as one plain card"
    );
    assert!(text(&a).contains("test model · 1 note used"));
    assert_eq!(
        a.chat_history.last().unwrap(),
        &(
            "assistant".to_string(),
            "Invoices are paid monthly.".to_string()
        )
    );
    // control characters in a piece never reach the screen
    let r = enter(&mut a, "again");
    delta(&mut a, r[0].id, "ok\u{1b}[31mred\u{7}");
    assert!(!text(&a).contains('\u{1b}') && !text(&a).contains('\u{7}'));
    // Esc while waiting asks the network thread to stop, and does not clear the composer
    typed(&mut a, "draft");
    a.handle(key(KeyCode::Esc));
    assert!(
        !a.cancel.load(std::sync::atomic::Ordering::SeqCst),
        "one Esc only arms"
    );
    assert!(footer_text(&a).contains("Esc again to stop the answer"));
    a.handle(key(KeyCode::Esc));
    assert!(a.cancel.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(a.input, "draft");
    a.handle(ok(&r[0], 200, json!({"stopped": true})));
    assert!(text(&a).contains("Stopped. This partial answer is not kept"));
    assert_eq!(
        a.chat_history
            .iter()
            .filter(|(r, c)| r == "user" && c == "again")
            .count(),
        0,
        "a stopped exchange leaves no half conversation"
    );
    // after that Esc clears the composer again, and the next message resets the cancel flag
    a.handle(key(KeyCode::Esc));
    assert!(a.input.is_empty());
    let r = enter(&mut a, "next");
    assert!(!a.cancel.load(std::sync::atomic::Ordering::SeqCst));
    // an error from the stream is shown and does not leave the screen waiting forever
    a.handle(ok(
        &r[0],
        502,
        json!({"error": {"code": "NO_MODEL", "message": "the local model did not answer"}}),
    ));
    assert!(text(&a).contains("the local model did not answer"));
    let again = enter(&mut a, "retry");
    assert_eq!(again.len(), 1, "a new message can be sent after an error");
    // deltas for another request are ignored
    delta(&mut a, 9999, "stray");
    assert!(!text(&a).contains("stray"));
}

fn with_mods(c: KeyCode, m: KeyModifiers) -> Event {
    Event::Key(KeyEvent {
        code: c,
        modifiers: m,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

#[test]
fn tu25_shift_or_alt_enter_adds_a_line_and_up_down_move_between_lines_before_history() {
    let mut a = booted();
    typed(&mut a, "first");
    assert!(a
        .handle(with_mods(KeyCode::Enter, KeyModifiers::SHIFT))
        .is_empty());
    typed(&mut a, "second");
    a.handle(with_mods(KeyCode::Enter, KeyModifiers::ALT));
    typed(&mut a, "third");
    a.handle(ctrl('j'));
    typed(&mut a, "fourth");
    assert_eq!(a.input, "first\nsecond\nthird\nfourth");
    a.handle(key(KeyCode::Up));
    a.handle(key(KeyCode::Up));
    a.handle(key(KeyCode::Up));
    assert_eq!(
        a.cursor, 5,
        "the cursor is at the end of the first line, not in history"
    );
    a.handle(key(KeyCode::Down));
    assert_eq!(a.cursor, 11);
    let r = a.handle(key(KeyCode::Enter));
    assert_eq!(
        r[0].body.as_ref().unwrap()["message"],
        "first\nsecond\nthird\nfourth",
        "newlines are part of the message"
    );
}

#[test]
fn tu26_a_long_paste_is_a_placeholder_until_sent_and_never_reaches_a_command() {
    let mut a = booted();
    let big = (0..30)
        .map(|i| format!("line {i} of a long log"))
        .collect::<Vec<_>>()
        .join("\n");
    typed(&mut a, "summarise: ");
    a.handle(Event::Paste(big.clone()));
    assert!(
        a.input.contains("[Pasted ~30 lines]") && a.input.len() < 60,
        "{}",
        a.input
    );
    a.handle(Event::Paste(big.clone()));
    assert!(
        a.input.contains("[Pasted ~30 lines #2]"),
        "a second paste gets its own label"
    );
    let r = a.handle(key(KeyCode::Enter));
    let msg = r[0].body.as_ref().unwrap()["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        msg.starts_with("summarise: line 0 of a long log")
            && msg.matches("line 29 of a long log").count() == 2
            && !msg.contains("[Pasted"),
        "both are expanded in the sent message"
    );
    assert!(
        text(&a).contains("[Pasted ~30 lines]") && !text(&a).contains("line 15 of"),
        "the screen keeps the short form"
    );
    // into a command line a paste is flattened and never folded
    let mut b = booted();
    typed(&mut b, "/remember ");
    b.handle(Event::Paste(big.clone()));
    assert!(
        !b.input.contains('\n') && !b.input.contains("[Pasted") && b.input.contains("line 29"),
        "{}",
        b.input
    );
    // a placeholder typed by hand is just text, and one pasted inside a command line is not expanded
    let mut c = booted();
    c.handle(Event::Paste(big.clone()));
    let label = c.input.clone();
    let reqs = c.handle(ctrl('a'));
    assert!(reqs.is_empty());
    typed(&mut c, "/remember ");
    let r = c.handle(key(KeyCode::Enter));
    assert!(
        r[0].path == "/api/memories"
            && !r[0].body.as_ref().unwrap()["text"]
                .as_str()
                .unwrap()
                .contains("line 29"),
        "{label}"
    );
    // too long to send: refused on the spot, nothing is cleared or sent
    let mut d = booted();
    d.handle(Event::Paste("x".repeat(5000)));
    let r = d.handle(key(KeyCode::Enter));
    assert!(
        r.is_empty() && text(&d).contains("the limit is 4000") && d.input.contains("[Pasted"),
        "the draft is kept so it can be shortened"
    );
}

#[test]
fn tu27_the_composer_grows_with_its_lines_and_shows_the_cursor_line() {
    let mut a = booted();
    for i in 0..9 {
        typed(&mut a, &format!("row{i}"));
        a.handle(with_mods(KeyCode::Enter, KeyModifiers::SHIFT));
    }
    typed(&mut a, "last");
    let (lines, _) = render(&a, 60, 20, true);
    let screen = lines.join("\n");
    assert!(
        screen.contains("last") && !screen.contains("row0"),
        "the window follows the cursor line:\n{screen}"
    );
    let (lines, _) = render(&a, 30, 8, false);
    assert!(
        lines.join("\n").contains("last"),
        "still readable in a tiny terminal"
    );
}

#[test]
fn tu28_messages_typed_while_an_answer_streams_are_queued_in_order_and_dropped_if_it_is_stopped() {
    let mut a = booted();
    let r = enter(&mut a, "one");
    assert!(enter(&mut a, "two").is_empty() && enter(&mut a, "three").is_empty());
    assert_eq!(a.queued, ["two", "three"]);
    assert!(text(&a).contains("Queued (2 waiting)"));
    // the first answer finishes: the next queued message goes out by itself, with the first exchange in its history
    let next = a.handle(ok(
        &r[0],
        200,
        json!({"done": true, "reply": "answer one", "route": {"simulated": true}, "notes_used": 0}),
    ));
    assert_eq!(next.len(), 1);
    let body = next[0].body.as_ref().unwrap();
    assert_eq!(body["message"], "two");
    assert_eq!(body["history"].as_array().unwrap().len(), 2);
    assert_eq!(a.queued, ["three"]);
    // stopping the answer drops what was waiting, and says so
    a.handle(key(KeyCode::Esc));
    a.handle(key(KeyCode::Esc));
    let after = a.handle(ok(&next[0], 200, json!({"stopped": true})));
    assert!(after.is_empty() && a.queued.is_empty());
    assert!(text(&a).contains("1 queued message was not sent"));
    // the queue is bounded
    let mut b = booted();
    enter(&mut b, "first");
    for i in 0..7 {
        enter(&mut b, &format!("q{i}"));
    }
    assert_eq!(b.queued.len(), 5);
    assert!(text(&b).contains("Five messages are already waiting"));
    // a command is still run at once while an answer streams
    let r = enter(&mut b, "/reviews");
    assert_eq!(r[0].path, "/api/jobs");
}

#[test]
fn tu29_proof_and_gateway_commands_show_what_was_verified_and_what_was_not_and_refuse_odd_names() {
    let mut a = booted();
    for bad in [
        "/proof a b",
        "/proof ../x;rm",
        "/proof ".to_string().as_str(),
    ] {
        let r = enter(&mut a, bad);
        if bad.trim() == "/proof" {
            assert_eq!(r[0].body.as_ref().unwrap()["model"], "Qwen/Qwen3.8-27B");
        } else {
            assert!(r.is_empty(), "{bad}");
        }
    }
    let r = enter(&mut a, "/proof good/model");
    assert_eq!(
        (r[0].path.as_str(), &r[0].body.as_ref().unwrap()["model"]),
        ("/api/route/attest", &json!("good/model"))
    );
    assert!(text(&a).contains("Only the model name is sent"));
    a.handle(ok(&r[0], 200, json!({"verified": true, "deployments": [{"tcb": "UpToDate", "gpu_verified": true, "signer": "0xsig", "nonce": "n9"}], "means": ["Intel TDX vouched."], "does_not_mean": ["Not a signed answer."]})));
    let t = text(&a);
    assert!(
        t.contains("Model verified")
            && t.contains("UpToDate")
            && t.contains("NVIDIA GPU evidence verified")
            && t.contains("0xsig")
            && t.contains("What this does not show")
            && t.contains("Not a signed answer.")
    );
    let r = enter(&mut a, "/proof refused/model");
    a.handle(ok(&r[0], 422, json!({"verified": false, "error": {"code": "NOT_VERIFIED", "message": "the route policy refuses it"}})));
    assert!(text(&a).contains("Not verified") && text(&a).contains("the route policy refuses it"));
    let r = enter(&mut a, "/gateway");
    assert_eq!(r[0].path, "/api/route/gateway");
    a.handle(ok(&r[0], 200, json!({"verified": true, "gateway": {"tcb": "UpToDate", "tls_bound": true, "signer": "gw", "nonce": "gn"}, "means": [], "does_not_mean": []})));
    assert!(
        text(&a).contains("Gateway verified")
            && text(&a).contains("encryption ends inside the verified software")
    );
}

#[test]
fn tu30_typed_lines_are_saved_in_the_background_and_loaded_once_without_secrets_or_surprises() {
    let mut a = booted();
    let first = a.drain_background();
    assert_eq!(
        first.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
        ["/api/history", "/api/away"],
        "the saved history and the list of reviews that stopped meanwhile are each requested once"
    );
    assert!(a.drain_background().is_empty());
    enter(&mut a, "/reviews");
    enter(&mut a, "hello there");
    let saves = a.drain_background();
    assert_eq!(
        saves
            .iter()
            .map(|r| r.body.as_ref().unwrap()["text"]
                .as_str()
                .unwrap()
                .to_string())
            .collect::<Vec<_>>(),
        ["/reviews", "hello there"]
    );
    assert!(a.drain_background().is_empty(), "each line is sent once");
    // the loaded history sits before what was typed in this run, and Up reaches it
    a.handle(ok(
        &first[0],
        200,
        json!({"lines": ["older one", "older two"]}),
    ));
    a.handle(key(KeyCode::Up));
    assert_eq!(a.input, "hello there");
    a.handle(key(KeyCode::Up));
    a.handle(key(KeyCode::Up));
    a.handle(key(KeyCode::Up));
    assert_eq!(a.input, "older one");
    // a failed load changes nothing, and clearing empties the local list and asks the service to forget
    let mut b = booted();
    let r = b.drain_background();
    b.handle(ok(&r[0], 500, json!({"error": {"message": "down"}})));
    let c = enter(&mut b, "/clearhistory");
    assert_eq!(c[0].path, "/api/history/clear");
    assert!(b.history.is_empty() || b.history == ["/clearhistory"]);
}

#[test]
fn tu31_typing_a_slash_offers_matching_commands_that_can_be_chosen_completed_or_hidden() {
    let mut a = booted();
    assert!(a.completions().is_empty());
    typed(&mut a, "/");
    assert!(
        a.completions().len() >= 25 && a.completions()[0] == "applyfix",
        "every command, in order"
    );
    typed(&mut a, "me");
    assert_eq!(
        a.completions()[0],
        "memory",
        "prefix matches come first, then names that merely contain the text"
    );
    a.handle(key(KeyCode::Backspace));
    a.handle(key(KeyCode::Backspace));
    typed(&mut a, "fi");
    assert_eq!(a.completions()[..3], ["find", "findings", "fixes"]);
    a.handle(key(KeyCode::Down));
    a.handle(key(KeyCode::Down));
    a.handle(key(KeyCode::Tab));
    assert_eq!(
        a.input, "/fixes ",
        "Tab completes the chosen command and leaves room for arguments"
    );
    assert!(
        a.completions().is_empty(),
        "the list closes once a space is typed"
    );
    // Enter on a command that needs arguments completes it instead of running it
    a.handle(ctrl('u'));
    typed(&mut a, "/forg");
    assert!(a.handle(key(KeyCode::Enter)).is_empty());
    assert_eq!(a.input, "/forget ");
    // a command typed out in full runs as typed, even when a longer name also matches
    a.handle(ctrl('u'));
    typed(&mut a, "/review");
    // letters that appear in order also match, so a longer name that contains them is offered last
    assert_eq!(a.completions(), ["review", "reviews", "forgetreview"]);
    assert!(a.handle(key(KeyCode::Enter)).is_empty());
    assert!(
        a.input.is_empty() && text(&a).contains("usage: /review FOLDER for RECIPIENT"),
        "it ran and printed its usage"
    );
    // Enter on a command with no arguments runs it
    a.handle(ctrl('u'));
    typed(&mut a, "/revie");
    let ws = a.completions();
    assert_eq!(ws, ["review", "reviews", "forgetreview"]);
    a.handle(key(KeyCode::Down));
    let r = a.handle(key(KeyCode::Enter));
    assert_eq!(r[0].path, "/api/jobs", "/reviews ran");
    // Esc hides the list until the line changes; the arrow keys still walk history then
    typed(&mut a, "/he");
    assert_eq!(a.completions(), ["help", "where"]);
    a.handle(key(KeyCode::Esc));
    assert!(a.completions().is_empty() && a.input == "/he");
    typed(&mut a, "l");
    assert_eq!(
        a.completions(),
        ["help"],
        "typing again brings the list back"
    );
    // wrap-around
    a.handle(ctrl('u'));
    typed(&mut a, "/");
    a.handle(key(KeyCode::Up));
    a.handle(key(KeyCode::Tab));
    assert_eq!(
        a.input, "/where ",
        "Up from the first wraps to the last alphabetical command that is not quit... "
    );
}

#[test]
fn tu32_the_completion_list_is_drawn_above_the_composer_and_fits_small_terminals() {
    let mut a = booted();
    typed(&mut a, "/re");
    let (lines, _) = render(&a, 80, 24, true);
    let s = lines.join("\n");
    assert!(
        s.contains("/release") && s.contains("what would be sent")
            || s.contains("show exactly what would be sent"),
        "{s}"
    );
    for (w, h) in [(80u16, 24u16), (40, 12), (120, 40), (20, 5)] {
        let _ = render(&a, w, h, true);
    }
}

#[test]
fn tu33_a_half_typed_line_can_be_put_aside_and_brought_back_without_losing_what_was_typed_since() {
    let mut a = booted();
    typed(&mut a, "a long question I am not ready to send");
    a.handle(ctrl('s'));
    assert!(a.input.is_empty() && a.stashed.len() == 1);
    typed(&mut a, "quick one");
    a.handle(ctrl('r'));
    assert_eq!(a.input, "a long question I am not ready to send");
    assert_eq!(
        a.stashed,
        ["quick one"],
        "the line typed since went onto the stash instead of being lost"
    );
    a.handle(ctrl('u'));
    a.handle(ctrl('r'));
    assert_eq!(a.input, "quick one");
    a.handle(ctrl('u'));
    a.handle(ctrl('r'));
    assert!(text(&a).contains("Nothing is stashed."));
    // empty input stashes nothing; commands do the same; the stash is bounded
    a.handle(ctrl('s'));
    assert!(a.stashed.is_empty());
    typed(&mut a, "/stash");
    a.handle(key(KeyCode::Enter));
    assert!(text(&a).contains("Type something first"));
    for i in 0..12 {
        typed(&mut a, &format!("line {i}"));
        a.handle(ctrl('s'));
    }
    assert_eq!(a.stashed.len(), 10);
    assert_eq!(a.stashed[0], "line 2", "the oldest are dropped first");
    typed(&mut a, "/pop");
    a.handle(key(KeyCode::Enter));
    assert_eq!(a.input, "line 11");
    // a stashed line is never sent by stashing or popping
    assert!(a
        .drain_background()
        .iter()
        .all(|r| r.path == "/api/history" || r.path == "/api/away"));
}

#[test]
fn tu34_the_footer_shows_the_context_estimate_and_older_turns_left_out() {
    let mut a = booted();
    let r = enter(&mut a, "hello");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 0, "usage": {"context_tokens_estimate": 1240, "history_turns_dropped": 6}})));
    let t = text(&a);
    assert!(
        t.contains("about 1240 tokens of context sent")
            && t.contains("6 older messages were not sent"),
        "{t}"
    );
    let r = enter(&mut a, "again");
    a.handle(ok(
        &r[0],
        200,
        json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 0}),
    ));
    assert_eq!(
        text(&a).matches("tokens of context sent").count(),
        1,
        "no figure is invented when the service sent none"
    );
}

#[test]
fn tu35_search_and_read_show_exactly_what_goes_out_send_nothing_without_a_yes_and_add_only_approved_pages(
) {
    let mut a = booted();
    // proposing sends nothing yet: the only request is the proposal to the local service
    let r = enter(&mut a, "/search tokio async runtime");
    assert_eq!((r.len(), r[0].path.as_str()), (1, "/api/search/propose"));
    assert_eq!(r[0].body.as_ref().unwrap()["query"], "tokio async runtime");
    let r = a.handle(ok(&r[0], 200, json!({"id": "search-1", "digest": "d1", "query": "tokio async runtime", "will_send_to": "search.example", "stays_here": "Only these words go to this one site. Nothing has been sent yet."})));
    assert!(r.is_empty());
    let t = text(&a);
    assert!(
        t.contains("search.example")
            && t.contains("tokio async runtime")
            && t.contains("Nothing has been sent yet"),
        "{t}"
    );
    assert!(matches!(a.mode, Mode::Confirm(_)));
    // anything but y cancels and sends nothing
    assert!(a.handle(key(KeyCode::Char('n'))).is_empty());
    assert!(matches!(a.mode, Mode::Normal) && text(&a).contains("Cancelled. Nothing was sent."));
    // y sends exactly the approved digest
    let r = enter(&mut a, "/search tokio async runtime");
    a.handle(ok(&r[0], 200, json!({"id": "search-2", "digest": "d2", "query": "tokio async runtime", "will_send_to": "search.example", "stays_here": "x"})));
    let r = a.handle(key(KeyCode::Char('y')));
    assert_eq!(
        (r.len(), r[0].path.as_str()),
        (1, "/api/search/search-2/run")
    );
    assert_eq!(
        r[0].body.as_ref().unwrap(),
        &json!({"approve": true, "digest": "d2"})
    );
    a.handle(ok(&r[0], 200, json!({"results": [
        {"title": "Tokio tutorial", "snippet": "Learn async Rust.", "url": "https://tokio.rs/tutorial", "host": "tokio.rs", "withheld": false},
        {"title": "", "snippet": "", "url": "https://evil.example/p", "host": "evil.example", "withheld": true}
    ]})));
    let t = text(&a);
    assert!(
        t.contains("1. https://tokio.rs/tutorial")
            && t.contains("Learn async Rust.")
            && t.contains("2. https://evil.example/p")
            && t.contains("reads like instructions")
            && t.contains("Nothing has been requested"),
        "{t}"
    );
    // a number refers to a result; a number that is not there is refused locally
    let r = enter(&mut a, "/read 9");
    assert!(r.is_empty() && text(&a).contains("no search result number 9"));
    let r = enter(&mut a, "/read 1");
    assert_eq!((r.len(), r[0].path.as_str()), (1, "/api/research/propose"));
    assert_eq!(
        r[0].body.as_ref().unwrap()["urls"],
        json!(["https://tokio.rs/tutorial"])
    );
    a.handle(ok(&r[0], 200, json!({"id": "research-3", "digest": "d3", "items": [{"url": "https://tokio.rs/tutorial", "allowed": true}], "will_request": ["https://tokio.rs/tutorial"], "stays_here": "Only these exact addresses are requested."})));
    let r = a.handle(key(KeyCode::Char('y')));
    assert_eq!(r[0].path, "/api/research/research-3/fetch");
    a.handle(ok(&r[0], 200, json!({"pages": [
        {"page_id": "page-4", "url": "https://tokio.rs/tutorial", "final_url": "https://tokio.rs/tutorial", "title": "Tokio", "looks_like_instructions": false},
        {"page_id": "page-5", "url": "https://x.example/", "final_url": "https://x.example/", "title": "Bad", "looks_like_instructions": true},
        {"url": "https://gone.example/", "error": "Status(404)"}
    ]})));
    assert_eq!(
        a.web_pages,
        vec![(
            "page-4".to_string(),
            "https://tokio.rs/tutorial".to_string()
        )],
        "only the clean page is added"
    );
    let t = text(&a);
    assert!(
        t.contains("not given to the model") && t.contains("could not be read"),
        "{t}"
    );
    // the next chat message carries the approved page and nothing else
    let r = enter(&mut a, "what does it say?");
    assert_eq!(r[0].body.as_ref().unwrap()["sources"], json!(["page-4"]));
    a.handle(ok(
        &r[0],
        200,
        json!({"done": true, "reply": "ok", "route": {"simulated": true}, "notes_used": 0}),
    ));
    // /web clear forgets them, and a new chat starts without them
    enter(&mut a, "/web clear");
    let r = enter(&mut a, "and now?");
    assert!(r[0].body.as_ref().unwrap().get("sources").is_none());
}

#[test]
fn tu35b_an_empty_search_answer_says_why_and_names_the_engines_that_failed_and_clears_old_numbers()
{
    let mut a = booted();
    let r = enter(&mut a, "/search zzzz");
    a.handle(ok(&r[0], 200, json!({"id": "search-1", "digest": "d1", "query": "zzzz", "will_send_to": "search.example", "stays_here": "x"})));
    let r = a.handle(key(KeyCode::Char('y')));
    a.handle(ok(&r[0], 200, json!({"results": [{"title": "T", "snippet": "", "url": "https://tokio.rs/tutorial", "host": "tokio.rs", "withheld": false}], "engines_failed": [], "empty_because": null})));
    assert_eq!(a.search_hits.len(), 1);
    let r = enter(&mut a, "/search zzzz");
    a.handle(ok(&r[0], 200, json!({"id": "search-2", "digest": "d2", "query": "zzzz", "will_send_to": "search.example", "stays_here": "x"})));
    let r = a.handle(key(KeyCode::Char('y')));
    a.handle(ok(&r[0], 200, json!({"results": [], "engines_failed": ["duckduckgo: CAPTCHA", "brave\u{202e}: HTTP error"], "empty_because": "the search site found nothing, and some of its search engines did not answer (listed below); try again later or use another search site"})));
    let t = text(&a);
    assert!(a.search_hits.is_empty(), "old numbers are gone");
    assert!(
        t.contains("No usable results.")
            && t.contains("try again later or use another search site")
            && t.contains("duckduckgo: CAPTCHA")
            && t.contains(": HTTP error")
            && !t.contains('\u{202e}'),
        "{t}"
    );
}

#[test]
fn tu35c_a_freshness_flag_is_parsed_strictly_sent_and_shown_before_the_yes() {
    assert_eq!(
        cmd::parse("/search tokio async"),
        Ok(Command::Search("tokio async".into(), None))
    );
    assert_eq!(
        cmd::parse("/search --week tokio  async"),
        Ok(Command::Search("tokio  async".into(), Some("week".into())))
    );
    for bad in [
        "/search --decade x",
        "/search --week",
        "/search --Week x",
        "/search --day --week x",
        "/search -- x",
    ] {
        assert!(cmd::parse(bad).is_err(), "{bad}");
    }
    let mut a = booted();
    let r = enter(&mut a, "/search --month tokio");
    assert_eq!(
        r[0].body.as_ref().unwrap(),
        &json!({"query": "tokio", "recency": "month"})
    );
    a.handle(ok(&r[0], 200, json!({"id": "search-1", "digest": "d1", "query": "tokio", "recency": "month", "will_send_to": "search.example", "stays_here": "x"})));
    let t = text(&a);
    assert!(t.contains("last month"), "{t}");
    assert!(a.handle(key(KeyCode::Char('n'))).is_empty());
    let r = enter(&mut a, "/search tokio");
    assert_eq!(r[0].body.as_ref().unwrap(), &json!({"query": "tokio"}));
}

#[test]
fn tu36_search_and_read_refusals_from_the_service_are_shown_and_nothing_is_pending() {
    let mut a = booted();
    let r = enter(&mut a, "/search my secret words");
    a.handle(ok(&r[0], 409, json!({"error": {"code": "NO_SEARCH_PROVIDER", "message": "web search is not set up: set KAKUREGA_SEARCH_URL"}})));
    assert!(text(&a).contains("KAKUREGA_SEARCH_URL") && matches!(a.mode, Mode::Normal));
    let r = enter(
        &mut a,
        "/read http://insecure.example/ https://127.0.0.1/admin",
    );
    a.handle(ok(&r[0], 400, json!({"error": {"code": "NOTHING_TO_FETCH", "message": "none of those addresses can be fetched"}, "items": [{"url": "http://insecure.example/", "allowed": false, "reason": "NotHttps"}]})));
    let t = text(&a);
    assert!(
        t.contains("none of those addresses")
            && t.contains("insecure.example")
            && matches!(a.mode, Mode::Normal),
        "{t}"
    );
    for bad in ["/search", "/read", "/web now"] {
        let r = enter(&mut a, bad);
        assert!(r.is_empty(), "{bad}");
    }
    assert!(text(&a).contains("usage: /search [--day|--week|--month|--year] WORDS"));
}

#[test]
fn tu37_remember_can_name_a_project_and_the_memory_list_shows_it() {
    let mut a = booted();
    let r = enter(
        &mut a,
        "/remember in payments-service: Invoices are rounded half up",
    );
    assert_eq!(r[0].path, "/api/memories");
    let body = r[0].body.as_ref().unwrap();
    assert_eq!(
        (body["text"].as_str(), body["project"].as_str()),
        (
            Some("Invoices are rounded half up"),
            Some("payments-service")
        )
    );
    let r = enter(&mut a, "/remember Plain note for everywhere");
    assert!(r[0].body.as_ref().unwrap().get("project").is_none());
    for bad in [
        "/remember in : text",
        "/remember in a/b: text",
        "/remember in payments:",
        "/remember in payments",
    ] {
        assert!(enter(&mut a, bad).is_empty(), "{bad}");
    }
    assert!(text(&a).contains("usage: /remember [in PROJECT:] TEXT"));
    let r = enter(&mut a, "/memory");
    a.handle(ok(&r[0], 200, json!({"memories": [
        {"id": "m1", "text": "Invoices are rounded half up", "level": "internal", "untrusted": false, "why": "you added it", "project": "payments-service"},
        {"id": "m2", "text": "Plain note", "level": "internal", "untrusted": false, "why": "you added it", "project": null}
    ]})));
    let t = text(&a);
    assert!(
        t.contains("m1  [internal] project payments-service  Invoices")
            && t.contains("m2  [internal]  Plain note"),
        "{t}"
    );
}

#[test]
fn tu38_ctrl_g_hands_the_whole_message_to_the_editor_and_takes_back_what_is_saved() {
    let mut a = booted();
    typed(&mut a, "draft ");
    a.handle(Event::Paste(
        "line one\nline two\nline three\nline four".into(),
    ));
    assert!(a.input.contains("[Pasted ~4 lines]"));
    a.handle(ctrl('g'));
    let sent = a.editor_request.take().expect("Ctrl-G asks for the editor");
    assert_eq!(
        sent, "draft line one\nline two\nline three\nline four",
        "folded pastes are shown in full to the editor"
    );
    // a failed edit leaves the composer as it was and says why
    let before = a.input.clone();
    a.finish_edit(Err(
        "`vi` ended with an error, so your text is unchanged".into()
    ));
    assert_eq!(a.input, before);
    assert!(text(&a).contains("ended with an error"));
    // a saved edit replaces the text; control characters are dropped; the cursor goes to the end
    a.finish_edit(Ok("rewritten \u{1b}[31mmessage\r\nsecond line\n\n".into()));
    assert!(
        !a.input.contains('\u{1b}') && !a.input.contains('\r'),
        "no control characters survive"
    );
    assert!(a.input.starts_with("rewritten ") && a.input.ends_with("message\nsecond line"));
    assert!(a.pasted.is_empty() && a.cursor == a.input.chars().count());
    let r = a.handle(key(KeyCode::Enter));
    assert!(r[0].body.as_ref().unwrap()["message"]
        .as_str()
        .unwrap()
        .ends_with("message\nsecond line"));
}

#[test]
fn tu38b_an_emptied_editor_file_is_a_cancel_and_keeps_the_draft() {
    let mut a = booted();
    typed(&mut a, "my careful draft");
    let before = a.input.clone();
    for empty in ["", "\n", "   \n\n", "\r\n\r\n"] {
        a.finish_edit(Ok(empty.into()));
        assert_eq!(a.input, before, "{empty:?} must not erase the draft");
    }
    assert!(text(&a).contains("returned nothing"));
    // text that is only whitespace around real words is still taken
    a.finish_edit(Ok("  kept words \n".into()));
    assert_eq!(a.input.trim(), "kept words");
}

#[test]
fn tu53_project_shows_sets_and_clears_the_project_a_conversation_is_about() {
    let mut a = booted();
    assert!(enter(&mut a, "/project").is_empty());
    assert!(text(&a).contains("no project"), "{}", text(&a));
    assert!(enter(&mut a, "/project papermark").is_empty());
    assert_eq!(a.chat_project.as_deref(), Some("papermark"));
    assert!(text(&a).contains("Project set to"));
    enter(&mut a, "/project");
    assert!(text(&a).contains("about the project \"papermark\""));
    assert!(
        a.status_line().contains("project papermark"),
        "{}",
        a.status_line()
    );
    enter(&mut a, "/project none");
    assert_eq!(a.chat_project, None);
    assert!(!a.status_line().contains("project"), "{}", a.status_line());
    // odd names are refused with the usage line and change nothing
    enter(&mut a, "/project papermark");
    for bad in ["a/b", "a\\b", &"x".repeat(81)] {
        enter(&mut a, &format!("/project {bad}"));
        assert_eq!(a.chat_project.as_deref(), Some("papermark"), "{bad:?}");
    }
    assert!(text(&a).contains("usage: /project"));
}

#[test]
fn tu54_the_project_travels_with_every_message_and_a_kept_conversation_remembers_and_can_forget_it()
{
    let mut a = booted();
    // no project: nothing is sent
    let r = enter(&mut a, "no project here");
    assert!(r[0].body.as_ref().unwrap().get("project").is_none());
    a.handle(ok(
        &r[0],
        200,
        json!({"reply": "ok", "route": {"simulated": false}, "notes_used": 0}),
    ));
    enter(&mut a, "/project papermark");
    let r = enter(&mut a, "about papermark");
    assert_eq!(r[0].body.as_ref().unwrap()["project"], "papermark");
    assert!(r[0].body.as_ref().unwrap().get("session").is_none());
    a.handle(ok(
        &r[0],
        200,
        json!({"reply": "hi", "route": {"simulated": false}, "notes_used": 1, "project": "papermark"}),
    ));
    assert!(
        text(&a).contains("1 note used · project papermark"),
        "the reply line says which project it used: {}",
        text(&a)
    );
    // keeping the conversation saves its project
    let k = enter(&mut a, "/keep");
    assert_eq!(k[0].body.as_ref().unwrap()["project"], "papermark");
    a.handle(ok(&k[0], 201, json!({"id": "chat-pp11"})));
    let r = enter(&mut a, "still about it");
    assert_eq!(r[0].body.as_ref().unwrap()["session"], "chat-pp11");
    assert_eq!(r[0].body.as_ref().unwrap()["project"], "papermark");
    a.handle(ok(&r[0], 200, json!({"reply": "yes", "route": {"simulated": false}, "notes_used": 1, "saved": true, "project": "papermark"})));
    // clearing the project of a kept conversation sends an empty name, which tells the service to forget it
    enter(&mut a, "/project none");
    let r = enter(&mut a, "now general");
    assert_eq!(
        r[0].body.as_ref().unwrap()["project"],
        "",
        "{:?}",
        r[0].body
    );
}

#[test]
fn tu55_opening_a_kept_conversation_takes_its_project_and_one_without_clears_it() {
    let mut a = booted();
    enter(&mut a, "/project other");
    let r = enter(&mut a, "/sessions");
    a.handle(ok(
        &r[0],
        200,
        json!({"sessions": [
            {"id": "chat-2", "title": "with project", "created": 200, "updated": 200, "turns": 2, "project": "papermark"},
            {"id": "chat-1", "title": "no project", "created": 100, "updated": 100, "turns": 2, "project": null}
        ]}),
    ));
    let r = a.handle(key(KeyCode::Enter));
    assert_eq!(r[0].path, "/api/sessions/chat-2");
    a.handle(ok(
        &r[0],
        200,
        json!({"id": "chat-2", "project": "papermark", "turns": [{"role": "user", "content": "q"}, {"role": "assistant", "content": "a"}]}),
    ));
    assert_eq!(a.chat_project.as_deref(), Some("papermark"));
    let r = enter(&mut a, "/sessions");
    a.handle(ok(
        &r[0],
        200,
        json!({"sessions": [{"id": "chat-1", "title": "no project", "created": 100, "updated": 100, "turns": 2, "project": null}]}),
    ));
    let r = a.handle(key(KeyCode::Enter));
    a.handle(ok(
        &r[0],
        200,
        json!({"id": "chat-1", "project": null, "turns": [{"role": "user", "content": "q"}, {"role": "assistant", "content": "a"}]}),
    ));
    assert_eq!(
        a.chat_project, None,
        "a conversation without a project clears the old one"
    );
}

#[test]
fn tu56_similar_lists_look_alike_notes_changes_nothing_and_says_how_much_was_compared() {
    let mut a = booted();
    let r = enter(&mut a, "/similar");
    assert_eq!(
        (r[0].method, r[0].path.as_str()),
        (Method::Get, "/api/memories/similar")
    );
    a.handle(ok(
        &r[0],
        200,
        json!({
            "groups": [[
                {"id": "m-2", "text": "Login sessions kept in a dictionary.", "project": null},
                {"id": "m-1", "text": "Login sessions are kept in a dictionary", "project": "alpha"}
            ]],
            "scanned": 7, "complete": true, "note": "x"
        }),
    ));
    let t = text(&a);
    assert!(t.contains("2 notes say almost the same"), "{t}");
    assert!(
        t.contains("m-2") && t.contains("m-1") && t.contains("[project alpha]"),
        "{t}"
    );
    assert!(
        t.contains("7 notes compared") && t.contains("Nothing was merged or forgotten"),
        "{t}"
    );
    assert!(!t.contains("only the newest"), "{t}");
    // an empty answer says what was compared, and a partial scan says so
    let r = enter(&mut a, "/similar");
    a.handle(ok(
        &r[0],
        200,
        json!({"groups": [], "scanned": 200, "complete": false, "note": "x"}),
    ));
    let t = text(&a);
    assert!(
        t.contains("No notes say almost the same thing (200 notes compared)"),
        "{t}"
    );
    assert!(t.contains("only the newest 200 were compared"), "{t}");
    // a failure is shown, not hidden
    let r = enter(&mut a, "/similar");
    a.handle(ok(
        &r[0],
        500,
        json!({"error": {"code": "STORE", "message": "disk trouble"}}),
    ));
    assert!(text(&a).contains("disk trouble"));
}

fn footer_text(a: &App) -> String {
    let mut term = Terminal::new(TestBackend::new(110, 30)).unwrap();
    term.draw(|f| draw(f, a, &Theme::from_env())).unwrap();
    let buf = term.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn tu39_the_first_esc_only_arms_and_the_arming_runs_out() {
    let mut a = booted();
    let r = enter(&mut a, "hello");
    assert!(footer_text(&a).contains("Esc twice stops the answer"));
    a.handle(key(KeyCode::Esc));
    assert!(a.esc_armed > 0 && !a.cancel.load(std::sync::atomic::Ordering::SeqCst));
    for _ in 0..kk_tui::app::ESC_ARM_TICKS {
        a.handle(Event::Tick);
    }
    assert_eq!(a.esc_armed, 0, "left alone, the arming expires");
    a.handle(key(KeyCode::Esc));
    assert!(
        !a.cancel.load(std::sync::atomic::Ordering::SeqCst),
        "an Esc after it expired arms again instead of stopping"
    );
    a.handle(key(KeyCode::Esc));
    assert!(a.cancel.load(std::sync::atomic::Ordering::SeqCst));
    a.handle(ok(&r[0], 200, json!({"stopped": true})));
    assert_eq!(a.esc_armed, 0);
    assert!(
        !footer_text(&a).contains("Esc twice stops"),
        "the hint goes away with the answer"
    );
}

#[test]
fn tu40_arrow_keys_recall_history_but_never_replace_a_draft_or_edits_to_a_recalled_line() {
    let mut a = booted();
    enter(&mut a, "/status");
    enter(&mut a, "/reviews");
    // an empty composer walks the history
    a.handle(key(KeyCode::Up));
    assert_eq!(a.input, "/reviews");
    a.handle(key(KeyCode::Up));
    assert_eq!(a.input, "/status");
    a.handle(key(KeyCode::Down));
    assert_eq!(a.input, "/reviews");
    // editing a recalled line makes it a draft: Up and Down leave it alone
    typed(&mut a, " now");
    a.handle(key(KeyCode::Up));
    assert_eq!(a.input, "/reviews now", "Up does not throw the edit away");
    a.handle(key(KeyCode::Down));
    assert_eq!(a.input, "/reviews now", "Down does not clear it");
    // a fresh draft is never replaced
    a.handle(key(KeyCode::Esc));
    assert!(a.input.is_empty());
    typed(&mut a, "half a thought");
    a.handle(key(KeyCode::Up));
    assert_eq!(a.input, "half a thought");
    a.handle(key(KeyCode::Down));
    assert_eq!(a.input, "half a thought");
    // clearing the composer brings history back
    a.handle(key(KeyCode::Esc));
    a.handle(key(KeyCode::Up));
    assert_eq!(a.input, "/reviews");
    // Down past the newest entry empties the composer, as before
    a.handle(key(KeyCode::Down));
    assert!(a.input.is_empty());
}

#[test]
fn tu41_the_release_view_lists_personal_data_counts_between_what_is_withheld_and_what_is_not_established(
) {
    let lines = kk_tui::app::release_lines(&json!({
        "available": true, "digest": "abc", "recipient": "acme", "files": [{"name": "sample-1.json", "bytes": 10, "sha256": "x"}],
        "removed": [], "not_confirmed": [], "not_established": ["That no other defects exist."],
        "personal_data": [{"file": "sample-1.json", "found": "2 email addresses, 1 payment card number"}]
    }));
    let text = lines.join("\n");
    let w = text.find("Withheld and never released").unwrap();
    let p = text.find("Personal data noticed in these files").unwrap();
    let n = text.find("This release does not establish").unwrap();
    assert!(w < p && p < n, "{text}");
    assert!(
        text.contains("sample-1.json: 2 email addresses, 1 payment card number")
            && text.contains("blocks nothing")
    );
    let none = kk_tui::app::release_lines(&json!({"available": true, "digest": "abc", "recipient": "a", "files": [], "removed": [], "not_confirmed": [], "not_established": [], "personal_data": []})).join("\n");
    assert!(
        !none.contains("Personal data noticed"),
        "nothing is said when nothing was noticed"
    );
    let hostile = kk_tui::app::release_lines(&json!({"available": true, "digest": "a", "recipient": "a", "files": [], "removed": [], "not_confirmed": [], "not_established": [], "personal_data": [{"file": "a\u{1b}[2Jb", "found": "x\u{7}y"}]})).join("\n");
    assert!(!hostile.contains('\u{1b}') && !hostile.contains('\u{7}'));
}

fn open_approval_for_job(app: &mut App) {
    app.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    open_approval(app);
    assert!(matches!(app.mode, Mode::Approval(_)));
}

#[test]
fn tu42_an_approval_view_closes_by_itself_when_the_approval_was_decided_somewhere_else() {
    let mut a = booted();
    open_approval_for_job(&mut a);
    // while it is still waiting, the view stays; the watch asks the service which approvals are waiting
    let r = a.handle(Event::Tick);
    let watch = r
        .iter()
        .find(|q| q.path == "/admin/approvals")
        .expect("a watch request");
    assert!(watch.admin && matches!(watch.method, Method::Get));
    assert!(
        a.handle(Event::Tick)
            .iter()
            .all(|q| q.path != "/admin/approvals"),
        "one watch at a time"
    );
    a.handle(ok(
        watch,
        200,
        json!({"approvals": [{"id": "appr-9", "job": "job-4"}]}),
    ));
    assert!(
        matches!(a.mode, Mode::Approval(_)),
        "still waiting, so the view stays"
    );
    // the next look finds it gone (approved or refused in the desktop window)
    let r = a.handle(Event::Tick);
    let watch = r.iter().find(|q| q.path == "/admin/approvals").unwrap();
    a.handle(ok(watch, 200, json!({"approvals": []})));
    assert!(matches!(a.mode, Mode::Normal));
    assert!(text(&a).contains("Approval closed"));
    // a failed look changes nothing
    let mut b = booted();
    open_approval_for_job(&mut b);
    let r = b.handle(Event::Tick);
    let watch = r.iter().find(|q| q.path == "/admin/approvals").unwrap();
    b.handle(Event::Resp(Resp {
        id: watch.id,
        result: Err("down".into()),
    }));
    assert!(matches!(b.mode, Mode::Approval(_)));
    assert!(
        !text(&b).contains("Could not reach"),
        "a background look stays quiet"
    );
    // another approval being gone does not close this one
    let r = b.handle(Event::Tick);
    let watch = r.iter().find(|q| q.path == "/admin/approvals").unwrap();
    b.handle(ok(
        watch,
        200,
        json!({"approvals": [{"id": "appr-77", "job": "job-9"}]}),
    ));
    assert!(
        matches!(b.mode, Mode::Normal),
        "this one is not in the list"
    );
}

#[test]
fn tu43_while_the_owner_is_approving_here_the_watch_does_not_interfere() {
    let mut a = booted();
    open_approval_for_job(&mut a);
    typed(&mut a, "abcdef012345");
    let sent = a.handle(key(KeyCode::Enter));
    assert!(
        sent.iter().any(|q| q.path.ends_with("/approve")),
        "the approval was sent"
    );
    let ticks = a.handle(Event::Tick);
    assert!(
        ticks.iter().all(|q| q.path != "/admin/approvals"),
        "no watch while our own answer is on its way"
    );
    let approve = sent.iter().find(|q| q.path.ends_with("/approve")).unwrap();
    a.handle(ok(approve, 200, json!({"state": "approved"})));
    assert!(matches!(a.mode, Mode::Normal));
    assert!(text(&a).contains("Approved. The files are fixed."));
    assert!(!text(&a).contains("Approval closed"));
}

#[test]
fn tu44_a_chat_answer_lists_the_notes_that_shaped_it() {
    let mut a = booted();
    let r = enter(&mut a, "what is the staging port?");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "8081", "route": {"simulated": true}, "notes_used": 1, "notes": [{"id": "m1", "text": "The staging server listens on port 8081", "why": "you asked to remember it; matched by keywords", "untrusted": false, "project": null}]})));
    let t = text(&a);
    assert!(t.contains("1 note used") && t.contains("note: The staging server listens on port 8081 (you asked to remember it; matched by keywords)"), "{t}");
    let r = enter(&mut a, "again");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "ok", "route": {"simulated": true}, "notes_used": 1, "notes": [{"id": "m2", "text": "x\u{1b}[31mred", "why": "w\u{7}", "untrusted": true}]})));
    assert!(!text(&a).contains('\u{1b}') && !text(&a).contains('\u{7}'));
}

#[test]
fn tu45_a_fix_whose_files_need_attention_is_listed_with_the_files_named_and_no_instruction_to_apply(
) {
    let mut a = booted();
    a.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    let r = enter(&mut a, "/fixes");
    a.handle(ok(&r[0], 200, json!({"fixes": [
        {"index": 0, "title": "t", "file": "a.py", "explanation": "e", "status": "fixes_the_reproducer", "verified": true, "diff": "-a\n+b\n", "applied": false, "state": "attention", "attention": "payments/auth/tokens.py\u{1b}[31m"},
        {"index": 1, "title": "t", "file": "b.py", "explanation": "e", "status": "fixes_the_reproducer", "verified": true, "diff": "-a\n+b\n", "applied": false, "state": "applying", "attention": null}]})));
    let t = text(&a);
    assert!(
        t.contains("not as Kakurega left them: payments/auth/tokens.py")
            && t.contains("A change to your files is being made right now"),
        "{t}"
    );
    assert!(
        !t.contains("to apply."),
        "no invitation to apply a fix that needs attention: {t}"
    );
    assert!(
        !t.contains('\u{1b}'),
        "file names from the service are cleaned before they reach the screen"
    );
}

#[test]
fn tu46_the_footer_shows_the_model_s_own_count_against_the_window_and_warns_near_the_limit() {
    let mut a = booted();
    let r = enter(&mut a, "hello");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": false}, "notes_used": 0, "usage": {"context_tokens": 4321, "context_tokens_estimate": 5000, "context_window": 16384, "estimate": false, "history_turns_dropped": 0}})));
    let t = text(&a);
    assert!(
        t.contains("4321 of 16384 tokens of context used (26%, counted by the model)"),
        "{t}"
    );
    assert!(!t.contains("close to the model's limit"), "{t}");
    let r = enter(&mut a, "again");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": false}, "notes_used": 0, "usage": {"context_tokens": 900, "context_tokens_estimate": 900, "context_window": 1000, "estimate": true}})));
    let t = text(&a);
    assert!(
        t.contains("about 900 of 1000 tokens of context used (90%)"),
        "{t}"
    );
    assert!(t.contains("close to the model's limit"), "{t}");
}

#[test]
fn tu47_the_footer_says_how_many_web_pages_were_used_cut_short_or_left_out() {
    let mut a = booted();
    let r = enter(&mut a, "what does the report say");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 0, "web_sources_used": 2, "web_sources_trimmed": 1, "web_sources_left_out": 3})));
    let t = text(&a);
    assert!(t.contains("2 web pages used"), "{t}");
    assert!(
        t.contains("1 page cut short to fit the model's window"),
        "{t}"
    );
    assert!(
        t.contains("3 pages left out: no room in the model's window"),
        "{t}"
    );
    let r = enter(&mut a, "again");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 0, "web_sources_used": 0})));
    assert_eq!(
        text(&a).matches("cut short").count(),
        1,
        "nothing is said when no page was cut"
    );
}

#[test]
fn tu48_the_footer_says_how_many_matching_notes_were_left_out() {
    let mut a = booted();
    let r = enter(&mut a, "what do my notes say");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 2, "notes_left_out": 3})));
    assert!(
        text(&a).contains("3 matching notes left out: no room in the model's window"),
        "{}",
        text(&a)
    );
    let r = enter(&mut a, "again");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 1, "notes_left_out": 0})));
    assert_eq!(
        text(&a).matches("matching notes left out").count(),
        1,
        "nothing is said when none were left out"
    );
}

#[test]
fn tu49_a_kept_exchange_that_was_not_saved_names_the_real_reason() {
    let mut a = booted();
    a.session = Some("s-1".into());
    for (reason, expected) in [
        (
            json!("forgotten"),
            "not saved: this conversation was forgotten",
        ),
        (
            json!("error"),
            "not saved: the encrypted store could not be written",
        ),
        (json!("full"), "not saved (this conversation is full)"),
    ] {
        let r = enter(&mut a, "hello");
        a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 0, "saved": false, "not_saved": reason})));
        assert!(text(&a).contains(expected), "{expected}: {}", text(&a));
    }
}

#[test]
fn tu50_the_footer_says_when_older_messages_are_summarised_or_could_not_be() {
    let mut a = booted();
    let r = enter(&mut a, "hello");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 0, "usage": {"context_tokens_estimate": 900, "history_turns_summarized": 12, "compacted_now": true, "history_turns_dropped": 0}})));
    assert!(
        text(&a).contains("The oldest 12 messages were just summarised"),
        "{}",
        text(&a)
    );
    let r = enter(&mut a, "again");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 0, "usage": {"context_tokens_estimate": 900, "history_turns_summarized": 12, "compacted_now": false, "history_turns_dropped": 0}})));
    assert!(
        text(&a).contains("The oldest 12 messages are replaced by a summary"),
        "{}",
        text(&a)
    );
    let r = enter(&mut a, "third");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 0, "usage": {"context_tokens_estimate": 900, "history_turns_summarized": 0, "compaction_failed": "unavailable", "history_turns_dropped": 2}})));
    assert!(text(&a).contains("could not be summarised"), "{}", text(&a));
    let r = enter(&mut a, "fourth");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 0, "usage": {"context_tokens_estimate": 900, "history_turns_dropped": 0}})));
    assert_eq!(
        text(&a).matches("could not be summarised").count(),
        1,
        "nothing is said when nothing was summarised"
    );
}

#[test]
fn tu51_an_undo_that_kept_later_edits_says_so() {
    let mut a = booted();
    a.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    let r = enter(&mut a, "/fixes");
    a.handle(ok(&r[0], 200, json!({"fixes": [{"index": 0, "title": "t", "file": "a.py", "explanation": "e", "status": "fixes_the_reproducer", "verified": true, "diff": "-a\n+b\n", "applied": true, "state": "applied"}]})));
    for (kept, expected) in [
        (true, "Restored a.py. Your later edits to it were kept."),
        (false, "Restored a.py."),
    ] {
        enter(&mut a, "/undofix 1");
        let r = a.handle(key(KeyCode::Char('y')));
        a.handle(ok(
            &r[0],
            200,
            json!({"undone": true, "kept_later_edits": kept, "file": "a.py"}),
        ));
        let t = text(&a);
        assert!(t.contains(expected), "{expected}: {t}");
        if !kept {
            assert_eq!(
                t.matches("later edits to it were kept").count(),
                1,
                "said only for the undo that kept something"
            );
        }
    }
}

#[test]
fn tu52_the_footer_shows_the_summary_the_model_is_sent_in_place_of_the_oldest_messages() {
    let mut a = booted();
    let r = enter(&mut a, "hello");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 0, "usage": {"context_tokens_estimate": 900, "history_turns_summarized": 12, "compacted_now": true, "summary_text": "Goal: plan a trip\u{1b}[31m", "history_turns_dropped": 0}})));
    let t = text(&a);
    assert!(
        t.contains("Summary sent in their place: Goal: plan a trip"),
        "{t}"
    );
    assert!(
        !t.contains('\u{1b}'),
        "the summary is cleaned before it reaches the screen"
    );
    let r = enter(&mut a, "again");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 0, "usage": {"context_tokens_estimate": 900, "history_turns_summarized": 0, "summary_text": "stale text", "history_turns_dropped": 0}})));
    assert!(
        !text(&a).contains("stale text"),
        "no summary is shown when none stands in for anything"
    );
}

#[test]
fn tu60_a_finding_says_which_language_its_check_ran_in_and_never_trusts_the_name() {
    let mut app = booted();
    app.jobs.push(kk_tui::app::JobRow {
        id: "job-7".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    let r = enter(&mut app, "/findings job-7");
    app.handle(ok(&r[0], 200, json!({"findings": [
        {"title": "Py", "file": "a.py", "line": 1, "status": "reproduced", "explanation": "e", "reproducer": "print(1)", "language": "python"},
        {"title": "Js", "file": "a.js", "line": 2, "status": "reproduced", "explanation": "e", "reproducer": "x", "language": "javascript"},
        {"title": "Ts", "file": "a.ts", "line": 2, "status": "reproduced", "explanation": "e", "reproducer": "x", "language": "typescript"},
        {"title": "Old", "file": "b.py", "line": 3, "status": "reproduced", "explanation": "e", "reproducer": "x"},
        {"title": "None", "file": "c.js", "line": 4, "status": "not_reproduced", "explanation": "e", "reproducer": "", "language": "javascript"},
        {"title": "Odd", "file": "d.x", "line": 5, "status": "reproduced", "explanation": "e", "reproducer": "x", "language": "ru\u{1b}[31mby\u{202e}"}
    ]})));
    let card = |title: &str| {
        let c = app
            .cards
            .iter()
            .find(|c| c.title.as_deref() == Some(title))
            .unwrap_or_else(|| panic!("no card {title}"));
        c.lines.join("\n")
    };
    assert!(card("Py").contains("Checked with Python"), "{}", card("Py"));
    assert!(
        card("Js").contains("Checked with JavaScript"),
        "{}",
        card("Js")
    );
    assert!(
        card("Ts").contains("Checked with TypeScript"),
        "{}",
        card("Ts")
    );
    assert!(
        card("Old").contains("Checked with Python"),
        "a service that does not say reads as python: {}",
        card("Old")
    );
    assert!(
        !card("None").contains("Checked with"),
        "no reproducer, no language line: {}",
        card("None")
    );
    let odd = card("Odd");
    assert!(odd.contains("Checked with unknown language"), "{odd}");
    assert!(
        !odd.chars().any(|c| c.is_control() && c != '\n')
            && !odd.contains('\u{202e}')
            && !odd.contains("ruby"),
        "{odd}"
    );
}

#[test]
fn tu61_an_optional_check_that_fails_is_marked_optional_not_todo() {
    let mut app = booted();
    let r = enter(&mut app, "/status");
    app.handle(ok(&r[0], 200, json!({"ready": true, "checks": [
        {"id": "a", "label": "Model", "ok": true, "detail": "m", "fix": ""},
        {"id": "node", "label": "JavaScript checks", "required": false, "ok": false, "detail": "Node.js was not found", "fix": "Install Node.js 22"},
        {"id": "b", "label": "Sandbox", "ok": false, "detail": "none", "fix": "install it"},
        {"id": "c", "label": "Odd", "required": "false", "ok": false, "detail": "x", "fix": "y"}
    ]})));
    let t = text(&app);
    assert!(t.contains("[ok]   Model"), "{t}");
    assert!(t.contains("[optional] JavaScript checks"), "{t}");
    assert!(t.contains("[todo] Sandbox"), "{t}");
    assert!(
        t.contains("[todo] Odd"),
        "only a real false makes a check optional: {t}"
    );
}

#[test]
fn tu40_forgetting_everything_from_a_review_shows_what_goes_asks_first_and_reports_what_went() {
    use kk_tui::cmd;
    assert!(cmd::parse("/forgetreview").is_err());
    assert!(cmd::parse("/forgetreview a b").is_err());
    assert!(cmd::parse("/forgetreview job-4").is_ok());
    let mut app = booted();
    // the first step only looks: it asks the service what would be removed
    let r = enter(&mut app, "/forgetreview job-4");
    assert_eq!(
        (r[0].path.as_str(), r[0].admin),
        ("/api/memories/sources/job-4", false)
    );
    assert_eq!(r[0].method, Method::Get, "looking is a read");
    assert!(
        matches!(app.mode, Mode::Normal),
        "nothing to confirm before the counts are known"
    );
    app.handle(ok(
        &r[0],
        200,
        json!({"job": "job-4", "notes": 3, "suggestions": 2}),
    ));
    let prompt = match &app.mode {
        Mode::Confirm(c) => c.prompt.clone(),
        other => panic!("not asking: {other:?}"),
    };
    assert!(
        prompt.contains("Forget 3 notes and 2 waiting suggestions")
            && prompt.contains("job-4")
            && prompt.ends_with("[y/N]"),
        "{prompt}"
    );
    // Enter and n are not a yes
    for k in [KeyCode::Enter, KeyCode::Char('n'), KeyCode::Esc] {
        let mut a = booted();
        let r = enter(&mut a, "/forgetreview job-4");
        a.handle(ok(
            &r[0],
            200,
            json!({"job": "job-4", "notes": 3, "suggestions": 2}),
        ));
        let out = a.handle(key(k));
        assert!(
            out.is_empty()
                && matches!(a.mode, Mode::Normal)
                && text(&a).contains("Nothing was forgotten"),
            "{}",
            text(&a)
        );
    }
    let r = app.handle(key(KeyCode::Char('y')));
    assert_eq!(
        (r[0].path.as_str(), r[0].admin),
        ("/api/memories/sources/job-4/forget", false)
    );
    assert_eq!(r[0].method, Method::Post);
    app.handle(ok(
        &r[0],
        200,
        json!({"forgotten": true, "job": "job-4", "notes": 3, "suggestions": 2}),
    ));
    assert!(
        text(&app).contains("Forgot 3 notes and 2 suggestions"),
        "{}",
        text(&app)
    );
    // a review that left nothing, and a refusal, are said plainly
    let mut a = booted();
    let r = enter(&mut a, "/forgetreview job-9");
    a.handle(ok(
        &r[0],
        404,
        json!({"error": {"code": "NO_REVIEW", "message": "no such review"}}),
    ));
    assert!(text(&a).contains("no such review") && matches!(a.mode, Mode::Normal));
    let mut a = booted();
    let r = enter(&mut a, "/forgetreview job-5");
    a.handle(ok(
        &r[0],
        200,
        json!({"job": "job-5", "notes": 0, "suggestions": 0}),
    ));
    assert!(
        matches!(a.mode, Mode::Normal) && text(&a).contains("left nothing"),
        "{}",
        text(&a)
    );
}

#[test]
fn tu41_the_terminal_shows_sets_and_clears_the_owners_standing_instructions() {
    use kk_tui::cmd::InstructionsAction;
    assert_eq!(
        cmd::parse("/instructions").unwrap(),
        Command::Instructions(InstructionsAction::Show)
    );
    assert_eq!(
        cmd::parse("/instructions set Answer in short bullets").unwrap(),
        Command::Instructions(InstructionsAction::Set {
            project: None,
            text: "Answer in short bullets".into()
        })
    );
    assert_eq!(
        cmd::parse("/instructions set in kestrel: Prices are in euros").unwrap(),
        Command::Instructions(InstructionsAction::Set {
            project: Some("kestrel".into()),
            text: "Prices are in euros".into()
        })
    );
    assert_eq!(
        cmd::parse("/instructions clear").unwrap(),
        Command::Instructions(InstructionsAction::Clear(None))
    );
    assert_eq!(
        cmd::parse("/instructions clear kestrel").unwrap(),
        Command::Instructions(InstructionsAction::Clear(Some("kestrel".into())))
    );
    for bad in [
        "/instructions set",
        "/instructions set   ",
        "/instructions set in : text",
        "/instructions set in a/b: text",
        "/instructions set in kestrel:",
        "/instructions clear a b",
        "/instructions clear a/b",
        "/instructions frobnicate",
        "/instructions set in kestrel text without a colon",
        "/instructions set in ..: text",
        "/instructions set in .: text",
        "/instructions clear ..",
        "/instructions clear .",
    ] {
        assert!(cmd::parse(bad).is_err(), "{bad}");
    }
    // show: a request, then the texts in the transcript
    let mut app = booted();
    let r = enter(&mut app, "/instructions");
    assert_eq!(
        (r[0].path.as_str(), r[0].admin),
        ("/api/instructions", false)
    );
    assert_eq!(r[0].method, Method::Get);
    app.handle(ok(&r[0], 200, json!({"global": {"text": "Answer in short bullets", "updated_at": 5}, "projects": [{"project": "kestrel", "text": "Prices are in euros", "updated_at": 6}], "max_chars": 4000})));
    let t = text(&app);
    assert!(
        t.contains("Answer in short bullets")
            && t.contains("kestrel")
            && t.contains("Prices are in euros"),
        "{t}"
    );
    let mut a = booted();
    let r = enter(&mut a, "/instructions");
    a.handle(ok(
        &r[0],
        200,
        json!({"global": null, "projects": [], "max_chars": 4000}),
    ));
    assert!(
        text(&a).contains("No standing instructions"),
        "{}",
        text(&a)
    );
    // set and clear send exactly what was typed and say what happened
    let mut a = booted();
    let r = enter(&mut a, "/instructions set Answer in short bullets");
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/instructions", Method::Post)
    );
    assert_eq!(r[0].body, Some(json!({"text": "Answer in short bullets"})));
    a.handle(ok(&r[0], 200, json!({"saved": true})));
    assert!(text(&a).contains("Saved"), "{}", text(&a));
    let r = enter(&mut a, "/instructions set in kestrel: Prices are in euros");
    assert_eq!(
        r[0].body,
        Some(json!({"project": "kestrel", "text": "Prices are in euros"}))
    );
    let r = enter(&mut a, "/instructions clear kestrel");
    assert_eq!(r[0].body, Some(json!({"project": "kestrel", "text": ""})));
    a.handle(ok(&r[0], 200, json!({"removed": true, "saved": false})));
    assert!(text(&a).contains("Removed"), "{}", text(&a));
    let r = enter(&mut a, "/instructions clear");
    assert_eq!(r[0].body, Some(json!({"text": ""})));
    // a refused secret is scrubbed from the transcript and the history, like a note
    let mut a = booted();
    let r = enter(&mut a, "/instructions set use the key hunter2hunter2");
    a.handle(ok(&r[0], 422, json!({"error": {"code": "INSTRUCTION_REFUSED", "message": "this looks like a secret; instructions are sent to the model, so keep secrets out of them"}})));
    assert!(!text(&a).contains("hunter2"), "{}", text(&a));
    assert!(text(&a).contains("keep secrets out"));
    a.handle(key(KeyCode::Up));
    assert!(!a.input.contains("hunter2"), "{:?}", a.input);
}

#[test]
fn tu42_the_terminal_lists_notes_that_disagree_and_keeps_one_or_says_both_are_right() {
    assert_eq!(cmd::parse("/conflicts").unwrap(), Command::Conflicts);
    assert_eq!(
        cmd::parse("/choose m-1 m-2").unwrap(),
        Command::Choose {
            keep: "m-1".into(),
            replaced: "m-2".into()
        }
    );
    assert_eq!(
        cmd::parse("/bothright m-1 m-2").unwrap(),
        Command::BothRight {
            a: "m-1".into(),
            b: "m-2".into()
        }
    );
    for bad in [
        "/choose",
        "/choose m-1",
        "/choose m-1 m-2 m-3",
        "/choose m-1 m-1",
        "/bothright m-1",
        "/bothright a/b c",
        "/conflicts now",
    ] {
        assert!(cmd::parse(bad).is_err(), "{bad}");
    }
    let mut app = booted();
    let r = enter(&mut app, "/conflicts");
    assert_eq!(
        (r[0].path.as_str(), r[0].admin, r[0].method),
        ("/api/memories/conflicts", false, Method::Get)
    );
    app.handle(ok(&r[0], 200, json!({"pairs": [{"a": {"id": "m-1", "text": "Acme pays on the 15th"}, "b": {"id": "m-2", "text": "Acme pays on the 20th"}, "why": "other_claim", "subject": null}, {"a": {"id": "m-3", "text": "We deploy to Azure"}, "b": {"id": "m-4", "text": "Releases go to AWS"}, "why": "same_subject", "subject": "deploy target"}], "scanned": 4, "complete": true})));
    let t = text(&app);
    for needle in [
        "m-1",
        "m-2",
        "Acme pays on the 15th",
        "Acme pays on the 20th",
        "deploy target",
        "/choose",
        "/bothright",
    ] {
        assert!(t.contains(needle), "{needle}: {t}");
    }
    let mut a = booted();
    let r = enter(&mut a, "/conflicts");
    a.handle(ok(
        &r[0],
        200,
        json!({"pairs": [], "scanned": 3, "complete": true}),
    ));
    assert!(text(&a).contains("No notes disagree"), "{}", text(&a));
    // choosing asks first: only y sends, and it says which note stays
    let mut a = booted();
    assert!(enter(&mut a, "/choose m-2 m-1").is_empty());
    let prompt = match &a.mode {
        Mode::Confirm(c) => c.prompt.clone(),
        other => panic!("not asking: {other:?}"),
    };
    assert!(
        prompt.contains("m-2") && prompt.contains("m-1") && prompt.ends_with("[y/N]"),
        "{prompt}"
    );
    for k in [KeyCode::Enter, KeyCode::Char('n'), KeyCode::Esc] {
        let mut b = booted();
        enter(&mut b, "/choose m-2 m-1");
        assert!(b.handle(key(k)).is_empty() && matches!(b.mode, Mode::Normal));
    }
    let r = a.handle(key(KeyCode::Char('y')));
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/memories/conflicts/keep", Method::Post)
    );
    assert_eq!(r[0].body, Some(json!({"keep": "m-2", "replaced": "m-1"})));
    a.handle(ok(&r[0], 200, json!({"kept": "m-2", "replaced": "m-1"})));
    assert!(text(&a).contains("Kept"), "{}", text(&a));
    // both are right needs no question: nothing is lost
    let mut a = booted();
    let r = enter(&mut a, "/bothright m-1 m-2");
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/memories/conflicts/both", Method::Post)
    );
    assert_eq!(r[0].body, Some(json!({"a": "m-1", "b": "m-2"})));
    a.handle(ok(&r[0], 200, json!({"dismissed": true})));
    assert!(text(&a).contains("Both notes stay"), "{}", text(&a));
    let r = enter(&mut a, "/choose m-9 m-8");
    assert!(r.is_empty());
    a.handle(key(KeyCode::Char('y')));
    let mut a2 = booted();
    enter(&mut a2, "/choose m-9 m-8");
    let r = a2.handle(key(KeyCode::Char('y')));
    a2.handle(ok(
        &r[0],
        404,
        json!({"error": {"code": "NO_MEMORY", "message": "no such memory"}}),
    ));
    assert!(text(&a2).contains("no such memory"));
}

#[test]
fn tu43_the_terminal_finds_kept_conversations_by_words_and_renames_one() {
    assert_eq!(
        cmd::parse("/find the deadline").unwrap(),
        Command::Find("the deadline".into())
    );
    assert_eq!(
        cmd::parse("/rename s-3 Client call").unwrap(),
        Command::RenameChat {
            id: "s-3".into(),
            title: "Client call".into()
        }
    );
    for bad in [
        "/find",
        "/find   ",
        "/rename",
        "/rename s-3",
        "/rename a/b title",
        "/rename s-3   ",
    ] {
        assert!(cmd::parse(bad).is_err(), "{bad}");
    }
    let mut app = booted();
    let r = enter(&mut app, "/find deadline friday");
    assert_eq!(
        (r[0].path.as_str(), r[0].admin, r[0].method),
        (
            "/api/sessions/search?q=deadline%20friday",
            false,
            Method::Get
        )
    );
    app.handle(ok(&r[0], 200, json!({"results": [{"id": "s-3", "title": "Client call", "updated": 5, "project": null, "snippets": [{"role": "user", "text": "the client said the deadline moved to Friday"}]}]})));
    let t = text(&app);
    assert!(
        t.contains("s-3") && t.contains("Client call") && t.contains("deadline moved to Friday"),
        "{t}"
    );
    let mut a = booted();
    let r = enter(&mut a, "/find nothing here");
    a.handle(ok(&r[0], 200, json!({"results": []})));
    assert!(
        text(&a).contains("No kept conversation has all of those words"),
        "{}",
        text(&a)
    );
    // odd characters in the words are encoded, never sent as part of the path or query structure
    let mut a = booted();
    let r = enter(&mut a, "/find a&b=c#d %20é");
    assert!(
        r[0].path.starts_with("/api/sessions/search?q=")
            && !r[0].path[23..].contains(['&', '#', ' ', '=']),
        "{}",
        r[0].path
    );
    let r = enter(&mut a, "/rename s-3 Client call, Friday");
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/sessions/s-3/rename", Method::Post)
    );
    assert_eq!(r[0].body, Some(json!({"title": "Client call, Friday"})));
    a.handle(ok(&r[0], 200, json!({"renamed": true})));
    assert!(text(&a).contains("Renamed"), "{}", text(&a));
    let r = enter(&mut a, "/rename s-9 x");
    a.handle(ok(
        &r[0],
        404,
        json!({"error": {"code": "NO_SESSION", "message": "there is no such kept conversation"}}),
    ));
    assert!(text(&a).contains("no such kept conversation"));
}

#[test]
fn tu44_the_terminal_shows_model_usage_for_a_number_of_days() {
    assert_eq!(cmd::parse("/usage").unwrap(), Command::Usage(30));
    assert_eq!(cmd::parse("/usage 7").unwrap(), Command::Usage(7));
    for bad in [
        "/usage 0",
        "/usage 367",
        "/usage abc",
        "/usage 7 8",
        "/usage -1",
    ] {
        assert!(cmd::parse(bad).is_err(), "{bad}");
    }
    let mut app = booted();
    let r = enter(&mut app, "/usage 7");
    assert_eq!(
        (
            r[0].path.starts_with("/api/usage?from="),
            r[0].admin,
            r[0].method
        ),
        (true, false, Method::Get)
    );
    let q = &r[0].path["/api/usage?from=".len()..];
    let (from, to) = q.split_once("&to=").expect("a range");
    let (from, to): (i64, i64) = (from.parse().unwrap(), to.parse().unwrap());
    assert_eq!(
        to - from,
        7 * 86_400,
        "seven days ending one second ahead of now: {q}"
    );
    app.handle(ok(&r[0], 200, json!({"from": from, "to": to, "routes": [{"route": "ollama:qwen2.5:3b", "class": "device_only", "simulated": false, "turns": 12, "prompt_tokens": 3400, "measured_turns": 12, "reply_tokens": 900, "avg_ms": 2100, "max_ms": 5000, "reply_tokens_per_second": 14.5}], "total": {"turns": 12, "prompt_tokens": 3400, "reply_tokens": 900}, "note": "n"})));
    let t = text(&app);
    for needle in ["ollama:qwen2.5:3b", "12", "3400", "900", "14.5", "Total"] {
        assert!(t.contains(needle), "{needle}: {t}");
    }
    // some replies had no count from the model: the figure is marked as partly an estimate
    let mut e = booted();
    let r = enter(&mut e, "/usage");
    e.handle(ok(&r[0], 200, json!({"from": 0, "to": 1, "routes": [{"route": "r", "class": "device_only", "simulated": false, "turns": 12, "prompt_tokens": 3400, "measured_turns": 3, "reply_tokens": 900, "avg_ms": 2100, "max_ms": 5000, "reply_tokens_per_second": 14.5}], "total": {"turns": 12, "prompt_tokens": 3400, "reply_tokens": 900}, "note": "n"})));
    assert!(text(&e).contains("3400* tokens read"), "{}", text(&e));
    let mut a = booted();
    let r = enter(&mut a, "/usage");
    a.handle(ok(&r[0], 200, json!({"from": 0, "to": 1, "routes": [], "total": {"turns": 0, "prompt_tokens": 0, "reply_tokens": 0}, "note": "n"})));
    assert!(text(&a).contains("No replies"), "{}", text(&a));
}

fn b64_decode(s: &str) -> Vec<u8> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("not standard base64: {}", c as char),
    };
    let bytes: Vec<u8> = s.bytes().filter(|b| *b != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let n = chunk
            .iter()
            .fold(0u32, |a, c| (a << 6) | u32::from(val(*c)))
            << (6 * (4 - chunk.len()));
        for i in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    out
}

#[test]
fn tu45_the_clipboard_sequence_is_standard_base64_and_refuses_what_a_terminal_cannot_take() {
    use kk_tui::app::{base64_standard, osc52};
    for (raw, want) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64_standard(raw.as_bytes()), want);
    }
    let text = "héllo → 世界\nsecond line\n";
    let seq = osc52(text).expect("small text");
    assert!(
        seq.starts_with("\x1b]52;c;") && seq.ends_with('\x07'),
        "{seq:?}"
    );
    let body = &seq["\x1b]52;c;".len()..seq.len() - 1];
    assert_eq!(
        b64_decode(body),
        text.as_bytes(),
        "exactly the bytes that were chosen"
    );
    assert!(
        osc52(&"x".repeat(75_000)).is_none(),
        "too large for a terminal to take"
    );
    assert!(osc52("").is_none(), "nothing to copy");
    // text can never end the sequence early or start another
    let hostile = "a\x07b\x1b]52;c;ZXZpbA==\x07c\x1b\\";
    let seq = osc52(hostile).unwrap();
    assert_eq!(seq.matches('\x07').count(), 1, "only the closing bell");
    assert_eq!(seq.matches('\x1b').count(), 1, "only the opening escape");
}

#[test]
fn tu46_the_terminal_searches_copies_and_exports_its_own_transcript() {
    assert_eq!(
        cmd::parse("/transcript deadline friday").unwrap(),
        Command::Transcript("deadline friday".into())
    );
    assert_eq!(cmd::parse("/copy").unwrap(), Command::Copy(false));
    assert_eq!(cmd::parse("/copy all").unwrap(), Command::Copy(true));
    assert_eq!(
        cmd::parse("/export call-notes").unwrap(),
        Command::Export("call-notes".into())
    );
    for bad in [
        "/transcript",
        "/transcript   ",
        "/copy everything",
        "/copy all all",
        "/export",
        "/export a b",
    ] {
        assert!(cmd::parse(bad).is_err(), "{bad}");
    }
    let mut app = booted();
    for i in 0..2000 {
        app.cards.push(Card {
            kind: Kind::Info,
            title: None,
            lines: vec![format!("line {i} about nothing in particular")],
        });
    }
    app.cards.push(Card {
        kind: Kind::User,
        title: None,
        lines: vec!["What did the client say about the DEADLINE?".into()],
    });
    app.cards.push(Card {
        kind: Kind::Info,
        title: Some("Assistant".into()),
        lines: vec![
            "The client said the deadline moved to Friday.".into(),
            "Nothing else.".into(),
        ],
    });
    // search: case does not matter, every word must be on the line, and the match is marked without colour
    let newest = |app: &App| -> String {
        let t = text(app);
        t.rsplit("Search in this conversation")
            .next()
            .unwrap_or("")
            .to_string()
    };
    enter(&mut app, "/transcript deadline");
    let t = newest(&app);
    assert!(
        t.contains("[[DEADLINE]]")
            && t.contains("The client said the [[deadline]] moved to Friday"),
        "{t}"
    );
    assert!(
        t.contains("2 lines hold") || text(&app).contains("2 lines hold"),
        "{t}"
    );
    enter(&mut app, "/transcript client friday");
    assert!(
        newest(&app).contains("[[client]] said the deadline moved to [[Friday]]"),
        "{}",
        newest(&app)
    );
    // typing in capitals finds the same lines, and an earlier search never counts as part of the conversation
    enter(&mut app, "/transcript DEADLINE FRIDAY");
    assert!(
        newest(&app).contains("[[deadline]] moved to [[Friday]]"),
        "{}",
        newest(&app)
    );
    assert!(
        !app.transcript_text()
            .contains("lines hold all of those words"),
        "search results are not in the transcript"
    );
    enter(&mut app, "/transcript zebra");
    assert!(
        text(&app)
            .trim_end()
            .ends_with("No line has all of those words."),
        "{}",
        text(&app)
    );
    enter(&mut app, "/transcript line");
    let card = newest(&app);
    let shown = card
        .lines()
        .filter(|l| {
            l.trim_start()
                .split_once(": ")
                .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
        .count();
    assert_eq!(shown, 30, "at most thirty lines are shown");
    assert!(
        card.contains("lines hold all of those words (the first 30 are shown)"),
        "and it says how many there are: {}",
        &card[..card.len().min(200)]
    );
    // copy: asks first, the last reply only, exactly its text
    let mut a = booted();
    a.cards.push(Card {
        kind: Kind::User,
        title: None,
        lines: vec!["question".into()],
    });
    a.cards.push(Card {
        kind: Kind::Info,
        title: Some("Assistant".into()),
        lines: vec!["the answer".into(), "second line".into()],
    });
    assert!(enter(&mut a, "/copy").is_empty());
    let prompt = match &a.mode {
        Mode::Confirm(c) => c.prompt.clone(),
        other => panic!("not asking: {other:?}"),
    };
    assert!(
        prompt.contains("clipboard")
            && prompt.contains("Other programs")
            && prompt.ends_with("[y/N]"),
        "{prompt}"
    );
    for k in [KeyCode::Enter, KeyCode::Char('n'), KeyCode::Esc] {
        let mut b = booted();
        b.cards.push(Card {
            kind: Kind::Info,
            title: Some("Assistant".into()),
            lines: vec!["x".into()],
        });
        enter(&mut b, "/copy");
        b.handle(key(k));
        assert!(
            b.clipboard_request.is_none(),
            "nothing is copied without a yes"
        );
    }
    assert!(a.clipboard_request.is_none());
    a.handle(key(KeyCode::Char('y')));
    assert_eq!(
        a.clipboard_request.as_deref(),
        Some("the answer\nsecond line")
    );
    let mut none = booted();
    enter(&mut none, "/copy");
    assert!(text(&none).contains("no reply to copy"), "{}", text(&none));
    // copy all: the whole transcript text, asked about first
    let mut all = booted();
    all.cards.push(Card {
        kind: Kind::User,
        title: None,
        lines: vec!["question".into()],
    });
    all.cards.push(Card {
        kind: Kind::Info,
        title: Some("Assistant".into()),
        lines: vec!["the answer".into()],
    });
    enter(&mut all, "/copy all");
    all.handle(key(KeyCode::Char('y')));
    let copied = all.clipboard_request.clone().unwrap();
    assert!(
        copied.contains("You: question") && copied.contains("the answer"),
        "{copied}"
    );
    // export: one request with the transcript text, and what comes back is said plainly
    let mut e = booted();
    e.cards.push(Card {
        kind: Kind::User,
        title: None,
        lines: vec!["question".into()],
    });
    e.cards.push(Card {
        kind: Kind::Info,
        title: Some("Assistant".into()),
        lines: vec!["the answer".into()],
    });
    let r = enter(&mut e, "/export call-notes");
    assert_eq!(
        (r[0].path.as_str(), r[0].method, r[0].admin),
        ("/api/exports/transcript", Method::Post, false)
    );
    let body = r[0].body.clone().unwrap();
    assert_eq!(body["name"], "call-notes");
    let sent = body["text"].as_str().unwrap();
    assert!(
        sent.contains("You: question") && sent.contains("the answer") && !sent.contains("/export"),
        "{sent}"
    );
    e.handle(ok(
        &r[0],
        200,
        json!({"path": "/home/p/.kakurega/exports/call-notes.md"}),
    ));
    assert!(
        text(&e).contains("Written to /home/p/.kakurega/exports/call-notes.md"),
        "{}",
        text(&e)
    );
    let r = enter(&mut e, "/export call-notes");
    e.handle(ok(&r[0], 409, json!({"error": {"code": "EXISTS", "message": "a file with that name is already there; choose another name"}})));
    assert!(text(&e).contains("already there"));
}

#[test]
fn tu47_the_status_line_carries_how_full_the_models_window_is_after_each_answer_and_clears_with_the_conversation(
) {
    let mut a = booted();
    assert!(!a.status_line().contains("context"), "{}", a.status_line());
    let r = enter(&mut a, "hello");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": false}, "notes_used": 0, "usage": {"context_tokens": 4321, "context_tokens_estimate": 5000, "context_window": 16384, "estimate": false, "history_turns_dropped": 0}})));
    assert!(
        a.status_line().ends_with("· context 26% (4.3k of 16.4k)"),
        "{}",
        a.status_line()
    );
    // an estimate is marked as one, and a missing window is not invented
    let r = enter(&mut a, "again");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": false}, "notes_used": 0, "usage": {"context_tokens_estimate": 900, "history_turns_dropped": 0}})));
    assert!(
        a.status_line().ends_with("· context ~900"),
        "{}",
        a.status_line()
    );
    enter(&mut a, "/clear");
    assert!(
        !a.status_line().contains("context"),
        "a fresh conversation starts empty: {}",
        a.status_line()
    );
}

#[test]
fn tu48_the_short_context_figure_is_exact_at_its_edges() {
    use kk_tui::app::context_meter;
    assert_eq!(context_meter(0, Some(1000), true), "context 0% (0 of 1.0k)");
    assert_eq!(context_meter(999, None, true), "context 999");
    assert_eq!(context_meter(1000, None, false), "context ~1.0k");
    assert_eq!(
        context_meter(16384, Some(16384), true),
        "context 100% (16.4k of 16.4k)"
    );
    assert_eq!(
        context_meter(40_000, Some(16384), true),
        "context 244% (40.0k of 16.4k)"
    );
    assert!(
        context_meter(u64::MAX, Some(1), true).starts_with("context 999%"),
        "huge figures neither overflow nor exceed 999%"
    );
    assert_eq!(
        context_meter(5, Some(0), true),
        "context 5",
        "a window of zero is no window"
    );
}

#[test]
fn tu49_branch_copies_a_kept_conversation_at_a_chosen_message_and_opens_the_copy() {
    let mut a = booted();
    let r = enter(&mut a, "/branch");
    assert!(
        r.is_empty() && text(&a).contains("Only a kept conversation can be branched"),
        "{}",
        text(&a)
    );
    // keep a conversation holding one exchange
    let r = enter(&mut a, "/keep");
    a.handle(ok(&r[0], 201, json!({"id": "chat-aaa"})));
    let r = enter(&mut a, "first question");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "first answer", "route": {"simulated": true}, "notes_used": 0})));
    // all of it, by default
    let r = enter(&mut a, "/branch");
    assert_eq!(r[0].path, "/api/sessions/chat-aaa/fork");
    assert_eq!(r[0].body, Some(json!({"at": 2})));
    // the copy is opened: its messages come back and the original is untouched
    let open = a.handle(ok(
        &r[0],
        201,
        json!({"id": "chat-bbb", "parent": "chat-aaa", "fork_at": 2}),
    ));
    assert_eq!(open[0].path, "/api/sessions/chat-bbb");
    a.handle(ok(&open[0], 200, json!({"id": "chat-bbb", "turns": [{"role": "user", "content": "first question"}, {"role": "assistant", "content": "first answer"}]})));
    assert!(
        text(&a).contains("Opened the kept conversation"),
        "{}",
        text(&a)
    );
    // a cut after one message, a point past the end, and words are refused without asking the service
    let r = enter(&mut a, "/branch 1");
    assert_eq!(r[0].body, Some(json!({"at": 1})));
    assert!(
        enter(&mut a, "/branch 9").is_empty() && text(&a).contains("has 2 messages"),
        "{}",
        text(&a)
    );
    assert!(
        enter(&mut a, "/branch soon").is_empty() && text(&a).contains("usage: /branch [N | pick]"),
        "{}",
        text(&a)
    );
    assert!(enter(&mut a, "/branch -1").is_empty());
    let shown = text(&a).matches("usage: /branch [N | pick]").count();
    assert!(enter(&mut a, "/branch 100001").is_empty());
    assert_eq!(
        text(&a).matches("usage: /branch [N | pick]").count(),
        shown + 1,
        "a number beyond any conversation is refused by the command itself"
    );
    // a refused branch says why and opens nothing
    let r = enter(&mut a, "/branch 0");
    let out = a.handle(ok(
        &r[0],
        409,
        json!({"error": {"code": "TOO_MANY", "message": "forget some kept conversations first"}}),
    ));
    assert!(
        out.is_empty() && text(&a).contains("forget some kept conversations first"),
        "{}",
        text(&a)
    );
}

#[test]
fn tu50_resume_at_start_opens_the_newest_kept_conversation_and_says_so_when_there_is_none() {
    let mut a = booted();
    let r = a.resume_at_start(None, false);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].path, "/api/sessions");
    // the service lists newest first: the first one is opened
    let open = a.handle(ok(&r[0], 200, json!({"sessions": [{"id": "chat-new", "title": "", "updated": 9, "turns": 2}, {"id": "chat-old", "title": "", "updated": 1, "turns": 4}]})));
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].path, "/api/sessions/chat-new");
    a.handle(ok(&open[0], 200, json!({"id": "chat-new", "turns": [{"role": "user", "content": "where did we stop"}, {"role": "assistant", "content": "at the audit"}]})));
    assert!(
        text(&a).contains("where did we stop") && text(&a).contains("Opened the kept conversation"),
        "{}",
        text(&a)
    );
    // none kept: a plain sentence, nothing opened
    let mut b = booted();
    let r = b.resume_at_start(None, false);
    let out = b.handle(ok(&r[0], 200, json!({"sessions": []})));
    assert!(
        out.is_empty() && text(&b).contains("no kept conversation to resume"),
        "{}",
        text(&b)
    );
    // the service unreachable or refusing: its reason, nothing opened
    let mut c = booted();
    let r = c.resume_at_start(None, false);
    let out = c.handle(ok(
        &r[0],
        500,
        json!({"error": {"code": "STORE", "message": "could not read the conversations"}}),
    ));
    assert!(
        out.is_empty() && text(&c).contains("could not read the conversations"),
        "{}",
        text(&c)
    );
    // a malformed entry never opens anything
    let mut d = booted();
    let r = d.resume_at_start(None, false);
    assert!(d
        .handle(ok(&r[0], 200, json!({"sessions": [{"title": "no id"}]})))
        .is_empty());
}

fn finish_review(app: &mut App, state: &str) {
    let r = enter(app, "/review payments for acme-audit");
    app.handle(ok(&r[0], 202, json!({"id": "job-4"})));
    let t = app.handle(Event::Tick);
    let body = |s: &str| json!({"state": s, "recipient": "acme-audit", "note": "stopped for a reason", "events": [{"stage": "queued", "detail": ""}]});
    app.handle(ok(&t[0], 200, body("running")));
    let t = app.handle(Event::Tick);
    app.handle(ok(&t[0], 200, body(state)));
}

#[test]
fn tu51_a_finished_review_asks_for_one_bell_and_a_title_that_names_nothing_of_the_job() {
    use kk_tui::attention::Attention;
    let mut app = booted();
    assert!(app.attention.is_none() && app.waiting.is_none());
    finish_review(&mut app, "ready");
    assert_eq!(app.attention.take(), Some(Attention::ReviewReady));
    assert!(app.attention.is_none(), "taken once, not twice");
    assert_eq!(app.waiting, Some(Attention::ReviewReady));
    let phrase = Attention::ReviewReady.phrase();
    for secret in ["acme-audit", "payments", "job-4", "stopped for a reason"] {
        assert!(!phrase.contains(secret));
    }
    let seq = kk_tui::attention::title(None, app.waiting).unwrap();
    assert!(seq.contains("a review is ready") && !seq.contains("acme") && !seq.contains("job-4"));
    // the title clears when the terminal is looked at again
    app.handle(Event::Focus(false));
    assert_eq!(app.focused, Some(false));
    assert!(app.waiting.is_some());
    app.handle(Event::Focus(true));
    assert_eq!(app.focused, Some(true));
    assert!(app.waiting.is_none());
    // a review that stopped is a different, equally generic, notice
    let mut stopped = booted();
    finish_review(&mut stopped, "failed");
    assert_eq!(stopped.attention.take(), Some(Attention::ReviewStopped));
    // a key press is the owner acting: the title clears
    stopped.handle(key(KeyCode::Char('x')));
    assert!(stopped.waiting.is_none());
    // while the review is still running nothing is raised
    let mut running = booted();
    let r = enter(&mut running, "/review payments for acme-audit");
    running.handle(ok(&r[0], 202, json!({"id": "job-4"})));
    assert!(running.attention.is_none() && running.waiting.is_none());
}

fn tab_and_ask(app: &mut App) -> Option<Req> {
    app.handle(key(KeyCode::Tab));
    app.drain_background()
        .into_iter()
        .find(|r| r.path.starts_with("/api/fs"))
}

fn dirs(names: &[&str]) -> Value {
    json!({"path": "/home/u/projects", "parent": null, "dirs": names.iter().map(|n| json!({"name": n, "path": format!("/home/u/projects/{n}")})).collect::<Vec<_>>()})
}

#[test]
fn tu52_tab_after_review_finishes_the_folder_from_the_services_listing_of_allowed_folders() {
    let mut app = booted();
    typed(&mut app, "/review pay");
    let r = tab_and_ask(&mut app).expect("asks the service");
    assert_eq!(
        r.path, "/api/fs?path=/home/u/projects/",
        "the service's root, not this terminal's folder"
    );
    app.handle(ok(
        &r,
        200,
        dirs(&["payments", "web", "Pay-docs", "toppay"]),
    ));
    assert_eq!(
        app.input, "/review pay",
        "two matches that share no more than what is typed change nothing"
    );
    assert!(
        text(&app).contains("Pay-docs/")
            && text(&app).contains("payments/")
            && !text(&app).contains("web/")
            && !text(&app).contains("toppay")
    );
    // one match completes and ends with a slash so the next level can be asked for
    let mut app = booted();
    typed(&mut app, "/review we");
    let r = tab_and_ask(&mut app).unwrap();
    app.handle(ok(&r, 200, dirs(&["payments", "web"])));
    assert_eq!(app.input, "/review web/");
    // a shared start is filled in
    let mut app = booted();
    typed(&mut app, "/review p");
    let r = tab_and_ask(&mut app).unwrap();
    app.handle(ok(&r, 200, dirs(&["payments-api", "payments-web"])));
    assert_eq!(app.input, "/review payments-");
    // deeper levels ask for that folder
    let mut app = booted();
    typed(&mut app, "/review ./payments-api/s");
    let r = tab_and_ask(&mut app).unwrap();
    assert_eq!(r.path, "/api/fs?path=/home/u/projects/payments-api/");
    app.handle(ok(&r, 200, dirs(&["src", "scripts"])));
    assert_eq!(app.input, "/review ./payments-api/s");
    let mut app = booted();
    typed(&mut app, "/review /home/u/projects/payments-api/sr");
    let r = tab_and_ask(&mut app).unwrap();
    assert_eq!(r.path, "/api/fs?path=/home/u/projects/payments-api/");
    app.handle(ok(&r, 200, dirs(&["src"])));
    assert_eq!(app.input, "/review /home/u/projects/payments-api/src/");
}

#[test]
fn tu53_path_completion_is_asked_only_where_it_makes_sense_and_hostile_answers_change_nothing() {
    // not after /review's folder is finished, not for other commands, not for odd spacing
    for line in [
        "/review a for acme",
        "/ask a : what",
        "/code a : fix",
        "/memory pa",
        "/open pa",
        "pa",
        "/review  pa",
        "/ask  pa",
    ] {
        let mut app = booted();
        typed(&mut app, line);
        assert!(tab_and_ask(&mut app).is_none(), "{line:?}");
    }
    // names with spaces or control characters are never offered
    let mut app = booted();
    typed(&mut app, "/review a");
    let r = tab_and_ask(&mut app).unwrap();
    app.handle(ok(&r, 200, dirs(&["a b", "a\u{1b}[2J", "a\ttab", "a-ok"])));
    assert_eq!(app.input, "/review a-ok/");
    // an answer that arrives after the line changed is dropped
    let mut app = booted();
    typed(&mut app, "/review a");
    let r = tab_and_ask(&mut app).unwrap();
    typed(&mut app, "x");
    app.handle(ok(&r, 200, dirs(&["abc"])));
    assert_eq!(app.input, "/review ax");
    // a refusal or a failure changes nothing and says nothing
    let mut app = booted();
    typed(&mut app, "/review ../../etc/");
    let r = tab_and_ask(&mut app).unwrap();
    let before = text(&app);
    app.handle(ok(
        &r,
        403,
        json!({"error": {"code": "NOT_ALLOWED", "message": "that folder is not available"}}),
    ));
    assert_eq!(app.input, "/review ../../etc/");
    assert_eq!(text(&app), before);
    // characters that could add a query parameter are encoded
    let mut app = booted();
    typed(&mut app, "/review x&path=/etc/");
    let r = tab_and_ask(&mut app).unwrap();
    assert!(
        !r.path.contains('&') && r.path.contains("%26"),
        "{}",
        r.path
    );
    // no roots known yet: nothing to ask
    let mut fresh = App::new();
    typed(&mut fresh, "/review pa");
    assert!(tab_and_ask(&mut fresh).is_none());
}

#[test]
fn tu54_an_aside_is_asked_without_a_session_shown_apart_and_kept_out_of_the_conversation() {
    let mut app = booted();
    app.session = Some("chat-1".into());
    app.chat_history
        .push(("user".into(), "earlier question".into()));
    app.chat_history
        .push(("assistant".into(), "earlier answer".into()));
    let r = enter(&mut app, "/aside what is the capital of Peru");
    assert_eq!(r.len(), 1);
    assert_eq!(
        r[0].path, "/api/chat",
        "the plain chat, not the stream that appends to a kept conversation"
    );
    let body = r[0].body.as_ref().unwrap();
    assert!(
        body.get("session").is_none(),
        "no session id: nothing can be appended"
    );
    assert_eq!(body["message"], "what is the capital of Peru");
    assert_eq!(
        body["history"].as_array().unwrap().len(),
        2,
        "it sees the conversation so far"
    );
    assert!(!app.chat_waiting, "the conversation is not marked busy");
    app.handle(ok(&r[0], 200, json!({"reply": "Lima.", "saved": false})));
    let all = text(&app);
    assert!(
        all.contains("Aside") && all.contains("Lima.") && all.contains("not saved"),
        "{all}"
    );
    assert_eq!(
        app.chat_history.len(),
        2,
        "neither the question nor the answer joined the history"
    );
    let conv = app.transcript_text();
    assert!(
        !conv.contains("Lima.") && !conv.contains("Peru"),
        "not in what is exported, copied or searched: {conv}"
    );
    // later turns do not carry it
    let r = enter(&mut app, "next real question");
    let sent = r[0].body.as_ref().unwrap()["history"].to_string();
    assert!(!sent.contains("Lima") && !sent.contains("Peru"), "{sent}");
    // refused while an answer is arriving, empty, or too long
    let mut busy = booted();
    enter(&mut busy, "a real question");
    assert!(enter(&mut busy, "/aside hi").is_empty());
    assert!(text(&busy).contains("still arriving"));
    let mut a = booted();
    assert!(enter(&mut a, "/aside").is_empty());
    assert!(text(&a).contains("usage: /aside QUESTION"));
    assert!(enter(&mut a, &format!("/aside {}", "x".repeat(4001))).is_empty());
    assert!(text(&a).contains("4000"));
    // a failure is an error, not an answer; hostile characters in the reply are cleaned
    let r = enter(&mut a, "/aside hello");
    a.handle(ok(&r[0], 200, json!({"reply": "fine\u{1b}[2Jgone"})));
    assert!(!text(&a).contains('\u{1b}'));
    let r = enter(&mut a, "/aside again");
    a.handle(ok(
        &r[0],
        502,
        json!({"error": {"code": "NO_MODEL", "message": "no model is running"}}),
    ));
    assert!(text(&a).contains("no model"), "{}", text(&a));
}

fn many_cards(n: usize) -> Vec<kk_tui::app::Card> {
    (0..n)
        .map(|i| kk_tui::app::Card {
            kind: if i % 3 == 0 { Kind::User } else { Kind::Info },
            title: (i % 2 == 0).then(|| format!("Card {i}")),
            lines: (0..(i % 5) + 1)
                .map(|j| format!("card {i} line {j} {}", "word ".repeat((i + j) % 40)))
                .collect(),
        })
        .collect()
}

#[test]
fn tu55_the_window_of_a_long_conversation_is_the_same_as_cutting_it_from_the_whole_and_costs_only_what_is_shown(
) {
    use kk_tui::ui::{window_lines, wrap_calls};
    let th = Theme {
        color: false,
        palette: kk_tui::ui::Palette::Auto,
    };
    let cards = many_cards(300);
    for width in [20usize, 47, 100] {
        let full = window_lines(&cards, width, &th, 1_000_000, 0).0;
        assert!(full.len() > 600);
        for h in [1usize, 5, 30] {
            for back in [
                0usize,
                1,
                7,
                100,
                full.len() - h,
                full.len(),
                full.len() + 500,
            ] {
                let (got, used) = window_lines(&cards, width, &th, h, back);
                let want_back = back.min(full.len().saturating_sub(h));
                let start = full.len().saturating_sub(h + want_back);
                let want: Vec<_> = full[start..].iter().take(h).cloned().collect();
                assert_eq!(
                    (got, used),
                    (want, want_back),
                    "width {width} h {h} back {back}"
                );
            }
        }
    }
    // five thousand cards: a frame wraps a handful of them, not all
    let big = many_cards(5000);
    let before = wrap_calls();
    let (lines, back) = window_lines(&big, 80, &th, 30, 0);
    let used = wrap_calls() - before;
    assert_eq!((lines.len(), back), (30, 0));
    assert!(used < 80, "wrapped {used} times for a 30-line window");
    // scrolled back by 200 lines it still does not touch the whole conversation
    let before = wrap_calls();
    window_lines(&big, 80, &th, 30, 200);
    assert!(wrap_calls() - before < 400);
    // empty, and a window taller than the conversation
    assert_eq!(window_lines(&[], 80, &th, 10, 5), (Vec::new(), 0));
    let few = many_cards(2);
    let (l, b) = window_lines(&few, 80, &th, 100, 0);
    assert!(l.len() < 100 && b == 0);
}

mod linear_mode {
    use super::*;
    use kk_tui::linear::{card_text, feed_line, mode_text, news, Printed};
    use kk_tui::picker::{Item, ItemKind, Picker};

    fn echoing() -> Printed {
        Printed::new(true)
    }

    #[test]
    fn tu56_a_card_is_plain_lines_that_start_with_what_kind_of_message_it_is() {
        let c = |kind, title: Option<&str>, lines: &[&str]| kk_tui::app::Card {
            kind,
            title: title.map(String::from),
            lines: lines.iter().map(|l| l.to_string()).collect(),
        };
        assert_eq!(
            card_text(&c(Kind::Warn, None, &["one line"])),
            "[Warning] one line"
        );
        assert_eq!(
            card_text(&c(Kind::Error, Some("Title"), &[])),
            "[Error] Title"
        );
        assert_eq!(
            card_text(&c(Kind::Ok, Some("T"), &["a", "b", "", " "])),
            "[Done] T\n  a\n  b",
            "blank trailing lines are not read out"
        );
        assert_eq!(
            card_text(&c(Kind::Info, None, &["a", "b"])),
            "[Note]\n  a\n  b"
        );
        assert_eq!(card_text(&c(Kind::User, None, &["/help"])), "[You] /help");
    }

    #[test]
    fn tu57_everything_is_written_once_in_order_and_the_owners_own_lines_only_where_nothing_echoes_them(
    ) {
        let mut app = booted();
        let mut quiet = Printed::new(false);
        let mut loud = echoing();
        let first_q = news(&mut app, &mut quiet);
        let first_l = news(&mut app, &mut loud);
        assert!(first_q.contains("[Status]") && first_q == first_l);
        assert_eq!(
            news(&mut app, &mut quiet),
            "",
            "nothing new, nothing written"
        );
        feed_line(&mut app, "/help");
        let q = news(&mut app, &mut quiet);
        let l = news(&mut app, &mut loud);
        assert!(
            !q.contains("[You]") && l.starts_with("[You] /help\n"),
            "{q} | {l}"
        );
        assert!(q.contains("/review FOLDER for RECIPIENT"));
        assert_eq!(news(&mut app, &mut quiet), "");
        assert!(
            !q.chars().any(|c| c.is_control() && c != '\n'),
            "no control character"
        );
    }

    #[test]
    fn tu58_an_answer_in_progress_is_announced_once_and_written_whole_when_it_is_finished() {
        let mut app = booted();
        let mut seen = Printed::new(false);
        news(&mut app, &mut seen);
        let r = feed_line(&mut app, "a question");
        assert!(r[0].stream);
        let id = r[0].id;
        let waiting = news(&mut app, &mut seen);
        assert_eq!(waiting.matches("The assistant is answering").count(), 1);
        for piece in ["Par", "tial ", "answer"] {
            app.handle(Event::Delta {
                id,
                text: piece.into(),
            });
            let n = news(&mut app, &mut seen);
            assert!(
                !n.contains("Par"),
                "a half-written answer is not read out: {n}"
            );
            assert!(
                !n.contains("The assistant is answering"),
                "announced once: {n}"
            );
        }
        app.handle(ok(
            &r[0],
            200,
            json!({"reply": "Partial answer", "notes_used": 0, "route": {"simulated": true}}),
        ));
        let done = news(&mut app, &mut seen);
        assert_eq!(done.matches("Partial answer").count(), 1, "{done}");
        assert_eq!(news(&mut app, &mut seen), "");
    }

    #[test]
    fn tu59_a_yes_or_no_question_takes_yes_and_treats_anything_else_as_no() {
        let mut app = booted();
        let mut seen = Printed::new(false);
        feed_line(&mut app, "/forget note-1");
        let q = news(&mut app, &mut seen);
        assert!(
            q.contains("[Question]") && q.contains("Type y and press Enter to say yes"),
            "{q}"
        );
        let r = feed_line(&mut app, "maybe");
        assert!(
            r.is_empty() && matches!(app.mode, Mode::Normal),
            "anything else is no, and the question closes"
        );
        assert!(news(&mut app, &mut seen).contains("Cancelled"));
        feed_line(&mut app, "/forget note-1");
        let r = feed_line(&mut app, " YES ");
        assert!(
            !r.is_empty() && r[0].path.contains("note-1"),
            "yes sends the request: {r:?}"
        );
    }

    #[test]
    fn tu60_the_approval_is_a_sequential_prompt_that_only_the_right_prefix_or_the_word_refuse_answers(
    ) {
        let mut app = booted();
        app.jobs.push(kk_tui::app::JobRow {
            id: "job-4".into(),
            recipient: "x".into(),
            state: "ready".into(),
        });
        open_approval(&mut app);
        let mut seen = Printed::new(false);
        let p = news(&mut app, &mut seen);
        assert!(
            p.contains("[Approval needed] release job-4")
                && p.contains(
                    "type the first 12 characters of the digest, abcdef012345, and press Enter"
                )
                && p.contains("type refuse"),
            "{p}"
        );
        // a wrong answer writes only the error, not the whole release again
        feed_line(&mut app, "ffffffffffff");
        let e = news(&mut app, &mut seen);
        assert!(
            e.starts_with("[Error] That is not the first 12 characters")
                && !e.contains("Files that would be sent"),
            "{e}"
        );
        feed_line(&mut app, "ffffffffffff");
        assert_eq!(
            news(&mut app, &mut seen),
            "",
            "the same error is not read out twice in a row"
        );
        // letters that are not hexadecimal are dropped, never typed into the approval
        feed_line(&mut app, "zzz abcdef012345 qqq");
        let Mode::Approval(v) = &app.mode else {
            panic!()
        };
        assert_eq!(v.typed, "abcdef012345");
        // the word refuse refuses
        let mut other = booted();
        other.jobs.push(kk_tui::app::JobRow {
            id: "job-4".into(),
            recipient: "x".into(),
            state: "ready".into(),
        });
        open_approval(&mut other);
        let mut seen2 = Printed::new(false);
        news(&mut other, &mut seen2);
        feed_line(&mut other, "ffffffffffff");
        assert!(news(&mut other, &mut seen2).contains("[Error]"));
        let r = feed_line(&mut other, "REFUSE");
        news(&mut other, &mut seen2);
        open_approval(&mut other);
        news(&mut other, &mut seen2);
        feed_line(&mut other, "ffffffffffff");
        assert!(
            news(&mut other, &mut seen2).contains("[Error]"),
            "the same error in a new prompt is read out again"
        );
        let r2 = feed_line(&mut other, "REFUSE");
        assert!(!r2.is_empty());

        assert!(matches!(other.mode, Mode::Normal));
        assert!(
            r.iter()
                .any(|q| q.path.contains("deny") || q.path.contains("refus")),
            "{r:?}"
        );
    }

    #[test]
    fn tu61_a_list_is_numbered_and_chosen_by_number_narrowed_by_words_or_closed() {
        let mut app = booted();
        let mut p = Picker::new("Conversations and reviews");
        p.loading = false;
        for (i, name) in ["alpha chat", "beta chat", "gamma review"]
            .iter()
            .enumerate()
        {
            p.items.push(Item {
                id: format!("id-{i}"),
                label: name.to_string(),
                detail: String::new(),
                when: 100 - i as i64,
                kind: ItemKind::Chat,
                project: None,
                archived: false,
            });
        }
        app.mode = Mode::Picker(p);
        let text = mode_text(&app).unwrap();
        assert!(
            text.contains("1. alpha chat")
                && text.contains("3. gamma review")
                && text.contains("Type a number"),
            "{text}"
        );
        feed_line(&mut app, "review");
        let text = mode_text(&app).unwrap();
        assert!(
            text.contains("1. gamma review") && !text.contains("alpha"),
            "{text}"
        );
        let mut seen = Printed::new(false);
        news(&mut app, &mut seen);
        for bad in ["0", "4", "9"] {
            feed_line(&mut app, bad);
            assert!(
                news(&mut app, &mut seen).contains("[Error] There is no number"),
                "{bad}"
            );
            assert!(matches!(app.mode, Mode::Picker(_)));
            feed_line(&mut app, "review");
            news(&mut app, &mut seen);
        }
        feed_line(&mut app, "9");
        assert!(news(&mut app, &mut seen).contains("[Error] There is no number 9"));
        assert!(
            matches!(app.mode, Mode::Picker(_)),
            "a bad number leaves the list open"
        );
        let r = feed_line(&mut app, "1");
        assert!(
            matches!(app.mode, Mode::Normal) && r.iter().any(|q| q.path.contains("id-2")),
            "{r:?}"
        );
        let mut again = booted();
        again.mode = Mode::Picker(Picker::new("x"));
        feed_line(&mut again, "cancel");
        assert!(matches!(again.mode, Mode::Normal));
        // a list still loading says so and offers no number
        let mut loading = booted();
        loading.mode = Mode::Picker(Picker::new("x"));
        assert!(mode_text(&loading).unwrap().contains("Loading"));
    }
}

#[test]
fn tu62_pii_lists_adds_and_removes_through_the_service_and_reports_what_happened() {
    let mut app = booted();
    let r = enter(&mut app, "/pii");
    assert_eq!(
        (r[0].path.as_str(), r[0].body.is_none()),
        ("/api/pii-allow", true)
    );
    app.handle(ok(&r[0], 200, json!({"entries": [], "max_entries": 200})));
    assert!(text(&app).contains("Nothing is on your list"));
    let r = enter(&mut app, "/pii add @acme.com");
    assert_eq!(r[0].body.as_ref().unwrap(), &json!({"add": "@acme.com"}));
    app.handle(ok(&r[0], 200, json!({"changed": true, "entries": 1})));
    assert!(text(&app).contains("The list now has 1"));
    let r = enter(&mut app, "/pii remove @acme.com");
    assert_eq!(r[0].body.as_ref().unwrap(), &json!({"remove": "@acme.com"}));
    app.handle(ok(&r[0], 200, json!({"changed": false, "entries": 0})));
    assert!(text(&app).contains("No change"));
    let r = enter(&mut app, "/pii add *");
    app.handle(ok(&r[0], 422, json!({"error": {"code": "PII_ALLOW_REFUSED", "message": "an entry needs at least four letters"}})));
    assert!(text(&app).contains("at least four letters"));
    let r = enter(&mut app, "/pii");
    app.handle(ok(
        &r[0],
        200,
        json!({"entries": [{"entry": "a@b.com\u{1b}[2J", "added_at": 1}]}),
    ));
    assert!(!text(&app).contains('\u{1b}') && text(&app).contains("1 value"));
    for bad in ["/pii add", "/pii remove", "/pii wipe x", "/pii add"] {
        assert!(enter(&mut app, bad).is_empty(), "{bad}");
        assert!(text(&app).contains("usage: /pii"));
    }
}

#[test]
fn tu63_findings_show_pattern_matches_as_a_second_opinion_and_say_when_the_rules_could_not_run() {
    let mut app = booted();
    let finding = json!({"title": "Token not revoked", "file": "a.py", "line": 3, "explanation": "x", "reproducer": "", "status": "reproduced",
        "pattern_matches": [{"rule": "kk-py-eval-exec", "line": 9, "what": "eval or exec of a value\u{1b}[2J that is not a literal"}]});
    app.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    let r = enter(&mut app, "/findings");
    app.handle(ok(&r[0], 200, json!({"findings": [finding], "unparsed": 0, "pattern_scan": {"status": "ran", "files": 12, "matches": 2}})));
    let all = text(&app);
    assert!(
        all.contains("Pattern rules also matched in this file")
            && all.contains("not a reproduction"),
        "{all}"
    );
    assert!(all.contains("line 9: eval or exec"), "{all}");
    assert!(!all.contains('\u{1b}'));
    assert!(all.contains("Pattern rules ran over 12 files and matched 2 places"));
    let mut b = booted();
    b.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    let r = enter(&mut b, "/findings");
    b.handle(ok(&r[0], 200, json!({"findings": [], "unparsed": 0, "pattern_scan": {"status": "failed", "reason": "semgrep did not finish in time"}})));
    assert!(text(&b).contains("Pattern rules could not run: semgrep did not finish in time"));
    let mut c = booted();
    c.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    let r = enter(&mut c, "/findings");
    c.handle(ok(&r[0], 200, json!({"findings": [], "unparsed": 0, "pattern_scan": {"status": "not_run", "reason": "pattern rules were not asked for"}})));
    assert!(
        !text(&c).contains("Pattern rules"),
        "not asked for, so nothing is said"
    );
}

#[test]
fn tu64_how_long_a_release_can_still_be_approved_is_said_in_words_and_never_as_valid_when_unknown()
{
    use kk_tui::app::{expiry_line, release_lines_at, span_text};
    assert_eq!(span_text(0), "0 s");
    assert_eq!(span_text(59), "59 s");
    assert_eq!(span_text(61), "1 min 1 s");
    assert_eq!(span_text(3600), "1 h");
    assert_eq!(span_text(86399), "23 h 59 min");
    assert_eq!(span_text(-4), "0 s");
    assert_eq!(
        expiry_line(None, 1000),
        None,
        "no deadline known, nothing is claimed"
    );
    assert!(expiry_line(Some(1000 + 3 * 3600), 1000)
        .unwrap()
        .contains("another 3 h"));
    let soon = expiry_line(Some(1000 + 600), 1000).unwrap();
    assert!(soon.starts_with("Only 10 min left"), "{soon}");
    assert!(expiry_line(Some(1000 + 601), 1000)
        .unwrap()
        .contains("another 10 min 1 s"));
    for past in [1000, 999, 0, -5] {
        assert!(
            expiry_line(Some(past), 1000)
                .unwrap()
                .contains("has expired"),
            "{past}"
        );
    }
    let mut body = release_body();
    body["expires"] = json!(5000);
    let lines = release_lines_at(&body, 1000).join("\n");
    assert!(
        lines.contains("can be approved for another 1 h 6 min"),
        "{lines}"
    );
    body["expires"] = json!(900);
    assert!(release_lines_at(&body, 1000)
        .join("\n")
        .contains("has expired"));
    body.as_object_mut().unwrap().remove("expires");
    let none = release_lines_at(&body, 1000).join("\n");
    assert!(
        !none.contains("approved for another") && !none.contains("expired"),
        "{none}"
    );
}

#[test]
fn tu65_a_release_past_its_deadline_cannot_be_approved_from_the_terminal_and_nothing_is_sent() {
    let mut app = booted();
    app.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    open_approval(&mut app);
    let Mode::Approval(v) = &mut app.mode else {
        panic!()
    };
    v.statement = json!({"expires": 1});
    typed(&mut app, "abcdef012345");
    let r = app.handle(key(KeyCode::Enter));
    assert!(
        r.is_empty(),
        "no approval request goes out after the deadline: {r:?}"
    );
    let Mode::Approval(v) = &app.mode else {
        panic!("the view stays so it can be refused")
    };
    assert!(
        v.error.as_deref().unwrap().contains("has expired"),
        "{:?}",
        v.error
    );
    // before the deadline the same keys approve
    let mut ok_app = booted();
    ok_app.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    open_approval(&mut ok_app);
    let Mode::Approval(v) = &mut ok_app.mode else {
        panic!()
    };
    v.statement = json!({"expires": kk_tui::app::now_secs() + 3600});
    typed(&mut ok_app, "abcdef012345");
    let r = ok_app.handle(key(KeyCode::Enter));
    assert!(r.iter().any(|q| q.path.contains("/approve")), "{r:?}");
}

#[test]
fn tu66_an_answer_is_shown_as_formatted_text_while_the_export_and_plain_mode_keep_the_words_as_written(
) {
    let mut app = booted();
    app.cards.push(Card {
        kind: Kind::Info,
        title: Some("Assistant".into()),
        lines: [
            "Here is **bold** and a link [docs](https://example.com/a):",
            "- one",
            "- two",
            "",
            "```py",
            "x = 1",
            "```",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
    });
    let (rows, t) = render(&app, 70, 24, true);
    let screen = rows.join("\n");
    assert!(
        !screen.contains("**") && !screen.contains("```"),
        "no markup characters on the screen:\n{screen}"
    );
    assert!(
        screen.contains("Here is bold and a link docs (https://example.com/a):"),
        "{screen}"
    );
    assert!(
        screen.contains("• one") && screen.contains("• two"),
        "{screen}"
    );
    assert!(
        screen.contains("  py") && screen.contains("  x = 1"),
        "{screen}"
    );
    // the word that was between the stars is bold on the screen
    let buf = t.backend().buffer();
    let row = rows
        .iter()
        .position(|r| r.contains("Here is bold"))
        .unwrap() as u16;
    let col = rows[row as usize].find("bold").unwrap() as u16;
    assert!(buf[(col, row)]
        .modifier
        .contains(ratatui::style::Modifier::BOLD));
    assert!(
        !buf[(col - 3, row)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD),
        "the words before it are not"
    );
    // what is copied, exported and read in plain-line mode is the text as the model wrote it
    let exported = app.transcript_text();
    assert!(
        exported.contains("**bold**") && exported.contains("```py"),
        "{exported}"
    );
    let card = app.cards.last().unwrap();
    assert!(kk_tui::linear::card_text(card).contains("**bold**"));
    // other cards are not reformatted
    let mut plain = booted();
    plain.cards.push(Card {
        kind: Kind::Info,
        title: Some("Note".into()),
        lines: vec!["keep **these** stars".into()],
    });
    assert!(render(&plain, 70, 24, true)
        .0
        .join("\n")
        .contains("keep **these** stars"));
    // no colour: the formatting survives as weight and reverse video, never colour alone
    let (rows, t) = render(&app, 70, 24, false);
    let r = rows
        .iter()
        .position(|r| r.contains("Here is bold"))
        .unwrap() as u16;
    let c = rows[r as usize].find("bold").unwrap() as u16;
    assert!(t.backend().buffer()[(c, r)]
        .modifier
        .contains(ratatui::style::Modifier::BOLD));
}

#[test]
fn tu67_the_change_in_a_proposed_fix_is_coloured_and_still_distinguishable_without_colour() {
    use ratatui::style::{Color, Modifier};
    let mut app = booted();
    app.cards.push(Card {
        kind: Kind::Ok,
        title: Some("Fix 1".into()),
        lines: [
            "a.py: Make logout remove the token",
            "",
            "--- a/a.py",
            "+++ b/a.py",
            "@@ -1,2 +1,2 @@",
            " keep",
            "-old line",
            "+new line",
            "",
            "The reproducer stops reproducing.",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
    });
    let find = |rows: &[String], needle: &str| -> (u16, u16) {
        let y = rows
            .iter()
            .position(|r| r.contains(needle))
            .unwrap_or_else(|| panic!("{needle} not on screen:\n{}", rows.join("\n")));
        (rows[y].find(needle).unwrap() as u16, y as u16)
    };
    let (rows, t) = render(&app, 70, 24, true);
    let b = t.backend().buffer();
    let (x, y) = find(&rows, "+new line");
    assert_eq!(b[(x, y)].fg, Color::Green);
    let (x, y) = find(&rows, "-old line");
    assert_eq!(b[(x, y)].fg, Color::Red);
    let (x, y) = find(&rows, "@@ -1,2");
    assert_eq!(b[(x, y)].fg, Color::Cyan);
    let (x, y) = find(&rows, " keep");
    assert_eq!(b[(x + 1, y)].fg, Color::Reset, "context lines are plain");
    let (x, y) = find(&rows, "+++ b/a.py");
    assert_eq!(
        b[(x, y)].fg,
        Color::Reset,
        "a file header is not an added line"
    );
    let (x, y) = find(&rows, "--- a/a.py");
    assert_eq!(b[(x, y)].fg, Color::Reset, "nor a removed one");
    // no colour: the markers are still there and the weight differs
    let (rows, t) = render(&app, 70, 24, false);
    let b = t.backend().buffer();
    let (ax, ay) = find(&rows, "+new line");
    let (rx, ry) = find(&rows, "-old line");
    assert!(b[(ax, ay)].modifier.contains(Modifier::BOLD) && b[(ax, ay)].fg == Color::Reset);
    assert!(b[(rx, ry)].modifier.contains(Modifier::DIM));
    // other cards that start a line with + or - are not coloured
    let mut plain = booted();
    plain.cards.push(Card {
        kind: Kind::Info,
        title: Some("Note".into()),
        lines: vec!["+not a diff".into()],
    });
    let (rows, t) = render(&plain, 70, 24, true);
    let (x, y) = find(&rows, "+not a diff");
    assert_eq!(t.backend().buffer()[(x, y)].fg, Color::Reset);
}

#[test]
fn tu68_a_long_result_is_folded_to_its_first_lines_and_ctrl_o_shows_all_and_folds_again() {
    let mut app = booted();
    let long: Vec<String> = (1..=40).map(|i| format!("finding line {i}")).collect();
    app.cards.push(Card {
        kind: Kind::Info,
        title: Some("Findings".into()),
        lines: long.clone(),
    });
    app.cards.push(Card {
        kind: Kind::Info,
        title: Some("Assistant".into()),
        lines: long.clone(),
    });
    let short: Vec<String> = (1..=24).map(|i| format!("short line {i}")).collect();
    app.cards.push(Card {
        kind: Kind::Info,
        title: Some("Release".into()),
        lines: short,
    });
    let all = |app: &App| render(app, 80, 200, true).0.join("\n");
    let screen = all(&app);
    assert!(
        screen.contains("finding line 10")
            && !screen.contains("finding line 11\n")
            && screen.contains("… 30 more lines. Ctrl-O shows everything"),
        "{screen}"
    );
    assert!(
        screen.matches("finding line 40").count() == 1,
        "the assistant's answer is never folded, the findings card is"
    );
    assert!(
        screen.contains("short line 24") && !screen.contains("more lines\n  short"),
        "24 lines is not over the limit"
    );
    app.handle(ctrl('o'));
    assert!(app.expand_all);
    let screen = all(&app);
    assert!(
        screen.matches("finding line 40").count() == 2 && !screen.contains("more lines"),
        "everything is shown"
    );
    app.handle(ctrl('o'));
    assert!(!app.expand_all);
    assert!(all(&app).contains("… 30 more lines"));
    // the key is listed in /help, and it resets the scroll to the newest line so the screen does not jump into the middle of a card
    let r = enter(&mut app, "/help");
    assert!(r.is_empty() && text(&app).contains("Ctrl-O"));
    app.scroll_back = 50;
    app.handle(ctrl('o'));
    assert_eq!(app.scroll_back, 0);
}

#[test]
fn tu69_tab_finishes_a_review_id_after_the_commands_that_take_one_and_lists_the_choices() {
    let jobs = |app: &mut App, ids: &[(&str, &str, &str)]| {
        for (id, state, who) in ids {
            app.jobs.push(kk_tui::app::JobRow {
                id: id.to_string(),
                recipient: who.to_string(),
                state: state.to_string(),
            });
        }
    };
    let tab = |app: &mut App, line: &str| {
        app.input = line.to_string();
        app.cursor = line.chars().count();
        app.handle(key(KeyCode::Tab));
        app.input.clone()
    };
    // one review: any start of its id, or nothing yet, completes it
    let mut a = booted();
    jobs(&mut a, &[("job-12", "ready", "acme-audit")]);
    assert_eq!(tab(&mut a, "/open jo"), "/open job-12 ");
    assert_eq!(tab(&mut a, "/findings "), "/findings job-12 ");
    assert_eq!(
        tab(&mut a, "/approve JOB-1"),
        "/approve job-12 ",
        "case does not matter"
    );
    assert_eq!(
        tab(&mut a, "/applyfix 2 j"),
        "/applyfix 2 job-12 ",
        "the number comes first for applyfix"
    );
    assert_eq!(
        tab(&mut a, "/applyfix j"),
        "/applyfix j",
        "a number is not a review id"
    );
    assert_eq!(
        tab(&mut a, "/open zz"),
        "/open zz",
        "no match changes nothing"
    );
    assert_eq!(
        tab(&mut a, "/open 12"),
        "/open 12",
        "the start of an id, not any part of it"
    );
    assert_eq!(
        tab(&mut a, "/ask jo"),
        "/ask jo",
        "other commands are left alone"
    );
    // several: what they share is filled in and they are listed with who each is for
    let mut b = booted();
    jobs(
        &mut b,
        &[
            ("job-12", "ready", "acme-audit"),
            ("job-13", "running", "beta"),
            ("job-2", "failed", "x"),
        ],
    );
    assert_eq!(tab(&mut b, "/open jo"), "/open job-");
    assert!(
        text(&b).contains("job-12  ready  for acme-audit")
            && text(&b).contains("job-13  running  for beta")
            && text(&b).contains("job-2  failed  for x"),
        "{}",
        text(&b)
    );
    assert_eq!(tab(&mut b, "/open job-1"), "/open job-1");
    assert_eq!(tab(&mut b, "/open job-13"), "/open job-13 ");
    // the review id is never sent anywhere by completing
    assert!(b
        .drain_background()
        .iter()
        .all(|r| !r.path.starts_with("/api/jobs")));
    // many reviews are listed in a bounded way
    let mut c = booted();
    let many: Vec<(String, &str, &str)> = (100..140)
        .map(|i| (format!("job-{i}"), "ready", "r"))
        .collect();
    for (id, s, w) in &many {
        c.jobs.push(kk_tui::app::JobRow {
            id: id.clone(),
            recipient: w.to_string(),
            state: s.to_string(),
        });
    }
    tab(&mut c, "/open job-1");
    assert!(
        text(&c).matches("job-1").count() <= 14,
        "a long list is cut"
    );
}

#[test]
fn tu70_quiet_hides_the_steps_of_a_review_but_not_how_it_ended_and_toggles_back() {
    let mut app = booted();
    let r = enter(&mut app, "/quiet");
    assert!(r.is_empty() && app.quiet && text(&app).contains("only how it ended"));
    let r = enter(&mut app, "/review payments for acme-audit");
    app.handle(ok(&r[0], 202, json!({"id": "job-4"})));
    let t = app.handle(Event::Tick);
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
    assert!(
        !all.contains("job-4  import") && !all.contains("job-4  review"),
        "steps are hidden: {all}"
    );
    assert!(
        all.contains("job-4 finished: ready") && all.contains("/approve"),
        "the end is still told"
    );
    // an error is never hidden
    let r = enter(&mut app, "/review payments for acme-audit");
    app.handle(ok(&r[0], 202, json!({"id": "job-5"})));
    let t = app.handle(Event::Tick);
    app.handle(ok(&t[0], 200, json!({"state": "failed", "recipient": "x", "note": "the model stopped", "events": [{"stage": "queued", "detail": ""}]})));
    assert!(text(&app).contains("It stopped: the model stopped"));
    // toggling back shows the steps again
    enter(&mut app, "/quiet");
    assert!(!app.quiet && text(&app).contains("Every step of a review is shown"));
}

#[test]
fn tu71_a_proposed_fix_is_drawn_with_old_and_new_line_numbers_and_its_markers_first() {
    let mut a = booted();
    a.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: "x".into(),
        state: "ready".into(),
    });
    let r = enter(&mut a, "/fixes");
    a.handle(ok(
        &r[0],
        200,
        json!({"fixes": [{"index": 0, "title": "t", "file": "payments/auth/session.py", "explanation": "remove the entry", "status": "fixes_the_reproducer", "verified": true,
            "diff": "--- a/payments/auth/session.py\n+++ b/payments/auth/session.py\n@@ -21,3 +21,2 @@\n keep\n-del _sessions[token]\n+_sessions.pop(token, None)\n", "applied": false}]}),
    ));
    let all = text(&a);
    assert!(all.contains("  21  21 keep"), "{all}");
    assert!(all.contains("- 22     del _sessions[token]"), "{all}");
    assert!(all.contains("+     22 _sessions.pop(token, None)"), "{all}");
    assert!(
        all.contains("@@ -21,3 +21,2 @@") && all.contains("--- a/payments/auth/session.py"),
        "headers are kept as they are"
    );
}

#[test]
fn tu72_tab_finishes_a_note_id_after_correct_and_forget_from_the_last_memory_listing() {
    let mut a = booted();
    let tab = |app: &mut App, line: &str| {
        app.input = line.to_string();
        app.cursor = line.chars().count();
        app.handle(key(KeyCode::Tab));
        app.input.clone()
    };
    assert_eq!(
        tab(&mut a, "/forget mem"),
        "/forget mem",
        "nothing is known before /memory has been run"
    );
    a.input.clear();
    a.cursor = 0;
    let r = enter(&mut a, "/memory");
    a.handle(ok(&r[0], 200, json!({"memories": [
        {"id": "mem-aa11", "level": "internal", "text": "Acme pays invoices on the 20th", "why": "you said so"},
        {"id": "mem-aa22", "level": "internal", "text": "Beta ships on mondays", "why": "you said so"},
        {"id": "mem-bb33", "level": "internal", "text": "Gamma prefers email", "why": "you said so"}]})));
    assert_eq!(tab(&mut a, "/forget mem-b"), "/forget mem-bb33 ");
    assert_eq!(
        tab(&mut a, "/correct MEM-BB"),
        "/correct mem-bb33 ",
        "case does not matter"
    );
    assert_eq!(
        tab(&mut a, "/forget mem-a"),
        "/forget mem-aa",
        "what the candidates share is filled in"
    );
    assert!(
        text(&a).contains("mem-aa11  Acme pays invoices on the 20th")
            && text(&a).contains("mem-aa22  Beta ships on mondays"),
        "{}",
        text(&a)
    );
    assert_eq!(
        tab(&mut a, "/forget aa11"),
        "/forget aa11",
        "the start of an id, not any part of it"
    );
    assert_eq!(
        tab(&mut a, "/forget zzz"),
        "/forget zzz",
        "no match changes nothing"
    );
    // after the id the words are the new text and are never completed
    assert_eq!(
        tab(&mut a, "/correct mem-bb33 Gamma prefers m"),
        "/correct mem-bb33 Gamma prefers m"
    );
    assert_eq!(
        tab(&mut a, "/remember mem-b"),
        "/remember mem-b",
        "other commands are left alone"
    );
    // a hostile id in the listing cannot put control characters into the line
    a.input.clear();
    a.cursor = 0;
    let r = enter(&mut a, "/memory");
    a.handle(ok(&r[0], 200, json!({"memories": [{"id": "mem-\u{1b}[2Jx1", "level": "internal", "text": "t\u{7}ext", "why": "w"}]})));
    let done = tab(&mut a, "/forget mem-");
    assert!(!done.chars().any(|c| c.is_control()), "{done:?}");
}

#[test]
fn tu73_tab_finishes_a_project_name_after_project_and_a_recipient_after_for() {
    let mut a = booted();
    let tab = |app: &mut App, line: &str| {
        app.input = line.to_string();
        app.cursor = line.chars().count();
        app.handle(key(KeyCode::Tab));
        app.input.clone()
    };
    assert_eq!(
        tab(&mut a, "/project no"),
        "/project none",
        "none is always a choice"
    );
    a.input.clear();
    a.cursor = 0;
    let r = enter(&mut a, "/memory");
    a.handle(ok(&r[0], 200, json!({"memories": [
        {"id": "m1", "level": "internal", "text": "a", "why": "w", "project": "payments-service"},
        {"id": "m2", "level": "internal", "text": "b", "why": "w", "project": "payments-ledger"},
        {"id": "m3", "level": "internal", "text": "c", "why": "w", "project": "web"},
        {"id": "m4", "level": "internal", "text": "d", "why": "w", "project": "bad\u{1b}[2Jname"}]})));
    assert_eq!(tab(&mut a, "/project w"), "/project web");
    assert_eq!(tab(&mut a, "/project pay"), "/project payments-");
    assert!(text(&a).contains("payments-service") && text(&a).contains("payments-ledger"));
    assert_eq!(tab(&mut a, "/project zzz"), "/project zzz");
    let hostile = tab(&mut a, "/project bad");
    assert!(!hostile.chars().any(|c| c.is_control()), "{hostile:?}");
    assert_eq!(
        tab(&mut a, "/project web extra"),
        "/project web extra",
        "only the first word is a project"
    );
    // recipients come from the reviews already listed
    a.jobs.push(kk_tui::app::JobRow {
        id: "job-1".into(),
        recipient: "acme-audit".into(),
        state: "ready".into(),
    });
    a.jobs.push(kk_tui::app::JobRow {
        id: "job-2".into(),
        recipient: "acme-audit".into(),
        state: "ready".into(),
    });
    a.jobs.push(kk_tui::app::JobRow {
        id: "job-3".into(),
        recipient: "beta-legal".into(),
        state: "ready".into(),
    });
    a.jobs.push(kk_tui::app::JobRow {
        id: "job-4".into(),
        recipient: String::new(),
        state: "running".into(),
    });
    assert_eq!(
        tab(&mut a, "/review payments for ac"),
        "/review payments for acme-audit",
        "a recipient used twice is offered once"
    );
    assert_eq!(
        tab(&mut a, "/review payments ac"),
        "/review payments ac",
        "without the word for, a recipient is never offered"
    );
    assert_eq!(
        tab(&mut a, "/project eb"),
        "/project eb",
        "the start of a name, not any part of it"
    );
    assert_eq!(
        tab(&mut a, "/review payments for b"),
        "/review payments for beta-legal"
    );
    assert_eq!(
        tab(&mut a, "/review payments for "),
        "/review payments for ",
        "two recipients and nothing typed: listed, nothing filled in"
    );
    assert!(text(&a).contains("acme-audit") && text(&a).contains("beta-legal"));
    assert_eq!(
        a.cards.last().unwrap().lines,
        vec!["acme-audit".to_string(), "beta-legal".to_string()],
        "no empty recipient, each once"
    );
    assert_eq!(
        tab(&mut a, "/review payments fo"),
        "/review payments fo",
        "not after the word for: left to folder completion"
    );
}

#[test]
fn tu74_rewind_makes_a_copy_that_stops_before_one_of_your_messages_and_puts_it_back_in_the_box() {
    let mut a = booted();
    assert!(
        enter(&mut a, "/rewind 1").is_empty()
            && text(&a).contains("Only a kept conversation can be rewound")
    );
    a.session = Some("chat-1".into());
    a.chat_history = vec![
        ("user".into(), "first question".into()),
        ("assistant".into(), "first answer".into()),
        ("user".into(), "second question".into()),
        ("assistant".into(), "second answer".into()),
    ];
    for bad in ["/rewind 0", "/rewind x", "/rewind -1", "/rewind 1 2"] {
        assert!(enter(&mut a, bad).is_empty(), "{bad}");
    }
    assert!(
        enter(&mut a, "/rewind 2").is_empty() && text(&a).contains("Message 2 is not one of yours")
    );
    assert!(enter(&mut a, "/rewind 9").is_empty() && text(&a).contains("has 4 messages"));
    assert!(
        a.rewound.is_none(),
        "a refused rewind leaves nothing waiting"
    );
    let r = enter(&mut a, "/rewind 3");
    assert_eq!(
        (r.len(), r[0].path.as_str()),
        (1, "/api/sessions/chat-1/fork")
    );
    assert_eq!(
        r[0].body.as_ref().unwrap(),
        &json!({"at": 2}),
        "the copy keeps the two messages before the third"
    );
    let open = a.handle(ok(&r[0], 201, json!({"id": "chat-2"})));
    assert_eq!(open[0].path, "/api/sessions/chat-2");
    a.handle(ok(&open[0], 200, json!({"turns": [{"role": "user", "content": "first question"}, {"role": "assistant", "content": "first answer"}]})));
    assert_eq!(a.session.as_deref(), Some("chat-2"));
    assert_eq!(
        a.input, "second question",
        "the message is back in the box to change"
    );
    assert_eq!(a.chat_history.len(), 2);
    assert!(text(&a).contains("The original conversation is unchanged"));
    // a failed copy leaves nothing waiting, so a later open does not fill the box
    a.input.clear();
    a.cursor = 0;
    a.chat_history = vec![("user".into(), "again".into())];
    let r = enter(&mut a, "/rewind 1");
    a.handle(ok(
        &r[0],
        500,
        json!({"error": {"code": "STORE", "message": "could not write"}}),
    ));
    assert!(a.rewound.is_none());
    let r = enter(&mut a, "/branch");
    let open = a.handle(ok(&r[0], 201, json!({"id": "chat-3"})));
    a.handle(ok(&open[0], 200, json!({"turns": []})));
    assert_eq!(
        a.input, "",
        "an ordinary branch does not put a message in the box"
    );
}

#[test]
fn tu75_a_long_session_keeps_a_bounded_number_of_cards_and_says_how_many_went() {
    let mut a = booted();
    let before = a.cards.len();
    let n = 2600; // each command adds two cards: the line typed and the answer
    for _ in 0..n {
        a.input.clear();
        a.cursor = 0;
        enter(&mut a, "/quiet");
    }
    assert!(
        a.cards.len() <= kk_tui::app::MAX_CARDS + 1,
        "{}",
        a.cards.len()
    );
    let first = a.cards[0].lines[0].clone();
    assert!(
        first.starts_with("Earlier output was removed from this screen to save memory"),
        "{first}"
    );
    assert!(first.contains("The conversation itself is unchanged"));
    let gone: usize = first
        .split(" (")
        .nth(1)
        .unwrap()
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        a.cards.len() - 1 + gone,
        before + 2 * n,
        "every card is either still here or counted as gone: {first}"
    );
    assert_eq!(
        a.cards
            .iter()
            .filter(|c| c.lines[0].starts_with("Earlier output was removed"))
            .count(),
        1,
        "one marker, not one per drop"
    );
    assert!(
        a.cards.last().unwrap().lines[0].contains("Quiet")
            || a.cards.last().unwrap().lines[0].contains("Every step"),
        "the newest card is kept"
    );
}

#[test]
fn tu76_one_enormous_result_is_cut_to_a_bounded_number_of_lines_with_a_note() {
    let mut a = booted();
    let r = enter(&mut a, "/memory");
    let many: Vec<_> = (0..30_000)
        .map(|i| json!({"id": format!("m{i}"), "level": "internal", "text": "t", "why": "w"}))
        .collect();
    a.handle(ok(&r[0], 200, json!({"memories": many})));
    let card = a.cards.last().unwrap();
    assert!(
        card.lines.len() <= kk_tui::app::MAX_CARD_LINES + 1,
        "{}",
        card.lines.len()
    );
    assert!(
        card.lines
            .last()
            .unwrap()
            .contains("more lines were not kept on the screen"),
        "{:?}",
        card.lines.last()
    );
}

#[test]
fn tu77_a_chosen_conversation_opens_by_id_from_the_command_line_and_from_the_prompt_and_a_bad_id_opens_nothing(
) {
    // `kaku --resume ID`: no list is fetched, that conversation is requested
    let mut a = booted();
    let r = a.resume_at_start(Some("chat-old"), false);
    assert_eq!(r.len(), 1);
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/sessions/chat-old", Method::Get)
    );
    let shown = a.handle(ok(&r[0], 200, json!({"id": "chat-old", "turns": [{"role": "user", "content": "the older question"}, {"role": "assistant", "content": "the older answer"}]})));
    assert!(
        shown.is_empty()
            && text(&a).contains("the older question")
            && text(&a).contains("Opened the kept conversation"),
        "{}",
        text(&a)
    );
    // `/resume ID` in the prompt does the same without the picker
    let mut b = booted();
    let r = enter(&mut b, "/resume chat-7");
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].path, "/api/sessions/chat-7");
    assert!(
        !matches!(b.mode, Mode::Picker(_)),
        "an id given means no picker"
    );
    // `/resume` alone still opens the picker
    let mut c = booted();
    let r = enter(&mut c, "/resume");
    assert!(matches!(c.mode, Mode::Picker(_)) && r[0].path == "/api/sessions?archived=1");
    // an id that is not an id never becomes part of a request path
    for bad in [
        "../admin/secrets",
        "a/b",
        "chat 1",
        "",
        "x".repeat(65).as_str(),
        "id?x=1",
        "%2e%2e",
    ] {
        let mut d = booted();
        let out = d.resume_at_start(Some(bad), false);
        assert!(
            out.is_empty() && text(&d).contains("not a conversation id"),
            "{bad:?}: {}",
            text(&d)
        );
    }
    let mut e = booted();
    let out = enter(&mut e, "/resume ../../admin");
    assert!(
        out.is_empty() && text(&e).contains("not a conversation id"),
        "{}",
        text(&e)
    );
    // an id that is well formed but unknown: the service's reason is shown, nothing else opens
    let mut f = booted();
    let r = f.resume_at_start(Some("nothing-here"), false);
    let out = f.handle(ok(
        &r[0],
        404,
        json!({"error": {"code": "NOT_FOUND", "message": "no such conversation"}}),
    ));
    assert!(
        out.is_empty() && text(&f).contains("no such conversation"),
        "{}",
        text(&f)
    );
}

#[test]
fn tu78_resume_opens_the_newest_of_this_project_not_the_pinned_first_and_fork_opens_a_branch_instead(
) {
    let list = json!({"sessions": [
        {"id": "chat-pinned-old", "title": "", "updated": 5, "turns": 2, "project": "payments-service", "pinned": true},
        {"id": "chat-other", "title": "", "updated": 90, "turns": 3, "project": "billing"},
        {"id": "chat-here-new", "title": "", "updated": 70, "turns": 4, "project": "payments-service"},
        {"id": "chat-here-old", "title": "", "updated": 10, "turns": 1, "project": "payments-service"},
    ]});
    // in project payments-service: the newest of that project, though a pinned one is listed first and another project's is newer
    let mut a = booted();
    a.chat_project = Some("payments-service".into());
    let r = a.resume_at_start(None, false);
    let open = a.handle(ok(&r[0], 200, list.clone()));
    assert_eq!(open[0].path, "/api/sessions/chat-here-new", "{}", text(&a));
    // in a project with none kept: the newest of all, and the screen says whose it is
    let mut b = booted();
    b.chat_project = Some("kakurega".into());
    let r = b.resume_at_start(None, false);
    let open = b.handle(ok(&r[0], 200, list.clone()));
    assert_eq!(open[0].path, "/api/sessions/chat-other");
    assert!(
        text(&b).contains("No kept conversation for project kakurega yet")
            && text(&b).contains("from project billing"),
        "{}",
        text(&b)
    );
    // outside any project: the newest of all, with nothing said about projects
    let mut c = booted();
    c.chat_project = None;
    let r = c.resume_at_start(None, false);
    let open = c.handle(ok(&r[0], 200, list.clone()));
    assert_eq!(open[0].path, "/api/sessions/chat-other");
    assert!(
        !text(&c).contains("No kept conversation for project"),
        "{}",
        text(&c)
    );
    // --fork: the newest of the project is branched with all its messages, and the branch is what opens
    let mut d = booted();
    d.chat_project = Some("payments-service".into());
    let r = d.resume_at_start(None, true);
    let fork = d.handle(ok(&r[0], 200, list.clone()));
    assert_eq!(
        (fork[0].path.as_str(), fork[0].method),
        ("/api/sessions/chat-here-new/fork", Method::Post),
        "{}",
        text(&d)
    );
    assert_eq!(fork[0].body.as_ref().unwrap()["at"], 4);
    let open = d.handle(ok(&fork[0], 201, json!({"id": "chat-branch-1"})));
    assert_eq!(
        open[0].path, "/api/sessions/chat-branch-1",
        "the branch opens, the original is left alone"
    );
    // --resume ID --fork: that conversation, branched
    let mut e = booted();
    let r = e.resume_at_start(Some("chat-here-old"), true);
    assert_eq!(r[0].path, "/api/sessions");
    let fork = e.handle(ok(&r[0], 200, list.clone()));
    assert_eq!(fork[0].path, "/api/sessions/chat-here-old/fork");
    assert_eq!(fork[0].body.as_ref().unwrap()["at"], 1);
    // an id that is not listed, or not an id, branches nothing
    let mut f = booted();
    let r = f.resume_at_start(Some("chat-nowhere"), true);
    assert!(
        f.handle(ok(&r[0], 200, list.clone())).is_empty()
            && text(&f).contains("no such kept conversation"),
        "{}",
        text(&f)
    );
    let mut g = booted();
    assert!(
        g.resume_at_start(Some("../admin"), true).is_empty()
            && text(&g).contains("not a conversation id")
    );
    // a refusal while branching shows the service's reason and opens nothing
    let mut h = booted();
    let r = h.resume_at_start(None, true);
    let fork = h.handle(ok(&r[0], 200, list));
    let out = h.handle(ok(
        &fork[0],
        409,
        json!({"error": {"code": "TOO_MANY", "message": "forget some kept conversations first"}}),
    ));
    assert!(
        out.is_empty() && text(&h).contains("forget some kept conversations first"),
        "{}",
        text(&h)
    );
}

fn open_picker_with(app: &mut App, sessions: Value) -> Vec<String> {
    let r = enter(app, "/sessions");
    let sess = r
        .iter()
        .find(|q| q.path.starts_with("/api/sessions"))
        .expect("the list is requested");
    app.handle(ok(sess, 200, sessions));
    picker_rows(app)
}

#[test]
fn tu79_the_picker_starts_on_this_projects_conversations_and_tab_widens_it_to_all_projects() {
    let list = json!({"sessions": [
        {"id": "c-here-1", "title": "payments retry", "updated": 50, "turns": 2, "project": "payments-service"},
        {"id": "c-other", "title": "billing export", "updated": 90, "turns": 3, "project": "billing"},
        {"id": "c-none", "title": "loose chat", "updated": 70, "turns": 1, "project": null},
        {"id": "c-here-2", "title": "payments audit", "updated": 20, "turns": 4, "project": "payments-service"},
    ]});
    let mut a = booted();
    a.chat_project = Some("payments-service".into());
    a.jobs.push(kk_tui::app::JobRow {
        id: "job-1".into(),
        recipient: "acme".into(),
        state: "ready".into(),
    });
    // opens on this project's conversations (newest first); a review is always listed
    let rows = open_picker_with(&mut a, list.clone());
    assert_eq!(
        rows,
        ["c-here-1", "c-here-2", "job-1"],
        "only this project's chats, and the review"
    );
    let (lines, _) = render(&a, 110, 24, false);
    let screen = lines.join("\n");
    assert!(
        screen.contains("[project payments-service · Tab: all projects]"),
        "{screen}"
    );
    assert!(!screen.contains("billing export"), "{screen}");
    // Tab shows every project, with the project named in the detail
    a.handle(key(KeyCode::Tab));
    assert_eq!(
        picker_rows(&a),
        ["c-other", "c-none", "c-here-1", "c-here-2", "job-1"]
    );
    let (lines, _) = render(&a, 110, 24, false);
    let screen = lines.join("\n");
    assert!(
        screen.contains("[all projects · Tab: only payments-service]")
            && screen.contains("billing export")
            && screen.contains("project billing"),
        "{screen}"
    );
    // Tab again narrows it back
    a.handle(key(KeyCode::Tab));
    assert_eq!(picker_rows(&a), ["c-here-1", "c-here-2", "job-1"]);
    // typing a filter works inside the scope: "billing" finds nothing until Tab widens it
    for c in "billing".chars() {
        a.handle(key(KeyCode::Char(c)));
    }
    assert!(picker_rows(&a).is_empty());
    a.handle(key(KeyCode::Tab));
    assert_eq!(picker_rows(&a), ["c-other"]);
    // a project with nothing kept shows everything and says why
    let mut b = booted();
    b.chat_project = Some("kakurega".into());
    let rows = open_picker_with(&mut b, list.clone());
    assert_eq!(rows.len(), 4, "{rows:?}");
    let (lines, _) = render(&b, 110, 24, false);
    assert!(lines
        .join("\n")
        .contains("No kept conversation for project kakurega yet: showing every project."));
    // no project (a folder with none): everything, no scope shown, Tab does nothing
    let mut c = booted();
    c.chat_project = None;
    let rows = open_picker_with(&mut c, list);
    assert_eq!(rows.len(), 4);
    c.handle(key(KeyCode::Tab));
    assert_eq!(picker_rows(&c).len(), 4);
    let (lines, _) = render(&c, 110, 24, false);
    assert!(!lines.join("\n").contains("Tab: "), "{}", lines.join("\n"));
    // deleting inside the scope still needs the second press and only touches the chosen conversation
    let mut d = booted();
    d.chat_project = Some("payments-service".into());
    let _ = open_picker_with(
        &mut d,
        json!({"sessions": [{"id": "c-1", "title": "a", "updated": 5, "turns": 1, "project": "payments-service"}, {"id": "c-2", "title": "b", "updated": 4, "turns": 1, "project": "billing"}]}),
    );
    assert_eq!(picker_rows(&d), ["c-1"]);
}

#[test]
fn tu80_right_arrow_previews_the_last_messages_of_the_chosen_conversation_and_moving_or_left_closes_it(
) {
    let list = json!({"sessions": [
        {"id": "c-1", "title": "payments retry", "updated": 50, "turns": 6, "project": null},
        {"id": "c-2", "title": "billing export", "updated": 40, "turns": 2, "project": null},
    ]});
    let mut a = booted();
    a.chat_project = None;
    a.jobs.push(kk_tui::app::JobRow {
        id: "job-1".into(),
        recipient: "acme".into(),
        state: "ready".into(),
    });
    open_picker_with(&mut a, list.clone());
    let r = a.handle(key(KeyCode::Right));
    assert_eq!(
        (r[0].method, r[0].path.as_str()),
        (Method::Get, "/api/sessions/c-1")
    );
    let turns: Vec<Value> = (1..=6).map(|n| json!({"role": if n % 2 == 1 { "user" } else { "assistant" }, "content": format!("message number {n}\twith\u{1b}[31m a control")})).collect();
    a.handle(ok(&r[0], 200, json!({"id": "c-1", "turns": turns})));
    let (lines, _) = render(&a, 110, 24, false);
    let screen = lines.join("\n");
    assert!(
        screen.contains("assistant: message number 6") && screen.contains("you: message number 3"),
        "{screen}"
    );
    assert!(
        !screen.contains("message number 2") && !screen.contains("\u{1b}"),
        "only the last four, with controls stripped: {screen}"
    );
    // moving closes it; a second Right on the same row opens it again, and Right again closes it
    a.handle(key(KeyCode::Down));
    assert!(!render(&a, 110, 24, false)
        .0
        .join("\n")
        .contains("message number 6"));
    a.handle(key(KeyCode::Up));
    let r = a.handle(key(KeyCode::Right));
    a.handle(ok(&r[0], 200, json!({"id": "c-1", "turns": []})));
    assert!(render(&a, 110, 24, false)
        .0
        .join("\n")
        .contains("(no messages)"));
    assert!(a.handle(key(KeyCode::Right)).is_empty());
    assert!(
        !render(&a, 110, 24, false)
            .0
            .join("\n")
            .contains("(no messages)"),
        "Right on an open preview closes it"
    );
    // Left closes
    let r = a.handle(key(KeyCode::Right));
    a.handle(ok(
        &r[0],
        200,
        json!({"id": "c-1", "turns": [{"role": "user", "content": "hello there"}]}),
    ));
    assert!(render(&a, 110, 24, false)
        .0
        .join("\n")
        .contains("you: hello there"));
    a.handle(key(KeyCode::Left));
    assert!(!render(&a, 110, 24, false)
        .0
        .join("\n")
        .contains("hello there"));
    // an answer that arrives after the owner moved to another row is dropped
    let r = a.handle(key(KeyCode::Right));
    a.handle(key(KeyCode::Down));
    a.handle(ok(
        &r[0],
        200,
        json!({"id": "c-1", "turns": [{"role": "user", "content": "late answer"}]}),
    ));
    assert!(!render(&a, 110, 24, false)
        .0
        .join("\n")
        .contains("late answer"));
    // a failure is shown in the list and the picker stays open
    let r = a.handle(key(KeyCode::Right));
    a.handle(ok(
        &r[0],
        500,
        json!({"error": {"code": "STORE", "message": "could not read the conversation"}}),
    ));
    assert!(
        matches!(a.mode, Mode::Picker(_))
            && render(&a, 110, 24, false)
                .0
                .join("\n")
                .contains("could not read the conversation")
    );
    // a long message is cut with an ellipsis
    let mut b = booted();
    b.chat_project = None;
    open_picker_with(&mut b, list);
    let r = b.handle(key(KeyCode::Right));
    b.handle(ok(
        &r[0],
        200,
        json!({"id": "c-1", "turns": [{"role": "user", "content": "w".repeat(300)}]}),
    ));
    let screen = render(&b, 150, 24, false).0.join("\n");
    assert!(
        screen.contains(&format!("you: {}…", "w".repeat(100)))
            && !screen.contains(&"w".repeat(101)),
        "{screen}"
    );
    // reviews have no preview
    let mut c = booted();
    c.chat_project = None;
    c.jobs.push(kk_tui::app::JobRow {
        id: "job-1".into(),
        recipient: "acme".into(),
        state: "ready".into(),
    });
    open_picker_with(&mut c, json!({"sessions": []}));
    assert!(c.handle(key(KeyCode::Right)).is_empty());
}

#[test]
fn tu81_what_stopped_while_the_owner_was_away_is_shown_once_at_launch_and_marked_seen_and_again_on_leaving(
) {
    let mut a = booted();
    let bg = a.drain_background();
    let away = bg
        .iter()
        .find(|r| r.path == "/api/away")
        .expect("asked at launch");
    assert_eq!(away.method, Method::Get);
    // asked only once
    assert!(a.drain_background().iter().all(|r| r.path != "/api/away"));
    let next = a.handle(ok(away, 200, json!({"since": 100, "finished": [
        {"id": "job-4", "recipient": "acme-audit", "state": "ready", "note": "", "at": 150},
        {"id": "job-3", "recipient": "beta", "state": "failed", "note": "the model could not be reached\u{1b}[31m", "at": 140},
        {"id": "job-2", "recipient": "gamma", "state": "nothing_to_release", "note": "", "at": 120},
    ]})));
    let t = text(&a);
    assert!(t.contains("Stopped while you were away"), "{t}");
    assert!(
        t.contains("job-4") && t.contains("acme-audit") && t.contains("ready for your approval"),
        "{t}"
    );
    assert!(
        t.contains("job-3")
            && t.contains("stopped: the model could not be reached")
            && !t.contains("\u{1b}"),
        "{t}"
    );
    assert!(
        t.contains("job-2") && t.contains("nothing to release") && t.contains("/open ID shows one"),
        "{t}"
    );
    // told once: the list is marked seen straight away
    assert_eq!(
        (next[0].method, next[0].path.as_str()),
        (Method::Post, "/api/away/seen")
    );
    // nothing stopped: no card, but still marked seen (the first launch sets the mark)
    let mut b = booted();
    let away = b
        .drain_background()
        .into_iter()
        .find(|r| r.path == "/api/away")
        .unwrap();
    let next = b.handle(ok(&away, 200, json!({"since": null, "finished": []})));
    assert!(!text(&b).contains("Stopped while you were away"));
    assert_eq!(next[0].path, "/api/away/seen");
    // the service cannot be asked: no card, no error shown, no mark set
    let mut c = booted();
    let away = c
        .drain_background()
        .into_iter()
        .find(|r| r.path == "/api/away")
        .unwrap();
    let next = c.handle(ok(
        &away,
        500,
        json!({"error": {"code": "STORE", "message": "x"}}),
    ));
    assert!(next.is_empty() && !text(&c).contains("Stopped while you were away"));
    // leaving marks the list seen
    let last = a.leaving();
    assert_eq!(
        (last.method, last.path.as_str()),
        (Method::Post, "/api/away/seen")
    );
}

fn down(req: &Req) -> Event {
    Event::Resp(Resp {
        id: req.id,
        result: Err("connection refused".into()),
    })
}

#[test]
fn tu82_the_owner_is_alerted_once_when_the_service_stops_answering_and_again_only_after_it_came_back(
) {
    let mut a = booted();
    let r = enter(&mut a, "/status");
    a.handle(down(&r[0]));
    assert!(text(&a).contains("Could not reach the service: connection refused"));
    assert_eq!(
        a.attention.take(),
        Some(kk_tui::attention::Attention::ServiceDown),
        "the first failure alerts"
    );
    // more failures while it is still down: the card says so each time, the alert does not repeat
    let r = enter(&mut a, "/status");
    a.handle(down(&r[0]));
    assert_eq!(
        a.attention.take(),
        None,
        "no second alert while it stays down"
    );
    // any answer means it is back; the next failure alerts again
    let r = enter(&mut a, "/status");
    a.handle(ok(&r[0], 200, json!({"jobs": []})));
    assert_eq!(a.attention.take(), None);
    let r = enter(&mut a, "/status");
    a.handle(down(&r[0]));
    assert_eq!(
        a.attention.take(),
        Some(kk_tui::attention::Attention::ServiceDown),
        "alerted again after it came back and went away"
    );
    // an error answer is still an answer: the service is up, so no alert and the flag is cleared
    let r = enter(&mut a, "/status");
    a.handle(ok(
        &r[0],
        500,
        json!({"error": {"code": "STORE", "message": "x"}}),
    ));
    assert_eq!(a.attention.take(), None);
    let r = enter(&mut a, "/status");
    a.handle(down(&r[0]));
    assert_eq!(
        a.attention.take(),
        Some(kk_tui::attention::Attention::ServiceDown)
    );
}

#[test]
fn tu83_rewind_alone_and_escape_twice_list_the_owners_messages_and_choosing_one_rewinds_to_it() {
    // nothing kept: the same sentence as /rewind N
    let mut a = booted();
    assert!(
        enter(&mut a, "/rewind").is_empty()
            && text(&a).contains("Only a kept conversation can be rewound")
    );
    assert!(!matches!(a.mode, Mode::Picker(_)));
    // kept, but nothing written yet
    a.session = Some("chat-1".into());
    assert!(
        enter(&mut a, "/rewind").is_empty() && text(&a).contains("You have not written a message")
    );
    a.chat_history = vec![
        ("user".into(), "first question".into()),
        ("assistant".into(), "first answer".into()),
        (
            "user".into(),
            format!("second   question\n{}", "long ".repeat(40)),
        ),
        ("assistant".into(), "second answer".into()),
        ("user".into(), "third question".into()),
    ];
    enter(&mut a, "/rewind");
    // only the owner's messages, newest first, numbered as /rewind N numbers them
    assert_eq!(picker_rows(&a), ["5", "3", "1"]);
    let (lines, _) = render(&a, 110, 24, false);
    let screen = lines.join("\n");
    assert!(
        screen.contains("Go back to before one of your messages")
            && screen.contains("Your messages, newest first"),
        "{screen}"
    );
    assert!(
        screen.contains("third question")
            && screen.contains("message 5")
            && screen.contains("first question"),
        "{screen}"
    );
    assert!(
        !screen.contains("first answer") && !screen.contains("second answer"),
        "{screen}"
    );
    assert!(
        screen.contains("second question long") && screen.contains("…"),
        "a long message is one cut line: {screen}"
    );
    // choosing the middle one rewinds to it, like /rewind 3
    a.handle(key(KeyCode::Down));
    let r = a.handle(key(KeyCode::Enter));
    assert!(!matches!(a.mode, Mode::Picker(_)));
    assert_eq!(
        (r[0].method, r[0].path.as_str()),
        (Method::Post, "/api/sessions/chat-1/fork")
    );
    assert_eq!(r[0].body.as_ref().unwrap(), &json!({"at": 2}));
    assert!(
        a.rewound
            .as_deref()
            .is_some_and(|m| m.starts_with("second   question")),
        "the message is put back in the box"
    );
    // Ctrl-D on a message forgets nothing
    enter(&mut a, "/rewind");
    let none = a.handle(ctrl('d'));
    assert!(
        none.is_empty()
            && render(&a, 110, 24, false)
                .0
                .join("\n")
                .contains("Your messages are not forgotten here; Enter goes back to one.")
    );
    a.handle(key(KeyCode::Esc));
    // Esc twice on an empty box opens the same list; once does not
    let mut b = booted();
    b.session = Some("chat-9".into());
    b.chat_history = vec![
        ("user".into(), "only question".into()),
        ("assistant".into(), "answer".into()),
    ];
    b.handle(key(KeyCode::Esc));
    assert!(!matches!(b.mode, Mode::Picker(_)));
    b.handle(key(KeyCode::Esc));
    assert!(matches!(b.mode, Mode::Picker(_)));
    assert_eq!(picker_rows(&b), ["1"]);
    // with something typed, Esc clears it and does not open anything
    let mut c = booted();
    c.session = Some("chat-9".into());
    c.chat_history = vec![("user".into(), "q".into())];
    typed(&mut c, "half a thought");
    c.handle(key(KeyCode::Esc));
    c.handle(key(KeyCode::Esc));
    assert!(c.input.is_empty() && !matches!(c.mode, Mode::Picker(_)));
    // the first Esc of a pair expires, so a stray Esc long before another opens nothing
    let mut d = booted();
    d.session = Some("chat-9".into());
    d.chat_history = vec![("user".into(), "q".into())];
    d.handle(key(KeyCode::Esc));
    for _ in 0..10 {
        d.handle(Event::Tick);
    }
    d.handle(key(KeyCode::Esc));
    assert!(!matches!(d.mode, Mode::Picker(_)));
    // a stray Esc before a chat starts does not make the first Esc during the answer stop it
    let mut e = booted();
    e.handle(key(KeyCode::Esc));
    typed(&mut e, "hello there");
    let r = e.handle(key(KeyCode::Enter));
    assert!(r.iter().any(|q| q.path.contains("chat")) || e.chat_waiting);
    e.handle(key(KeyCode::Esc));
    assert!(
        !e.cancel.load(std::sync::atomic::Ordering::SeqCst),
        "one Esc arms, it does not stop the answer"
    );
}

#[test]
fn tu84_a_conversation_can_be_put_away_from_the_picker_and_found_again_in_the_archived_view() {
    let list = json!({"sessions": [
        {"id": "c-use", "title": "in use", "updated": 50, "turns": 2, "project": "payments-service", "archived": false},
        {"id": "c-away", "title": "put away", "updated": 40, "turns": 3, "project": "billing", "archived": true},
        {"id": "c-use2", "title": "also in use", "updated": 30, "turns": 1, "project": null, "archived": false},
    ]});
    let mut a = booted();
    a.chat_project = None;
    a.jobs.push(kk_tui::app::JobRow {
        id: "job-1".into(),
        recipient: "acme".into(),
        state: "ready".into(),
    });
    // the list in use does not show the archived one
    assert_eq!(
        open_picker_with(&mut a, list.clone()),
        ["c-use", "c-use2", "job-1"]
    );
    // Ctrl-R shows only what was put away, from every project, and names the view
    a.handle(ctrl('r'));
    assert_eq!(picker_rows(&a), ["c-away"]);
    let screen = render(&a, 110, 24, false).0.join("\n");
    assert!(
        screen.contains("[archived · Ctrl-R: back to the list in use]")
            && screen.contains("put away"),
        "{screen}"
    );
    // Ctrl-A there brings it back: the request says so, and after the service agrees it moves to the list in use
    let r = a.handle(ctrl('a'));
    assert_eq!(
        (r[0].method, r[0].path.as_str()),
        (Method::Post, "/api/sessions/c-away/archive")
    );
    assert_eq!(r[0].body.as_ref().unwrap(), &json!({"archived": false}));
    a.handle(ok(&r[0], 200, json!({"ok": true})));
    assert!(picker_rows(&a).is_empty(), "it left the archived view");
    a.handle(ctrl('r'));
    assert_eq!(
        picker_rows(&a),
        ["c-use", "c-away", "c-use2", "job-1"],
        "and it is in the list in use now, in its place by date"
    );
    // Ctrl-A on a conversation in use puts it away
    a.handle(key(KeyCode::Down));
    let r = a.handle(ctrl('a'));
    assert_eq!(r[0].path, "/api/sessions/c-away/archive");
    assert_eq!(r[0].body.as_ref().unwrap(), &json!({"archived": true}));
    // a refusal is shown in the list, nothing moves, the picker stays open
    a.handle(ok(
        &r[0],
        404,
        json!({"error": {"code": "NO_SESSION", "message": "there is no such kept conversation"}}),
    ));
    assert!(matches!(a.mode, Mode::Picker(_)));
    assert!(picker_rows(&a).contains(&"c-away".to_string()));
    assert!(render(&a, 110, 24, false)
        .0
        .join("\n")
        .contains("there is no such kept conversation"));
    // a review cannot be put away
    a.handle(key(KeyCode::Down));
    a.handle(key(KeyCode::Down));
    assert_eq!(a.handle(ctrl('a')).len(), 0);
    assert!(render(&a, 110, 24, false)
        .0
        .join("\n")
        .contains("Only kept conversations can be put away."));
    // reviews are not listed in the archived view
    a.handle(ctrl('r'));
    assert!(!picker_rows(&a).contains(&"job-1".to_string()));
    // with a project scope on, the archived view still looks across every project
    let mut b = booted();
    b.chat_project = Some("payments-service".into());
    assert_eq!(
        open_picker_with(&mut b, list),
        ["c-use"],
        "scoped to the project"
    );
    b.handle(ctrl('r'));
    assert_eq!(
        picker_rows(&b),
        ["c-away"],
        "the archived one of another project is found"
    );
}

#[test]
fn tu85_the_terminal_usage_shows_a_cost_only_where_a_price_was_entered_and_marks_an_estimate() {
    let mut app = booted();
    let r = enter(&mut app, "/usage");
    app.handle(ok(&r[0], 200, json!({"from": 0, "to": 1, "routes": [
        {"route": "models.example.com/m1", "class": "device_only", "simulated": false, "turns": 3, "prompt_tokens": 1200, "measured_turns": 1, "reply_tokens": 300, "avg_ms": 900, "max_ms": 1500, "reply_tokens_per_second": 30.0, "cost": {"amount": 0.0123, "currency": "USD", "tokens_estimated": true}},
        {"route": "ollama:qwen", "class": "device_only", "simulated": false, "turns": 2, "prompt_tokens": 50, "measured_turns": 2, "reply_tokens": 20, "avg_ms": 10, "max_ms": 12, "reply_tokens_per_second": 0.0, "cost": null},
    ], "total": {"turns": 5, "prompt_tokens": 1250, "reply_tokens": 320, "cost": [{"currency": "USD", "amount": 0.0123}, {"currency": "EUR", "amount": 0.5}]}, "note": "n"})));
    let t = text(&app);
    assert!(
        t.contains("models.example.com/m1") && t.contains("about USD 0.0123*"),
        "{t}"
    );
    assert!(
        t.contains("about USD 0.0123 and EUR 0.5000"),
        "totals by currency: {t}"
    );
    assert!(
        t.contains("Costs are your own prices times the tokens counted."),
        "{t}"
    );
    let line = t
        .lines()
        .find(|l| l.starts_with("ollama:qwen"))
        .expect("the unpriced route");
    assert!(!line.contains("about"), "no price, no cost: {line}");
    // no cost anywhere: nothing about a cost is claimed, and the way to get one is said
    let mut b = booted();
    let r = enter(&mut b, "/usage");
    b.handle(ok(&r[0], 200, json!({"from": 0, "to": 1, "routes": [{"route": "r", "class": "device_only", "simulated": false, "turns": 1, "prompt_tokens": 10, "measured_turns": 1, "reply_tokens": 5, "avg_ms": 1, "max_ms": 1, "reply_tokens_per_second": 1.0, "cost": null}], "total": {"turns": 1, "prompt_tokens": 10, "reply_tokens": 5, "cost": []}, "note": "n"})));
    let t = text(&b);
    assert!(
        !t.contains("about") && t.contains("A cost shows only for a route whose price you entered"),
        "{t}"
    );
    // an older service that sends no cost field at all is read as no cost
    let mut c = booted();
    let r = enter(&mut c, "/usage");
    c.handle(ok(&r[0], 200, json!({"from": 0, "to": 1, "routes": [{"route": "r", "class": "device_only", "simulated": false, "turns": 1, "prompt_tokens": 10, "measured_turns": 1, "reply_tokens": 5, "avg_ms": 1, "max_ms": 1, "reply_tokens_per_second": 1.0}], "total": {"turns": 1, "prompt_tokens": 10, "reply_tokens": 5}, "note": "n"})));
    assert!(!text(&c).contains("about"), "{}", text(&c));
    // control characters in a currency never reach the screen
    let mut d = booted();
    let r = enter(&mut d, "/usage");
    d.handle(ok(&r[0], 200, json!({"from": 0, "to": 1, "routes": [{"route": "r", "class": "device_only", "simulated": false, "turns": 1, "prompt_tokens": 10, "measured_turns": 1, "reply_tokens": 5, "avg_ms": 1, "max_ms": 1, "reply_tokens_per_second": 1.0, "cost": {"amount": 1.0, "currency": "U\u{1b}[2JD", "tokens_estimated": false}}], "total": {"turns": 1, "prompt_tokens": 10, "reply_tokens": 5, "cost": []}, "note": "n"})));
    assert!(!text(&d).contains('\u{1b}'), "{}", text(&d));
}

#[test]
fn tu86_an_answer_with_direction_and_invisible_characters_never_draws_them_on_the_screen() {
    let hostile = "safe\u{202e}gnirts desrever\u{202c} zero\u{200b}width bom\u{feff}mark isolate\u{2066}x\u{2069} sep\u{2028}arator \u{1b}[2Jescape";
    let mut app = booted();
    let r = enter(&mut app, "hello");
    app.handle(ok(
        &r[0],
        200,
        json!({"reply": hostile, "route": {"simulated": false}, "notes_used": 0}),
    ));
    for width in [30u16, 80, 140] {
        let (rows, _) = render(&app, width, 30, false);
        let screen = rows.join("\n");
        assert!(
            screen.contains("safe") && screen.contains("width"),
            "the answer is shown: {screen}"
        );
        for bad in [
            '\u{202e}', '\u{202c}', '\u{200b}', '\u{feff}', '\u{2066}', '\u{2069}', '\u{2028}',
            '\u{1b}',
        ] {
            assert!(
                !screen.contains(bad),
                "U+{:04X} reached the screen at width {width}: {screen}",
                bad as u32
            );
        }
    }
}

#[test]
fn tu87_clean_turns_control_direction_and_invisible_characters_into_the_replacement_mark_and_keeps_everything_else(
) {
    use kk_tui::app::clean;
    for bad in [
        '\u{1b}', '\u{7}', '\u{0}', '\u{9f}', '\u{200b}', '\u{200c}', '\u{200d}', '\u{200e}',
        '\u{200f}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}',
        '\u{2067}', '\u{2068}', '\u{2069}', '\u{2028}', '\u{2029}', '\u{feff}',
    ] {
        assert_eq!(
            clean(&format!("a{bad}b")),
            "a\u{fffd}b",
            "U+{:04X}",
            bad as u32
        );
    }
    assert_eq!(clean("a\nb\tc é 日本語 😀"), "a\nb\tc é 日本語 😀");
}

fn forget_note(app: &mut kk_tui::app::App) -> kk_tui::app::Req {
    enter(app, "/forget m-12");
    let mut r = app.handle(key(KeyCode::Char('y')));
    r.remove(0)
}

#[test]
fn tu88_forgetting_a_note_in_the_terminal_names_what_is_left_and_how_to_remove_it() {
    let mut app = booted();
    let req = forget_note(&mut app);
    let follow = app.handle(ok(&req, 200, json!({
        "forgotten": true,
        "copies_known": {
            "reviews": ["job-3"], "exports": ["call-one.md"],
            "conversations": [{"id": "s-1", "why": "text"}, {"id": "s-2", "why": "source"}, {"id": "s-9", "why": "text"}],
            "conversations_more": 4, "prompt_history": 2, "search_complete": false
        },
        "not_reached": ["Backups you made with kaku backup: they still hold the text.", "Capsules you delivered: the recipient has their own copy."]
    })));
    let t = text(&app);
    assert!(
        t.contains("Forgotten. The text is deleted and will not be saved again."),
        "{t}"
    );
    assert!(
        t.contains("in reviews: job-3") && t.contains("in exports: call-one.md"),
        "{t}"
    );
    assert!(
        t.contains("Lines of your typed history that hold it: 2") && t.contains("/clearhistory"),
        "{t}"
    );
    assert!(
        t.contains("stopped early"),
        "an incomplete search is said: {t}"
    );
    assert!(
        t.contains("Not removed by this: Backups you made")
            && t.contains("Not removed by this: Capsules"),
        "{t}"
    );
    // the conversations are looked up by title in the owner's own list
    assert_eq!(follow.len(), 1, "one follow-up request");
    assert_eq!(follow[0].path, "/api/sessions?archived=1");
    assert!(!follow[0].admin);
    app.handle(ok(
        &follow[0],
        200,
        json!({"sessions": [
            {"id": "s-1", "title": "Invoices with Acme", "turns": 4, "updated": 1},
            {"id": "s-2", "title": "", "turns": 2, "updated": 2},
            {"id": "s-other", "title": "Unrelated", "turns": 1, "updated": 3}
        ]}),
    ));
    let t = text(&app);
    assert!(
        t.contains("Invoices with Acme: its words hold the note"),
        "{t}"
    );
    assert!(
        t.contains("Untitled conversation: the note was accepted from it"),
        "{t}"
    );
    assert!(
        t.contains("- Untitled conversation: its words hold the note"),
        "a conversation missing from the list still shows: {t}"
    );
    assert!(
        t.contains("and 4 more") && t.contains("/resume, choose it and press Delete twice"),
        "{t}"
    );
    assert!(
        !t.contains("Unrelated"),
        "only the named conversations: {t}"
    );
}

#[test]
fn tu89_a_forget_with_nothing_left_stays_short_an_old_service_is_read_as_nothing_and_hostile_names_are_cleaned(
) {
    // nothing known: no follow-up request, no copies lines
    let mut app = booted();
    let req = forget_note(&mut app);
    let follow = app.handle(ok(&req, 200, json!({"forgotten": true, "copies_known": {"reviews": [], "exports": [], "conversations": [], "conversations_more": 0, "prompt_history": 0, "search_complete": true}, "not_reached": []})));
    assert!(follow.is_empty());
    let t = text(&app);
    assert!(
        t.contains("The text is deleted")
            && !t.contains("Still on this computer")
            && !t.contains("typed history that hold it"),
        "{t}"
    );
    // a service that sends no copies at all (older): the same short message, no crash
    let mut old = booted();
    let req = forget_note(&mut old);
    assert!(old
        .handle(ok(&req, 200, json!({"forgotten": true})))
        .is_empty());
    assert!(
        text(&old).contains("Forgotten. The text is deleted and will not be saved again."),
        "{}",
        text(&old)
    );
    // a failure is an error, not a success card
    let mut bad = booted();
    let req = forget_note(&mut bad);
    bad.handle(ok(
        &req,
        404,
        json!({"error": {"code": "NOT_FOUND", "message": "there is no such note"}}),
    ));
    assert!(
        text(&bad).contains("there is no such note") && !text(&bad).contains("The text is deleted"),
        "{}",
        text(&bad)
    );
    // control characters and bidi marks in names and titles never reach the screen
    let mut h = booted();
    let req = forget_note(&mut h);
    let follow = h.handle(ok(&req, 200, json!({"forgotten": true, "copies_known": {"reviews": ["job\u{1b}[2J"], "exports": ["a\u{202e}b.md"], "conversations": [{"id": "s-1", "why": "text"}], "conversations_more": 0, "prompt_history": 0, "search_complete": true}, "not_reached": ["x\u{7}y"]})));
    h.handle(ok(&follow[0], 200, json!({"sessions": [{"id": "s-1", "title": "T\u{1b}[31mitle\u{202e}", "turns": 1, "updated": 1}]})));
    let t = text(&h);
    for bad in ['\u{1b}', '\u{202e}', '\u{7}'] {
        assert!(
            !t.contains(bad),
            "U+{:04X} reached the screen: {t}",
            bad as u32
        );
    }
    assert!(
        t.contains("itle") && t.contains("its words hold the note"),
        "the title is still shown, cleaned: {t}"
    );
}

#[test]
fn tu90_display_settings_set_the_starting_verbosity_and_say_a_notice_once_after_the_welcome() {
    use kk_tui::settings::Settings;
    let base = || Settings {
        notify: None,
        color: true,
        theme: kk_tui::ui::Palette::Auto,
        quiet: false,
        notice: None,
    };
    // defaults: nothing changes
    let mut app = App::new();
    app.apply_settings(&base());
    assert!(!app.quiet);
    assert_eq!(app.cards.len(), 1, "only the welcome");
    // quiet starts on, and /quiet then turns it off, so it is the same switch
    let mut app = App::new();
    app.apply_settings(&Settings {
        quiet: true,
        ..base()
    });
    assert!(app.quiet);
    enter(&mut app, "/quiet");
    assert!(!app.quiet, "the command toggles what the setting started");
    // a notice is a warning card right after the welcome, once
    let mut app = App::new();
    app.apply_settings(&Settings {
        notice: Some(
            "config.toml was not applied, so the defaults are in use: quite: unknown setting."
                .into(),
        ),
        ..base()
    });
    assert_eq!(app.cards.len(), 2);
    assert_eq!(app.cards[1].kind, kk_tui::app::Kind::Warn);
    let t = text(&app);
    assert!(t.contains("config.toml was not applied"), "{t}");
    // a notice with control characters is cleaned by the card like every other line
    let mut app = App::new();
    app.apply_settings(&Settings {
        notice: Some("bad\u{1b}[2J\u{202e}".into()),
        ..base()
    });
    let t = text(&app);
    assert!(!t.contains('\u{1b}') && !t.contains('\u{202e}'), "{t}");
}

#[test]
fn tu91_the_plain_line_mode_prints_the_notice_as_a_warning_and_nothing_when_there_is_none() {
    use kk_tui::linear::{news, Printed};
    use kk_tui::settings::Settings;
    let with = |notice: Option<&str>| {
        let mut app = App::new();
        app.apply_settings(&Settings {
            notify: None,
            color: true,
            theme: kk_tui::ui::Palette::Auto,
            quiet: false,
            notice: notice.map(String::from),
        });
        let mut seen = Printed::new(false);
        news(&mut app, &mut seen)
    };
    let out = with(Some(
        "config.toml was not applied, so the defaults are in use.",
    ));
    assert!(
        out.contains("[Warning] config.toml was not applied"),
        "{out}"
    );
    assert!(!with(None).contains("[Warning]"));
}

#[test]
fn tu92_the_keeping_commands_parse_strictly_and_a_bad_form_gets_its_usage_line() {
    assert_eq!(cmd::parse("/dormant"), Ok(Command::Dormant(None)));
    assert_eq!(cmd::parse("/dormant 30"), Ok(Command::Dormant(Some(30))));
    assert_eq!(
        cmd::parse("/dormant 3650"),
        Ok(Command::Dormant(Some(3650)))
    );
    for bad in [
        "/dormant 29",
        "/dormant 3651",
        "/dormant -1",
        "/dormant abc",
        "/dormant 1.5",
        "/dormant 30 60",
        "/dormant 99999999999999999999",
    ] {
        assert_eq!(
            cmd::parse(bad),
            Err(format!("usage: {}", cmd::usage("dormant"))),
            "{bad}"
        );
    }
    assert_eq!(cmd::parse("/archived"), Ok(Command::Archived));
    assert!(cmd::parse("/archived now").is_err());
    for (line, action) in [
        ("/archive m-1", "archive"),
        ("/restore m-1", "restore"),
        ("/pin m-1", "pin"),
        ("/unpin m-1", "unpin"),
        ("/stilltrue m-1", "keep"),
    ] {
        assert_eq!(
            cmd::parse(line),
            Ok(Command::NoteKeeping {
                action,
                id: "m-1".into()
            }),
            "{line}"
        );
    }
    for name in ["archive", "restore", "pin", "unpin", "stilltrue"] {
        for bad in [
            String::new(),
            " ".into(),
            "a b".into(),
            "../x".into(),
            "a/b".into(),
            "x".repeat(65),
            "ü".into(),
            "a\nb".into(),
        ] {
            let r = cmd::parse(&format!("/{name} {bad}"));
            assert_eq!(
                r,
                Err(format!("usage: {}", cmd::usage(name))),
                "/{name} {bad:?}"
            );
        }
    }
    // every new name is listed with a description and a usage line, and nothing is shadowed
    for name in [
        "dormant",
        "archived",
        "archive",
        "restore",
        "pin",
        "unpin",
        "stilltrue",
    ] {
        assert!(cmd::NAMES.contains(&name), "{name}");
        assert!(
            !cmd::describe(name).is_empty() && cmd::usage(name).starts_with(&format!("/{name}")),
            "{name}"
        );
    }
    assert_eq!(
        cmd::parse("/keep"),
        Ok(Command::Keep),
        "the older /keep and /quiet keep their meaning"
    );
    assert_eq!(cmd::parse("/quiet"), Ok(Command::Quiet));
}

#[test]
fn tu93_dormant_lists_the_quiet_notes_with_why_and_says_nothing_was_put_away() {
    let mut a = booted();
    let r = enter(&mut a, "/dormant");
    assert_eq!(
        (r[0].method, r[0].path.as_str()),
        (Method::Get, "/api/memories/dormant?days=180")
    );
    a.handle(ok(
        &r[0],
        200,
        json!({"days": 180, "note": "x", "dormant": [
            {"id": "m-9", "text": "Old fact\u{202e}about leases", "project": "alpha", "idle_days": 412, "uses": 3, "level": "internal"},
            {"id": "m-8", "text": "Another old fact", "project": null, "idle_days": 200, "uses": 0, "level": "internal"}
        ]}),
    ));
    let t = text(&a);
    assert!(
        t.contains("m-9 [project alpha]  quiet 412 days, given to the assistant 3 times"),
        "{t}"
    );
    assert!(
        t.contains("m-8  quiet 200 days, given to the assistant 0 times"),
        "{t}"
    );
    assert!(
        t.contains("Nothing was put away")
            && t.contains("/archive ID")
            && t.contains("/stilltrue ID")
            && t.contains("/pin ID"),
        "{t}"
    );
    assert!(
        !t.contains('\u{202e}'),
        "direction characters from a note are removed"
    );
    let r = enter(&mut a, "/dormant 365");
    assert_eq!(r[0].path, "/api/memories/dormant?days=365");
    a.handle(ok(
        &r[0],
        200,
        json!({"days": 365, "note": "x", "dormant": []}),
    ));
    assert!(text(&a).contains("No note has been quiet for 365 days or more."));
    let r = enter(&mut a, "/dormant");
    a.handle(ok(&r[0], 422, json!({"error": {"code": "MEMORY_REFUSED", "message": "days must be a whole number from 30 to 3650"}})));
    assert!(text(&a).contains("days must be a whole number"));
}

#[test]
fn tu94_archived_lists_put_away_notes_and_each_keeping_command_sends_one_post_and_reports_the_outcome(
) {
    let mut a = booted();
    let r = enter(&mut a, "/archived");
    assert_eq!(
        (r[0].method, r[0].path.as_str()),
        (Method::Get, "/api/memories/archived")
    );
    a.handle(ok(&r[0], 200, json!({"archived": [{"id": "m-5", "text": "Put-away fact", "project": null, "level": "internal"}]})));
    let t = text(&a);
    assert!(
        t.contains("m-5  Put-away fact") && t.contains("/restore ID") && t.contains("/forget ID"),
        "{t}"
    );
    let r = enter(&mut a, "/archived");
    a.handle(ok(&r[0], 200, json!({"archived": []})));
    assert!(text(&a).contains("No notes are archived."));
    for (line, path, said) in [
        ("/archive m-5", "/api/memories/m-5/archive", "Put away."),
        ("/restore m-5", "/api/memories/m-5/restore", "Brought back."),
        ("/pin m-5", "/api/memories/m-5/pin", "Pinned."),
        ("/unpin m-5", "/api/memories/m-5/unpin", "Unpinned."),
        (
            "/stilltrue m-5",
            "/api/memories/m-5/keep",
            "Marked still true.",
        ),
    ] {
        let r = enter(&mut a, line);
        assert_eq!(r.len(), 1, "{line}");
        assert_eq!(
            (r[0].method, r[0].path.as_str()),
            (Method::Post, path),
            "{line}"
        );
        a.handle(ok(&r[0], 200, json!({"done": "x", "id": "m-5"})));
        assert!(text(&a).contains(said), "{line}: {}", text(&a));
        let r = enter(&mut a, line);
        a.handle(ok(&r[0], 422, json!({"error": {"code": "MEMORY_REFUSED", "message": "only a current note can be archived"}})));
        assert!(
            text(&a).contains("only a current note can be archived"),
            "{line}"
        );
        let r = enter(&mut a, line);
        a.handle(ok(
            &r[0],
            404,
            json!({"error": {"code": "NO_MEMORY", "message": "no such memory"}}),
        ));
        assert!(text(&a).contains("no such memory"), "{line}");
    }
}

#[test]
fn tu95_held_and_notsecret_parse_strictly() {
    assert_eq!(cmd::parse("/quarantine"), Ok(Command::Held));
    assert!(cmd::parse("/quarantine now").is_err());
    assert_eq!(
        cmd::parse("/notsecret m-3"),
        Ok(Command::NotSecret("m-3".into()))
    );
    for bad in ["", " ", "a b", "../x", "a/b", "ü", "a\nb"] {
        assert_eq!(
            cmd::parse(&format!("/notsecret {bad}")),
            Err(format!("usage: {}", cmd::usage("notsecret"))),
            "{bad:?}"
        );
    }
    assert_eq!(
        cmd::parse(&format!("/notsecret {}", "x".repeat(65))),
        Err(format!("usage: {}", cmd::usage("notsecret")))
    );
    for name in ["quarantine", "notsecret"] {
        assert!(
            cmd::NAMES.contains(&name)
                && !cmd::describe(name).is_empty()
                && cmd::usage(name).starts_with(&format!("/{name}"))
        );
    }
}

#[test]
fn tu96_held_lists_what_was_kept_back_without_hidden_characters_and_notsecret_reports_the_outcome()
{
    let mut a = booted();
    let r = enter(&mut a, "/quarantine");
    assert_eq!(
        (r[0].method, r[0].path.as_str()),
        (Method::Get, "/api/memories/held")
    );
    a.handle(ok(&r[0], 200, json!({"held": [{"id": "m-7", "text": "Deploy token\u{202e} abc", "kind": "github token\u{200b}", "project": null, "created_at": 1}], "note": "x"})));
    let t = text(&a);
    assert!(
        t.contains("m-7  looks like: github token\u{fffd}  Deploy token\u{fffd} abc"),
        "{t}"
    );
    assert!(
        t.contains("in no answer, review or list")
            && t.contains("/forget ID")
            && t.contains("/notsecret ID"),
        "{t}"
    );
    assert!(!t.contains('\u{202e}') && !t.contains('\u{200b}'));
    let r = enter(&mut a, "/quarantine");
    a.handle(ok(&r[0], 200, json!({"held": [], "note": "x"})));
    assert!(text(&a).contains("No notes are held back."));
    let r = enter(&mut a, "/quarantine");
    a.handle(ok(
        &r[0],
        500,
        json!({"error": {"code": "MEMORY_STORAGE", "message": "the memory store failed"}}),
    ));
    assert!(text(&a).contains("the memory store failed"));
    let r = enter(&mut a, "/notsecret m-7");
    assert_eq!(
        (r[0].method, r[0].path.as_str()),
        (Method::Post, "/api/memories/m-7/release-hold")
    );
    a.handle(ok(&r[0], 200, json!({"released": true})));
    assert!(text(&a).contains("Released. The note is offered again."));
    let r = enter(&mut a, "/notsecret m-7");
    a.handle(ok(
        &r[0],
        404,
        json!({"error": {"code": "NO_MEMORY", "message": "no such memory"}}),
    ));
    assert!(text(&a).contains("no such memory"));
}

fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    let f = |c: u8| {
        let c = c as f64 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}

fn contrast(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[test]
fn tu97_each_fixed_palette_is_readable_on_its_backgrounds_by_computed_contrast() {
    use kk_tui::ui::{Palette, ROLES};
    // pure background: at least 7:1 (WCAG AAA); the lightest dark and darkest light backgrounds a terminal commonly has: at least 4.5:1 (AA)
    let sets = [
        (Palette::Dark, (0, 0, 0), (0x30, 0x30, 0x30)),
        (Palette::Light, (255, 255, 255), (0xe4, 0xe4, 0xe4)),
    ];
    for (p, pure, worst) in sets {
        let mut colours: Vec<(String, (u8, u8, u8))> = ROLES
            .iter()
            .map(|r| {
                (
                    format!("{r:?}"),
                    p.rgb(*r).expect("a fixed palette names every role"),
                )
            })
            .collect();
        colours.push((
            "muted".into(),
            p.muted_rgb().expect("a fixed palette has a muted colour"),
        ));
        for (name, c) in colours {
            let a = contrast(c, pure);
            let b = contrast(c, worst);
            assert!(a >= 7.0, "{p:?} {name} {c:?} is {a:.2}:1 on {pure:?}");
            assert!(b >= 4.5, "{p:?} {name} {c:?} is {b:.2}:1 on {worst:?}");
        }
    }
    // the terminal's own palette cannot be measured, so it names no colours
    assert!(
        ROLES.iter().all(|r| Palette::Auto.rgb(*r).is_none())
            && Palette::Auto.muted_rgb().is_none()
    );
    // the roles stay apart from each other, so a colour never stands for two meanings
    for p in [Palette::Dark, Palette::Light] {
        let all: Vec<_> = ROLES.iter().map(|r| p.rgb(*r).unwrap()).collect();
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                assert_ne!(all[i], all[j], "{p:?}");
            }
        }
    }
}

#[test]
fn tu98_palette_words_are_a_closed_set() {
    use kk_tui::ui::Palette;
    assert_eq!(Palette::parse("auto"), Some(Palette::Auto));
    assert_eq!(Palette::parse(" Dark "), Some(Palette::Dark));
    assert_eq!(Palette::parse("LIGHT"), Some(Palette::Light));
    for bad in [
        "",
        "blue",
        "dark light",
        "darker",
        "contrast",
        "d",
        "ligth",
        "dark\u{0}",
        "../x",
    ] {
        assert_eq!(Palette::parse(bad), None, "{bad:?}");
    }
    for p in [Palette::Auto, Palette::Dark, Palette::Light] {
        assert_eq!(Palette::parse(p.name()), Some(p));
    }
}

fn colours_in(
    app: &App,
    palette: kk_tui::ui::Palette,
    color: bool,
) -> (Vec<ratatui::style::Color>, bool) {
    let mut t = Terminal::new(TestBackend::new(110, 30)).unwrap();
    t.draw(|f| draw(f, app, &Theme { color, palette })).unwrap();
    let buf = t.backend().buffer().clone();
    let mut fgs = Vec::new();
    let mut dim = false;
    for c in buf.content() {
        if c.fg != ratatui::style::Color::Reset {
            fgs.push(c.fg);
        }
        if c.modifier.contains(ratatui::style::Modifier::DIM) {
            dim = true;
        }
    }
    (fgs, dim)
}

#[test]
fn tu99_the_chosen_palette_is_what_is_drawn_and_no_colour_means_no_colour_whatever_the_palette() {
    use kk_tui::ui::Palette;
    use ratatui::style::Color;
    let mut a = booted();
    let r = enter(&mut a, "/similar");
    a.handle(ok(
        &r[0],
        500,
        json!({"error": {"code": "STORE", "message": "disk trouble"}}),
    ));
    // auto: the terminal's named colours, and dimmed secondary text, exactly as before
    let (auto, auto_dim) = colours_in(&a, Palette::Auto, true);
    assert!(
        auto.contains(&Color::Red) && auto.contains(&Color::Cyan),
        "{auto:?}"
    );
    assert!(
        auto.iter().all(|c| !matches!(c, Color::Rgb(..))),
        "auto never uses fixed colours"
    );
    assert!(auto_dim, "auto dims secondary text");
    // dark and light: only the fixed colours of that palette, no named colours, and nothing dimmed
    for (p, bad, accent) in [
        (
            Palette::Dark,
            Color::Rgb(0xff, 0x87, 0x87),
            Color::Rgb(0x5f, 0xd7, 0xff),
        ),
        (
            Palette::Light,
            Color::Rgb(0xaf, 0, 0),
            Color::Rgb(0, 0x5f, 0x87),
        ),
    ] {
        let (fgs, dim) = colours_in(&a, p, true);
        assert!(
            fgs.contains(&bad) && fgs.contains(&accent),
            "{p:?}: {fgs:?}"
        );
        assert!(
            fgs.iter().all(|c| matches!(c, Color::Rgb(..))),
            "{p:?} drew a named colour: {fgs:?}"
        );
        assert!(!dim, "{p:?} dims text, which lowers its contrast");
    }
    // colour off beats any palette: not one coloured cell, and dimming stays so meaning does not depend on colour
    for p in [Palette::Auto, Palette::Dark, Palette::Light] {
        let (fgs, _) = colours_in(&a, p, false);
        assert!(
            fgs.is_empty(),
            "{p:?} coloured text with colour off: {fgs:?}"
        );
    }
    // the words are the same in every palette
    let words = |p| {
        let mut t = Terminal::new(TestBackend::new(110, 30)).unwrap();
        t.draw(|f| {
            draw(
                f,
                &a,
                &Theme {
                    color: true,
                    palette: p,
                },
            )
        })
        .unwrap();
        t.backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol().to_string())
            .collect::<String>()
    };
    assert_eq!(words(Palette::Auto), words(Palette::Dark));
    assert_eq!(words(Palette::Auto), words(Palette::Light));
}

#[test]
fn tu100_a_proposed_fix_in_a_fixed_palette_keeps_its_colours_readable_and_is_never_dimmed() {
    use kk_tui::ui::Palette;
    use ratatui::style::{Color, Modifier};
    let mut app = booted();
    app.cards.push(Card {
        kind: Kind::Ok,
        title: Some("Fix 1".into()),
        lines: [
            "a.py: Make logout remove the token",
            "",
            "--- a/a.py",
            "+++ b/a.py",
            "@@ -1,2 +1,2 @@",
            " keep",
            "-old line",
            "+new line",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
    });
    let cell_for = |p: Palette, needle: &str| {
        let mut t = Terminal::new(TestBackend::new(110, 40)).unwrap();
        t.draw(|f| {
            draw(
                f,
                &app,
                &Theme {
                    color: true,
                    palette: p,
                },
            )
        })
        .unwrap();
        let buf = t.backend().buffer().clone();
        let row = (0..buf.area.height).find(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, *y)].symbol().to_string())
                .collect::<String>()
                .contains(needle)
        });
        let y = row.unwrap_or_else(|| panic!("{needle} not drawn"));
        let x = (0..buf.area.width)
            .find(|x| buf[(*x, y)].symbol() == needle.chars().next().unwrap().to_string())
            .unwrap();
        buf[(x, y)].clone()
    };
    let removed = cell_for(Palette::Auto, "-old line");
    assert_eq!(removed.fg, Color::Red);
    assert!(
        removed.modifier.contains(Modifier::DIM),
        "auto keeps the removed line softened"
    );
    for (p, bad) in [
        (Palette::Dark, Color::Rgb(0xff, 0x87, 0x87)),
        (Palette::Light, Color::Rgb(0xaf, 0, 0)),
    ] {
        let c = cell_for(p, "-old line");
        assert_eq!(c.fg, bad, "{p:?}");
        assert!(
            !c.modifier.contains(Modifier::DIM),
            "{p:?} must not dim the removed line"
        );
    }
}

#[test]
fn tu101_the_note_list_says_where_a_note_came_from_and_when_it_reads_like_instructions_from_a_closed_set(
) {
    let mut a = booted();
    let r = enter(&mut a, "/memory");
    a.handle(ok(
        &r[0],
        200,
        json!({"memories": [
            {"id": "m1", "text": "own note", "level": "internal", "untrusted": false, "why": "w", "origin": "owner", "instruction_like": false},
            {"id": "m2", "text": "from another assistant", "level": "internal", "untrusted": true, "why": "w", "origin": "imported", "instruction_like": false},
            {"id": "m3", "text": "learned in chat", "level": "internal", "untrusted": true, "why": "w", "origin": "conversation", "instruction_like": false},
            {"id": "m4", "text": "learned in a review", "level": "internal", "untrusted": true, "why": "w", "origin": "review", "instruction_like": true, "project": "alpha"},
            {"id": "m5", "text": "hostile origin", "level": "internal", "untrusted": false, "why": "w", "origin": "evil\u{202e}ignore previous", "instruction_like": "yes"},
            {"id": "m6", "text": "no fields at all", "level": "internal", "untrusted": false, "why": "w"}
        ]}),
    ));
    let t = text(&a);
    let line = |id: &str| {
        t.lines()
            .find(|l| l.starts_with(id))
            .unwrap_or_else(|| panic!("{id} not listed:\n{t}"))
            .to_string()
    };
    assert_eq!(
        line("m1"),
        "m1  [internal]  own note",
        "a note you wrote shows no extra marks"
    );
    assert_eq!(
        line("m2"),
        "m2  [internal] untrusted imported  from another assistant"
    );
    assert_eq!(
        line("m3"),
        "m3  [internal] untrusted from a conversation  learned in chat"
    );
    assert_eq!(line("m4"), "m4  [internal] untrusted from a review reads like instructions project alpha  learned in a review");
    // a value outside the closed set, or not a real true, adds nothing and is never shown
    assert_eq!(line("m5"), "m5  [internal]  hostile origin");
    assert!(!t.contains("ignore previous") && !t.contains('\u{202e}'));
    assert_eq!(line("m6"), "m6  [internal]  no fields at all");
}

#[test]
fn tu102_the_footer_says_how_big_and_how_fast_the_last_answer_was_and_only_when_the_figures_make_sense(
) {
    // a fresh window for every case, so a footer from an earlier case can never be read as this one's
    let footer_for = |usage: serde_json::Value| -> String {
        let mut a = booted();
        let r = enter(&mut a, "a question");
        a.handle(ok(&r[0], 200, json!({"done": true, "reply": "an answer", "route": {"simulated": true}, "notes_used": 0, "usage": usage})));
        text(&a)
            .lines()
            .find(|l| l.contains("answer: about"))
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let with = |elapsed: serde_json::Value, tokens: serde_json::Value| json!({"context_tokens_estimate": 100, "elapsed_ms": elapsed, "reply_tokens_estimate": tokens});
    // a normal answer: size, time, and a whole-answer speed
    assert!(footer_for(with(json!(3200), json!(140)))
        .ends_with("answer: about 140 tokens in 3.2 s (about 43 tokens a second overall)"));
    // exactly at the speed limit and just under it
    assert!(footer_for(with(json!(500), json!(10)))
        .ends_with("answer: about 10 tokens in 500 ms (about 20 tokens a second overall)"));
    assert!(
        footer_for(with(json!(499), json!(10))).ends_with("answer: about 10 tokens in 499 ms"),
        "too short for a speed to mean anything"
    );
    assert!(footer_for(with(json!(0), json!(5))).ends_with("answer: about 5 tokens in 0 ms"));
    // the switch from milliseconds to seconds is at exactly one second
    assert!(footer_for(with(json!(999), json!(10))).contains("in 999 ms"));
    assert!(footer_for(with(json!(1000), json!(10))).contains("in 1.0 s"));
    assert!(footer_for(with(json!(1500), json!(10)))
        .contains("in 1.5 s (about 6 tokens a second overall)"));
    // a very slow answer gives no speed rather than "0 tokens a second"
    assert!(
        footer_for(with(json!(600_000), json!(10))).ends_with("answer: about 10 tokens in 600.0 s")
    );
    // a reply with no length, a missing figure, or a figure that is not a whole number gives no line at all
    for (elapsed, tokens) in [
        (json!(1000), json!(0)),
        (json!(null), json!(10)),
        (json!(1000), json!(null)),
        (json!("1000"), json!(10)),
        (json!(1.5), json!(10)),
        (json!(-5), json!(10)),
        (json!(1000), json!(-1)),
        (json!([1000]), json!(10)),
    ] {
        assert_eq!(
            footer_for(with(elapsed.clone(), tokens.clone())),
            "",
            "{elapsed} {tokens}"
        );
    }
    // enormous figures neither overflow nor panic
    let big = footer_for(with(json!(u64::MAX), json!(u64::MAX)));
    assert!(
        big.starts_with("answer: about 18446744073709551615 tokens in "),
        "{big}"
    );
    let fast = footer_for(with(json!(1000), json!(u64::MAX)));
    assert!(fast.contains("tokens a second overall"), "{fast}");
    // an old service that sends no time still gets its context line, and no speed line
    let mut a = booted();
    let r = enter(&mut a, "once more");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "ok", "route": {"simulated": true}, "notes_used": 0, "usage": {"context_tokens_estimate": 77, "reply_tokens_estimate": 9}})));
    assert!(text(&a).contains("about 77 tokens of context sent"));
}

#[test]
fn tu103_two_reviews_running_at_once_each_keep_their_own_progress_and_the_header_follows_them() {
    let mut app = booted();
    let r = enter(&mut app, "/reviews");
    app.handle(ok(&r[0], 200, json!({"jobs": [{"id": "job-1", "recipient": "acme", "state": "running"}, {"id": "job-2", "recipient": "globex", "state": "running"}]})));
    assert!(
        app.status_line().contains("2 reviews") && app.status_line().contains("2 running"),
        "{}",
        app.status_line()
    );
    let ev = |stage: &str, detail: &str, state: &str, recipient: &str| json!({"state": state, "recipient": recipient, "note": "", "events": [{"stage": "queued", "detail": ""}, {"stage": stage, "detail": detail}]});
    // one tick asks about each running review, once each
    let t = app.handle(Event::Tick);
    let mut paths: Vec<_> = t.iter().map(|q| q.path.clone()).collect();
    paths.sort();
    assert_eq!(paths, ["/api/jobs/job-1", "/api/jobs/job-2"], "{paths:?}");
    for q in &t {
        if q.path.ends_with("job-1") {
            app.handle(ok(
                q,
                200,
                ev("import", "reading acme files", "running", "acme"),
            ));
        } else {
            app.handle(ok(
                q,
                200,
                ev("review", "agent working for globex", "running", "globex"),
            ));
        }
    }
    let all = text(&app);
    assert!(
        all.contains("job-1  import") && all.contains("reading acme files"),
        "{all}"
    );
    assert!(
        all.contains("job-2  review") && all.contains("agent working for globex"),
        "{all}"
    );
    assert!(
        !all.contains("job-1  review") && !all.contains("job-2  import"),
        "progress never lands on the other review: {all}"
    );
    // the first finishes: it leaves the running count and is polled no more, the second carries on
    let t = app.handle(Event::Tick);
    for q in &t {
        if q.path.ends_with("job-1") {
            app.handle(ok(q, 200, ev("finished", "", "ready", "acme")));
        } else {
            app.handle(ok(
                q,
                200,
                ev("review", "agent working for globex", "running", "globex"),
            ));
        }
    }
    assert!(text(&app).contains("job-1 finished: ready"));
    assert!(
        app.status_line().contains("1 running"),
        "{}",
        app.status_line()
    );
    let t = app.handle(Event::Tick);
    assert_eq!(
        t.iter().map(|q| q.path.as_str()).collect::<Vec<_>>(),
        ["/api/jobs/job-2"]
    );
    // the second fails: nothing is left running and the failure names the right review
    app.handle(ok(&t[0], 200, ev("finished", "", "failed", "globex")));
    assert!(
        text(&app).contains("job-2 finished: failed"),
        "{}",
        text(&app)
    );
    assert!(
        !app.status_line().contains("running"),
        "{}",
        app.status_line()
    );
    assert!(app.handle(Event::Tick).is_empty());
}

#[test]
fn tu104_a_review_whose_status_cannot_be_read_stops_being_polled_and_the_other_keeps_going() {
    let mut app = booted();
    let r = enter(&mut app, "/reviews");
    app.handle(ok(&r[0], 200, json!({"jobs": [{"id": "job-1", "recipient": "acme", "state": "running"}, {"id": "job-2", "recipient": "globex", "state": "running"}]})));
    let t = app.handle(Event::Tick);
    assert_eq!(t.len(), 2);
    for q in &t {
        if q.path.ends_with("job-1") {
            app.handle(ok(
                q,
                404,
                json!({"error": {"code": "NO_REVIEW", "message": "no such review"}}),
            ));
        } else {
            app.handle(ok(q, 200, json!({"state": "running", "recipient": "globex", "note": "", "events": [{"stage": "review", "detail": "agent working"}]})));
        }
    }
    assert!(
        text(&app).contains("job-1: no such review (404)"),
        "the failure is shown and names the review: {}",
        text(&app)
    );
    assert!(
        !text(&app).contains("job-2: "),
        "and does not name the other one"
    );
    assert!(
        app.status_line().contains("1 running"),
        "{}",
        app.status_line()
    );
    let t = app.handle(Event::Tick);
    assert_eq!(
        t.iter().map(|q| q.path.as_str()).collect::<Vec<_>>(),
        ["/api/jobs/job-2"],
        "the unreadable review is not polled again"
    );
}

#[test]
fn tu105_a_busy_answer_when_starting_a_review_is_shown_as_the_service_said_it_and_starts_nothing() {
    let mut app = booted();
    let r = enter(&mut app, "/review payments for acme-audit");
    let msg = "4 reviews are already running; wait for one to finish, then start this one again";
    app.handle(ok(
        &r[0],
        429,
        json!({"error": {"code": "BUSY", "message": msg}}),
    ));
    let all = text(&app);
    assert!(all.contains(&format!("{msg} (429)")), "{all}");
    assert!(
        !all.contains("Review job-") && !app.status_line().contains("running"),
        "{all}"
    );
    assert!(
        app.handle(Event::Tick).is_empty(),
        "nothing is polled for a review that never started"
    );
}

#[test]
fn tu106_model_alone_asks_the_service_which_models_are_installed_and_marks_the_one_in_use() {
    let mut app = booted();
    let r = enter(&mut app, "/model");
    assert_eq!(r.len(), 1);
    assert_eq!(
        (r[0].path.as_str(), r[0].body.clone()),
        ("/api/models", None)
    );
    app.handle(ok(
        &r[0],
        200,
        json!({"current": "qwen2.5:1.5b", "installed": ["qwen2.5:1.5b", "qwen2.5:3b"], "can_choose": true}),
    ));
    let all = text(&app);
    assert!(all.contains("Answering with: qwen2.5:1.5b"), "{all}");
    assert!(
        all.contains("* qwen2.5:1.5b") && all.contains("  qwen2.5:3b"),
        "{all}"
    );
    assert!(all.contains("/model NAME"), "{all}");
}

#[test]
fn tu107_when_there_is_nothing_to_choose_it_says_why_and_lists_nothing() {
    let mut app = booted();
    let r = enter(&mut app, "/model");
    app.handle(ok(
        &r[0],
        200,
        json!({"current": "only:1b", "installed": [], "can_choose": false}),
    ));
    let all = text(&app);
    assert!(
        all.contains("Answering with: only:1b") && all.contains("Nothing to choose"),
        "{all}"
    );
    assert!(!all.contains("Installed on this computer"), "{all}");
}

#[test]
fn tu108_a_named_model_is_sent_as_one_plain_name_and_the_answer_is_shown_or_the_refusal_as_said() {
    let mut app = booted();
    let r = enter(&mut app, "/model qwen2.5:3b");
    assert_eq!(r[0].path, "/api/model");
    assert_eq!(r[0].body, Some(json!({"model": "qwen2.5:3b"})));
    app.handle(ok(&r[0], 200, json!({"route": {"model": "qwen2.5:3b"}})));
    assert!(
        text(&app).contains("qwen2.5:3b will answer from now on"),
        "{}",
        text(&app)
    );
    let mut app = booted();
    let r = enter(&mut app, "/model llama3:70b");
    app.handle(ok(
        &r[0],
        400,
        json!({"error": {"code": "BAD_MODEL", "message": "that model is not installed on this computer"}}),
    ));
    let all = text(&app);
    assert!(
        all.contains("that model is not installed on this computer"),
        "{all}"
    );
    assert!(!all.contains("will answer from now on"), "{all}");
}

#[test]
fn tu109_a_name_that_is_not_plain_is_refused_before_anything_is_sent_and_a_hostile_reply_is_cleaned(
) {
    for bad in [
        "a b",
        "x;y",
        "$(id)",
        "`id`",
        "x|y",
        "m\u{202e}x",
        &format!("/model {}", "a".repeat(101)),
    ] {
        let mut app = booted();
        let line = if bad.starts_with("/model ") {
            bad.to_string()
        } else {
            format!("/model {bad}")
        };
        let r = enter(&mut app, &line);
        assert!(r.is_empty(), "{line:?} must not reach the service");
        assert!(
            text(&app).contains("usage: /model [NAME]"),
            "{line:?}: {}",
            text(&app)
        );
    }
    let mut app = booted();
    let r = enter(&mut app, "/model");
    app.handle(ok(
        &r[0],
        200,
        json!({"current": "ok\u{1b}[31m:1b", "installed": ["ok\u{1b}[31m:1b", "b\u{202e}:2b"], "can_choose": true}),
    ));
    let all = text(&app);
    assert!(
        !all.contains('\u{1b}') && !all.contains('\u{202e}'),
        "{all:?}"
    );
}

#[test]
fn tu110_model_is_in_the_command_list_with_help_and_a_usage_line() {
    assert!(cmd::NAMES.contains(&"model"));
    assert!(!cmd::describe("model").is_empty());
    assert_eq!(cmd::usage("model"), "/model [NAME]");
    assert_eq!(cmd::parse("/model").unwrap(), Command::Model(None));
    assert_eq!(
        cmd::parse("/model m1").unwrap(),
        Command::Model(Some("m1".into()))
    );
}

#[test]
fn tu111_an_empty_model_reply_to_a_document_question_is_said_in_words_and_never_shown_raw() {
    let mut a = booted();
    let r = enter(&mut a, "/ask /x : What is the notice period?");
    a.handle(ok(
        &r[0],
        200,
        json!({"checkable": false, "no_answer": true, "unchecked_text": "{\"answer\": \"\", \"citations\": []}",
               "files_used": [], "files_withheld": [], "files_skipped": [], "injection_suspects": []}),
    ));
    let text = a
        .cards
        .iter()
        .flat_map(|c| c.lines.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("The model gave no answer, even when asked again with fewer passages"),
        "{text}"
    );
    assert!(
        !text.contains("citations") && !text.contains("trust none of it"),
        "{text}"
    );
    let r2 = enter(&mut a, "/ask /x : second");
    a.handle(ok(
        &r2[0],
        200,
        json!({"checkable": false, "no_answer": false, "unchecked_text": "free text",
               "files_used": [], "files_withheld": [], "files_skipped": [], "injection_suspects": []}),
    ));
    let text = a
        .cards
        .iter()
        .flat_map(|c| c.lines.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("did not give a checkable answer") && text.contains("free text"),
        "{text}"
    );
}

fn code_ready(digest: &str) -> Value {
    json!({
        "id": "code-1", "state": "ready", "progress": ["the model works on 2 files"],
        "ended": "the model said it was done", "summary": "fixed the discount",
        "tests": {"passed": true, "output": "2 passed, 0 failed"}, "test_command": "python3",
        "concerns": [], "can_apply": true, "digest": digest,
        "changes": [{"path": "pricing.py", "kind": "modified", "added": 1, "removed": 1,
            "diff": "--- a/pricing.py\n+++ b/pricing.py\n@@ -1,2 +1,2 @@\n def f(p, x):\n-    return p - x\n+    return p * (100 - x) / 100\n"}]
    })
}

#[test]
fn tu_code01_a_task_is_followed_to_its_patch_and_applied_only_after_yes_with_the_digest_shown() {
    let mut app = booted();
    let r = enter(&mut app, "/code /home/u/projects/shop : fix the discount");
    assert_eq!(
        (r.len(), r[0].path.as_str(), r[0].method),
        (1, "/api/code", Method::Post)
    );
    assert_eq!(r[0].body.as_ref().unwrap()["task"], "fix the discount");
    app.handle(ok(&r[0], 202, json!({"id": "code-1"})));
    let poll = app.handle(Event::Tick);
    let p = poll
        .iter()
        .find(|q| q.path == "/api/code/code-1")
        .expect("the task is polled");
    app.handle(ok(
        p,
        200,
        json!({"id": "code-1", "state": "running", "progress": ["the model works on 2 files"]}),
    ));
    assert!(text(&app).contains("the model works on 2 files"));
    let poll = app.handle(Event::Tick);
    let p = poll.iter().find(|q| q.path == "/api/code/code-1").unwrap();
    app.handle(ok(p, 200, code_ready(&"a".repeat(64))));
    let t = text(&app);
    assert!(
        t.contains("Tests pass on the changed files")
            && t.contains("return p * (100 - x) / 100")
            && t.contains("/code apply"),
        "{t}"
    );
    assert_eq!(
        t.matches("the model works on 2 files").count(),
        1,
        "a progress line is shown once"
    );
    assert!(
        app.handle(Event::Tick)
            .iter()
            .all(|q| !q.path.starts_with("/api/code")),
        "a finished task is not polled again"
    );
    // no is the default: anything but y changes nothing
    assert!(enter(&mut app, "/code apply").is_empty());
    assert!(app.handle(key(KeyCode::Char('n'))).is_empty());
    assert!(text(&app).contains("Nothing on disk was changed"));
    enter(&mut app, "/code apply");
    let a = app.handle(key(KeyCode::Char('y')));
    assert_eq!(
        (
            a[0].path.as_str(),
            a[0].body.as_ref().unwrap()["digest"].as_str().unwrap()
        ),
        ("/api/code/code-1/apply", "a".repeat(64).as_str())
    );
    app.handle(ok(&a[0], 200, json!({"applied": true, "files": 1})));
    assert!(text(&app).contains("Applied code-1"));
    enter(&mut app, "/code undo");
    let u = app.handle(key(KeyCode::Char('y')));
    assert_eq!(u[0].path, "/api/code/code-1/undo");
}

#[test]
fn tu_code02_apply_needs_a_patch_read_on_this_screen_and_bad_forms_are_refused() {
    let mut app = booted();
    assert!(enter(&mut app, "/code apply").is_empty());
    assert!(text(&app).contains("No coding task yet"));
    assert!(
        enter(&mut app, "/code apply code-7").is_empty(),
        "a task whose patch was not shown here cannot be applied"
    );
    assert!(text(&app).contains("Read the patch first"));
    for bad in [
        "/code",
        "/code fix it",
        "/code apply code-x",
        "/code show 7",
        "/code : task",
        "/code apply code-1 extra",
    ] {
        assert!(cmd::parse(bad).is_err(), "{bad}");
    }
    assert!(matches!(
        cmd::parse("/code show code-3"),
        Ok(Command::CodeAct("show", Some(_)))
    ));
    // a patch that cannot be applied gives no way to apply it
    let r = enter(&mut app, "/code show code-2");
    app.handle(ok(&r[0], 200, json!({"id": "code-2", "state": "ready", "can_apply": false, "changes": [], "concerns": ["pricing.py adds what looks like a secret; this patch will not be applied"], "tests": null})));
    let t = text(&app);
    assert!(
        t.contains("cannot be applied") && t.contains("looks like a secret"),
        "{t}"
    );
    assert!(enter(&mut app, "/code apply code-2").is_empty());
}

#[test]
fn tu_ns01_the_terminal_turns_a_projects_notes_only_mode_on_and_off_and_lists_it() {
    use kk_tui::cmd::NotesOnlyAction as N;
    assert_eq!(
        cmd::parse("/notesonly").unwrap(),
        Command::NotesOnly(N::Show)
    );
    assert_eq!(
        cmd::parse("/notesonly on").unwrap(),
        Command::NotesOnly(N::Set {
            project: None,
            on: true
        })
    );
    assert_eq!(
        cmd::parse("/notesonly off kestrel").unwrap(),
        Command::NotesOnly(N::Set {
            project: Some("kestrel".into()),
            on: false
        })
    );
    for bad in [
        "/notesonly maybe",
        "/notesonly on a/b",
        "/notesonly on ..",
        "/notesonly on a b",
        "/notesonly off \u{7}",
        "/notesonly kestrel",
    ] {
        assert!(cmd::parse(bad).is_err(), "{bad}");
    }
    // list
    let mut a = booted();
    let r = enter(&mut a, "/notesonly");
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/project-settings", Method::Get)
    );
    a.handle(ok(&r[0], 200, json!({"projects": [{"project": "kestrel", "notes_only": true, "updated_at": 5}], "max": 30})));
    assert!(text(&a).contains("kestrel"), "{}", text(&a));
    let mut a = booted();
    let r = enter(&mut a, "/notesonly");
    a.handle(ok(&r[0], 200, json!({"projects": [], "max": 30})));
    assert!(text(&a).contains("No project answers only"), "{}", text(&a));
    // on with a name; on without one needs the conversation's project
    let mut a = booted();
    let r = enter(&mut a, "/notesonly on");
    assert!(r.is_empty(), "no project named and none set");
    assert!(text(&a).contains("Name a project"), "{}", text(&a));
    let r = enter(&mut a, "/project kestrel");
    assert!(r.is_empty());
    let r = enter(&mut a, "/notesonly on");
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/project-settings", Method::Post)
    );
    assert_eq!(
        r[0].body,
        Some(json!({"project": "kestrel", "notes_only": true}))
    );
    a.handle(ok(
        &r[0],
        200,
        json!({"project": "kestrel", "notes_only": true}),
    ));
    assert!(text(&a).contains("On for kestrel"), "{}", text(&a));
    let r = enter(&mut a, "/notesonly off other");
    assert_eq!(
        r[0].body,
        Some(json!({"project": "other", "notes_only": false}))
    );
    a.handle(ok(
        &r[0],
        200,
        json!({"project": "other", "notes_only": false}),
    ));
    assert!(text(&a).contains("Off for other"), "{}", text(&a));
    // a refusal is shown
    let r = enter(&mut a, "/notesonly on third");
    a.handle(ok(&r[0], 422, json!({"error": {"code": "PROJECT_REFUSED", "message": "at most 30 projects can have their own settings"}})));
    assert!(text(&a).contains("at most 30 projects"), "{}", text(&a));
}

#[test]
fn tu_ns02_a_reply_where_no_model_was_asked_says_so_and_shows_no_model_figures() {
    let mut app = booted();
    let r = enter(&mut app, "what is the rounding rule");
    app.handle(ok(&r[0], 200, json!({"reply": "Nothing in this project's notes or attached files answers that.", "route": {"id": "none", "class": "none", "simulated": false}, "notes_used": 0, "project": "kestrel",
        "usage": {"context_tokens_estimate": 220, "context_tokens": 220, "estimate": true, "context_window": 16384, "reply_tokens_estimate": 49, "elapsed_ms": 23}})));
    let t = text(&app);
    assert!(t.contains("no model was asked"), "{t}");
    assert!(
        !t.contains("private, on this computer") && !t.contains("tokens of context"),
        "{t}"
    );
    // an ordinary reply is unchanged
    let r = enter(&mut app, "another question");
    app.handle(ok(
        &r[0],
        200,
        json!({"reply": "ok", "route": {"id": "local", "simulated": false}, "notes_used": 1}),
    ));
    assert!(text(&app).contains("private, on this computer"));
}

fn at_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kk-at-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("docs")).unwrap();
    for f in [
        "a.txt",
        "b.md",
        "docs/c.txt",
        "d.txt",
        "e.txt",
        "f.txt",
        "g.txt",
    ] {
        std::fs::write(dir.join(f), "x").unwrap();
    }
    dir
}

#[test]
fn tu_at01_files_named_with_an_at_sign_are_found_and_anything_else_is_left_as_text() {
    use kk_tui::app::mentioned_files;
    let dir = at_dir("find");
    let names = |m: &str| -> Vec<String> {
        mentioned_files(m, &dir)
            .into_iter()
            .map(|(t, _)| t)
            .collect()
    };
    assert_eq!(names("summarise @a.txt please"), ["a.txt"]);
    assert_eq!(names("@a.txt and @docs/c.txt"), ["a.txt", "docs/c.txt"]);
    // closing punctuation is not part of the name; an @ inside a word or after a bracket is not a mention
    assert_eq!(names("then @b.md."), ["b.md"]);
    assert_eq!(names("then @b.md, and @a.txt!"), ["b.md", "a.txt"]);
    assert!(names("read (@a.txt) and x@b.md and a@a.txt").is_empty());
    // not a file here: a person, a folder, a missing name, an empty @, an @ alone
    assert!(names("ask @ada about @docs and @missing.txt and @ and @@a.txt").is_empty());
    // each once, and at most five
    assert_eq!(names("@a.txt @a.txt"), ["a.txt"]);
    assert_eq!(
        names("@a.txt @b.md @docs/c.txt @d.txt @e.txt @f.txt @g.txt").len(),
        5
    );
    // an absolute path works; a path with a control character does not
    let abs = dir.join("a.txt");
    assert_eq!(
        names(&format!("see @{}", abs.display())),
        [abs.to_string_lossy().to_string()]
    );
    assert!(names("see @a.txt\u{7}").is_empty());
}

#[test]
fn tu_at02_a_message_with_files_attaches_them_first_and_sends_the_ids_with_the_question() {
    let dir = at_dir("flow");
    let (a, b) = (dir.join("a.txt"), dir.join("b.md"));
    let mut app = booted();
    let msg = format!("compare @{} with @{}", a.display(), b.display());
    let r = enter(&mut app, &msg);
    assert_eq!(r.len(), 1);
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/attachments/path", Method::Post)
    );
    assert_eq!(r[0].body, Some(json!({"path": a.to_string_lossy()})));
    // while it waits, another message is queued and nothing else is sent
    assert!(enter(&mut app, "and another thing").is_empty());
    let r2 = app.handle(ok(
        &r[0],
        201,
        json!({"id": "att-1", "name": "a.txt", "kind_label": "text file"}),
    ));
    assert_eq!(r2[0].path, "/api/attachments/path");
    assert_eq!(r2[0].body, Some(json!({"path": b.to_string_lossy()})));
    let r3 = app.handle(ok(
        &r2[0],
        201,
        json!({"id": "att-2", "name": "b.md", "kind_label": "text file"}),
    ));
    assert_eq!(r3[0].path, "/api/chat/stream");
    let body = r3[0].body.as_ref().unwrap();
    assert_eq!(body["message"], msg.as_str());
    assert_eq!(body["attachments"], json!(["att-1", "att-2"]));
    assert!(
        text(&app).contains("Attached a.txt") && text(&app).contains("Attached b.md"),
        "{}",
        text(&app)
    );
    // the answer says how many files were read, and the queued message follows
    let next = app.handle(ok(&r3[0], 200, json!({"reply": "They differ.", "route": {"id": "local", "simulated": false}, "notes_used": 0,
        "files": {"used": 2, "trimmed": 1, "left_out": 0, "withheld_instructions": 1, "withheld_secrets": 1}})));
    let t = text(&app);
    assert!(
        t.contains("2 attached files read")
            && t.contains("1 attached file cut short")
            && t.contains("2 passages of attached files were not given"),
        "{t}"
    );
    assert_eq!(
        next[0].body.as_ref().unwrap()["message"],
        "and another thing"
    );
    assert!(next[0].body.as_ref().unwrap().get("attachments").is_none());
    // a plain message has no attachments field
    let mut plain = booted();
    let r = enter(&mut plain, "no files here, only @nobody");
    assert_eq!(r[0].path, "/api/chat/stream");
    assert!(r[0].body.as_ref().unwrap().get("attachments").is_none());
}

#[test]
fn tu_at03_a_file_that_cannot_be_attached_stops_the_message_and_gives_it_back() {
    let dir = at_dir("fail");
    let a = dir.join("a.txt");
    let mut app = booted();
    let msg = format!("read @{} now", a.display());
    let r = enter(&mut app, &msg);
    assert!(enter(&mut app, "queued one").is_empty());
    let out = app.handle(ok(&r[0], 403, json!({"error": {"code": "SECRET_FILE", "message": "that file is kept out of conversations: its name says it holds secrets or keys"}})));
    assert!(out.is_empty(), "nothing is sent after a refusal");
    let t = text(&app);
    assert!(
        t.contains("Could not attach")
            && t.contains("kept out of conversations")
            && t.contains("1 queued message"),
        "{t}"
    );
    assert_eq!(app.input, msg, "the message is back in the box");
    // and the next message goes out normally
    app.input.clear();
    let r = enter(&mut app, "hello");
    assert_eq!(r[0].path, "/api/chat/stream");
}

fn listing(dirs: &[&str], files: &[&str]) -> Value {
    json!({"path": "/x", "parent": null,
        "dirs": dirs.iter().map(|n| json!({"name": n, "path": format!("/x/{n}")})).collect::<Vec<_>>(),
        "files": files.iter().map(|n| json!({"name": n, "path": format!("/x/{n}"), "bytes": 10})).collect::<Vec<_>>()})
}

#[test]
fn tu_at04_tab_after_an_at_sign_finishes_a_file_or_folder_name_from_the_services_listing() {
    let cwd = std::env::current_dir().unwrap();
    let here = format!("{}/", cwd.to_string_lossy().trim_end_matches('/'));
    // one file completes with a space after it so the sentence goes on
    let mut app = booted();
    typed(&mut app, "look at @ter");
    let r = tab_and_ask(&mut app).expect("asks the service");
    assert_eq!(
        r.path,
        format!(
            "/api/fs?path={}&files=1",
            kk_tui::app::percent_encode(&here)
        )
    );
    app.handle(ok(&r, 200, listing(&["tests"], &["terms.txt", "notes.md"])));
    assert_eq!(app.input, "look at @terms.txt ");
    // a folder completes with a slash
    let mut app = booted();
    typed(&mut app, "see @te");
    let r = tab_and_ask(&mut app).unwrap();
    app.handle(ok(&r, 200, listing(&["tests"], &["notes.md"])));
    assert_eq!(app.input, "see @tests/");
    // several fill in what they share and are listed, folders with a slash
    let mut app = booted();
    typed(&mut app, "@re");
    let r = tab_and_ask(&mut app).unwrap();
    app.handle(ok(
        &r,
        200,
        listing(&["reports"], &["readme.md", "release.txt"]),
    ));
    assert_eq!(app.input, "@re");
    let t = text(&app);
    assert!(
        t.contains("reports/") && t.contains("readme.md") && t.contains("release.txt"),
        "{t}"
    );
    let mut app = booted();
    typed(&mut app, "@rea");
    let r = tab_and_ask(&mut app).unwrap();
    app.handle(ok(&r, 200, listing(&[], &["readme.md", "ready.txt"])));
    assert_eq!(app.input, "@read");
    // deeper levels ask for that folder; an absolute path is asked for as written
    let mut app = booted();
    typed(&mut app, "x @docs/c");
    let r = tab_and_ask(&mut app).unwrap();
    assert!(
        r.path.contains("docs%2F") || r.path.contains("docs/"),
        "{}",
        r.path
    );
    app.handle(ok(&r, 200, listing(&[], &["c.txt"])));
    assert_eq!(app.input, "x @docs/c.txt ");
    let mut app = booted();
    typed(&mut app, "x @/home/u/p/a");
    let r = tab_and_ask(&mut app).unwrap();
    assert!(
        r.path.starts_with("/api/fs?path=")
            && r.path.contains("home")
            && r.path.ends_with("&files=1"),
        "{}",
        r.path
    );
}

#[test]
fn tu_at05_at_completion_is_asked_only_for_a_real_mention_and_hostile_listings_change_nothing() {
    // not inside a word, not when the token has a space, not for a plain line, and /review keeps its own completion
    for line in ["mail x@te", "a @te b", "te", "(@te"] {
        let mut app = booted();
        typed(&mut app, line);
        assert!(tab_and_ask(&mut app).is_none(), "{line:?}");
    }
    let mut app = booted();
    typed(&mut app, "/review pay");
    let r = tab_and_ask(&mut app).unwrap();
    assert!(
        !r.path.contains("files=1"),
        "folder completion must not list files: {}",
        r.path
    );
    // names with spaces, escapes or no match are never put into the line
    for names in [
        listing(&[], &["a b.txt", "te\u{1b}[31m.txt"]),
        listing(&[], &["other.txt"]),
        listing(&[], &[]),
        json!({"error": {"message": "no"}}),
    ] {
        let mut app = booted();
        typed(&mut app, "q @te");
        let r = tab_and_ask(&mut app).unwrap();
        app.handle(ok(&r, 200, names));
        assert_eq!(app.input, "q @te");
    }
    // an answer that arrives after the line changed is ignored
    let mut app = booted();
    typed(&mut app, "q @te");
    let r = tab_and_ask(&mut app).unwrap();
    typed(&mut app, "x");
    app.handle(ok(&r, 200, listing(&[], &["terms.txt"])));
    assert_eq!(app.input, "q @tex");
}

#[test]
fn tu_br01_branch_pick_lists_every_message_and_choosing_one_branches_after_it() {
    assert_eq!(cmd::parse("/branch pick").unwrap(), Command::BranchPick);
    assert_eq!(cmd::parse("/branch PICK").unwrap(), Command::BranchPick);
    assert_eq!(cmd::parse("/branch").unwrap(), Command::Branch(None));
    assert_eq!(cmd::parse("/branch 3").unwrap(), Command::Branch(Some(3)));
    assert!(cmd::parse("/branch pick 3").is_err());
    // nothing kept, or nothing written: a sentence, no list
    let mut a = booted();
    assert!(
        enter(&mut a, "/branch pick").is_empty()
            && text(&a).contains("Only a kept conversation can be branched")
    );
    assert!(!matches!(a.mode, Mode::Picker(_)));
    a.session = Some("chat-1".into());
    assert!(
        enter(&mut a, "/branch pick").is_empty()
            && text(&a).contains("no messages to branch after")
    );
    a.chat_history = vec![
        ("user".into(), "first question".into()),
        ("assistant".into(), "first answer".into()),
        ("user".into(), "second question".into()),
        (
            "assistant".into(),
            format!("second answer {}", "long ".repeat(40)),
        ),
    ];
    enter(&mut a, "/branch pick");
    // every message, newest first, numbered as /branch N numbers them
    assert_eq!(picker_rows(&a), ["4", "3", "2", "1"]);
    let (lines, _) = render(&a, 110, 24, false);
    let screen = lines.join("\n");
    assert!(
        screen.contains("Branch the conversation after one of its messages")
            && screen.contains("you: first question")
            && screen.contains("assistant: first answer")
            && screen.contains("…"),
        "{screen}"
    );
    let row = |needle: &str| {
        lines
            .iter()
            .find(|l| l.contains(needle))
            .cloned()
            .unwrap_or_default()
    };
    assert!(
        row("assistant: first answer").contains("keeps 2 messages"),
        "{screen}"
    );
    assert!(
        row("you: first question").contains("keeps 1 message"),
        "{screen}"
    );
    // choosing the second one branches keeping the first two messages
    a.handle(key(KeyCode::Down));
    a.handle(key(KeyCode::Down));
    let r = a.handle(key(KeyCode::Enter));
    assert!(!matches!(a.mode, Mode::Picker(_)));
    assert_eq!(
        (r[0].method, r[0].path.as_str()),
        (Method::Post, "/api/sessions/chat-1/fork")
    );
    assert_eq!(r[0].body.as_ref().unwrap(), &json!({"at": 2}));
    // nothing is forgotten from this list
    enter(&mut a, "/branch pick");
    assert!(a.handle(ctrl('d')).is_empty());
    assert!(render(&a, 110, 24, false)
        .0
        .join("\n")
        .contains("Enter branches after the chosen one"));
}

#[test]
fn tu_at06_a_file_can_be_named_with_the_lines_wanted_and_only_those_are_asked_for() {
    use kk_tui::app::{mentioned_files, split_lines_suffix};
    assert_eq!(split_lines_suffix("a.txt#3-5"), ("a.txt", Some("3-5")));
    assert_eq!(split_lines_suffix("a.txt#7"), ("a.txt", Some("7")));
    assert_eq!(split_lines_suffix("a.txt"), ("a.txt", None));
    // not a range: left as part of the name
    for t in [
        "a.txt#",
        "a.txt#x",
        "a.txt#-3",
        "#3",
        "a#b#c",
        "a.txt#3x",
        "a.txt#3.5",
    ] {
        assert_eq!(split_lines_suffix(t).1, None, "{t}");
    }
    assert_eq!(split_lines_suffix("a#b#3").0, "a#b");
    let dir = at_dir("lines");
    let names = |m: &str| -> Vec<(String, String)> { mentioned_files(m, &dir) };
    let got = names("see @a.txt#3-5 and @b.md#2");
    assert_eq!(got[0].0, "a.txt#3-5");
    assert!(
        got[0].1.ends_with("a.txt#3-5") && got[1].1.ends_with("b.md#2"),
        "{got:?}"
    );
    // a range on a file that is not there is not a mention; the same file with two ranges is two selections
    assert!(names("@nothing.txt#3").is_empty());
    assert_eq!(names("@a.txt#1-2 @a.txt#4-5").len(), 2);
    // the request carries the file and the lines apart
    let mut app = booted();
    let msg = format!("read @{}#3-5 now", dir.join("a.txt").display());
    let r = enter(&mut app, &msg);
    assert_eq!(r[0].path, "/api/attachments/path");
    assert_eq!(
        r[0].body,
        Some(json!({"path": dir.join("a.txt").to_string_lossy(), "lines": "3-5"}))
    );
    let r2 = app.handle(ok(
        &r[0],
        201,
        json!({"id": "att-1", "name": "a.lines3-5.txt", "kind_label": "text file"}),
    ));
    assert_eq!(r2[0].path, "/api/chat/stream");
    assert_eq!(
        r2[0].body.as_ref().unwrap()["attachments"],
        json!(["att-1"])
    );
    // a refused range stops the message like any refusal
    let mut app = booted();
    let r = enter(&mut app, &msg);
    app.handle(ok(
        &r[0],
        400,
        json!({"error": {"code": "BAD_LINES", "message": "the file has 2 lines"}}),
    ));
    assert!(
        text(&app).contains("the file has 2 lines"),
        "{}",
        text(&app)
    );
    assert_eq!(app.input, msg);
}

#[test]
fn tu_fc01_tab_finishes_the_folder_after_ask_and_code_as_it_does_after_review() {
    for (cmd, line, head_after) in [
        ("/ask", "/ask we", "/ask web/"),
        ("/code", "/code we", "/code web/"),
        ("/ask", "/ask ./payments-api/s", "/ask ./payments-api/s"),
    ] {
        let mut app = booted();
        typed(&mut app, line);
        let r =
            tab_and_ask(&mut app).unwrap_or_else(|| panic!("{cmd}: nothing asked for {line:?}"));
        if line.contains("payments-api") {
            assert_eq!(r.path, "/api/fs?path=/home/u/projects/payments-api/");
            app.handle(ok(&r, 200, dirs(&["src", "scripts"])));
        } else {
            assert_eq!(r.path, "/api/fs?path=/home/u/projects/", "{cmd}");
            app.handle(ok(&r, 200, dirs(&["payments", "web"])));
        }
        assert_eq!(app.input, head_after, "{line:?}");
    }
    // a folder completion never offers files, even if the answer holds some
    for line in ["/review pa", "/ask pa", "/code pa"] {
        let mut app = booted();
        typed(&mut app, line);
        let r = tab_and_ask(&mut app).unwrap();
        assert!(!r.path.contains("files=1"), "{line:?}: {}", r.path);
        app.handle(ok(&r, 200, listing(&[], &["payments.txt"])));
        assert_eq!(app.input, line, "a file was offered for a folder: {line:?}");
        assert!(!text(&app).contains("payments.txt"), "{}", text(&app));
    }
    // a shared start is filled in after /code too, with the command kept
    let mut app = booted();
    typed(&mut app, "/code p");
    let r = tab_and_ask(&mut app).unwrap();
    app.handle(ok(&r, 200, dirs(&["payments-api", "payments-web"])));
    assert_eq!(app.input, "/code payments-");
    // once the folder is finished (a colon follows) or for /code's own subcommands' ids, nothing more is asked
    for line in [
        "/ask web/ : what",
        "/ask web :",
        "/code web/ :",
        "/code show",
    ] {
        let mut app = booted();
        typed(&mut app, line);
        let asked = tab_and_ask(&mut app);
        if line == "/code show" {
            // "show" is a word that may start a folder name: asking is harmless, and a folder is only ever filled in on a match
            if let Some(r) = asked {
                app.handle(ok(&r, 200, dirs(&["payments"])));
                assert_eq!(app.input, "/code show");
            }
        } else {
            assert!(asked.is_none(), "{line:?}");
        }
    }
}

#[test]
fn tu_me01_method_commands_parse_and_bad_forms_are_refused() {
    use kk_tui::cmd::MethodAction as M;
    assert_eq!(cmd::parse("/method").unwrap(), Command::Method(M::List));
    assert_eq!(
        cmd::parse("/method show review-style").unwrap(),
        Command::Method(M::Show("review-style".into()))
    );
    assert_eq!(
        cmd::parse("/method approve a.b_c").unwrap(),
        Command::Method(M::Approve("a.b_c".into()))
    );
    assert_eq!(
        cmd::parse("/method revoke x").unwrap(),
        Command::Method(M::Revoke("x".into()))
    );
    assert_eq!(
        cmd::parse("/method remove x").unwrap(),
        Command::Method(M::Remove("x".into()))
    );
    assert_eq!(
        cmd::parse("/method add style review,findings Be brief and plain.").unwrap(),
        Command::Method(M::Add {
            name: "style".into(),
            triggers: vec!["review".into(), "findings".into()],
            text: "Be brief and plain.".into()
        })
    );
    assert_eq!(
        cmd::parse("/method add only-name - Used only by name.").unwrap(),
        Command::Method(M::Add {
            name: "only-name".into(),
            triggers: vec![],
            text: "Used only by name.".into()
        })
    );
    for bad in [
        "/method show",
        "/method show a/b",
        "/method show a b",
        "/method approve",
        "/method frobnicate x",
        "/method add",
        "/method add a",
        "/method add a words",
        "/method add a/b x text",
        "/method show \u{7}x",
    ] {
        assert!(cmd::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn tu_me02_the_owner_must_read_a_method_before_approving_and_the_approval_sends_the_hash_they_read()
{
    let mut a = booted();
    let r = enter(&mut a, "/method approve style");
    assert!(
        r.is_empty(),
        "nothing is sent for a method that was never shown"
    );
    assert!(
        text(&a).contains("Read it first with /method show style"),
        "{}",
        text(&a)
    );
    let r = enter(&mut a, "/method show style");
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/methods", Method::Get)
    );
    a.handle(ok(&r[0], 200, json!({"methods": [{"name": "style", "description": "", "triggers": ["review"], "avoid": [], "body": "Be brief.\nUse bullets.", "hash": "ab".repeat(32), "approved": false}]})));
    let t = text(&a);
    assert!(
        t.contains("Be brief.") && t.contains("Use bullets.") && t.contains("Waiting for you"),
        "{t}"
    );
    let r = enter(&mut a, "/method approve style");
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/methods/approve", Method::Post)
    );
    assert_eq!(r[0].body.as_ref().unwrap()["hash"], json!("ab".repeat(32)));
    assert_eq!(r[0].body.as_ref().unwrap()["name"], json!("style"));
    a.handle(ok(&r[0], 200, json!({"approved": true})));
    assert!(
        text(&a).contains("Approved: it is used from now on."),
        "{}",
        text(&a)
    );
    // the text changed after it was shown: the service refuses and the terminal says to read it again
    let r = enter(&mut a, "/method approve style");
    a.handle(ok(
        &r[0],
        409,
        json!({"error": {"code": "METHOD_CHANGED", "message": "x"}}),
    ));
    assert!(
        text(&a).contains("changed after you read it"),
        "{}",
        text(&a)
    );
    // removing forgets what was shown
    let r = enter(&mut a, "/method remove style");
    a.handle(ok(&r[0], 200, json!({"removed": true})));
    let r = enter(&mut a, "/method approve style");
    assert!(r.is_empty());
}

#[test]
fn tu_me03_the_list_shows_state_and_cleans_names_and_a_secret_is_scrubbed_from_history() {
    let mut a = booted();
    let r = enter(&mut a, "/method");
    a.handle(ok(
        &r[0],
        200,
        json!({"methods": [
            {"name": "style", "triggers": ["review", "findings"], "approved": true},
            {"name": "late\u{1b}[31m", "triggers": [], "approved": false},
        {"name": "tricky", "triggers": ["red\u{1b}[31mword"], "approved": false}
        ]}),
    ));
    let t = text(&a);
    assert!(
        t.contains("style") && t.contains("approved") && t.contains("review, findings"),
        "{t}"
    );
    assert!(
        t.contains("waiting for you") && t.contains("(only /name)"),
        "{t}"
    );
    assert!(!t.contains('\u{1b}'), "control bytes are cleaned");
    let mut a = booted();
    let r = enter(&mut a, "/method");
    a.handle(ok(&r[0], 200, json!({"methods": []})));
    assert!(text(&a).contains("No methods yet"), "{}", text(&a));
    let mut a = booted();
    let key = format!("{}{}", "sk_li", "ve_abcdefghijklmnop0123");
    let r = enter(
        &mut a,
        &format!("/method add style review Use this key {key}"),
    );
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/methods", Method::Post)
    );
    assert_eq!(
        r[0].body.as_ref().unwrap()["body"],
        json!(format!("Use this key {key}"))
    );
    a.handle(ok(&r[0], 422, json!({"error": {"code": "METHOD_REFUSED", "message": "this looks like a secret; methods are sent to the model, so keep secrets out of them"}})));
    assert!(text(&a).contains("looks like a secret"), "{}", text(&a));
    assert!(
        !text(&a).contains(&key),
        "the secret is not left in the transcript: {}",
        text(&a)
    );
    assert!(text(&a).contains("[text hidden"), "{}", text(&a));
}

#[test]
fn tu_mc01_mcp_commands_parse_and_bad_forms_are_refused() {
    use kk_tui::cmd::McpAction as M;
    assert_eq!(cmd::parse("/mcp").unwrap(), Command::Mcp(M::List));
    assert_eq!(
        cmd::parse("/mcp check docs").unwrap(),
        Command::Mcp(M::Check("docs".into()))
    );
    assert_eq!(
        cmd::parse("/mcp allow docs lookup").unwrap(),
        Command::Mcp(M::Allow("docs".into(), "lookup".into(), false))
    );
    assert_eq!(
        cmd::parse("/mcp allow docs lookup knowingly").unwrap(),
        Command::Mcp(M::Allow("docs".into(), "lookup".into(), true))
    );
    assert!(cmd::parse("/mcp allow docs lookup sure").is_err());
    assert_eq!(
        cmd::parse("/mcp off docs lookup").unwrap(),
        Command::Mcp(M::Off("docs".into(), "lookup".into()))
    );
    assert_eq!(
        cmd::parse("/mcp remove docs").unwrap(),
        Command::Mcp(M::Remove("docs".into()))
    );
    assert_eq!(
        cmd::parse("/mcp add docs /usr/bin/node server.js --flag").unwrap(),
        Command::Mcp(M::Add {
            name: "docs".into(),
            program: "/usr/bin/node".into(),
            args: vec!["server.js".into(), "--flag".into()]
        })
    );
    for bad in [
        "/mcp check",
        "/mcp allow docs",
        "/mcp allow a/b t",
        "/mcp add docs node",
        "/mcp add docs",
        "/mcp add a b/c /x",
        "/mcp frob x",
        "/mcp remove a b",
        "/mcp check \u{7}x",
    ] {
        assert!(cmd::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn tu_mc02_a_tool_is_allowed_only_with_the_pin_the_list_showed() {
    let mut a = booted();
    let r = enter(&mut a, "/mcp allow docs lookup");
    assert!(r.is_empty());
    assert!(text(&a).contains("Look at the list first"), "{}", text(&a));
    let r = enter(&mut a, "/mcp");
    assert_eq!((r[0].path.as_str(), r[0].method), ("/api/mcp", Method::Get));
    a.handle(ok(&r[0], 200, json!({"servers": [{"name": "docs", "program": "/usr/bin/x\u{1b}[2J", "args": [], "tools": [
        {"name": "lookup", "description": "Looks up a term\u{1b}[31m", "pin": "cd".repeat(32), "mode": "off", "effect": "may_change"}]}]})));
    let t = text(&a);
    assert!(
        t.contains("docs") && t.contains("lookup") && t.contains("off") && !t.contains('\u{1b}'),
        "{t}"
    );
    assert!(t.contains("server says: can change things"), "{t}");
    let r = enter(&mut a, "/mcp allow docs lookup");
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/mcp/tools", Method::Post)
    );
    let body = r[0].body.as_ref().unwrap();
    assert_eq!(
        (body["mode"].clone(), body["pin"].clone()),
        (json!("allow"), json!("cd".repeat(32)))
    );
    assert_eq!(body["knowing"], json!(false), "knowingly was not written");
    a.handle(ok(&r[0], 200, json!({"mode": "allow"})));
    assert!(
        text(&a).contains("Allowed: a coding task may call it."),
        "{}",
        text(&a)
    );
    let r = enter(&mut a, "/mcp allow docs lookup");
    a.handle(ok(
        &r[0],
        409,
        json!({"error": {"code": "MCP_TOOL_CHANGED", "message": "x"}}),
    ));
    assert!(
        text(&a).contains("changed after you read it"),
        "{}",
        text(&a)
    );
    let r = enter(&mut a, "/mcp allow docs lookup");
    a.handle(ok(
        &r[0],
        409,
        json!({"error": {"code": "MCP_MAY_CHANGE", "message": "x"}}),
    ));
    assert!(
        text(&a).contains("/mcp allow SERVER TOOL knowingly"),
        "{}",
        text(&a)
    );
    let r = enter(&mut a, "/mcp allow docs lookup knowingly");
    assert_eq!(r[0].body.as_ref().unwrap()["knowing"], json!(true));
    let r = enter(&mut a, "/mcp off docs lookup");
    assert_eq!(r[0].body.as_ref().unwrap()["mode"], json!("off"));
}

#[test]
fn tu_mc03_check_add_remove_say_what_happened_and_an_empty_list_explains_itself() {
    let mut a = booted();
    let r = enter(&mut a, "/mcp");
    a.handle(ok(&r[0], 200, json!({"servers": []})));
    assert!(text(&a).contains("No outside tools yet"), "{}", text(&a));
    let r = enter(&mut a, "/mcp check docs");
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/mcp/discover", Method::Post)
    );
    a.handle(ok(&r[0], 200, json!({"tools": 3, "changed": ["lookup"]})));
    let t = text(&a);
    assert!(
        t.contains("3 tools") && t.contains("Changed since you allowed them, now off: lookup"),
        "{t}"
    );
    let r = enter(&mut a, "/mcp add docs /usr/bin/node a.js");
    assert_eq!(
        r[0].body.as_ref().unwrap()["program"],
        json!("/usr/bin/node")
    );
    assert_eq!(
        r[0].body.as_ref().unwrap()["env"],
        json!({}),
        "settings with values are added in the web page, never typed here"
    );
    a.handle(ok(&r[0], 422, json!({"error": {"code": "MCP_REFUSED", "message": "the program is not a file that exists"}})));
    assert!(text(&a).contains("not a file that exists"), "{}", text(&a));
    let r = enter(&mut a, "/mcp remove docs");
    assert_eq!(r[0].path.as_str(), "/api/mcp/servers/remove");
}

#[test]
fn tu_mc04_ask_pending_approve_and_deny_work_and_a_waiting_list_is_cleaned() {
    use kk_tui::cmd::McpAction as M;
    assert_eq!(
        cmd::parse("/mcp pending").unwrap(),
        Command::Mcp(M::Pending)
    );
    assert_eq!(
        cmd::parse("/mcp approve mc-3").unwrap(),
        Command::Mcp(M::Decide("mc-3".into(), true))
    );
    assert_eq!(
        cmd::parse("/mcp deny mc-3").unwrap(),
        Command::Mcp(M::Decide("mc-3".into(), false))
    );
    assert_eq!(
        cmd::parse("/mcp ask docs lookup").unwrap(),
        Command::Mcp(M::Ask("docs".into(), "lookup".into(), false))
    );
    assert_eq!(
        cmd::parse("/mcp ask docs lookup knowingly").unwrap(),
        Command::Mcp(M::Ask("docs".into(), "lookup".into(), true))
    );
    for bad in [
        "/mcp approve",
        "/mcp deny a b",
        "/mcp ask docs",
        "/mcp pending now",
    ] {
        assert!(cmd::parse(bad).is_err(), "{bad}");
    }
    let mut a = booted();
    let r = enter(&mut a, "/mcp pending");
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/mcp/pending", Method::Get)
    );
    a.handle(ok(&r[0], 200, json!({"pending": [{"id": "mc-1\u{1b}[2J", "server": "docs", "tool": "look\u{1b}[2Jup", "args": "{\"term\":\"x\u{1b}[31m\"}", "age_secs": 2}]})));
    let t = text(&a);
    assert!(
        t.contains("mc-1") && t.contains("docs") && t.contains("term") && !t.contains('\u{1b}'),
        "{t}"
    );
    let r = enter(&mut a, "/mcp pending");
    a.handle(ok(&r[0], 200, json!({"pending": []})));
    assert!(
        text(&a).contains("No call is waiting for you."),
        "{}",
        text(&a)
    );
    let r = enter(&mut a, "/mcp approve mc-1");
    assert_eq!(
        r[0].body.as_ref().unwrap(),
        &json!({"id": "mc-1", "approve": true})
    );
    a.handle(ok(&r[0], 200, json!({"approved": true})));
    assert!(
        text(&a).contains("Approved: the call goes ahead."),
        "{}",
        text(&a)
    );
    let r = enter(&mut a, "/mcp deny mc-2");
    assert_eq!(
        r[0].body.as_ref().unwrap(),
        &json!({"id": "mc-2", "approve": false})
    );
    let r = enter(&mut a, "/mcp ask docs lookup");
    assert!(r.is_empty(), "choose from the list first");
    let r = enter(&mut a, "/mcp");
    a.handle(ok(&r[0], 200, json!({"servers": [{"name": "docs", "program": "/x", "args": [], "tools": [{"name": "lookup", "description": "d", "pin": "ab".repeat(32), "mode": "ask", "effect": "read_only"}]}]})));
    assert!(text(&a).contains("asks you"), "{}", text(&a));
    let r = enter(&mut a, "/mcp ask docs lookup");
    let b = r[0].body.as_ref().unwrap();
    assert_eq!(
        (b["mode"].clone(), b["pin"].clone()),
        (json!("ask"), json!("ab".repeat(32)))
    );
}

#[test]
fn tu_me04_suggested_methods_can_be_listed_kept_and_dismissed_and_the_text_is_cleaned() {
    use kk_tui::cmd::MethodAction as M;
    assert_eq!(
        cmd::parse("/method suggestions").unwrap(),
        Command::Method(M::Suggestions)
    );
    assert_eq!(
        cmd::parse("/method keep 3").unwrap(),
        Command::Method(M::Keep(3))
    );
    assert_eq!(
        cmd::parse("/method dismiss 12").unwrap(),
        Command::Method(M::Dismiss(12))
    );
    for bad in [
        "/method keep",
        "/method keep x",
        "/method keep 0",
        "/method keep -1",
        "/method dismiss 1 2",
        "/method suggestions now",
    ] {
        assert!(cmd::parse(bad).is_err(), "{bad}");
    }
    let mut a = booted();
    let r = enter(&mut a, "/method suggestions");
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/method-proposals", Method::Get)
    );
    a.handle(ok(&r[0], 200, json!({"proposals": []})));
    assert!(
        text(&a).contains("No review has suggested a method."),
        "{}",
        text(&a)
    );
    let r = enter(&mut a, "/method suggestions");
    a.handle(ok(&r[0], 200, json!({"proposals": [{"id": 4, "name": "contract\u{1b}[2Jcheck", "description": "d", "triggers": ["contract"], "body": "Quote clause numbers.\nBe brief\u{1b}[31m."}]})));
    let t = text(&a);
    assert!(
        t.contains("Quote clause numbers.") && t.contains("read them with care"),
        "{t}"
    );
    assert!(!t.contains('\u{1b}'), "{t}");
    let r = enter(&mut a, "/method keep 4");
    assert_eq!(
        (r[0].path.as_str(), r[0].method),
        ("/api/method-proposals/keep", Method::Post)
    );
    assert_eq!(r[0].body.as_ref().unwrap(), &json!({"id": 4}));
    a.handle(ok(&r[0], 200, json!({"name": "x", "approved": false})));
    assert!(text(&a).contains("Kept as a method."), "{}", text(&a));
    let r = enter(&mut a, "/method dismiss 5");
    assert_eq!(r[0].body.as_ref().unwrap(), &json!({"id": 5}));
}

#[test]
fn tu112_a_roundtable_fills_a_card_per_expert_then_the_resolution_and_stays_out_of_the_conversation(
) {
    let mut app = booted();
    app.session = Some("chat-1".into());
    // alone, the command lists who can be asked
    let r = enter(&mut app, "/roundtable");
    assert_eq!((r.len(), r[0].path.as_str()), (1, "/api/experts"));
    app.handle(ok(&r[0], 200, json!({"experts": [{"id": "security-reviewer", "name": "Security reviewer", "group": "Engineering", "summary": "Looks for leaks."}], "yours": [{"id": "method:house-counsel", "name": "house-counsel", "summary": "Our own reading"}], "min": 2, "max": 5})));
    let all = text(&app);
    assert!(
        all.contains("security-reviewer")
            && all.contains("method:house-counsel")
            && all.contains("your approved method")
            && all.contains("Ask 2 to 5"),
        "{all}"
    );
    // malformed asks are refused with the usage, before anything is sent
    for bad in [
        "/roundtable security-reviewer : q",
        "/roundtable a,b",
        "/roundtable a,b : ",
        "/roundtable a b,c : q",
    ] {
        assert!(enter(&mut app, bad).is_empty(), "{bad}");
    }
    assert!(text(&app).contains("usage: /roundtable"));
    let r = enter(
        &mut app,
        "/roundtable security-reviewer, data-analyst : vault or code?",
    );
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].path, "/api/chat/roundtable");
    assert!(r[0].stream);
    let body = r[0].body.as_ref().unwrap();
    assert_eq!(
        body["experts"],
        json!(["security-reviewer", "data-analyst"])
    );
    assert_eq!(body["message"], "vault or code?");
    assert!(
        body.get("session").is_none(),
        "nothing is appended to the kept conversation"
    );
    assert!(app.chat_waiting, "the composer is busy, so Esc can stop it");
    assert!(enter(&mut app, "/roundtable a,b : again").is_empty());
    assert!(text(&app).contains("still arriving"));
    let id = r[0].id;
    let ev = |app: &mut App, line: Value| {
        app.handle(Event::Table { id, line });
    };
    ev(
        &mut app,
        json!({"expert": "security-reviewer", "name": "Security reviewer", "state": "asking"}),
    );
    assert!(text(&app).contains("Roundtable: Security reviewer\n(being asked)"));
    ev(
        &mut app,
        json!({"expert": "security-reviewer", "delta": "Use a "}),
    );
    ev(
        &mut app,
        json!({"expert": "security-reviewer", "delta": "vault\u{1b}[2J."}),
    );
    let all = text(&app);
    assert!(
        all.contains("Use a vault\u{fffd}[2J.")
            && !all.contains('\u{1b}')
            && !all.contains("(being asked)"),
        "{all}"
    );
    ev(
        &mut app,
        json!({"expert": "security-reviewer", "answer": "Use a vault.", "truncated_for_lead": false}),
    );
    ev(
        &mut app,
        json!({"expert": "data-analyst", "name": "Data analyst", "state": "asking"}),
    );
    ev(
        &mut app,
        json!({"expert": "data-analyst", "failed": "the model is not running"}),
    );
    let all = text(&app);
    assert!(
        all.contains("Roundtable: Data analyst\nNo answer: the model is not running"),
        "{all}"
    );
    ev(
        &mut app,
        json!({"resolution": {"structured": true, "agreed": "A vault.", "disputed": "", "decision": "Move the key.", "open_questions": "Which vault?", "text": "x"}}),
    );
    let all = text(&app);
    let at = |s: &str| {
        all.find(s)
            .unwrap_or_else(|| panic!("{s} missing in {all}"))
    };
    assert!(
        at("Roundtable: resolution") < at("**Decision**")
            && at("Move the key.") < at("**Agreed**")
            && at("**Disputed**") < at("(the lead wrote nothing here)")
    );
    assert!(all.contains("advice, not a check"));
    app.handle(ok(&r[0], 200, json!({"done": true})));
    assert!(!app.chat_waiting);
    assert_eq!(
        app.cards
            .iter()
            .filter(|c| c
                .title
                .as_deref()
                .is_some_and(|t| t.starts_with("Roundtable")))
            .count(),
        3,
        "one card per expert and one resolution, none doubled"
    );
    assert!(
        app.chat_history.is_empty(),
        "neither the question nor an answer joined the history"
    );
    let conv = app.transcript_text();
    assert!(
        !conv.contains("Use a vault") && !conv.contains("Move the key"),
        "{conv}"
    );
    // an event for another request is ignored
    app.handle(Event::Table {
        id: id + 999,
        line: json!({"expert": "x", "answer": "stray"}),
    });
    assert!(!text(&app).contains("stray"));

    // unstructured and missing resolutions say so; a refusal and a stop free the composer
    let mut b = booted();
    let r = enter(&mut b, "/roundtable a,b : q");
    b.handle(Event::Table {
        id: r[0].id,
        line: json!({"resolution": {"structured": false, "text": "just do it"}}),
    });
    assert!(text(&b).contains("did not use the four parts") && text(&b).contains("just do it"));
    b.handle(Event::Table { id: r[0].id, line: json!({"resolution": null, "reason": "no expert answered, so there is nothing to resolve"}) });
    assert!(text(&b).contains("No resolution: no expert answered"));
    b.handle(ok(
        &r[0],
        502,
        json!({"error": {"code": "NO_MODEL", "message": "no expert could be asked"}}),
    ));
    assert!(text(&b).contains("no expert could be asked") && !b.chat_waiting);
    let r = enter(&mut b, "/roundtable a,the-boss : q");
    b.handle(ok(&r[0], 400, json!({"error": {"code": "NO_SUCH_EXPERT", "message": "there is no expert with that name"}})));
    assert!(text(&b).contains("no expert with that name") && !b.chat_waiting);
    let r = enter(&mut b, "/roundtable a,b : q");
    b.handle(key(KeyCode::Esc));
    b.handle(key(KeyCode::Esc));
    assert!(
        b.cancel.load(std::sync::atomic::Ordering::SeqCst),
        "Esc twice stops a roundtable like an answer"
    );
    b.handle(ok(&r[0], 200, json!({"stopped": true})));
    assert!(text(&b).contains("Stopped. No one else was asked.") && !b.chat_waiting);
    // the service going away mid-table frees the composer too
    let r = enter(&mut b, "/roundtable a,b : q");
    b.handle(Event::Resp(Resp {
        id: r[0].id,
        result: Err("connection reset".into()),
    }));
    assert!(!b.chat_waiting);
}

#[test]
fn tu113_a_chat_answer_that_reached_the_models_length_limit_says_so_first() {
    let mut app = booted();
    let r = enter(&mut app, "explain everything");
    app.handle(ok(&r[0], 200, json!({"done": true, "reply": "It starts and then", "cut_short": true, "route": {"class": "device_only", "simulated": false}})));
    let all = text(&app);
    assert!(
        all.contains("reached the model's length limit and stops there"),
        "{all}"
    );
    let mut b = booted();
    let r = enter(&mut b, "short one");
    b.handle(ok(&r[0], 200, json!({"done": true, "reply": "Done.", "cut_short": false, "route": {"class": "device_only", "simulated": false}})));
    assert!(!text(&b).contains("length limit"));
}

fn team_body(status: &str, working: bool, states: [&str; 2], question: &str) -> Value {
    json!({
        "id": "team-0123456789ab", "goal": "Decide where the key lives.", "status": status, "working": working,
        "final": if status == "done" { json!("**Use the vault.**") } else { Value::Null }, "final_note": "",
        "tasks": [
            {"n": 1, "expert": "security-reviewer", "expert_name": "Security reviewer", "after": [], "task": "List leaks.", "state": states[0], "result": if states[0] == "done" { "1. logs\u{1b}[2J" } else { "" }, "note": "", "question": question, "asked": [], "attempts": 1},
            {"n": 2, "expert": "plain-writer", "expert_name": "Plain-language writer", "after": [1], "task": "Write it up.", "state": states[1], "result": if states[1] == "done" { "Moved." } else { "" }, "note": "", "question": "", "asked": [], "attempts": 0},
        ],
    })
}

#[test]
fn tu114_a_team_run_is_planned_started_followed_answered_and_shown_in_the_terminal() {
    use kk_tui::attention::Attention;
    let mut app = booted();
    // nothing shown yet: the short forms have nothing to mean
    for line in [
        "/team start",
        "/team stop",
        "/team show",
        "/team answer 1 x",
    ] {
        assert!(enter(&mut app, line).is_empty(), "{line}");
    }
    assert!(text(&app).contains("No team run has been shown here yet"));
    for bad in [
        "/team plan",
        "/team show nope",
        "/team start team-XYZ",
        "/team answer",
        "/team answer x y",
        "/team answer 0 y",
        "/team answer 2",
        "/team delete",
        "/team delete nope",
        "/team fly",
    ] {
        assert!(enter(&mut app, bad).is_empty(), "{bad}");
    }
    assert!(text(&app).contains("usage: /team plan GOAL"));
    let r = enter(&mut app, "/team");
    assert_eq!((r[0].path.as_str(), r.len()), ("/api/team", 1));
    app.handle(ok(&r[0], 200, json!({"runs": []})));
    assert!(text(&app).contains("No team runs yet. /team plan GOAL"));

    let r = enter(&mut app, "/team plan Decide where the key lives.");
    assert_eq!(r[0].path, "/api/team");
    assert_eq!(
        r[0].body.as_ref().unwrap()["goal"],
        "Decide where the key lives."
    );
    app.handle(ok(
        &r[0],
        201,
        team_body("planned", false, ["waiting", "waiting"], ""),
    ));
    let all = text(&app);
    assert!(
        all.contains("Team run team-0123456789ab")
            && all.contains("1. [waiting] Security reviewer: List leaks.")
            && all.contains("2. [waiting] Plain-language writer (after 1): Write it up.")
            && all.contains("Nothing has run. /team start runs this plan"),
        "{all}"
    );
    assert!(
        app.team_watch.is_none(),
        "a plan is not followed: nothing is running"
    );
    assert!(app
        .handle(Event::Tick)
        .iter()
        .all(|q| !q.path.starts_with("/api/team")));

    // start: the short form means the run just shown; while it works it is looked at on the tick, one look at a time
    let r = enter(&mut app, "/team start");
    assert_eq!(r[0].path, "/api/team/team-0123456789ab/start");
    app.handle(ok(
        &r[0],
        200,
        team_body("running", true, ["running", "waiting"], ""),
    ));
    let looks: Vec<Req> = app
        .handle(Event::Tick)
        .into_iter()
        .filter(|q| q.path == "/api/team/team-0123456789ab")
        .collect();
    assert_eq!(looks.len(), 1);
    assert!(
        app.handle(Event::Tick)
            .iter()
            .all(|q| q.path != "/api/team/team-0123456789ab"),
        "no second look while one is on its way"
    );
    app.handle(ok(
        &looks[0],
        200,
        team_body("running", true, ["done", "running"], ""),
    ));
    let all = text(&app);
    assert!(
        all.contains("Task 1 (Security reviewer) is done.")
            && all.contains("Task 2 (Plain-language writer) is being asked."),
        "{all}"
    );
    // it stops to ask: the owner is told how to answer, and the terminal asks for attention
    let look = app
        .handle(Event::Tick)
        .into_iter()
        .find(|q| q.path == "/api/team/team-0123456789ab")
        .unwrap();
    app.handle(ok(
        &look,
        200,
        team_body(
            "waiting",
            false,
            ["needs_input", "waiting"],
            "Which vault\u{1b}[2J?",
        ),
    ));
    let all = text(&app);
    assert!(
        all.contains("Task 1 asks: Which vault\u{fffd}[2J?")
            && all.contains("Answer with: /team answer 1 YOUR ANSWER")
            && !all.contains('\u{1b}'),
        "{all}"
    );
    assert_eq!(app.attention.take(), Some(Attention::TeamAsks));
    assert!(
        app.team_watch.is_none()
            && app
                .handle(Event::Tick)
                .iter()
                .all(|q| !q.path.starts_with("/api/team")),
        "a run that waits for the owner is not polled"
    );
    let r = enter(&mut app, "/team answer 1 The company one.");
    assert_eq!(r[0].path, "/api/team/team-0123456789ab/answer");
    assert_eq!(
        r[0].body.as_ref().unwrap(),
        &json!({"task": 1, "answer": "The company one."})
    );
    app.handle(ok(
        &r[0],
        200,
        team_body("running", true, ["running", "waiting"], ""),
    ));
    let look = app
        .handle(Event::Tick)
        .into_iter()
        .find(|q| q.path == "/api/team/team-0123456789ab")
        .unwrap();
    app.handle(ok(
        &look,
        200,
        team_body("done", false, ["done", "done"], ""),
    ));
    let all = text(&app);
    assert!(
        all.contains("Team: task 1 · Security reviewer")
            && all.contains("Team: result")
            && all.contains("**Use the vault.**")
            && all.contains("advice, not a check")
            && !all.contains('\u{1b}'),
        "{all}"
    );
    assert!(app.team_watch.is_none());
    assert!(
        !app.transcript_text().contains("Use the vault"),
        "a team run is not part of the conversation"
    );

    // stop: followed until the service says it stopped; a refusal and a lost look end the following
    let r = enter(&mut app, "/team start");
    app.handle(ok(
        &r[0],
        200,
        team_body("running", true, ["running", "waiting"], ""),
    ));
    let r = enter(&mut app, "/team stop");
    assert_eq!(r[0].path, "/api/team/team-0123456789ab/stop");
    app.handle(ok(
        &r[0],
        200,
        json!({"id": "team-0123456789ab", "stopping": true}),
    ));
    assert!(text(&app).contains("Stopping: the answer being written is cut off"));
    let look = app
        .handle(Event::Tick)
        .into_iter()
        .find(|q| q.path == "/api/team/team-0123456789ab")
        .unwrap();
    app.handle(ok(
        &look,
        200,
        team_body("stopped", false, ["waiting", "waiting"], ""),
    ));
    assert!(
        text(&app).contains("Stopped. Finished tasks are kept; /team start carries on.")
            && app.team_watch.is_none()
    );
    let r = enter(&mut app, "/team start");
    app.handle(ok(
        &r[0],
        409,
        json!({"error": {"code": "RUN_ENDED", "message": "this run has ended; plan a new one"}}),
    ));
    assert!(text(&app).contains("this run has ended"));
    let r = enter(&mut app, "/team delete team-0123456789ab");
    assert_eq!(r[0].path, "/api/team/team-0123456789ab/delete");
    app.handle(ok(&r[0], 200, json!({"deleted": "team-0123456789ab"})));
    assert!(
        app.team_last.is_none() && enter(&mut app, "/team show").is_empty(),
        "the deleted run is no longer what the short forms mean"
    );
}

#[test]
fn tu115_stop_lists_what_is_working_and_stops_one_thing_by_its_id() {
    let mut app = booted();
    let r = enter(&mut app, "/stop");
    assert_eq!(r[0].path, "/api/work");
    app.handle(ok(&r[0], 200, json!({"items": [{"kind": "review", "id": "job-3", "title": "acme\u{1b}[2J", "quiet": true, "quiet_for": 1300, "can_stop": true}, {"kind": "team", "id": "team-0123456789ab", "title": "Decide", "quiet": false, "quiet_for": 20, "can_stop": true}], "count": 2, "quiet": 1})));
    let all = text(&app);
    assert!(
        all.contains("job-3")
            && all.contains("no sign of progress for 21 min")
            && all.contains("progress 1 min ago")
            && all.contains("/stop ID stops one")
            && !all.contains('\u{1b}'),
        "{all}"
    );
    for (line, path) in [
        ("/stop job-3", "/api/jobs/job-3/stop"),
        ("/stop code-12", "/api/code/code-12/stop"),
        (
            "/stop team-0123456789ab",
            "/api/team/team-0123456789ab/stop",
        ),
    ] {
        let r = enter(&mut app, line);
        assert_eq!((r.len(), r[0].path.as_str()), (1, path), "{line}");
    }
    for bad in [
        "/stop everything",
        "/stop job-",
        "/stop job-1/../x",
        "/stop team-XYZ",
        "/stop code-1 now",
    ] {
        assert!(enter(&mut app, bad).is_empty(), "{bad}");
    }
    assert!(text(&app).contains("usage: /stop ID"));
    let r = enter(&mut app, "/stop job-3");
    app.handle(ok(
        &r[0],
        200,
        json!({"id": "job-3", "stopping": true, "at_once": true}),
    ));
    assert!(text(&app).contains("Stopping now. Nothing it found will be offered."));
    let r = enter(&mut app, "/stop job-3");
    app.handle(ok(
        &r[0],
        200,
        json!({"id": "job-3", "stopping": true, "at_once": false}),
    ));
    assert!(text(&app).contains("It will stop at the end of the step in progress."));
    let r = enter(&mut app, "/stop job-3");
    app.handle(ok(
        &r[0],
        409,
        json!({"error": {"code": "NOT_RUNNING", "message": "nothing with that id is running"}}),
    ));
    assert!(text(&app).contains("nothing with that id is running"));
    let mut b = booted();
    let r = enter(&mut b, "/stop");
    b.handle(ok(&r[0], 200, json!({"items": [], "count": 0, "quiet": 0})));
    assert!(text(&b).contains("Nothing is working right now."));
}

#[test]
fn tu116_the_bundled_skills_are_listed_and_one_is_taken_still_unapproved() {
    let mut app = booted();
    let r = enter(&mut app, "/method bundled");
    assert_eq!(r[0].path, "/api/methods");
    app.handle(ok(&r[0], 200, json!({"methods": [], "bundled": [{"name": "code-review", "group": "Engineering", "description": "Review a change\u{1b}[2J", "hash": "h", "added": false}, {"name": "plain-language", "group": "Writing", "description": "Rewrite simply", "hash": "h", "added": true}]})));
    let all = text(&app);
    assert!(
        all.contains("Skills that come with Kakurega")
            && all.contains("code-review")
            && all.contains("(in your list) Rewrite simply")
            && all.contains("/method take NAME")
            && !all.contains('\u{1b}'),
        "{all}"
    );
    let r = enter(&mut app, "/method take code-review");
    assert_eq!(
        (r[0].path.as_str(), r[0].body.as_ref().unwrap()),
        ("/api/methods/bundled", &json!({"name": "code-review"}))
    );
    app.handle(ok(
        &r[0],
        200,
        json!({"name": "code-review", "hash": "h", "approved": false}),
    ));
    assert!(text(&app).contains("Added code-review. /method show code-review shows its text; /method approve code-review puts it in use."));
    let r = enter(&mut app, "/method take code-review");
    app.handle(ok(&r[0], 409, json!({"error": {"code": "METHOD_EXISTS", "message": "you already have a method with that name"}})));
    assert!(text(&app).contains("you already have a method with that name"));
    for bad in [
        "/method take",
        "/method take ../x",
        "/method take a b",
        "/method bundled now",
    ] {
        assert!(enter(&mut app, bad).is_empty(), "{bad}");
    }
}

#[test]
fn tu_counted_words_get_their_real_form_wherever_the_count_stands() {
    use kk_tui::app::counted;
    for (written, shown) in [
        ("1 file(s) read", "1 file read"),
        ("3 file(s) read", "3 files read"),
        ("0 note(s) used", "0 notes used"),
        (
            "1 note(s) were not given to the model",
            "1 note was not given to the model",
        ),
        (
            "2 note(s) were not given to the model",
            "2 notes were not given to the model",
        ),
        (
            "1 attached file(s) read for this answer",
            "1 attached file read for this answer",
        ),
        (
            "… 40000 more line(s) were not kept",
            "… 40000 more lines were not kept",
        ),
        ("… 1 more line(s) below", "… 1 more line below"),
        (
            "ready · 1 review(s) · project x",
            "ready · 1 review · project x",
        ),
        ("(1 earlier release(s))", "(1 earlier release)"),
        ("11 file(s), 1 withheld", "11 files, 1 withheld"),
        ("no number here, file(s) only", "no number here, files only"),
        ("nothing to change", "nothing to change"),
    ] {
        assert_eq!(counted(written), shown, "{written}");
    }
}

#[test]
fn tu_shown_data_is_never_reworded_and_earlier_messages_carry_their_level() {
    // what a model, a file or the owner wrote is shown as it is, "(s)" and all
    let mut a = booted();
    let r = enter(&mut a, "what does len(s) do in 1 file(s)?");
    assert!(
        r[0].body.as_ref().unwrap().get("held").is_none(),
        "a first message has nothing earlier to protect"
    );
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "len(s) counts; print(s) shows 1 item(s) were returned", "level": 3, "route": {"simulated": false}, "notes_used": 0})));
    let t = text(&a);
    assert!(
        t.contains("len(s) counts; print(s) shows 1 item(s) were returned"),
        "{t}"
    );
    assert!(t.contains("what does len(s) do in 1 file(s)?"), "{t}");
    // the next message says how privately the earlier ones were held; so does an aside
    let r = enter(&mut a, "and then?");
    assert_eq!(r[0].body.as_ref().unwrap()["held"], 3);
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "then nothing", "level": 2, "route": {"simulated": false}, "notes_used": 0})));
    let r = enter(&mut a, "/aside quick one");
    assert_eq!(
        (r[0].path.as_str(), &r[0].body.as_ref().unwrap()["held"]),
        ("/api/chat", &json!(3)),
        "the most private level any of them had"
    );
    // a new conversation starts with nothing held
    enter(&mut a, "/new");
    let r = enter(&mut a, "fresh");
    assert!(r[0].body.as_ref().unwrap().get("held").is_none());
}

#[test]
fn tu_ex01_a_question_is_followed_to_its_answer_and_a_line_not_in_the_file_is_never_shown_as_code()
{
    let mut app = booted();
    // the depth word is optional; a folder name is read under the first root the service gave
    let r = enter(
        &mut app,
        "/explore thorough /home/u/shop : Does logout revoke the token?",
    );
    assert_eq!(
        (r.len(), r[0].path.as_str(), r[0].method),
        (1, "/api/questions", Method::Post)
    );
    let body = r[0].body.as_ref().unwrap();
    assert_eq!(
        (
            body["folder"].as_str(),
            body["question"].as_str(),
            body["depth"].as_str()
        ),
        (
            Some("/home/u/shop"),
            Some("Does logout revoke the token?"),
            Some("thorough")
        )
    );
    app.handle(ok(&r[0], 202, json!({"id": "question-1"})));
    let poll = app.handle(Event::Tick);
    let p = poll
        .iter()
        .find(|q| q.path == "/api/questions/question-1")
        .expect("the question is polled");
    app.handle(ok(
        p,
        200,
        json!({"id": "question-1", "state": "running", "progress": ["the model reads 2 files"]}),
    ));
    assert!(text(&app).contains("the model reads 2 files"));
    let busy = enter(&mut app, "/explore /home/u/shop : another?");
    assert!(busy.is_empty() && text(&app).contains("still being answered"));
    let poll = app.handle(Event::Tick);
    let p = poll
        .iter()
        .find(|q| q.path == "/api/questions/question-1")
        .unwrap();
    app.handle(ok(p, 200, json!({
        "id": "question-1", "state": "answered", "question": "Does logout revoke the token?",
        "answer": {"ending": "answered", "text": "No: logout returns True.", "not_found": "",
            "places": [
                {"file": "app/session.py", "line": 6, "quote": "return True", "found": {"state": "at_line"}, "text": "    return True"},
                {"file": "app/session.py", "line": 2, "quote": "SESSIONS.pop(token)", "found": {"state": "not_found"}, "text": ""}],
            "checked": 1, "unchecked": 1,
            "checks": [{"language": "python", "script": "print(1)", "run": {"ran": true, "exit_ok": true, "timed_out": false, "output": "still logged in: alice\n"}}],
            "files_read": ["app/session.py"], "steps": [], "turns": 4, "withheld": 1, "corrected": true, "route": "ollama:x", "elapsed_ms": 3200}
    })));
    let t = text(&app);
    assert!(t.contains("No: logout returns True."), "{t}");
    assert!(
        t.contains("1 of the 2 lines it rests on were found in the files:"),
        "{t}"
    );
    assert!(
        t.contains("app/session.py:6  found") && t.contains("      return True"),
        "{t}"
    );
    assert!(
        t.contains("app/session.py:2  NOT in the file; the agent wrote: SESSIONS.pop(token)"),
        "{t}"
    );
    assert!(
        t.contains(
            "Check 1 (python), run in the sealed sandbox: ended without an error. It printed:"
        ) && t.contains("still logged in: alice"),
        "{t}"
    );
    assert!(
        t.contains("1 file read, 4 steps, 3 s. 1 file was withheld from the agent."),
        "{t}"
    );
    assert!(
        app.handle(Event::Tick)
            .iter()
            .all(|q| !q.path.starts_with("/api/questions")),
        "an answered question is not polled again"
    );
    // shown again on asking, and a bad name is refused before anything is sent
    let r = enter(&mut app, "/explore show");
    assert_eq!(r[0].path, "/api/questions/question-1");
    assert!(enter(&mut app, "/explore show ../etc").is_empty());
    assert!(
        enter(&mut app, "/explore no colon here").is_empty()
            && text(&app).contains("usage: /explore")
    );
}
