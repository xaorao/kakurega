use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A stand-in for `kaku serve`'s admin door. It records every request that carried the right token.
struct FakeAdmin {
    port: u16,
    token: String,
    seen: Arc<Mutex<Vec<String>>>,
    refused: Arc<Mutex<usize>>,
}

impl FakeAdmin {
    fn start(token: &str) -> FakeAdmin {
        FakeAdmin::start_slow(token, 0)
    }

    /// Like `start`, but every answer waits `delay_ms` first.
    fn start_slow(token: &str, delay_ms: u64) -> FakeAdmin {
        FakeAdmin::start_with(
            token,
            delay_ms,
            r#"{"ready":true,"reviews":{},"jobs":[],"id":"job-1"}"#,
        )
    }

    /// Like `start_slow`, with the exact body every answer carries.
    fn start_with(token: &str, delay_ms: u64, payload: &str) -> FakeAdmin {
        let payload = payload.to_string();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let refused = Arc::new(Mutex::new(0usize));
        let (seen_t, refused_t, expected) = (seen.clone(), refused.clone(), token.to_string());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let Ok(clone) = stream.try_clone() else {
                    continue;
                };
                let mut reader = BufReader::new(clone);
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    continue;
                }
                let (mut authorized, mut length) = (false, 0usize);
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 {
                        break;
                    }
                    let header = header.trim_end().to_string();
                    if header.is_empty() {
                        break;
                    }
                    let lower = header.to_lowercase();
                    if lower.starts_with("authorization:") {
                        authorized = header.ends_with(&format!("Bearer {expected}"));
                    }
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; length];
                let _ = reader.read_exact(&mut body);
                std::thread::sleep(Duration::from_millis(delay_ms));
                if authorized {
                    seen_t
                        .lock()
                        .unwrap()
                        .push(request_line.trim_end().to_string());
                } else {
                    *refused_t.lock().unwrap() += 1;
                }
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{payload}",
                    payload.len()
                );
            }
        });
        FakeAdmin {
            port,
            token: token.into(),
            seen,
            refused,
        }
    }

    fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    fn refused(&self) -> usize {
        *self.refused.lock().unwrap()
    }

    fn publish(&self, dir: &Path) {
        std::fs::write(
            dir.join("admin.json"),
            json!({"port": self.port, "token": self.token}).to_string(),
        )
        .unwrap();
    }
}

/// One running `kaku mcp` that is talked to line by line, the way an MCP client does.
struct Live {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<String>,
}

fn fresh(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kk-mcpf-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn live(dir: &Path) -> Live {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kaku"))
        .args(["mcp", "--data-dir"])
        .arg(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let out = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    Live { child, stdin, rx }
}

impl Live {
    fn send(&mut self, line: &str) {
        self.send_bytes(line.as_bytes());
    }

    fn send_bytes(&mut self, bytes: &[u8]) {
        self.stdin.write_all(bytes).unwrap();
        self.stdin.write_all(b"\n").unwrap();
        self.stdin.flush().unwrap();
    }

    fn recv(&self) -> Value {
        let line = self
            .rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the server answered");
        serde_json::from_str(&line).expect("every stdout line is a JSON-RPC frame")
    }

    /// Ends the session and returns whatever else was written. A test that expects silence asserts this is empty.
    fn close(self) -> Vec<Value> {
        let Live {
            mut child,
            stdin,
            rx,
        } = self;
        drop(stdin);
        assert!(child.wait().unwrap().success());
        rx.iter()
            .map(|l| serde_json::from_str(&l).expect("every stdout line is a JSON-RPC frame"))
            .collect()
    }
}

fn ping(id: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "method": "ping"}).to_string()
}

fn call(id: u32, name: &str, args: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": args}}).to_string()
}

#[test]
fn mc07_a_message_without_an_id_is_a_notification_and_never_runs_a_tool() {
    let dir = fresh("mc07");
    let admin = FakeAdmin::start("tok-7");
    admin.publish(&dir);
    let mut s = live(&dir);
    s.send(&json!({"jsonrpc": "2.0", "method": "tools/call", "params": {"name": "start_review", "arguments": {"repo": "/tmp", "recipient": "x"}}}).to_string());
    s.send(&json!({"jsonrpc": "2.0", "method": "tools/call", "params": {"name": "request_approval", "arguments": {"id": "job-1"}}}).to_string());
    s.send(&ping(json!(1)));
    let first = s.recv();
    assert_eq!(
        first["id"], 1,
        "the first answer is the ping's, so the two messages before it got none: {first}"
    );
    assert_eq!(first["result"], json!({}));
    assert!(
        admin.seen().is_empty(),
        "nothing reached the service: {:?}",
        admin.seen()
    );
    // the control: the same kind of call with an id does reach the service, so this test can fail
    s.send(&call(2, "overview", json!({})));
    let r = s.recv();
    assert_eq!(r["id"], 2);
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(admin.seen(), ["GET /admin/overview HTTP/1.1"]);
    assert_eq!(admin.refused(), 0, "every request carried the token");
    assert!(s.close().is_empty());
}

