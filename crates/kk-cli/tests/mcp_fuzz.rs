//! 3,000 hostile frames sent to the MCP server over its real standard input: broken JSON, wrong shapes, junk parameters, deep nesting,
//! unknown and missing methods. The server must stay alive, answer only in valid JSON-RPC, never write to its error stream, and still
//! answer a plain ping at the end. A crash here would drop every agent connected to the workspace.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn rng(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}

fn pick<'a, T>(s: &mut u64, v: &'a [T]) -> &'a T {
    &v[(rng(s) % v.len() as u64) as usize]
}

fn junk_value(s: &mut u64, depth: u32) -> Value {
    match rng(s) % 12 {
        0 => Value::Null,
        1 => json!(true),
        2 => json!((rng(s) % 1000) as i64 - 500),
        3 => json!(1.5e300),
        4 => json!("x".repeat((rng(s) % 300) as usize)),
        5 => json!("\u{0}\u{1b}[2J\u{202e}é日本"),
        6 => json!(i64::MAX),
        7 => json!(u64::MAX),
        8 if depth < 6 => Value::Array(
            (0..(rng(s) % 4))
                .map(|_| junk_value(s, depth + 1))
                .collect(),
        ),
        9 if depth < 6 => {
            let mut m = serde_json::Map::new();
            for _ in 0..(rng(s) % 4) {
                m.insert(
                    pick(
                        s,
                        &[
                            "id",
                            "name",
                            "arguments",
                            "after",
                            "limit",
                            "repo",
                            "recipient",
                            "wait_seconds",
                            "uri",
                            "_meta",
                            "progressToken",
                        ],
                    )
                    .to_string(),
                    junk_value(s, depth + 1),
                );
            }
            Value::Object(m)
        }
        _ => json!("review_events"),
    }
}

#[test]
fn mc95_three_thousand_hostile_frames_leave_the_mcp_server_alive_answering_only_valid_json_rpc_and_silent_on_stderr(
) {
    let dir = std::env::temp_dir().join(format!("kk-mcpfuzz-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_kaku"))
        .args(["mcp", "--data-dir"])
        .arg(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let out = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        BufReader::new(out)
            .lines()
            .map_while(Result::ok)
            .collect::<Vec<String>>()
    });
    let mut seed = 0xDEADBEEFCAFEF00Du64;
    let methods = [
        "initialize",
        "ping",
        "tools/list",
        "tools/call",
        "resources/list",
        "resources/read",
        "prompts/list",
        "prompts/get",
        "completion/complete",
        "logging/setLevel",
        "notifications/initialized",
        "notifications/cancelled",
        "resources/subscribe",
        "x/unknown",
        "",
        "tools/call",
    ];
    let mut sent_ids = 0;
    for n in 0..3000 {
        let kind = rng(&mut seed) % 10;
        let line = match kind {
            0 => "{".to_string(),
            1 => "[]".to_string(),
            2 => "null".to_string(),
            3 => "\u{feff}{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}".to_string(),
            4 => "[".repeat(500),
            _ => {
                let mut m = serde_json::Map::new();
                if !rng(&mut seed).is_multiple_of(8) {
                    m.insert("jsonrpc".into(), json!("2.0"));
                }
                if !rng(&mut seed).is_multiple_of(4) {
                    m.insert(
                        "id".into(),
                        if rng(&mut seed).is_multiple_of(3) {
                            junk_value(&mut seed, 0)
                        } else {
                            json!(n)
                        },
                    );
                    sent_ids += 1;
                }
                m.insert("method".into(), json!(pick(&mut seed, &methods)));
                if !rng(&mut seed).is_multiple_of(3) {
                    m.insert("params".into(), junk_value(&mut seed, 0));
                }
                Value::Object(m).to_string()
            }
        };
        if writeln!(stdin, "{line}").is_err() {
            break;
        }
    }
    // the server must still answer a plain ping after all of that
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": "final-ping", "method": "ping"})
    )
    .unwrap();
    drop(stdin);
    let status = child.wait().unwrap();
    let lines = reader.join().unwrap();
    let mut bad = Vec::new();
    for l in &lines {
        match serde_json::from_str::<Value>(l) {
            Ok(v) if v["jsonrpc"] == "2.0" => {}
            _ => bad.push(l.chars().take(120).collect::<String>()),
        }
    }
    let mut err = String::new();
    if let Some(mut e) = child.stderr.take() {
        use std::io::Read;
        let _ = e.read_to_string(&mut err);
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        status.code(),
        Some(0),
        "the server ended cleanly when its input closed"
    );
    assert!(bad.is_empty(), "lines that are not JSON-RPC: {bad:?}");
    assert!(
        lines.iter().any(|l| l.contains("final-ping")),
        "the plain ping at the end was answered"
    );
    assert!(
        lines.len() as u32 > sent_ids / 2,
        "most requests that carried an id were answered: {} of {sent_ids}",
        lines.len()
    );
    assert!(
        err.trim().is_empty(),
        "nothing is written to the error stream: {err:?}"
    );
}
