//! A question about a project: the agent reads, may run a check, and every place its answer rests on is checked.
use kk_review::code::Workspace;
use kk_review::explore::*;
use kk_review::import::ImportedFile;
use kk_route::{RouteClass, ScriptedRoute};
use kk_types::chat::{ChatMessage, ToolCall};
use kk_types::label::{Label, Level};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;

const SESSION: &str = "import time\n\nSESSIONS = {}\n\n\ndef login(user):\n    token = f\"t-{user}-{time.time()}\"\n    SESSIONS[token] = user\n    return token\n\n\ndef logout(token):\n    # the token is forgotten here\n    return True\n\n\ndef whoami(token):\n    return SESSIONS.get(token)\n";

fn file(path: &str, text: &str, level: Level) -> ImportedFile {
    let bytes = text.as_bytes().to_vec();
    ImportedFile {
        path: path.into(),
        sha256: kk_evidence::capsule::sha256_hex(&bytes),
        bytes,
        label: Label::new(level),
        notes: vec![],
    }
}

fn ws() -> Workspace {
    Workspace::from_imported(&[
        file("app/session.py", SESSION, Level::Internal),
        file("app/__init__.py", "", Level::Internal),
        file(".env", "API_TOKEN=tok-9f8e7d6c5b4a\n", Level::Restricted),
    ])
    .0
}

fn calls(list: &[(&str, Value)]) -> ChatMessage {
    let mut m = ChatMessage::new("assistant", "");
    m.tool_calls = list
        .iter()
        .enumerate()
        .map(|(i, (n, a))| ToolCall {
            id: format!("c{i}"),
            name: n.to_string(),
            arguments: a.clone(),
        })
        .collect();
    m
}

fn route(script: Vec<ChatMessage>) -> ScriptedRoute {
    ScriptedRoute::new("scripted", RouteClass::DeviceOnly, false, script)
}

struct FakeScripts {
    seen: RefCell<Vec<(BTreeMap<String, String>, String)>>,
}

impl ScriptRunner for FakeScripts {
    fn run(&self, files: &BTreeMap<String, String>, _language: &str, script: &str) -> ScriptRun {
        self.seen
            .borrow_mut()
            .push((files.clone(), script.to_string()));
        ScriptRun {
            ran: true,
            exit_ok: true,
            timed_out: false,
            output: "still logged in: alice\n".into(),
        }
    }
}

const NEVER: &dyn Fn() -> bool = &|| false;

#[test]
fn ex01_an_answer_whose_places_are_in_the_files_is_accepted_and_each_place_carries_the_real_line() {
    let r = route(vec![
        calls(&[("search_text", json!({"text": "def logout"}))]),
        calls(&[("read_file", json!({"path": "app/session.py"}))]),
        calls(&[(
            "answer",
            json!({"answer": "No. logout returns True and never removes the token from SESSIONS, so whoami still finds the user.",
                   "places": [{"file": "app/session.py", "line": 12, "text": "def logout(token):"},
                              {"file": "app/session.py", "line": 14, "text": "14\t    return True"},
                              {"file": "app/session.py", "line": 19, "text": "return SESSIONS.get(token)"}]}),
        )]),
    ]);
    let mut w = ws();
    let rep = run_question(
        &r,
        &mut w,
        "Does logout revoke the session token?",
        Depth::Normal,
        None,
        NEVER,
    );
    assert_eq!(rep.ending, Ending::Answered);
    assert!(rep.answer.starts_with("No. logout returns True"));
    let states: Vec<&Found> = rep.citations.iter().map(|c| &c.found).collect();
    assert_eq!(
        states,
        [&Found::AtLine, &Found::AtLine, &Found::Moved { line: 18 }],
        "{:?}",
        rep.citations
    );
    assert_eq!(
        rep.citations[1].text, "    return True",
        "the line as it is in the file, the number dropped"
    );
    assert_eq!(rep.files_read, ["app/session.py"]);
    assert_eq!(rep.withheld, 1);
    assert!(!rep.corrected);
}

