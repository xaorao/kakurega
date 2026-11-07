use kk_route::{ChatMessage, CompletionRequest, OllamaRoute, RouteError};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

/// A one-connection stand-in for Ollama: after the request it sends the chat lines, each after its own pause.
fn server(script: Vec<(u64, &'static str)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((mut c, _)) = listener.accept() {
            let mut buf = [0u8; 65536];
            let mut seen = Vec::new();
            loop {
                let n = c.read(&mut buf).unwrap_or(0);
                seen.extend_from_slice(&buf[..n]);
                if n == 0 || seen.windows(4).any(|w| w == b"\r\n\r\n") && seen.ends_with(b"}") {
                    break;
                }
            }
            let _ = c.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/x-ndjson\r\nconnection: close\r\n\r\n");
            for (pause_ms, line) in script {
                std::thread::sleep(Duration::from_millis(pause_ms));
                if c.write_all(line.as_bytes()).is_err() || c.write_all(b"\n").is_err() {
                    return;
                }
                let _ = c.flush();
            }
            std::thread::sleep(Duration::from_secs(30));
        }
    });
    format!("http://{addr}")
}

fn request() -> CompletionRequest {
    CompletionRequest {
        messages: vec![ChatMessage::new("user", "hi")],
        tools: vec![],
        max_tokens: 64,
        temperature: 0.0,
    }
}

const PIECE: &str = r#"{"message":{"content":"ab"},"done":false}"#;
const DONE: &str = r#"{"message":{"content":""},"done":true,"prompt_eval_count":3,"eval_count":5}"#;

#[test]
fn st01_a_streamed_answer_that_goes_quiet_is_given_up_after_the_stall_time_and_what_arrived_is_kept_out_of_the_error(
) {
    let base = server(vec![(0, PIECE), (0, PIECE)]);
    let route = OllamaRoute::new(&base, "m")
        .unwrap()
        .with_timeout(Duration::from_secs(20))
        .with_stall(Duration::from_millis(700));
    let mut got = String::new();
    let t = Instant::now();
    let r = route.stream(&request(), &mut |p| {
        got.push_str(p);
        true
    });
    assert!(
        matches!(r, Err(RouteError::TimedOut(ref m)) if m.contains("went quiet")),
        "{r:?}"
    );
    assert!(
        t.elapsed() < Duration::from_secs(5),
        "gave up after the stall time, not the call limit: {:?}",
        t.elapsed()
    );
    assert_eq!(
        got, "abab",
        "the pieces that arrived were delivered before the stall was noticed"
    );
}

#[test]
fn st02_a_living_answer_longer_than_the_call_limit_is_not_cut() {
    // five pieces 400 ms apart take two seconds in all; the call limit is one second and the stall time two
    let base = server(vec![
        (400, PIECE),
        (400, PIECE),
        (400, PIECE),
        (400, PIECE),
        (400, PIECE),
        (0, DONE),
    ]);
    let route = OllamaRoute::new(&base, "m")
        .unwrap()
        .with_timeout(Duration::from_secs(1))
        .with_stall(Duration::from_secs(2));
    let mut got = String::new();
    let t = Instant::now();
    let r = route.stream(&request(), &mut |p| {
        got.push_str(p);
        true
    });
    let done = r.unwrap_or_else(|e| panic!("{e:?} after {:?}", t.elapsed()));
    assert!(
        t.elapsed() > Duration::from_millis(1800),
        "it really took longer than the call limit: {:?}",
        t.elapsed()
    );
    assert_eq!(done.text, "ababababab");
    assert_eq!((done.prompt_tokens, done.completion_tokens), (3, 5));
}

#[test]
fn st03_an_answer_that_never_starts_is_given_up_after_the_call_limit() {
    let base = server(vec![(30_000, PIECE)]);
    let route = OllamaRoute::new(&base, "m")
        .unwrap()
        .with_timeout(Duration::from_millis(900))
        .with_stall(Duration::from_secs(60));
    let t = Instant::now();
    let r = route.stream(&request(), &mut |_| true);
    // the request itself is held by the server, so the call limit ends it before any line exists
    assert!(
        matches!(
            r,
            Err(RouteError::TimedOut(_)) | Err(RouteError::Unreachable(_))
        ),
        "{r:?}"
    );
    assert!(t.elapsed() < Duration::from_secs(8), "{:?}", t.elapsed());
}

#[test]
fn st04_stopping_early_still_returns_what_was_written_and_a_quiet_stream_after_the_stop_costs_nothing(
) {
    let base = server(vec![(0, PIECE), (0, PIECE), (0, PIECE)]);
    let route = OllamaRoute::new(&base, "m")
        .unwrap()
        .with_stall(Duration::from_secs(5));
    let mut n = 0;
    let t = Instant::now();
    let done = route
        .stream(&request(), &mut |_| {
            n += 1;
            n < 2
        })
        .unwrap();
    assert!(done.stopped && done.text == "abab", "{done:?}");
    assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
}

#[test]
fn st05_a_model_that_accepts_the_request_and_never_answers_is_given_up_at_the_call_limit() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let _held = listener.accept();
        std::thread::sleep(Duration::from_secs(30));
    });
    let route = OllamaRoute::new(&base, "m")
        .unwrap()
        .with_retry(kk_route::retry::Retry::none())
        .with_timeout(Duration::from_millis(800));
    let t = Instant::now();
    let r = route.stream(&request(), &mut |_| true);
    assert!(matches!(r, Err(RouteError::TimedOut(_))), "{r:?}");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
}