#[test]
fn mc08_ids_must_be_strings_or_integers_and_odd_frames_are_answered_or_ignored_as_the_protocol_says(
) {
    let dir = fresh("mc08");
    let mut s = live(&dir);
    for bad in [
        json!(null),
        json!(1.5),
        json!({"a": 1}),
        json!([1]),
        json!(true),
    ] {
        s.send(&ping(bad.clone()));
        let r = s.recv();
        assert_eq!(r["error"]["code"], -32600, "{bad}: {r}");
        assert_eq!(r["id"], Value::Null, "{bad}: {r}");
    }
    for good in [json!(7), json!("abc"), json!(-3)] {
        s.send(&ping(good.clone()));
        let r = s.recv();
        assert_eq!(r["id"], good, "{r}");
        assert_eq!(r["result"], json!({}));
    }
    // answers to requests this server never sent are dropped without a reply
    s.send(&json!({"jsonrpc": "2.0", "id": 5, "result": {}}).to_string());
    s.send(&json!({"jsonrpc": "2.0", "id": 6, "error": {"code": -1, "message": "x"}}).to_string());
    s.send(&ping(json!(8)));
    assert_eq!(s.recv()["id"], 8, "the two answers above got no reply");
    // a request with no method, or a method that is not text, is an invalid request that names its id
    s.send(&json!({"jsonrpc": "2.0", "id": 9}).to_string());
    let r = s.recv();
    assert_eq!(
        (r["error"]["code"].clone(), r["id"].clone()),
        (json!(-32600), json!(9))
    );
    s.send(&json!({"jsonrpc": "2.0", "id": 10, "method": 42}).to_string());
    let r = s.recv();
    assert_eq!(
        (r["error"]["code"].clone(), r["id"].clone()),
        (json!(-32600), json!(10))
    );
    // text that is not valid UTF-8 is a parse error, not silence, and the next request is still served
    s.send_bytes(&[0xff, 0xfe, 0x20, 0x7b]);
    let r = s.recv();
    assert_eq!(
        (r["error"]["code"].clone(), r["id"].clone()),
        (json!(-32700), Value::Null)
    );
    // a byte-order mark in front of a message is ignored
    s.send(&format!("\u{feff}{}", ping(json!(11))));
    let r = s.recv();
    assert_eq!(
        (r["id"].clone(), r["result"].clone()),
        (json!(11), json!({}))
    );
    assert!(s.close().is_empty());
}