#[test]
fn ex02_an_invented_place_is_shown_to_the_agent_once_and_marked_if_it_stays() {
    // the agent cites a line that is not in the file, is told, and cites it again: it stays marked as not found
    let invented = json!({"answer": "Yes, logout deletes the token.", "places": [{"file": "app/session.py", "line": 13, "text": "del SESSIONS[token]"}]});
    let r = route(vec![
        calls(&[("read_file", json!({"path": "app/session.py"}))]),
        calls(&[("answer", invented.clone())]),
        calls(&[("answer", invented)]),
    ]);
    let mut w = ws();
    let rep = run_question(
        &r,
        &mut w,
        "Does logout revoke the session token?",
        Depth::Normal,
        None,
        NEVER,
    );
    assert_eq!(rep.ending, Ending::Answered);
    assert!(rep.corrected, "it was told once");
    assert_eq!(rep.citations.len(), 1);
    assert_eq!(rep.citations[0].found, Found::NotFound);
    assert!(
        rep.citations[0].text.is_empty(),
        "nothing from the file is shown for a place that is not there"
    );

    // corrected on the second try, it is checked
    let r = route(vec![
        calls(&[("read_file", json!({"path": "app/session.py"}))]),
        calls(&[(
            "answer",
            json!({"answer": "No.", "places": [{"file": "app/session.py", "line": 3, "text": "del SESSIONS[token]"}]}),
        )]),
        calls(&[(
            "answer",
            json!({"answer": "No.", "places": [{"file": "app/session.py", "line": 14, "text": "return True"}]}),
        )]),
    ]);
    let rep = run_question(
        &r,
        &mut ws(),
        "Does logout revoke the token?",
        Depth::Quick,
        None,
        NEVER,
    );
    assert_eq!(
        (rep.corrected, &rep.citations[0].found),
        (true, &Found::AtLine)
    );
}

#[test]
fn ex03_a_withheld_file_is_never_read_or_checked_and_a_short_quote_proves_nothing() {
    let r = route(vec![
        calls(&[("read_file", json!({"path": ".env"}))]),
        calls(&[(
            "answer",
            json!({"answer": "The token is in .env.", "places": [
            {"file": ".env", "line": 1, "text": "API_TOKEN=tok-9f8e7d6c5b4a"},
            {"file": ".ENV", "line": 1, "text": "API_TOKEN"},
            {"file": "app/session.py", "line": 14, "text": "True"},
            {"file": "app/missing.py", "line": 1, "text": "anything at all here"}]}),
        )]),
        calls(&[(
            "answer",
            json!({"answer": "The token is in .env.", "places": [{"file": ".env", "line": 1, "text": "API_TOKEN=tok-9f8e7d6c5b4a"}]}),
        )]),
    ]);
    let mut w = ws();
    let rep = run_question(
        &r,
        &mut w,
        "Where is the API token?",
        Depth::Normal,
        None,
        NEVER,
    );
    assert!(
        rep.files_read.is_empty(),
        "the read was refused: {:?}",
        rep.files_read
    );
    assert_eq!(rep.citations[0].found, Found::Withheld);
    assert!(rep
        .citations
        .iter()
        .all(|c| !c.found.checked() && c.text.is_empty()));
    // the short one, tried alone
    let c = check_citation(&ws(), "app/session.py", 14, "True");
    assert_eq!(c.found, Found::TooShort);
    let c = check_citation(&ws(), "app/session.py", 9, "return token");
    assert_eq!(
        c.found,
        Found::AtLine,
        "a short line that stands once is still a place"
    );
    let c = check_citation(&ws(), "app/missing.py", 1, "anything");
    assert_eq!(c.found, Found::NoSuchFile);
}

