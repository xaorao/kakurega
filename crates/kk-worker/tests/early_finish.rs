use kk_types::chat::{ChatMessage, ToolCall};
use kk_worker::{run, AgentConfig, AgentState, Link, Outcome, WorkerError};
use serde_json::{json, Value};

/// A broker with a scripted model: each `model.complete` takes the next message; tools answer with a short result.
struct Script {
    replies: Vec<ChatMessage>,
    seen: Vec<Vec<ChatMessage>>,
}

impl Link for Script {
    fn call(&mut self, request: Value) -> Result<Value, WorkerError> {
        match request["op"].as_str().unwrap_or("") {
            "model.complete" => {
                let msgs: Vec<ChatMessage> =
                    serde_json::from_value(request["messages"].clone()).unwrap();
                self.seen.push(msgs);
                let m = self.replies.remove(0);
                Ok(json!({"ok": true, "result": {"message": m}}))
            }
            "tool.call" => Ok(json!({"ok": true, "result": {"files": ["a.py"]}})),
            "write.artifact" => Ok(json!({"ok": true, "result": {"handle": 1}})),
            _ => Ok(json!({"ok": true, "result": {}})),
        }
    }
}

fn config() -> AgentConfig {
    AgentConfig {
        tools: vec![],
        local_tools: vec![kk_worker::finding_tool()],
        max_turns: 12,
        max_tokens: 64,
        min_looks: 4,
        sweep_files: 0,
    }
}

fn say(text: &str) -> ChatMessage {
    ChatMessage::new("assistant", text)
}

fn call(id: &str, name: &str, args: Value) -> ChatMessage {
    let mut m = ChatMessage::new("assistant", "");
    m.tool_calls = vec![ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: args,
    }];
    m
}

fn nudges(seen: &[Vec<ChatMessage>]) -> usize {
    seen.last().map_or(0, |m| {
        m.iter()
            .filter(|x| x.role == "user" && x.content.contains("stopped after"))
            .count()
    })
}

fn finding() -> Value {
    json!({"title": "t", "file": "a.py", "line": 1, "explanation": "e", "reproducer": "print(1)"})
}

#[test]
fn ef01_a_model_that_stops_after_one_look_is_told_once_to_read_more_and_may_then_finish() {
    let mut link = Script {
        replies: vec![
            call("1", "list_files", json!({})),
            say("No defects."),
            say("Still none."),
        ],
        seen: vec![],
    };
    let r = run(
        &mut link,
        AgentState::new("sys", "review"),
        &config(),
        &|| false,
    )
    .unwrap();
    assert_eq!(r, Outcome::Finished("Still none.".into()));
    assert_eq!(link.seen.len(), 3, "the nudge cost one more model call");
    assert_eq!(nudges(&link.seen), 1, "told once, never twice");
}

#[test]
fn ef02_a_model_that_reported_a_finding_is_never_nudged() {
    let mut link = Script {
        replies: vec![
            call("1", "list_files", json!({})),
            call("2", "report_finding", finding()),
            say("Done."),
        ],
        seen: vec![],
    };
    let r = run(
        &mut link,
        AgentState::new("sys", "review"),
        &config(),
        &|| false,
    )
    .unwrap();
    assert_eq!(r, Outcome::Finished("Done.".into()));
    assert_eq!(link.seen.len(), 3);
    assert_eq!(nudges(&link.seen), 0);
}

#[test]
fn ef03_a_model_that_read_enough_before_finishing_is_not_nudged() {
    let mut link = Script {
        replies: vec![
            call("1", "list_files", json!({})),
            call("2", "read_file", json!({"path": "a.py"})),
            call("3", "read_file", json!({"path": "b.py"})),
            call("4", "read_file", json!({"path": "c.py"})),
            say("Nothing wrong."),
        ],
        seen: vec![],
    };
    let r = run(
        &mut link,
        AgentState::new("sys", "review"),
        &config(),
        &|| false,
    )
    .unwrap();
    assert_eq!(r, Outcome::Finished("Nothing wrong.".into()));
    assert_eq!(nudges(&link.seen), 0);
}

#[test]
fn ef04_the_nudge_is_not_repeated_after_a_restart_from_a_saved_state() {
    let mut state = AgentState::new("sys", "review");
    state.messages.push(call("1", "list_files", json!({})));
    state.messages.push(ChatMessage::new("tool", "{}"));
    state.messages.last_mut().unwrap().tool_call_id = Some("1".into());
    state.messages.push(say("None."));
    state.messages.push(ChatMessage::new(
        "user",
        "You stopped after reading only 1 files. Read more.",
    ));
    let mut link = Script {
        replies: vec![say("Really none.")],
        seen: vec![],
    };
    let r = run(&mut link, state, &config(), &|| false).unwrap();
    assert_eq!(r, Outcome::Finished("Really none.".into()));
    assert_eq!(link.seen.len(), 1);
}

#[test]
fn ef05_with_the_policy_off_a_short_run_ends_as_it_always_did() {
    let mut link = Script {
        replies: vec![say("None.")],
        seen: vec![],
    };
    let mut off = config();
    off.min_looks = 0;
    let r = run(&mut link, AgentState::new("sys", "review"), &off, &|| false).unwrap();
    assert_eq!(r, Outcome::Finished("None.".into()));
    assert_eq!(link.seen.len(), 1);
}
