use kk_broker::{Broker, BrokerConfig, SessionId};
use kk_kernel::JobKey;
use kk_policy::{Engine, WorkerFacts};
use kk_route::{ChatMessage, Route, RouteClass, RouteSet, ScriptedRoute};
use kk_types::chat::ToolCall;
use kk_types::label::{Label, Level};
use serde_json::{json, Value};

fn session(script: Vec<ChatMessage>) -> (Broker, SessionId) {
    let route = ScriptedRoute::new("local", RouteClass::DeviceOnly, false, script);
    let mut b = Broker::new(
        Engine::default_policy(),
        RouteSet::new(vec![Box::new(route) as Box<dyn Route>]),
    );
    let s = b.open_session(
        &JobKey::new("tenant", "job-1"),
        WorkerFacts {
            id: "w".into(),
            kind: "AGENT".into(),
            clearance: Label::new(Level::Internal),
            allowed_tools: vec![],
        },
        BrokerConfig::default(),
    );
    (b, s)
}

fn ask(b: &mut Broker, s: SessionId, tools: Value) -> Value {
    let line = json!({"id": 1, "op": "model.complete", "messages": [{"role": "user", "content": "go"}], "tools": tools, "max_tokens": 64}).to_string();
    serde_json::from_str(&b.handle_line(s, &line)).unwrap()
}

fn offered() -> Value {
    json!([{"name": "list_files", "description": "d", "parameters": {"type": "object"}},
           {"name": "read_file", "description": "d", "parameters": {"type": "object"}}])
}

#[test]
fn tc01_a_call_written_as_text_becomes_a_call_when_the_tool_was_offered() {
    let (mut b, s) = session(vec![ChatMessage::new(
        "assistant",
        r#"{"name": "list_files", "arguments": {}}"#,
    )]);
    let r = ask(&mut b, s, offered());
    assert_eq!(r["ok"], true, "{r}");
    let m = &r["result"]["message"];
    assert_eq!(m["tool_calls"][0]["name"], "list_files", "{m}");
    assert_eq!(
        m["content"], "",
        "the text that was the call is not kept as an answer: {m}"
    );
}

#[test]
fn tc02_a_tool_that_was_not_offered_stays_text() {
    let (mut b, s) = session(vec![ChatMessage::new(
        "assistant",
        r#"{"name": "delete_everything", "arguments": {}}"#,
    )]);
    let r = ask(&mut b, s, offered());
    let m = &r["result"]["message"];
    assert!(
        m.get("tool_calls")
            .is_none_or(|t| t.as_array().is_some_and(Vec::is_empty)),
        "{m}"
    );
    assert!(m["content"].as_str().unwrap().contains("delete_everything"));
}

#[test]
fn tc03_with_no_tools_offered_nothing_is_recovered() {
    let (mut b, s) = session(vec![ChatMessage::new(
        "assistant",
        r#"{"name": "list_files", "arguments": {}}"#,
    )]);
    let r = ask(&mut b, s, json!([]));
    let m = &r["result"]["message"];
    assert!(
        m.get("tool_calls")
            .is_none_or(|t| t.as_array().is_some_and(Vec::is_empty)),
        "{m}"
    );
}

#[test]
fn tc04_a_structured_call_is_left_exactly_as_it_came() {
    let mut msg = ChatMessage::new("assistant", "reading");
    msg.tool_calls = vec![ToolCall {
        id: "c1".into(),
        name: "read_file".into(),
        arguments: json!({"path": "a.py"}),
    }];
    let (mut b, s) = session(vec![msg]);
    let r = ask(&mut b, s, offered());
    let m = &r["result"]["message"];
    assert_eq!(m["tool_calls"].as_array().unwrap().len(), 1);
    assert_eq!(m["tool_calls"][0]["id"], "c1");
    assert_eq!(m["content"], "reading");
}

#[test]
fn tc05_prose_that_only_mentions_a_tool_is_an_answer() {
    let (mut b, s) = session(vec![ChatMessage::new(
        "assistant",
        "I would call list_files first, but the project looks fine.",
    )]);
    let r = ask(&mut b, s, offered());
    let m = &r["result"]["message"];
    assert!(m
        .get("tool_calls")
        .is_none_or(|t| t.as_array().is_some_and(Vec::is_empty)));
    assert!(m["content"].as_str().unwrap().contains("looks fine"));
}

#[test]
fn tc06_a_reply_with_a_real_call_does_not_also_run_a_call_written_in_its_text() {
    let mut msg = ChatMessage::new("assistant", r#"{"name": "list_files", "arguments": {}}"#);
    msg.tool_calls = vec![ToolCall {
        id: "c1".into(),
        name: "read_file".into(),
        arguments: json!({"path": "a.py"}),
    }];
    let (mut b, s) = session(vec![msg]);
    let r = ask(&mut b, s, offered());
    let m = &r["result"]["message"];
    assert_eq!(m["tool_calls"].as_array().unwrap().len(), 1, "{m}");
    assert_eq!(m["tool_calls"][0]["name"], "read_file");
}