#[test]
fn ex04_a_check_runs_on_the_readable_files_only_and_its_output_is_kept_as_evidence() {
    let script = "from app.session import login, logout, whoami\nt = login('alice')\nlogout(t)\nprint('still logged in:', whoami(t))\n";
    let r = route(vec![
        calls(&[("run_check", json!({"language": "python", "script": script}))]),
        calls(&[("run_check", json!({"language": "python", "script": script}))]),
        calls(&[(
            "answer",
            json!({"answer": "No: after logout whoami still returns alice.", "places": [{"file": "app/session.py", "line": 14, "text": "return True"}]}),
        )]),
    ]);
    let runner = FakeScripts {
        seen: RefCell::new(Vec::new()),
    };
    let mut w = ws();
    let rep = run_question(
        &r,
        &mut w,
        "Does logout revoke the token?",
        Depth::Quick,
        Some(&runner),
        NEVER,
    );
    assert_eq!(rep.ending, Ending::Answered);
    assert_eq!(
        rep.checks.len(),
        1,
        "a quick question runs one script at most"
    );
    assert_eq!(rep.checks[0].run.output, "still logged in: alice\n");
    let seen = runner.seen.borrow();
    assert_eq!(seen.len(), 1);
    assert!(
        !seen[0].0.contains_key(".env"),
        "the withheld file is not copied for the script"
    );
    assert!(seen[0].0.contains_key("app/session.py"));
}

#[test]
fn ex05_no_tool_that_writes_an_answer_without_the_tool_is_marked_and_a_stop_ends_it() {
    let r = route(vec![
        calls(&[(
            "edit_file",
            json!({"path": "app/session.py", "old_text": "return True", "new_text": "SESSIONS.pop(token)"}),
        )]),
        calls(&[(
            "create_file",
            json!({"path": "x.py", "content": "print(1)"}),
        )]),
        ChatMessage::new("assistant", "I think logout is fine."),
        ChatMessage::new("assistant", "Still fine."),
        ChatMessage::new("assistant", "Fine."),
    ]);
    let mut w = ws();
    let rep = run_question(&r, &mut w, "Is logout fine?", Depth::Normal, None, NEVER);
    assert_eq!(rep.ending, Ending::AnsweredWithoutTool);
    assert_eq!(rep.answer, "Fine.");
    assert!(rep.citations.is_empty());
    assert_eq!(
        w.file("app/session.py"),
        Some(SESSION),
        "nothing was changed"
    );
    assert!(w.changes().is_empty());

    let r = route(vec![calls(&[("list_files", json!({}))])]);
    let rep = run_question(&r, &mut ws(), "Anything?", Depth::Normal, None, &|| true);
    assert_eq!((rep.ending, rep.turns), (Ending::Stopped, 0));
}

#[test]
fn ex06_places_sent_as_text_and_line_numbers_as_text_are_read() {
    let r = route(vec![
        calls(&[("read_file", json!({"path": "app/session.py"}))]),
        calls(&[(
            "answer",
            json!({"answer": "No.", "places": "[{\"file\": \"./app/session.py\", \"line\": \"14\", \"text\": \"return True\"}]"}),
        )]),
    ]);
    let rep = run_question(
        &r,
        &mut ws(),
        "Does logout revoke the token?",
        Depth::Normal,
        None,
        NEVER,
    );
    assert_eq!((rep.ending, rep.citations.len()), (Ending::Answered, 1));
    assert_eq!(rep.citations[0].found, Found::AtLine);
}