#[test]
fn mc09_the_service_is_looked_up_for_every_call_so_starting_it_later_or_restarting_it_does_not_break_a_running_agent(
) {
    let dir = fresh("mc09");
    let mut s = live(&dir);
    s.send(&call(1, "overview", json!({})));
    let r = s.recv();
    assert_eq!(r["result"]["isError"], true);
    assert!(
        r["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("not running"),
        "{r}"
    );
    // kaku serve is started after the agent
    let a = FakeAdmin::start("tok-a");
    a.publish(&dir);
    s.send(&call(2, "overview", json!({})));
    assert_eq!(s.recv()["result"]["isError"], false);
    assert_eq!(a.seen().len(), 1);
    // kaku serve restarts: new port, new token, same data folder
    let b = FakeAdmin::start("tok-b");
    b.publish(&dir);
    s.send(&call(3, "list_reviews", json!({})));
    assert_eq!(s.recv()["result"]["isError"], false);
    assert_eq!(b.seen(), ["GET /admin/jobs HTTP/1.1"]);
    assert_eq!(a.seen().len(), 1, "the old service was not asked again");
    assert_eq!(
        a.refused() + b.refused(),
        0,
        "every request carried the token of the service it went to"
    );
    // and when the service is gone again the answer is the same plain error
    std::fs::remove_file(dir.join("admin.json")).unwrap();
    s.send(&call(4, "overview", json!({})));
    assert_eq!(s.recv()["result"]["isError"], true);
    assert!(s.close().is_empty());
}

#[test]
fn mc10_the_protocol_version_is_the_clients_own_when_supported_the_newest_otherwise_and_must_be_text(
) {
    let dir = fresh("mc10");
    let mut s = live(&dir);
    let init = |id: u32, params: Value| {
        json!({"jsonrpc": "2.0", "id": id, "method": "initialize", "params": params}).to_string()
    };
    for (asked, answered) in [
        ("2024-11-05", "2024-11-05"),
        ("2025-03-26", "2025-03-26"),
        ("2025-06-18", "2025-06-18"),
        ("2099-01-01", "2025-06-18"),
        ("garbage", "2025-06-18"),
    ] {
        s.send(&init(1, json!({"protocolVersion": asked})));
        assert_eq!(
            s.recv()["result"]["protocolVersion"],
            answered,
            "asked {asked}"
        );
    }
    s.send(&init(2, json!({})));
    assert_eq!(
        s.recv()["result"]["protocolVersion"],
        "2025-06-18",
        "none given: the newest"
    );
    s.send(&init(3, json!({"protocolVersion": 20250618})));
    let r = s.recv();
    assert_eq!(
        (r["error"]["code"].clone(), r["id"].clone()),
        (json!(-32602), json!(3)),
        "{r}"
    );
    assert!(s.close().is_empty());
}

#[test]
fn mc11_probing_clients_get_empty_lists_not_errors_and_a_cursor_on_the_single_tool_page_is_refused()
{
    let dir = fresh("mc11");
    let mut s = live(&dir);
    let ask = |id: u32, method: &str, params: Value| {
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string()
    };
    for (method, key) in [
        ("resources/list", "resources"),
        ("resources/templates/list", "resourceTemplates"),
        ("prompts/list", "prompts"),
    ] {
        s.send(&ask(1, method, json!({})));
        let r = s.recv();
        assert_eq!(r["result"][key], json!([]), "{method}: {r}");
    }
    s.send(&ask(2, "tools/list", json!({})));
    assert!(s.recv()["result"]["tools"].as_array().unwrap().len() >= 7);
    s.send(&ask(3, "tools/list", json!({"cursor": null})));
    assert!(
        s.recv()["result"]["tools"].is_array(),
        "a null cursor is the first page"
    );
    s.send(&ask(4, "tools/list", json!({"cursor": "page-2"})));
    let r = s.recv();
    assert_eq!(
        (r["error"]["code"].clone(), r["id"].clone()),
        (json!(-32602), json!(4)),
        "{r}"
    );
    s.send(&ask(5, "sampling/createMessage", json!({})));
    assert_eq!(
        s.recv()["error"]["code"],
        -32601,
        "what is not offered stays unknown"
    );
    // capabilities still advertise tools only
    s.send(&ask(6, "initialize", json!({})));
    let caps = s.recv()["result"]["capabilities"].clone();
    assert!(
        caps.get("resources").is_none() && caps.get("prompts").is_none(),
        "{caps}"
    );
    assert!(s.close().is_empty());
}

#[test]
fn mc12_a_ping_is_answered_while_a_service_call_is_still_waiting() {
    let dir = fresh("mc12");
    let admin = FakeAdmin::start_slow("tok-12", 1500);
    admin.publish(&dir);
    let mut s = live(&dir);
    s.send(&call(1, "overview", json!({})));
    s.send(&ping(json!(2)));
    let first = s.recv();
    assert_eq!(
        first["id"], 2,
        "the ping came back first, not stuck behind the call: {first}"
    );
    let second = s.recv();
    assert_eq!(
        (second["id"].clone(), second["result"]["isError"].clone()),
        (json!(1), json!(false)),
        "{second}"
    );
    assert!(s.close().is_empty());
}

#[test]
fn mc13_a_cancelled_call_gets_no_answer_and_the_next_request_is_served() {
    let dir = fresh("mc13");
    let admin = FakeAdmin::start_slow("tok-13", 1200);
    admin.publish(&dir);
    let mut s = live(&dir);
    s.send(&call(1, "overview", json!({})));
    s.send(
        &json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 1}})
            .to_string(),
    );
    s.send(&ping(json!(2)));
    assert_eq!(s.recv()["id"], 2);
    std::thread::sleep(Duration::from_millis(2200));
    s.send(&ping(json!(3)));
    assert_eq!(
        s.recv()["id"],
        3,
        "nothing else was sent in between: the cancelled call was never answered"
    );
    // a cancel for an id that is not running is ignored, and a later call with a new id works
    s.send(&json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 99}}).to_string());
    s.send(&call(4, "overview", json!({})));
    let r = s.recv();
    assert_eq!(
        (r["id"].clone(), r["result"]["isError"].clone()),
        (json!(4), json!(false)),
        "{r}"
    );
    assert!(s.close().is_empty());
}

