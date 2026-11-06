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
            && t.contains("2 note(s) used")
            && t.contains("1 note(s) were not given to the model"),
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

#[test]
fn tu16_when_the_service_requires_a_signed_approval_the_terminal_signs_the_exact_statement_or_refuses_to_send(
) {
    use kk_evidence::approval::{verify, Statement};
    use kk_evidence::keys::Signer;
    let statement = json!({"approval": "appr-9", "job": "job-4", "digest": "abcdef0123456789abcdef", "recipient": "acme-audit", "policy": "policy-1", "expires": 1_700_086_400, "nonce": "n-123"});
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
        expires: 1_700_086_400,
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
    assert_eq!(r[0].path, "/api/sessions");
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
    assert_eq!(a.cards.len(), before + 1, "still one card");
    assert!(text(&a).contains("test model · 1 note(s) used"));
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
    assert!(text(&a).contains("1 queued message(s) were not sent"));
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
        (first.len(), first[0].path.as_str()),
        (1, "/api/history"),
        "the saved history is requested once"
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
    assert_eq!(a.completions(), ["help"]);
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
        a.input, "/web ",
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
        .all(|r| r.path == "/api/history"));
}

#[test]
fn tu34_the_footer_shows_the_context_estimate_and_older_turns_left_out() {
    let mut a = booted();
    let r = enter(&mut a, "hello");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 0, "usage": {"context_tokens_estimate": 1240, "history_turns_dropped": 6}})));
    let t = text(&a);
    assert!(
        t.contains("about 1240 tokens of context sent")
            && t.contains("6 older message(s) were not sent"),
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
        text(&a).contains("1 note(s) used · project papermark"),
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
    assert!(t.contains("1 note(s) used") && t.contains("note: The staging server listens on port 8081 (you asked to remember it; matched by keywords)"), "{t}");
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
    assert!(t.contains("2 web page(s) used"), "{t}");
    assert!(
        t.contains("1 page(s) cut short to fit the model's window"),
        "{t}"
    );
    assert!(
        t.contains("3 page(s) left out: no room in the model's window"),
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
        text(&a).contains("3 matching note(s) left out: no room in the model's window"),
        "{}",
        text(&a)
    );
    let r = enter(&mut a, "again");
    a.handle(ok(&r[0], 200, json!({"done": true, "reply": "hi", "route": {"simulated": true}, "notes_used": 1, "notes_left_out": 0})));
    assert_eq!(
        text(&a).matches("matching note(s) left out").count(),
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