#[test]
fn ex07_an_answer_before_any_look_is_sent_back_once_with_where_to_start() {
    let r = route(vec![
        calls(&[(
            "search_text",
            json!({"text": "logout revoke session token"}),
        )]),
        calls(&[(
            "answer",
            json!({"answer": "It cannot be answered from the files."}),
        )]),
        calls(&[("read_file", json!({"path": "app/session.py"}))]),
        calls(&[(
            "answer",
            json!({"answer": "No: logout returns True.", "places": [{"file": "app/session.py", "line": 14, "text": "return True"}]}),
        )]),
    ]);
    let mut w = ws();
    let rep = run_question(
        &r,
        &mut w,
        "Does logout revoke the session token?",
        Depth::Normal,
        None,
        NEVER,
    );
    assert_eq!(
        (rep.ending.clone(), rep.citations.len()),
        (Ending::Answered, 1),
        "{rep:?}"
    );
    assert_eq!(rep.files_read, ["app/session.py"]);
    // the files that hold the question's words are named first, by name before text
    let starts = starting_points(&ws(), "Does logout revoke the session token?", 3);
    assert_eq!(starts[0].0, "app/session.py");
    assert!(
        starts[0].1.contains(&"logout".to_string()) && starts[0].1.contains(&"session".to_string())
    );
    assert_eq!(
        question_words("Does the old token still work after logout?"),
        ["token", "logout"]
    );
}

#[test]
fn ex08_an_answer_written_as_text_with_its_lines_is_read_and_checked_like_one_through_the_tool() {
    let text = "The logout function does not revoke the token.\n\nThe places it rests on are:\n- `app/session.py:12: def logout(token):`\n- `app/session.py:14: return True`\n- `app/session.py:13: del SESSIONS[token]`\n";
    let r = route(vec![
        calls(&[("read_file", json!({"path": "app/session.py"}))]),
        ChatMessage::new("assistant", text),
        ChatMessage::new("assistant", text),
    ]);
    let mut w = ws();
    let rep = run_question(
        &r,
        &mut w,
        "Does logout revoke the session token?",
        Depth::Normal,
        None,
        NEVER,
    );
    assert_eq!(rep.ending, Ending::Answered, "{rep:?}");
    assert_eq!(rep.answer, "The logout function does not revoke the token.");
    let states: Vec<&Found> = rep.citations.iter().map(|c| &c.found).collect();
    assert_eq!(states, [&Found::AtLine, &Found::AtLine, &Found::NotFound]);
    assert!(rep.corrected, "the made-up line was shown to it once");
    // a path the model may not read is never taken as a place, and a line without a number is not one
    let none = places_in_text(
        "see notes/elsewhere.txt:3: x\napp/session.py: no number here",
        &ws(),
    );
    assert!(none.is_empty(), "{none:?}");
    // a search for a file's name names the file
    let r = route(vec![
        calls(&[("search_text", json!({"text": "session"}))]),
        calls(&[(
            "answer",
            json!({"answer": "x", "places": [{"file": "app/session.py", "line": 14, "text": "return True"}]}),
        )]),
    ]);
    let rep = run_question(
        &r,
        &mut ws(),
        "Where is the session?",
        Depth::Normal,
        None,
        NEVER,
    );
    assert_eq!(
        rep.ending,
        Ending::ModelFailed("the scripted route has no more responses".into()),
        "a search alone is not a read: the answer is sent back once"
    );
    // the labelled form small models also use
    let labelled = "Randomness is added.\n- **File:** app/session.py\n- **Line:** 14\n- **Text:** return True\nThat is all.";
    let got = places_in_text(labelled, &ws());
    assert_eq!(
        got,
        vec![json!({"file": "app/session.py", "line": 14, "text": "return True"})]
    );
}