#[test]
fn mc14_a_call_still_running_when_input_ends_is_answered_before_the_process_leaves() {
    let dir = fresh("mc14");
    let admin = FakeAdmin::start_slow("tok-14", 800);
    admin.publish(&dir);
    let mut s = live(&dir);
    s.send(&call(1, "overview", json!({})));
    let rest = s.close();
    assert_eq!(rest.len(), 1, "{rest:?}");
    assert_eq!(
        (rest[0]["id"].clone(), rest[0]["result"]["isError"].clone()),
        (json!(1), json!(false))
    );
}

fn live_with(dir: &Path, extra: &[&str]) -> Live {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kaku"))
        .args(["mcp", "--data-dir"])
        .arg(dir)
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let out = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    Live { child, stdin, rx }
}

#[test]
fn mc15_an_allow_list_gives_the_agent_exactly_the_named_tools_and_refuses_the_rest() {
    let dir = fresh("mc15");
    let admin = FakeAdmin::start("tok-15");
    admin.publish(&dir);
    let mut s = live_with(&dir, &["--allow", "overview, verify_log"]);
    s.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string());
    let names: Vec<String> = s.recv()["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(names, ["overview", "verify_log"]);
    s.send(&call(2, "overview", json!({})));
    assert_eq!(s.recv()["result"]["isError"], false);
    for (id, tool, args) in [
        (3, "start_review", json!({"repo": "/tmp", "recipient": "x"})),
        (4, "request_approval", json!({"id": "job-1"})),
        (5, "list_reviews", json!({})),
        (6, "approve", json!({})),
    ] {
        s.send(&call(id, tool, args));
        let r = s.recv();
        assert_eq!(
            (r["error"]["code"].clone(), r["id"].clone()),
            (json!(-32602), json!(id)),
            "{tool}: {r}"
        );
    }
    assert_eq!(
        admin.seen(),
        ["GET /admin/overview HTTP/1.1"],
        "only the allowed call reached the service"
    );
    assert!(s.close().is_empty());
}

#[test]
fn mc16_an_allow_list_with_an_unknown_or_empty_name_stops_the_server_with_a_message() {
    let dir = fresh("mc16");
    for (arg, expect) in [
        ("overview,nonsense", "does not exist: nonsense"),
        (" , ", "at least one tool name"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_kaku"))
            .args(["mcp", "--data-dir"])
            .arg(&dir)
            .args(["--allow", arg])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{arg}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(expect),
            "{arg}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            out.stdout.is_empty(),
            "nothing but protocol frames ever goes to stdout"
        );
    }
}

#[test]
fn mc17_read_only_and_an_allow_list_together_give_only_what_both_allow() {
    let dir = fresh("mc17");
    let mut s = live_with(&dir, &["--read-only", "--allow", "overview,start_review"]);
    s.send(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string());
    let names: Vec<String> = s.recv()["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        ["overview"],
        "start_review is allowed by the list but hidden by read-only"
    );
    assert!(s.close().is_empty());
}

fn cancel(id: Value) -> String {
    json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": id}})
        .to_string()
}

#[test]
fn mc18_a_cancel_only_ever_reaches_a_request_that_is_running() {
    let dir = fresh("mc18");
    let admin = FakeAdmin::start_slow("tok-18", 300);
    admin.publish(&dir);
    let mut s = live(&dir);
    // a cancel for an id that was never sent must not swallow a later request that uses it
    s.send(&cancel(json!(7)));
    s.send(&call(7, "overview", json!({})));
    let r = s.recv();
    assert_eq!(
        (r["id"].clone(), r["result"]["isError"].clone()),
        (json!(7), json!(false)),
        "{r}"
    );
    // nor a cancel that arrives after the answer, whether the id is then reused or not
    s.send(&call(8, "overview", json!({})));
    assert_eq!(s.recv()["id"], 8);
    s.send(&cancel(json!(8)));
    s.send(&call(8, "overview", json!({})));
    let again = s.recv();
    assert_eq!(
        (again["id"].clone(), again["result"]["isError"].clone()),
        (json!(8), json!(false)),
        "a reused id after a stale cancel is still answered: {again}"
    );
    // string and integer ids are different ids: cancelling "9" does not cancel 9
    s.send(&call(9, "overview", json!({})));
    s.send(&cancel(json!("9")));
    let nine = s.recv();
    assert_eq!(nine["id"], 9, "{nine}");
    assert!(s.close().is_empty());
}

