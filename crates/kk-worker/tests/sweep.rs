use kk_types::chat::{ChatMessage, ToolCall};
use kk_worker::{
    run, source_files_ranked, suspicions_in, AgentConfig, AgentState, Link, Outcome, WorkerError,
};
use serde_json::{json, Value};

/// A broker with a small project and a model that answers by what it is asked: the review's own conversation takes the
/// scripted replies; a fresh "what could be wrong in this file" names a suspicion only for the session file; a fresh
/// "prove it" writes a script. The check confirms a script that holds the word GOOD and gives a note for any other.
struct Project {
    /// When set, every fresh question fails the way a model route that stopped answering does.
    model_down: bool,
    paths: Vec<&'static str>,
    replies: Vec<ChatMessage>,
    /// What the model answers to "prove it", in order; the last one is repeated.
    proofs: Vec<Value>,
    main_calls: usize,
    suspect_files: Vec<String>,
    prove_asks: Vec<Vec<ChatMessage>>,
    reads: Vec<String>,
    findings: Vec<Value>,
}

impl Link for Project {
    fn call(&mut self, request: Value) -> Result<Value, WorkerError> {
        match request["op"].as_str().unwrap_or("") {
            "model.complete" => {
                let msgs: Vec<ChatMessage> =
                    serde_json::from_value(request["messages"].clone()).unwrap();
                let system = msgs[0].content.clone();
                if self.model_down && !system.starts_with("sys") {
                    return Ok(
                        json!({"ok": false, "error": {"code": "TOOL_ERROR", "detail": "model route failed"}}),
                    );
                }
                let m = if system.starts_with("You are a security reviewer.") {
                    assert_eq!(
                        msgs.len(),
                        2,
                        "a second look starts with nothing of the review's conversation"
                    );
                    assert_eq!(request["tools"], json!([]));
                    let file = msgs[1]
                        .content
                        .lines()
                        .next()
                        .unwrap()
                        .trim_start_matches("FILE ")
                        .to_string();
                    self.suspect_files.push(file.clone());
                    if file.ends_with("session.py") {
                        ChatMessage::new("assistant", "```json\n[{\"line\": 3, \"summary\": \"logout keeps the token\"}, {\"line\": 0, \"summary\": \"bad line\"}, {\"line\": 9, \"summary\": \"\"}]\n```")
                    } else {
                        ChatMessage::new("assistant", "[]")
                    }
                } else if system.starts_with("You check one suspected security defect") {
                    self.prove_asks.push(msgs);
                    let v = if self.proofs.len() > 1 {
                        self.proofs.remove(0)
                    } else {
                        self.proofs[0].clone()
                    };
                    ChatMessage::new("assistant", &format!("Here it is:\n{v}"))
                } else {
                    self.main_calls += 1;
                    if self.replies.is_empty() {
                        ChatMessage::new("assistant", "Done.")
                    } else {
                        self.replies.remove(0)
                    }
                };
                Ok(json!({"ok": true, "result": {"message": m}}))
            }
            "tool.call" => match request["name"].as_str().unwrap() {
                "list_files" => Ok(json!({"ok": true, "result": {"paths": self.paths}})),
                "read_file" => {
                    let path = request["args"]["path"].as_str().unwrap().to_string();
                    assert!(
                        self.paths.contains(&path.as_str()),
                        "{path} was never listed"
                    );
                    self.reads.push(path.clone());
                    Ok(
                        json!({"ok": true, "result": {"path": path, "text": "def login():\n    pass\ndef logout():\n    pass\n"}}),
                    )
                }
                _ => Ok(json!({"ok": true, "result": {}})),
            },
            "write.artifact" => {
                let raw = kk_evidence::b64::decode(request["bytes_b64"].as_str().unwrap()).unwrap();
                let finding: Value = serde_json::from_slice(&raw).unwrap();
                let good = finding["reproducer"]
                    .as_str()
                    .unwrap_or("")
                    .contains("GOOD");
                self.findings.push(finding);
                let feedback = if good {
                    json!({"status": "confirmed", "note": "The script reproduced the defect."})
                } else {
                    json!({"status": "not_reproduced", "note": "The script printed CONTROL_OK but neither REPRODUCED nor NOT REPRODUCED."})
                };
                Ok(
                    json!({"ok": true, "result": {"handle": self.findings.len(), "feedback": feedback}}),
                )
            }
            _ => Ok(json!({"ok": true, "result": {}})),
        }
    }
}

fn project(replies: Vec<ChatMessage>, proofs: Vec<Value>) -> Project {
    Project {
        model_down: false,
        paths: vec![
            "README.md",
            "payments/__init__.py",
            "payments/config.py",
            "payments/util/backoff.py",
            "payments/auth/session.py",
            "payments/api/handlers.py",
            "tests/test_session.py",
            "docs/vendor-notes.md",
        ],
        replies,
        proofs,
        main_calls: 0,
        suspect_files: vec![],
        prove_asks: vec![],
        reads: vec![],
        findings: vec![],
    }
}