#[test]
fn ex09_only_lines_that_hold_places_leave_a_text_answer_and_scraps_of_text_are_not_places() {
    // a time, a ratio or an address in the answer's own words stays in it
    let text = "The login is refused at a ratio of 3:1 failures.\nIt is NOT refused at 10:30: the limit resets.\nCode: see below.\n- `app/session.py:14: return True`";
    let r = route(vec![
        calls(&[("read_file", json!({"path": "app/session.py"}))]),
        ChatMessage::new("assistant", text),
        ChatMessage::new("assistant", text),
    ]);
    let rep = run_question(
        &r,
        &mut ws(),
        "When is the login refused?",
        Depth::Normal,
        None,
        NEVER,
    );
    assert_eq!(rep.answer, "The login is refused at a ratio of 3:1 failures.\nIt is NOT refused at 10:30: the limit resets.\nCode: see below.", "{rep:?}");
    assert_eq!(rep.citations.len(), 1);
    // each quoted line is judged on its own: short pieces must be whole lines
    let w = ws();
    for scrap in [
        "x\ny\nz\nw\nv\nu\nt =",
        "return\n#\nTrue",
        "check\nreturn True",
    ] {
        assert!(
            !check_citation(&w, "app/session.py", 14, scrap)
                .found
                .checked(),
            "{scrap:?}"
        );
    }
    // what a model copies with a line is not held against it: the number with a colon, the mark of a cut line
    assert_eq!(
        check_citation(&w, "app/session.py", 14, "14:  return True").found,
        Found::AtLine
    );
    assert_eq!(
        check_citation(&w, "app/session.py", 14, "    return True... (cut)").found,
        Found::AtLine
    );
    // a blank line in the file between two quoted lines is not a mismatch
    let c = check_citation(&w, "app/session.py", 9, "return token\ndef logout(token):");
    assert_eq!(c.found, Found::AtLine, "{c:?}");
    assert_eq!(c.text, "    return token\n\n\ndef logout(token):");
    // a readable file is never taken for a withheld one whose name differs only by case
    let both = Workspace::from_imported(&[
        file("NOTES.TXT", "the plan is settled here\n", Level::Internal),
        file("notes.txt", "secret plan\n", Level::Restricted),
    ])
    .0;
    assert_eq!(
        check_citation(&both, "NOTES.TXT", 1, "the plan is settled here").found,
        Found::AtLine
    );
    assert_eq!(
        check_citation(&both, "notes.txt", 1, "secret plan").found,
        Found::Withheld
    );
}

