use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

fn session(lines: &[String]) -> Vec<Value> {
    session_in(None, lines)
}

fn session_in(cwd: Option<&std::path::Path>, lines: &[String]) -> Vec<Value> {
    let empty = std::env::temp_dir().join(format!("kk-mcp-empty-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kaku"));
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    let mut child = cmd
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

fn call(id: u32, name: &str, args: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": args}}).to_string()
}

#[test]
fn mc01_the_protocol_basics_hold_and_malformed_requests_are_answered_not_crashed() {
    let r = session(&[
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}}).to_string(),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string(),
        json!({"jsonrpc": "2.0", "id": 2, "method": "ping"}).to_string(),
        json!({"jsonrpc": "2.0", "id": 3, "method": "nope"}).to_string(),
        "not json".to_string(),
        json!({"id": 4, "method": "ping"}).to_string(),
        "[]".to_string(),
        json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {}}).to_string(),
    ]);
    assert_eq!(r.len(), 7, "the notification gets no reply: {r:?}");
    assert_eq!(r[0]["result"]["serverInfo"]["name"], "kakurega");
    assert_eq!(r[0]["result"]["protocolVersion"], "2025-06-18");
    assert!(r[0]["result"]["instructions"]
        .as_str()
        .unwrap()
        .contains("never returned"));
    assert_eq!(r[1]["result"], json!({}));
    assert_eq!(r[2]["error"]["code"], -32601);
    assert_eq!(r[3]["error"]["code"], -32700);
    assert_eq!(
        r[4]["error"]["code"], -32600,
        "a request without jsonrpc 2.0 is invalid"
    );
    assert_eq!(r[5]["error"]["code"], -32600);
    assert_eq!(r[6]["error"]["code"], -32602);
}

#[test]
fn mc02_the_tool_list_is_a_stable_typed_contract_and_offers_no_way_to_read_private_text_or_approve()
{
    let r = session(&[json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string()]);
    let tools = r[0]["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "overview",
            "list_reviews",
            "start_review",
            "review_status",
            "review_events",
            "request_approval",
            "verify_capsule",
            "verify_log"
        ]
    );
    for forbidden in [
        "approve", "deliver", "memory", "remember", "forget", "chat", "read", "release",
        "findings", "source", "file",
    ] {
        assert!(
            !names
                .iter()
                .any(|n| n.contains(forbidden) && *n != "request_approval"),
            "no tool may offer `{forbidden}`"
        );
    }
    for t in tools {
        assert_eq!(t["inputSchema"]["additionalProperties"], false, "{t}");
        assert_eq!(t["inputSchema"]["type"], "object");
        assert!(
            t["outputSchema"]["type"] == "object"
                && t["title"].is_string()
                && t["description"].as_str().unwrap().len() > 20
        );
        assert!(
            t["annotations"]["destructiveHint"] == false
                && t["annotations"]["openWorldHint"] == false
        );
        assert_eq!(
            t["annotations"]["readOnlyHint"],
            !matches!(
                t["name"].as_str().unwrap(),
                "start_review" | "request_approval"
            )
        );
    }
    let golden_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/mcp_tools.golden.json");
    let now = serde_json::to_string_pretty(&r[0]["result"]["tools"]).unwrap() + "\n";
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden_path, &now).unwrap();
    }
    let golden = std::fs::read_to_string(&golden_path).expect("tests/mcp_tools.golden.json exists; regenerate with UPDATE_GOLDEN=1 when a schema change is intended");
    assert_eq!(
        golden, now,
        "the published tool schemas changed: review the diff, then regenerate with UPDATE_GOLDEN=1"
    );
}

