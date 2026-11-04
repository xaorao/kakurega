use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

fn session(lines: &[String]) -> Vec<Value> {
    let empty = std::env::temp_dir().join(format!("kk-mcp-empty-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_kaku"))
        .args(["mcp", "--data-dir"])
        .arg(&empty)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for l in lines {
        writeln!(stdin, "{l}").unwrap();
    }
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn mc01_the_protocol_basics_and_the_tool_list_hold_and_there_is_no_approve_tool() {
    let r = session(&[
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}).to_string(),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string(),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}).to_string(),
        json!({"jsonrpc":"2.0","id":3,"method":"nope"}).to_string(),
        "not json".to_string(),
    ]);
    assert_eq!(r.len(), 4, "the notification gets no reply");
    assert_eq!(r[0]["result"]["serverInfo"]["name"], "kakurega");
    let names: Vec<&str> = r[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "list_reviews",
            "start_review",
            "review_status",
            "request_approval",
            "verify_capsule"
        ]
    );
    assert!(!names
        .iter()
        .any(|n| n.contains("approve") && *n != "request_approval"));
    assert_eq!(r[2]["error"]["code"], -32601);
    assert_eq!(r[3]["error"]["code"], -32700);
}

#[test]
fn mc02_tools_fail_closed_with_clear_errors_when_the_workspace_is_not_running_or_input_is_junk() {
    let call = |id: u32, name: &str, args: Value| {
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}}).to_string()
    };
    let r = session(&[
        call(1, "list_reviews", json!({})),
        call(2, "review_status", json!({"id": "../etc/passwd"})),
        call(3, "start_review", json!({"repo": "/x"})),
        call(4, "verify_capsule", json!({"path": "/definitely/not/here"})),
        call(5, "verify_capsule", json!({"path": "/etc/hosts"})),
        call(6, "approve", json!({"id": "a"})),
    ]);
    for (i, x) in r.iter().enumerate() {
        assert_eq!(x["result"]["isError"], i != 4, "call {}: {x}", i + 1);
    }
    assert!(r[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("not running"));
    let junk: Value =
        serde_json::from_str(r[4]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        junk["valid"], false,
        "a non-capsule file is reported invalid with a reason, never valid"
    );
    assert_eq!(junk["reason"], "capsule_not_json");
}