#[test]
fn ex10_quotes_in_quotation_marks_ambiguous_places_long_lines_and_mixed_notations() {
    let w = ws();
    // a model that wraps the line in quotation marks or backticks quoted it correctly
    assert_eq!(
        check_citation(&w, "app/session.py", 14, "\"    return True\"").found,
        Found::AtLine
    );
    assert_eq!(
        check_citation(&w, "app/session.py", 14, "`return True`").found,
        Found::AtLine
    );
    // part of a line is found only when it is most of the line, and never inside a comment it leaves the mark of out
    let code = Workspace::from_imported(&[file(
        "auth.py",
        "# never call disable_auth() in production code\ndef check(user):\n    return is_admin(user) and not user.locked and user.active\nx = 1\ny = 2\nx = 1\n",
        Level::Internal,
    )])
    .0;
    assert!(!check_citation(&code, "auth.py", 1, "disable_auth()")
        .found
        .checked());
    // most of a comment line, with the comment mark left out, is still not code that runs
    assert!(!check_citation(
        &code,
        "auth.py",
        1,
        "never call disable_auth() in production code"
    )
    .found
    .checked());
    assert_eq!(
        check_citation(
            &code,
            "auth.py",
            1,
            "# never call disable_auth() in production code"
        )
        .found,
        Found::AtLine
    );
    assert!(
        !check_citation(&code, "auth.py", 3, "is_admin(user)")
            .found
            .checked(),
        "a third of the line, the rest reverses it"
    );
    assert_eq!(
        check_citation(
            &code,
            "auth.py",
            3,
            "return is_admin(user) and not user.locked"
        )
        .found,
        Found::AtLine
    );
    // text that stands at two places and not at the cited line is not placed by guessing the nearer one
    let twice = Workspace::from_imported(&[file(
        "dup.py",
        "value = compute()\nother = 1\nvalue = compute()\n",
        Level::Internal,
    )])
    .0;
    assert!(!check_citation(&twice, "dup.py", 2, "value = compute()")
        .found
        .checked());
    assert_eq!(
        check_citation(&twice, "dup.py", 3, "value = compute()").found,
        Found::AtLine
    );
    // short whole lines that fit at several places are not a place, however many of them are quoted
    let short = Workspace::from_imported(&[file(
        "s.py",
        "a\nb\nc\nzzzzzzzzzz\na\nb\nc\n",
        Level::Internal,
    )])
    .0;
    assert!(!check_citation(&short, "s.py", 1, "a\nb\nc").found.checked());
    // a quote longer than what is kept for showing is compared whole: a wrong ending is not found
    let long_line = format!("key = \"{}\"", "a".repeat(600));
    let long =
        Workspace::from_imported(&[file("l.py", &format!("{long_line}\n"), Level::Internal)]).0;
    assert_eq!(
        check_citation(&long, "l.py", 1, &long_line).found,
        Found::AtLine
    );
    let wrong_end = format!("key = \"{}b\"", "a".repeat(599));
    assert!(!check_citation(&long, "l.py", 1, &wrong_end).found.checked());
    // a text answer with a labelled place whose text is in a code block, a `path:line:` place, a sentence that only looks
    // like a place, and its own closing line with a colon
    let text = "Logout does not revoke the token.\nFile: app/session.py\nLine: 14\nText:\n```python\n    return True\n```\n- app/session.py:11: def logout(token):\napp/session.py:9 is where the token is returned.\nIn short: nothing is removed:";
    let r = route(vec![
        calls(&[("read_file", json!({"path": "app/session.py"}))]),
        ChatMessage::new("assistant", text),
    ]);
    let rep = run_question(
        &r,
        &mut ws(),
        "Does logout revoke the token?",
        Depth::Normal,
        None,
        NEVER,
    );
    assert_eq!(rep.citations.len(), 2, "{rep:?}");
    assert!(rep.citations.iter().all(|c| c.found.checked()), "{rep:?}");
    assert_eq!(rep.answer, "Logout does not revoke the token.\napp/session.py:9 is where the token is returned.\nIn short: nothing is removed:", "{rep:?}");
    // two answers in one reply: the first is read, and nothing is called corrected when the agent was never told
    let r = route(vec![
        calls(&[("read_file", json!({"path": "app/session.py"}))]),
        calls(&[
            (
                "answer",
                json!({"answer": "It returns True.", "places": [{"file": "app/session.py", "line": 14, "text": "return True"}, {"file": "app/session.py", "line": 14, "text": "return True"}]}),
            ),
            (
                "answer",
                json!({"answer": "It deletes the token.", "places": [{"file": "app/session.py", "line": 14, "text": "return True"}]}),
            ),
        ]),
    ]);
    let rep = run_question(
        &r,
        &mut ws(),
        "What does logout return?",
        Depth::Normal,
        None,
        NEVER,
    );
    assert_eq!(
        (rep.answer.as_str(), rep.corrected, rep.citations.len()),
        ("It returns True.", false, 1),
        "{rep:?}"
    );
}

#[test]
fn ex_cr01_a_file_with_old_mac_line_endings_is_cited_by_the_lines_an_editor_shows() {
    let (mut w, _) = Workspace::from_imported(&[file(
        "legacy/conf.txt",
        "name = demo\rport = 8080\rmode = fast\r",
        Level::Internal,
    )]);
    w.lone_returns_as_lines();
    let c = check_citation(&w, "legacy/conf.txt", 2, "port = 8080");
    assert_eq!(c.found, Found::AtLine, "{c:?}");
    // a Windows file is left as it is
    let (mut w, _) =
        Workspace::from_imported(&[file("win.txt", "a = 1\r\nb = 2\r\n", Level::Internal)]);
    w.lone_returns_as_lines();
    assert_eq!(w.file("win.txt"), Some("a = 1\r\nb = 2\r\n"));
    assert_eq!(
        check_citation(&w, "win.txt", 2, "b = 2").found,
        Found::AtLine
    );
}
