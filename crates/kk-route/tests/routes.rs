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

mod streaming {
    use kk_route::{ChatMessage, CompletionRequest, OllamaRoute, RouteError};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// A one-shot fake model server. It writes `lines` one at a time with a short pause and reports whether the client
    /// closed the connection before the last line.
    fn serve(lines: Vec<String>) -> (String, Arc<AtomicBool>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let cut_short = Arc::new(AtomicBool::new(false));
        let flag = cut_short.clone();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let mut got = Vec::new();
            while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = s.read(&mut buf).unwrap();
                if n == 0 {
                    return;
                }
                got.extend_from_slice(&buf[..n]);
            }
            let head = String::from_utf8_lossy(&got).to_lowercase();
            let len: usize = head
                .split("content-length: ")
                .nth(1)
                .and_then(|r| r.split("\r\n").next())
                .and_then(|n| n.trim().parse().ok())
                .unwrap_or(0);
            let body_start = got.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
            let mut have = got.len() - body_start;
            while have < len {
                have += s.read(&mut buf).unwrap();
            }
            s.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/x-ndjson\r\nconnection: close\r\n\r\n").unwrap();
            for l in lines {
                if s.write_all(format!("{l}\n").as_bytes()).is_err() || s.flush().is_err() {
                    flag.store(true, Ordering::SeqCst);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
        });
        (url, cut_short)
    }

    fn req() -> CompletionRequest {
        CompletionRequest {
            messages: vec![ChatMessage::new("user", "hi")],
            tools: vec![],
            max_tokens: 50,
            temperature: 0.0,
        }
    }

    fn chunk(t: &str) -> String {
        format!(
            "{{\"message\":{{\"role\":\"assistant\",\"content\":{}}},\"done\":false}}",
            serde_json::to_string(t).unwrap()
        )
    }

    const DONE: &str = "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"prompt_eval_count\":7,\"eval_count\":3}";

    #[test]
    fn st01_chunks_arrive_in_order_and_the_totals_come_from_the_final_line() {
        let (url, _) = serve(vec![
            chunk("Hel"),
            chunk("lo "),
            chunk("wörld"),
            DONE.into(),
        ]);
        let route = OllamaRoute::new(&url, "m").unwrap();
        let mut seen = Vec::new();
        let out = route.stream(&req(), &mut |c| {
            seen.push(c.to_string());
            true
        });
        let out = out.unwrap();
        assert_eq!(seen, ["Hel", "lo ", "wörld"]);
        assert_eq!(
            (
                out.text.as_str(),
                out.stopped,
                out.prompt_tokens,
                out.completion_tokens
            ),
            ("Hello wörld", false, 7, 3)
        );
    }

    #[test]
    fn st02_stopping_early_closes_the_connection_so_the_model_stops() {
        let many: Vec<String> = (0..60).map(|i| chunk(&format!("w{i} "))).collect();
        let (url, cut_short) = serve(many);
        let route = OllamaRoute::new(&url, "m").unwrap();
        let mut n = 0;
        let out = route.stream(&req(), &mut |_| {
            n += 1;
            n < 3
        });
        let out = out.unwrap();
        assert!(out.stopped && out.text == "w0 w1 w2 ", "{out:?}");
        std::thread::sleep(std::time::Duration::from_millis(700));
        assert!(
            cut_short.load(Ordering::SeqCst),
            "the server saw the connection close long before it finished"
        );
    }

    #[test]
    fn st03_broken_streams_are_errors_not_partial_successes() {
        for (label, lines) in [
            ("ends without a final line", vec![chunk("a"), chunk("b")]),
            ("not json", vec![chunk("a"), "this is not json".to_string()]),
            (
                "model error",
                vec!["{\"error\":\"model not found\"}".to_string()],
            ),
        ] {
            let (url, _) = serve(lines);
            let route = OllamaRoute::new(&url, "m").unwrap();
            let r = route.stream(&req(), &mut |_| true);
            assert!(
                matches!(r, Err(RouteError::BadResponse(_))),
                "{label}: {r:?}"
            );
        }
        let (url, _) = serve(vec![
            chunk(&"x".repeat(200_000)),
            chunk(&"y".repeat(200_000)),
            DONE.into(),
        ]);
        let r = OllamaRoute::new(&url, "m")
            .unwrap()
            .stream(&req(), &mut |_| true);
        assert!(matches!(r, Err(RouteError::BadResponse(m)) if m.contains("longer than allowed")));
    }

    #[test]
    fn st04_pull_reports_progress_ends_on_success_and_treats_errors_and_cut_streams_as_failures() {
        let ok_lines = vec![
            "{\"status\":\"pulling manifest\"}".to_string(),
            "{\"status\":\"pulling abc\",\"total\":200,\"completed\":50}".to_string(),
            "{\"status\":\"pulling abc\",\"total\":200,\"completed\":999}".to_string(),
            "{\"status\":\"success\"}".to_string(),
        ];
        let (url, _) = serve(ok_lines);
        let mut seen: Vec<(String, Option<u8>)> = Vec::new();
        OllamaRoute::new(&url, "m")
            .unwrap()
            .pull(&mut |s, p| {
                seen.push((s.to_string(), p));
                true
            })
            .unwrap();
        assert_eq!(seen[0], ("pulling manifest".to_string(), None));
        assert_eq!(seen[1].1, Some(25));
        assert_eq!(seen[2].1, Some(100), "more than the total is capped at 100");
        assert_eq!(seen.last().unwrap().0, "success");
        for (label, lines) in [
            (
                "model error",
                vec!["{\"error\":\"pull model manifest: file does not exist\"}".to_string()],
            ),
            (
                "cut off",
                vec!["{\"status\":\"pulling manifest\"}".to_string()],
            ),
            ("not json", vec!["nonsense".to_string()]),
        ] {
            let (url, _) = serve(lines);
            let r = OllamaRoute::new(&url, "m").unwrap().pull(&mut |_, _| true);
            assert!(
                matches!(r, Err(RouteError::BadResponse(_))),
                "{label}: {r:?}"
            );
        }
        let (url, _) = serve(
            (0..40)
                .map(|i| format!("{{\"status\":\"step {i}\"}}"))
                .collect(),
        );
        let r = OllamaRoute::new(&url, "m").unwrap().pull(&mut |_, _| false);
        assert!(matches!(r, Err(RouteError::BadResponse(m)) if m.contains("stopped waiting")));
    }
}
