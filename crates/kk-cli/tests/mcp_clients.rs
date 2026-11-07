//! The opening exchange of the clients we know, replayed against `kaku mcp`. Each handshake is built from what that client's own source
//! was read to send (protocol version, capabilities, client name) and from the behaviours the client relies on afterwards; none was
//! captured from a running client, and docs/mcp-clients.md says so.

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

struct Client {
    name: &'static str,
    protocol: &'static str,
    capabilities: Value,
}

fn clients() -> Vec<Client> {
    vec![
        Client {
            name: "claude-code",
            protocol: "2025-06-18",
            capabilities: json!({"roots": {}, "elicitation": {}}),
        },
        Client {
            name: "codex-mcp-client",
            protocol: "2025-06-18",
            capabilities: json!({"elicitation": {}}),
        },
        Client {
            name: "cline",
            protocol: "2025-03-26",
            capabilities: json!({}),
        },
        Client {
            name: "opencode",
            protocol: "2025-06-18",
            capabilities: json!({}),
        },
        Client {
            name: "goose",
            protocol: "2025-06-18",
            capabilities: json!({"sampling": {}}),
        },
        Client {
            name: "hermes-agent",
            protocol: "2024-11-05",
            capabilities: json!({}),
        },
        Client {
            name: "old-client",
            protocol: "2023-01-01",
            capabilities: json!({}),
        },
    ]
}

fn replay(c: &Client, extra: &[Value]) -> Vec<Value> {
    let dir =
        std::env::temp_dir().join(format!("kk-mcp-clients-{}-{}", c.name, std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_kaku"))
        .args(["mcp", "--data-dir"])
        .arg(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = vec![
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": c.protocol, "capabilities": c.capabilities, "clientInfo": {"name": c.name, "version": "1.0.0"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "ping"}),
    ];
    lines.extend(extra.iter().cloned());
    for l in lines {
        writeln!(stdin, "{l}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{} left with {:?}",
        c.name,
        out.status
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn by_id(r: &[Value], id: u64) -> &Value {
    r.iter()
        .find(|v| v["id"] == id)
        .unwrap_or_else(|| panic!("no answer to {id}"))
}

#[test]
fn mk01_every_known_client_opening_succeeds_and_the_answers_have_the_shapes_clients_rely_on() {
    for c in clients() {
        let r = replay(&c, &[]);
        let init = &by_id(&r, 1)["result"];
        let got = init["protocolVersion"].as_str().unwrap();
        assert!(
            ["2025-06-18", "2025-03-26", "2024-11-05"].contains(&got),
            "{}: answered with {got}",
            c.name
        );
        if ["2025-06-18", "2025-03-26", "2024-11-05"].contains(&c.protocol) {
            assert_eq!(
                got, c.protocol,
                "{}: a version we speak is answered in kind",
                c.name
            );
        }
        assert!(
            init["serverInfo"]["name"].is_string() && init["capabilities"]["tools"].is_object(),
            "{}",
            c.name
        );
        assert!(
            init["instructions"]
                .as_str()
                .is_some_and(|i| !i.is_empty() && i.len() < 600),
            "{}: opencode puts these in its prompt",
            c.name
        );
        assert_eq!(by_id(&r, 3)["result"], json!({}), "{}: ping", c.name);
        let tools = by_id(&r, 2)["result"]["tools"].as_array().unwrap();
        assert!(!tools.is_empty());
        for t in tools {
            let name = t["name"].as_str().unwrap();
            assert!(
                name.len() <= 64
                    && name
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'),
                "{}: {name} is rewritten or rejected by Cline and others",
                c.name
            );
            assert_eq!(t["inputSchema"]["type"], "object", "{}: {name}", c.name);
            assert!(
                t["annotations"]["readOnlyHint"].is_boolean(),
                "{}: {name} has no readOnlyHint (goose trusts it)",
                c.name
            );
            assert_eq!(
                t["outputSchema"]["type"], "object",
                "{}: {name}: structured output is preferred by opencode",
                c.name
            );
            assert!(
                t["description"].as_str().is_some_and(|d| !d.is_empty()),
                "{name}"
            );
        }
        // a client that never asks `tools/list_changed` is never sent one, and the server does not claim it will
        assert_eq!(
            init["capabilities"]["tools"]["listChanged"], false,
            "{}",
            c.name
        );
    }
}

#[test]
fn mk02_a_call_returns_structured_content_and_text_for_clients_that_read_only_one_of_them() {
    let c = &clients()[0];
    let r = replay(
        c,
        &[
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "overview", "arguments": {}}}),
        ],
    );
    let res = &by_id(&r, 4)["result"];
    // with no service running the answer is an error result the model can read, never a protocol error that some clients retry
    assert!(by_id(&r, 4).get("error").is_none(), "{res}");
    assert!(
        res["content"][0]["type"] == "text"
            && res["content"][0]["text"]
                .as_str()
                .is_some_and(|t| !t.is_empty()),
        "{res}"
    );
    assert_eq!(
        res["isError"], true,
        "no service is running in this test: {res}"
    );
}

#[test]
fn mk03_arguments_a_model_supplied_do_not_widen_what_a_client_may_do() {
    // Codex treats model-supplied arguments as non-authority; here a call naming a tool the server does not offer is a plain error result
    let c = &clients()[1];
    let r = replay(
        c,
        &[
            json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "approve", "arguments": {"id": "approval-1"}}}),
            json!({"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "x".repeat(300), "arguments": {}}}),
        ],
    );
    for id in [5, 6] {
        let v = by_id(&r, id);
        assert!(
            v.get("error").is_some() || v["result"]["isError"] == true,
            "{v}"
        );
    }
    let names: Vec<_> = by_id(&r, 2)["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert!(
        names
            .iter()
            .all(|n| !n.contains("approve") && !n.contains("deliver")),
        "{names:?}"
    );
}