#[test]
fn mc03_arguments_are_validated_against_the_schema_and_files_are_reported_uniformly() {
    let dir = std::env::temp_dir().join(format!("kk-mcp-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("junk.kkg"), "not a capsule").unwrap();
    std::fs::write(dir.join("junk.json"), "not a log").unwrap();
    std::fs::write(dir.join("notes.txt"), "secret text").unwrap();
    let big = std::fs::File::create(dir.join("big.kkg")).unwrap();
    big.set_len(21 << 20).unwrap();
    let junk = dir.join("junk.kkg").to_string_lossy().to_string();
    // replies to calls that reach the service may arrive after later ones, so they are matched by id
    let mut r = session_in(Some(&dir), &[
        call(1, "overview", json!({})),
        call(2, "review_status", json!({"id": "../../etc/passwd"})),
        call(3, "review_status", json!({"id": "job-1", "extra": true})),
        call(4, "review_status", json!({})),
        call(5, "review_status", json!({"id": 7})),
        call(6, "review_status", json!({"id": ""})),
        call(7, "start_review", json!({"repo": "/x"})),
        call(8, "start_review", json!({"repo": "/x\u{1b}[2J", "recipient": "a"})),
        call(9, "start_review", json!({"repo": "x".repeat(5000), "recipient": "a"})),
        call(10, "verify_capsule", json!({"path": "/definitely/not/here.kkg"})),
        call(11, "verify_capsule", json!({"path": dir.join("notes.txt").to_string_lossy()})),
        call(12, "verify_capsule", json!({"path": "/etc"})),
        call(13, "verify_capsule", json!({"path": dir.join("big.kkg").to_string_lossy()})),
        call(14, "verify_capsule", json!({"path": junk})),
        call(15, "approve", json!({"id": "job-1"})),
        call(16, "verify_log", json!({"path": dir.join("junk.json").to_string_lossy(), "pin": "not-a-key"})),
        json!({"jsonrpc": "2.0", "id": 17, "method": "tools/call", "params": {"name": "overview", "arguments": "nope"}}).to_string(),
    ]);
    r.sort_by_key(|x| x["id"].as_u64().unwrap());
    let text = |i: usize| {
        r[i]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert!(r
        .iter()
        .take(14)
        .all(|x| x["result"]["isError"] == true || x["result"]["structuredContent"].is_object()));
    assert!(text(0).contains("not running"), "no service: {}", text(0));
    for (i, x) in r.iter().enumerate().take(9).skip(1) {
        assert_eq!(
            x["result"]["isError"],
            true,
            "call {} must fail: {}",
            i + 1,
            text(i)
        );
    }
    assert!(
        text(1).contains("not a valid id")
            && text(2).contains("unknown argument")
            && text(3).contains("required")
            && text(4).contains("must be a string")
            && text(5).contains("1 to")
    );
    let uniform: Vec<String> = (9..13).map(text).collect();
    assert!(uniform.iter().all(|t| t.contains("cannot read that file")), "a missing file, a wrong extension, a directory and a huge file are indistinguishable: {uniform:?}");
    assert!(!uniform
        .iter()
        .any(|t| t.contains("secret text") || t.contains("No such file")));
    let junk_answer: Value = serde_json::from_str(&text(13)).unwrap();
    assert_eq!(
        junk_answer["valid"], false,
        "an invalid capsule is a valid answer: {junk_answer}"
    );
    assert_eq!(r[13]["result"]["structuredContent"]["valid"], false);
    assert_eq!(
        r[14]["error"]["code"], -32602,
        "an unknown tool is a protocol error, not a tool result: {:?}",
        r[14]
    );
    assert!(r[14]["result"].is_null());
    assert!(r[15]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("pin"));
    assert_eq!(r[16]["result"]["isError"], true);
}

#[test]
fn mc04_verify_tools_read_only_inside_the_agents_folder_and_answer_every_other_path_the_same() {
    let base = std::env::temp_dir().join(format!("kk-mcp-paths-{}", std::process::id()));
    let inside = base.join("work");
    let outside = base.join("elsewhere");
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(inside.join("log.json"), "not a log").unwrap();
    std::fs::write(outside.join("admin.json"), "{\"token\":\"x\"}").unwrap();
    let outside_file = outside.join("admin.json").to_string_lossy().to_string();
    let link = inside.join("sneaky.json");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&outside_file, &link).unwrap();
    let r = session_in(
        Some(&inside),
        &[
            call(
                1,
                "verify_log",
                json!({"path": inside.join("log.json").to_string_lossy()}),
            ),
            call(2, "verify_log", json!({"path": outside_file})),
            call(
                3,
                "verify_log",
                json!({"path": outside.join("nope.json").to_string_lossy()}),
            ),
            call(
                4,
                "verify_log",
                json!({"path": inside.join("nope.json").to_string_lossy()}),
            ),
            call(5, "verify_log", json!({"path": link.to_string_lossy()})),
            call(6, "verify_log", json!({"path": "../elsewhere/admin.json"})),
        ],
    );
    let text = |i: usize| {
        r[i]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert!(
        text(0).contains("\"valid\":false"),
        "a file inside the folder is read: {}",
        text(0)
    );
    for (i, x) in r.iter().enumerate().take(6).skip(1) {
        assert!(
            text(i).contains("cannot read that file"),
            "path {} must look like any unreadable file: {}",
            i + 1,
            text(i)
        );
        assert_eq!(x["result"]["isError"], true);
    }
}

#[test]
fn mc05_an_oversized_line_is_refused_without_being_buffered_and_the_next_request_is_still_served() {
    let big = "x".repeat(3 << 20);
    let r = session(&[
        big,
        json!({"jsonrpc": "2.0", "id": 9, "method": "ping"}).to_string(),
    ]);
    assert_eq!(r.len(), 2, "{r:?}");
    assert_eq!(r[0]["error"]["code"], -32600);
    assert_eq!(r[1]["result"], json!({}));
}

fn session_args(extra: &[&str], lines: &[String]) -> Vec<Value> {
    let empty = std::env::temp_dir().join(format!("kk-mcp-ro-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_kaku"))
        .args(["mcp", "--data-dir"])
        .arg(&empty)
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for l in lines {
        writeln!(stdin, "{l}").unwrap();
    }
    drop(stdin);
    String::from_utf8(child.wait_with_output().unwrap().stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn mc06_read_only_mode_hides_and_refuses_every_tool_that_changes_anything() {
    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();
    let full = session_args(&[], std::slice::from_ref(&list));
    let ro = session_args(
        &["--read-only"],
        &[
            list.clone(),
            call(2, "start_review", json!({"repo": "/tmp", "recipient": "x"})),
            call(3, "request_approval", json!({"id": "job-1"})),
            call(4, "overview", json!({})),
        ],
    );
    let names = |r: &Value| {
        r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let (full_names, ro_names) = (names(&full[0]), names(&ro[0]));
    assert!(
        full_names.contains(&"start_review".to_string())
            && full_names.contains(&"request_approval".to_string())
    );
    assert!(
        !ro_names.contains(&"start_review".to_string())
            && !ro_names.contains(&"request_approval".to_string()),
        "{ro_names:?}"
    );
    assert_eq!(ro_names.len(), full_names.len() - 2);
    for t in ro[0]["result"]["tools"].as_array().unwrap() {
        assert_eq!(
            t["annotations"]["readOnlyHint"], true,
            "{} must be read-only",
            t["name"]
        );
    }
    assert_eq!(
        ro[1]["error"]["code"], -32602,
        "a hidden tool is unknown even when called by name"
    );
    assert_eq!(ro[2]["error"]["code"], -32602);
    assert!(ro[3].get("result").is_some(), "reading still works");
    let env = {
        let empty = std::env::temp_dir().join(format!("kk-mcp-ro-env-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_kaku"))
            .args(["mcp", "--data-dir"])
            .arg(&empty)
            .env("KAKU_MCP_READ_ONLY", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(child.stdin.take().unwrap(), "{list}").unwrap();
        let out = child.wait_with_output().unwrap();
        serde_json::from_str::<Value>(
            String::from_utf8(out.stdout)
                .unwrap()
                .lines()
                .next()
                .unwrap(),
        )
        .unwrap()
    };
    assert_eq!(
        names(&env).len(),
        ro_names.len(),
        "the environment switch does the same"
    );
}

#[test]
fn mc90_a_connection_that_calls_too_fast_is_told_to_wait_and_the_flags_are_checked() {
    let lines: Vec<String> = (1..=4).map(|i| call(i, "overview", json!({}))).collect();
    let r = session_args(&["--calls-per-minute", "2"], &lines);
    assert_eq!(r.len(), 4);
    // calls are answered as they finish, so only the counts are fixed: two go through, two are told to wait; four calls stay under the cap on requests in progress
    let limited: Vec<_> = r
        .iter()
        .filter(|v| {
            v["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|t| t.contains("rate limit"))
        })
        .collect();
    assert_eq!(limited.len(), 2, "{r:?}");
    for v in limited {
        assert_eq!(v["result"]["isError"], true, "{v}");
        assert!(v["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("seconds"));
        assert!(
            v.get("error").is_none(),
            "a limit is a tool error, not a protocol error"
        );
    }
    // a limit applies to calls, not to the protocol's other requests
    let mut more = vec![json!({"jsonrpc": "2.0", "id": 90, "method": "ping"}).to_string()];
    more.extend(lines);
    let r = session_args(&["--calls-per-minute", "1"], &more);
    assert_eq!(r[0]["result"], json!({}));
    // values must be numbers in range, otherwise the server does not start
    for bad in [
        ["--calls-per-minute", "0"],
        ["--calls-per-minute", "abc"],
        ["--starts-per-hour", "1001"],
        ["--starts-per-hour", "-3"],
    ] {
        let status = Command::new(env!("CARGO_BIN_EXE_kaku"))
            .args(["mcp"])
            .args(bad)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(2), "{bad:?}");
    }
}
