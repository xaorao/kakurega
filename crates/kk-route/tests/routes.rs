use kk_route::{
    is_loopback_http, ChatMessage, CompletionRequest, OllamaRoute, Route, RouteClass, RouteError,
    RouteSet, ScriptedRoute, ToolSpec,
};
use kk_types::label::{Label, Level};
use serde_json::json;

#[test]
fn only_loopback_http_endpoints_count_as_device_only() {
    for ok in [
        "http://127.0.0.1:11434",
        "http://localhost:11434/api",
        "http://[::1]:11434",
        "http://127.0.0.1",
    ] {
        assert!(is_loopback_http(ok), "{ok}");
    }
    for bad in [
        "https://127.0.0.1:11434",
        "http://10.0.0.5:11434",
        "http://127.0.0.1@evil.example/",
        "http://evil.example@127.0.0.1/",
        "http://127.0.0.1.evil.example/",
        "http://evil.example/127.0.0.1",
        "http://2130706433/",
        "http://0.0.0.0:11434",
        "http://localhost.evil.example",
        "ftp://127.0.0.1",
        "",
        "127.0.0.1",
    ] {
        assert!(!is_loopback_http(bad), "{bad}");
    }
}

#[test]
fn the_device_only_route_refuses_to_be_pointed_at_the_network() {
    assert!(matches!(
        OllamaRoute::new("http://api.example.com", "m"),
        Err(RouteError::EndpointNotLoopback(_))
    ));
    assert!(matches!(
        OllamaRoute::new("https://localhost:1", "m"),
        Err(RouteError::EndpointNotLoopback(_))
    ));
    assert!(OllamaRoute::new("http://127.0.0.1:11434", "m").is_ok());
}

#[test]
fn selection_never_falls_back_to_something_weaker() {
    let restricted = Label::new(Level::Restricted);
    let unverified: Vec<Box<dyn Route>> = vec![Box::new(ScriptedRoute::new(
        "tee-unverified",
        RouteClass::VerifiedTee,
        false,
        vec![],
    ))];
    assert!(matches!(
        RouteSet::new(unverified).select(&restricted),
        Err(RouteError::NoEligibleRoute)
    ));
    let public_only: Vec<Box<dyn Route>> = vec![Box::new(ScriptedRoute::new(
        "public",
        RouteClass::Public,
        false,
        vec![],
    ))];
    assert!(matches!(
        RouteSet::new(public_only).select(&Label::bottom()),
        Err(RouteError::NoEligibleRoute)
    ));
    let mixed: Vec<Box<dyn Route>> = vec![
        Box::new(ScriptedRoute::new(
            "public",
            RouteClass::Public,
            false,
            vec![],
        )),
        Box::new(ScriptedRoute::new(
            "tee-unverified",
            RouteClass::VerifiedTee,
            false,
            vec![],
        )),
        Box::new(ScriptedRoute::new(
            "local",
            RouteClass::DeviceOnly,
            false,
            vec![],
        )),
    ];
    assert_eq!(
        RouteSet::new(mixed)
            .select(&restricted)
            .map(|r| r.id().to_string())
            .unwrap(),
        "local"
    );
    let verified: Vec<Box<dyn Route>> = vec![Box::new(ScriptedRoute::new(
        "tee",
        RouteClass::VerifiedTee,
        true,
        vec![],
    ))];
    assert!(RouteSet::new(verified).select(&restricted).is_ok());
}

#[test]
fn the_scripted_route_is_labelled_simulated_and_runs_out() {
    let r = ScriptedRoute::new(
        "s",
        RouteClass::DeviceOnly,
        false,
        vec![ChatMessage::new("assistant", "one")],
    );
    assert!(r.evidence().simulated);
    let req = CompletionRequest {
        messages: vec![ChatMessage::new("user", "hi")],
        tools: vec![],
        max_tokens: 16,
        temperature: 0.0,
    };
    assert_eq!(r.complete(&req).unwrap().message.content, "one");
    assert!(matches!(r.complete(&req), Err(RouteError::ScriptExhausted)));
    assert_eq!(r.seen.lock().unwrap().len(), 2);
}

#[test]
#[ignore]
fn live_local_model_answers_and_can_call_a_tool() {
    let route = OllamaRoute::new("http://127.0.0.1:11434", "qwen2.5:1.5b").expect("route");
    let ev = route.evidence();
    println!("evidence: {}", serde_json::to_string(&ev).unwrap());
    assert!(
        !ev.simulated && ev.model_digest.is_some(),
        "digest must come from the live server"
    );
    let plain = route
        .complete(&CompletionRequest {
            messages: vec![ChatMessage::new(
                "user",
                "Reply with exactly one word: ready",
            )],
            tools: vec![],
            max_tokens: 16,
            temperature: 0.0,
        })
        .expect("completion");
    println!(
        "plain reply: {:?} ({} tokens)",
        plain.message.content, plain.completion_tokens
    );
    assert!(plain.message.content.to_lowercase().contains("ready"));
    let tools = vec![ToolSpec {
        name: "read_file".into(),
        description: "Read a file from the project and return its text.".into(),
        parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
    }];
    let call = route
        .complete(&CompletionRequest {
            messages: vec![
                ChatMessage::new(
                    "system",
                    "You review code. Use tools to read files; do not guess their contents.",
                ),
                ChatMessage::new("user", "Open src/auth/session.rs and tell me what it does."),
            ],
            tools,
            max_tokens: 128,
            temperature: 0.0,
        })
        .expect("tool completion");
    println!("tool calls: {:?}", call.message.tool_calls);
    assert!(
        !call.message.tool_calls.is_empty(),
        "the local model should ask to read the file"
    );
    assert_eq!(call.message.tool_calls[0].name, "read_file");
}