#[test]
fn mc19_a_request_id_that_is_already_running_is_refused_and_the_first_call_is_still_answered_once()
{
    let dir = fresh("mc19");
    let admin = FakeAdmin::start_slow("tok-19", 1000);
    admin.publish(&dir);
    let mut s = live(&dir);
    s.send(&call(1, "overview", json!({})));
    s.send(&call(1, "list_reviews", json!({})));
    let refusal = s.recv();
    assert_eq!(refusal["id"], 1, "{refusal}");
    assert_eq!(refusal["error"]["code"], -32600, "{refusal}");
    assert!(
        refusal["error"]["message"]
            .as_str()
            .unwrap()
            .contains("already"),
        "{refusal}"
    );
    let answer = s.recv();
    assert_eq!(
        (answer["id"].clone(), answer["result"]["isError"].clone()),
        (json!(1), json!(false)),
        "{answer}"
    );
    // once answered, the id is free again
    s.send(&call(1, "overview", json!({})));
    assert_eq!(s.recv()["id"], 1);
    assert!(s.close().is_empty());
}

#[test]
fn mc20_an_answer_that_does_not_match_the_tools_output_schema_is_never_passed_on_as_structured_data(
) {
    let bad_payloads = [
        (r#"{"ready":"yes","reviews":{}}"#, "ready is text"),
        (r#"{"reviews":{}}"#, "ready is missing"),
        (r#"{"ready":true}"#, "reviews is missing"),
        (r#"{"ready":true,"reviews":[]}"#, "reviews is a list"),
        (
            r#"{"ready":true,"reviews":{},"setup_problems":"none"}"#,
            "setup_problems is text",
        ),
        (
            r#"{"ready":true,"reviews":{},"setup_problems":[1]}"#,
            "setup_problems holds a number",
        ),
        (r#"{"ready":1,"reviews":{}}"#, "a number is not a boolean"),
    ];
    for (i, (payload, why)) in bad_payloads.iter().enumerate() {
        let dir = fresh(&format!("mc20-{i}"));
        let admin = FakeAdmin::start_with(&format!("tok-20-{i}"), 0, payload);
        admin.publish(&dir);
        let mut s = live(&dir);
        s.send(&call(1, "overview", json!({})));
        let r = s.recv();
        assert_eq!(r["result"]["isError"], true, "{why}: {r}");
        assert!(r["result"].get("structuredContent").is_none(), "{why}: {r}");
        assert!(
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("did not match"),
            "{why}: {r}"
        );
        assert!(s.close().is_empty());
    }
    // answers that fit are passed on untouched, including fields the schema does not list
    let dir = fresh("mc20-ok");
    let admin = FakeAdmin::start_with(
        "tok-20-ok",
        0,
        r#"{"ready":false,"reviews":{"ready":2},"setup_problems":["Sandbox"],"extra":{"anything":[1,2]}}"#,
    );
    admin.publish(&dir);
    let mut s = live(&dir);
    s.send(&call(1, "overview", json!({})));
    let r = s.recv();
    assert_eq!(r["result"]["isError"], false, "{r}");
    assert_eq!(
        r["result"]["structuredContent"]["extra"],
        json!({"anything": [1, 2]})
    );
    assert!(s.close().is_empty());
}

#[test]
fn mc21_at_most_four_service_calls_run_at_once_and_the_fifth_is_refused_at_once() {
    let dir = fresh("mc21");
    let admin = FakeAdmin::start_slow("tok-21", 1500);
    admin.publish(&dir);
    let mut s = live(&dir);
    for id in 1..=5 {
        s.send(&call(id, "overview", json!({})));
    }
    let first = s.recv();
    assert_eq!(
        first["id"], 5,
        "the fifth is turned away before any answer is ready: {first}"
    );
    assert_eq!(first["error"]["code"], -32000, "{first}");
    let mut answered: Vec<u64> = (0..4).map(|_| s.recv()["id"].as_u64().unwrap()).collect();
    answered.sort();
    assert_eq!(answered, [1, 2, 3, 4]);
    // and when they are done there is room again
    s.send(&call(5, "overview", json!({})));
    assert_eq!(s.recv()["id"], 5);
    assert!(s.close().is_empty());
}
