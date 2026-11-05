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
                if authorized {
                    seen_t
                        .lock()
                        .unwrap()
                        .push(request_line.trim_end().to_string());
                } else {
                    *refused_t.lock().unwrap() += 1;
                }
                let payload = r#"{"ready":true,"reviews":{},"jobs":[],"id":"job-1"}"#;
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