fn config(sweep_files: usize) -> AgentConfig {
    AgentConfig {
        tools: vec![],
        local_tools: vec![kk_worker::finding_tool()],
        max_turns: 40,
        max_tokens: 64,
        min_looks: 0,
        sweep_files,
    }
}

fn say(text: &str) -> ChatMessage {
    ChatMessage::new("assistant", text)
}

fn proof(script: &str) -> Value {
    json!({"real": true, "title": "Logout keeps the token", "line": 3, "explanation": "A logged-out token still validates.", "reproducer": script})
}

#[test]
fn ws01_every_source_file_gets_a_second_look_on_its_own_most_telling_first_and_a_proved_suspicion_becomes_a_finding(
) {
    let mut link = project(vec![say("No defects.")], vec![proof("print('GOOD')")]);
    let state = AgentState::new("sys", "review");
    let r = run(&mut link, state, &config(12), &|| false).unwrap();
    assert_eq!(
        r,
        Outcome::Finished("No defects.".into()),
        "the review's own ending stands"
    );
    assert_eq!(
        link.main_calls, 1,
        "the second look adds nothing to the review's conversation"
    );
    assert_eq!(
        link.suspect_files,
        vec![
            "payments/auth/session.py",
            "payments/api/handlers.py",
            "payments/config.py",
            "payments/__init__.py",
            "payments/util/backoff.py"
        ],
        "sessions first; never a test or a document"
    );
    assert_eq!(
        link.reads, link.suspect_files,
        "each file was read through the broker, once"
    );
    // one usable suspicion (a line of zero and an empty summary are dropped), asked about with only the file in front
    assert_eq!(link.prove_asks.len(), 1);
    let ask = &link.prove_asks[0];
    assert_eq!(ask.len(), 2);
    assert!(
        ask[1]
            .content
            .starts_with("FILE payments/auth/session.py\n1: def login():\n")
            && ask[1]
                .content
                .ends_with("Suspected defect at line 3: logout keeps the token"),
        "{}",
        ask[1].content
    );
    assert!(
        ask[0].content.contains("It is Python")
            && ask[0].content.contains("never with an assert")
            && ask[0].content.contains("from shop.cart import Cart")
    );
    assert_eq!(link.findings.len(), 1);
    assert_eq!(
        link.findings[0],
        json!({"title": "Logout keeps the token", "file": "payments/auth/session.py", "line": 3, "explanation": "A logged-out token still validates.", "reproducer": "print('GOOD')", "language": "python"})
    );
}

#[test]
fn ws02_a_script_the_check_does_not_confirm_gets_one_correction_with_the_checks_own_words_and_no_more(
) {
    // first script fails the check, the corrected one passes: two reports under one title
    let mut link = project(
        vec![say("Done.")],
        vec![
            proof("print('bad')"),
            json!({"real": true, "title": "Another name", "line": 3, "explanation": "e", "reproducer": "print('GOOD')"}),
        ],
    );
    run(
        &mut link,
        AgentState::new("sys", "review"),
        &config(12),
        &|| false,
    )
    .unwrap();
    assert_eq!(link.findings.len(), 2);
    assert_eq!(
        link.findings[1]["title"],
        json!("Logout keeps the token"),
        "the correction keeps the title, so it is the same finding"
    );
    let second = &link.prove_asks[1];
    assert_eq!(second.len(), 4);
    assert!(
        second[3]
            .content
            .contains("printed CONTROL_OK but neither REPRODUCED nor NOT REPRODUCED")
            && second[3].content.contains("same title"),
        "{}",
        second[3].content
    );

    // never confirmed: two reports, then it stops
    let mut never = project(vec![say("Done.")], vec![proof("print('bad')")]);
    run(
        &mut never,
        AgentState::new("sys", "review"),
        &config(12),
        &|| false,
    )
    .unwrap();
    assert_eq!((never.findings.len(), never.prove_asks.len()), (2, 2));

    // the model says the suspicion is not real, or answers with something unreadable: nothing is reported
    for answer in [
        json!({"real": false}),
        json!({"real": false, "title": "t", "reproducer": "print('GOOD')"}),
        json!({"real": true, "title": "t", "reproducer": "  "}),
        json!("no idea"),
    ] {
        let mut none = project(vec![say("Done.")], vec![answer]);
        run(
            &mut none,
            AgentState::new("sys", "review"),
            &config(12),
            &|| false,
        )
        .unwrap();
        assert_eq!((none.findings.len(), none.prove_asks.len()), (0, 1));
    }
}

#[test]
fn ws03_the_second_look_has_a_limit_can_be_off_skips_a_file_already_reported_and_survives_a_restart(
) {
    let mut two = project(vec![say("Done.")], vec![proof("print('GOOD')")]);
    run(
        &mut two,
        AgentState::new("sys", "review"),
        &config(2),
        &|| false,
    )
    .unwrap();
    assert_eq!(
        two.suspect_files,
        vec!["payments/auth/session.py", "payments/api/handlers.py"]
    );

    let mut off = project(vec![say("Done.")], vec![proof("x")]);
    run(
        &mut off,
        AgentState::new("sys", "review"),
        &config(0),
        &|| false,
    )
    .unwrap();
    assert!(off.suspect_files.is_empty() && off.reads.is_empty());

    // the review itself reported a finding in the session file: that file is not looked at again
    let mut reported = {
        let mut m = ChatMessage::new("assistant", "");
        m.tool_calls = vec![ToolCall {
            id: "f".into(),
            name: "report_finding".into(),
            arguments: json!({"title": "t", "file": "payments/auth/session.py", "line": 3, "explanation": "e", "reproducer": "print('GOOD')"}),
        }];
        project(vec![m, say("Done.")], vec![proof("x")])
    };
    run(
        &mut reported,
        AgentState::new("sys", "review"),
        &config(12),
        &|| false,
    )
    .unwrap();
    assert!(
        !reported
            .suspect_files
            .iter()
            .any(|f| f.ends_with("session.py")),
        "{:?}",
        reported.suspect_files
    );
    assert_eq!(reported.suspect_files.len(), 4);

    // a worker that starts again from its saved state looks at no file twice
    let mut state = AgentState::new("sys", "review");
    state.messages.push(say("Done."));
    state.turn = 1;
    state.looked = vec![
        "payments/auth/session.py".into(),
        "payments/api/handlers.py".into(),
    ];
    let saved = serde_json::to_value(&state).unwrap();
    let state: AgentState = serde_json::from_value(saved).unwrap();
    let mut again = project(vec![say("Done again.")], vec![proof("x")]);
    run(&mut again, state, &config(12), &|| false).unwrap();
    assert_eq!(
        again.suspect_files,
        vec![
            "payments/config.py",
            "payments/__init__.py",
            "payments/util/backoff.py"
        ]
    );
    // and a state saved before this existed still loads
    let old: AgentState =
        serde_json::from_value(json!({"version": 1, "messages": [], "turn": 0})).unwrap();
    assert!(old.looked.is_empty());
}

#[test]
fn ws04_the_ranking_and_the_reading_of_suspicions_are_what_they_say() {
    let paths: Vec<String> = [
        "src/zeta.rs",
        "src/login/form.ts",
        "src/a.rs",
        "lib/tokens.js",
        "src/__tests__/login.test.ts",
        "notes.txt",
        "Makefile",
        "src/Auth.PY",
        "spec/auth_spec.rb",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        source_files_ranked(&paths, &["src/a.rs".to_string()]),
        vec![
            "src/Auth.PY",
            "src/login/form.ts",
            "lib/tokens.js",
            "src/zeta.rs"
        ]
    );
    assert!(source_files_ranked(&[], &[]).is_empty());
    // the folder all files share does not decide the order
    let shared: Vec<String> = ["payments/config.py", "payments/auth/session.py"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        source_files_ranked(&shared, &[])[0],
        "payments/auth/session.py"
    );

    assert_eq!(suspicions_in("[]"), vec![]);
    assert_eq!(suspicions_in("I see nothing wrong."), vec![]);
    assert_eq!(suspicions_in("{\"line\": 3}"), vec![]);
    let three = "[{\"line\": 1, \"summary\": \"a\"}, {\"line\": 2, \"summary\": \"b\"}, {\"line\": 3, \"summary\": \"c\"}]";
    assert_eq!(
        suspicions_in(three),
        vec![(1, "a".to_string()), (2, "b".to_string())],
        "at most two are followed up"
    );
    assert_eq!(
        suspicions_in(&format!(
            "[{{\"line\": 4, \"summary\": \"{}\"}}]",
            "x".repeat(900)
        ))[0]
            .1
            .len(),
        400
    );
}

#[test]
fn ws05_a_model_that_stops_answering_during_the_second_look_ends_the_second_look_and_not_the_review(
) {
    let mut link = project(vec![say("No defects.")], vec![proof("print('GOOD')")]);
    link.model_down = true;
    let r = run(
        &mut link,
        AgentState::new("sys", "review"),
        &config(12),
        &|| false,
    )
    .unwrap();
    assert_eq!(r, Outcome::Finished("No defects.".into()));
    assert_eq!(
        link.reads,
        vec!["payments/auth/session.py"],
        "it stopped at the first file"
    );
    assert!(link.findings.is_empty());
    // a stop asked for by the owner is still a stop
    let mut stopped = project(vec![say("No defects.")], vec![proof("print('GOOD')")]);
    let asked = std::cell::Cell::new(0);
    let r = run(
        &mut stopped,
        AgentState::new("sys", "review"),
        &config(12),
        &|| {
            asked.set(asked.get() + 1);
            asked.get() > 2
        },
    )
    .unwrap();
    assert_eq!(r, Outcome::Cancelled);
}
