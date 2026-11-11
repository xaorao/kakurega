#![cfg(target_os = "linux")]

use kk_broker::{Broker, BrokerConfig};
use kk_kernel::{ArtifactHandle, JobKey};
use kk_policy::{Engine, WorkerFacts};
use kk_route::{Route, RouteClass, RouteSet, ScriptedRoute};
use kk_run::{run_script, run_worker, WorkerLaunch};
use kk_sandbox::{Exit, Limits, Transport};
use kk_types::chat::{ChatMessage, ToolCall};
use kk_types::label::{Label, Level};
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

fn job() -> JobKey {
    JobKey::new("tenant", "job-1")
}

fn broker(script: Vec<ChatMessage>) -> Broker {
    let routes = RouteSet::new(vec![Box::new(ScriptedRoute::new(
        "scripted",
        RouteClass::DeviceOnly,
        false,
        script,
    )) as Box<dyn Route>]);
    let mut b = Broker::new(Engine::default_policy(), routes);
    b.import_file(
        &job(),
        "src/auth.rs",
        b"fn logout() { /* token not revoked */ }".to_vec(),
        Label::new(Level::Internal),
    );
    b.import_file(
        &job(),
        "secrets/keys.env",
        b"API_KEY=hunter2".to_vec(),
        Label::new(Level::Restricted).with_compartment("secrets"),
    );
    b
}

fn open(b: &mut Broker) -> kk_broker::SessionId {
    b.open_session(
        &job(),
        WorkerFacts {
            id: "w".into(),
            kind: "AGENT".into(),
            clearance: Label::new(Level::Internal),
            allowed_tools: vec!["list_files".into(), "read_file".into()],
        },
        BrokerConfig::default(),
    )
}

fn calls(items: Vec<(&str, serde_json::Value)>) -> ChatMessage {
    let mut m = ChatMessage::new("assistant", "");
    m.tool_calls = items
        .into_iter()
        .enumerate()
        .map(|(i, (name, args))| ToolCall {
            id: format!("c{i}"),
            name: name.into(),
            arguments: args,
        })
        .collect();
    m
}

fn limits() -> Limits {
    Limits {
        wall: Duration::from_secs(40),
        ..Limits::default()
    }
}

fn worker_binary() -> PathBuf {
    PathBuf::from(std::env::var("KK_WORKER_BIN").expect(
        "KK_WORKER_BIN must point at the Linux kk-worker binary (scripts/test-linux sets it)",
    ))
}

#[test]
fn s01_the_real_worker_binary_completes_a_review_inside_the_sandbox() {
    let mut b = broker(vec![
        calls(vec![("list_files", json!({}))]),
        calls(vec![("read_file", json!({"path": "src/auth.rs"}))]),
        calls(vec![(
            "report_finding",
            json!({"title": "Token not revoked", "file": "src/auth.rs", "line": 1, "explanation": "logout leaves the token valid"}),
        )]),
        ChatMessage::new("assistant", "done"),
    ]);
    let s = open(&mut b);
    let launch = WorkerLaunch {
        binary: worker_binary(),
        system_prompt: "You review code.".into(),
        task: "Review the project.".into(),
        tools_json: json!([
            {"name": "list_files", "description": "", "parameters": {"type": "object"}},
            {"name": "read_file", "description": "", "parameters": {"type": "object"}}
        ])
        .to_string(),
        max_turns: 10,
        limits: limits(),
        transport: Transport::Local,
    };
    let out = run_worker(&mut b, s, &launch).expect("sandbox run");
    assert_eq!(
        out.exit,
        Exit::Code(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.oversize_line);
    let finding = b
        .kernel
        .label_of(&ArtifactHandle::of(&job(), 2))
        .expect("the finding was written through the broker");
    assert_eq!(finding.level, Level::Internal);
    assert!(b.audit.iter().all(|e| e.outcome == "ok"), "{:?}", b.audit);
    assert!(out.lines_served >= 9, "served {}", out.lines_served);
}

#[test]
fn s02_a_hostile_worker_can_only_talk_protocol_and_everything_else_fails() {
    let dir = std::env::temp_dir().join(format!("kk-hostile-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("hostile.py");
    std::fs::write(
        &script,
        r#"
import json, os, socket, sys
def say(obj):
    sys.stdout.write(json.dumps(obj) + "\n"); sys.stdout.flush()
    return json.loads(sys.stdin.readline())
out = {}
try:
    s = socket.socket(); s.settimeout(1); s.connect(("93.184.216.34", 80)); out["net"] = True
except Exception:
    out["net"] = False
out["etc_passwd"] = os.path.exists("/etc/passwd")
out["home"] = os.path.exists("/home") and bool(os.listdir("/home"))
out["garbage"] = say({"id": 1, "op": "exec.shell", "cmd": "id"})["error"]["code"]
sys.stdout.write("this is not json\n"); sys.stdout.flush()
out["garbage2"] = json.loads(sys.stdin.readline())["error"]["code"]
out["restricted"] = say({"id": 2, "op": "read.artifact", "handle": 1})["error"]["code"]
out["lie"] = say({"id": 3, "op": "write.artifact", "bytes_b64": "aGk", "derived_from_hint": [0], "kind": "note"})["ok"]
say({"id": 4, "op": "log.note", "code": json.dumps(out).replace('"', "-").replace(" ", "")[:60].replace("{", "").replace("}", "").replace(":", "_").replace(",", "")})
"#,
    )
    .unwrap();
    let mut b = broker(vec![]);
    let s = open(&mut b);
    let out = run_script(
        &mut b,
        s,
        "/usr/bin/python3",
        &script,
        limits(),
        &Transport::Local,
    )
    .expect("run");
    assert_eq!(
        out.exit,
        Exit::Code(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let outcomes: Vec<(String, String)> = b
        .audit
        .iter()
        .map(|e| (e.op.clone(), e.outcome.clone()))
        .collect();
    assert!(
        outcomes.contains(&("read.artifact".into(), "DenyPolicy".into())),
        "{outcomes:?}"
    );
    assert_eq!(
        b.taint_of(s).map(|t| t.level),
        None,
        "session is closed after the run"
    );
    assert!(
        !b.audit.iter().any(|e| e.op == "tool.call"),
        "the worker had no way to run anything else"
    );
}

#[test]
fn s03_an_oversized_line_kills_the_worker_and_a_silent_worker_times_out() {
    let dir = std::env::temp_dir().join(format!("kk-hostile2-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let flood = dir.join("flood.py");
    std::fs::write(&flood, "import sys\nsys.stdout.write('x' * (9 * 1024 * 1024) + '\\n')\nsys.stdout.flush()\nimport time; time.sleep(30)\n").unwrap();
    let mut b = broker(vec![]);
    let s = open(&mut b);
    let out = run_script(
        &mut b,
        s,
        "/usr/bin/python3",
        &flood,
        limits(),
        &Transport::Local,
    )
    .expect("run");
    assert!(out.oversize_line, "{:?}", out.exit);
    assert!(
        b.audit.is_empty(),
        "the oversized line never reached the broker"
    );
    let silent = dir.join("silent.py");
    std::fs::write(&silent, "import time; time.sleep(60)\n").unwrap();
    let mut b2 = broker(vec![]);
    let s2 = open(&mut b2);
    let mut short = limits();
    short.wall = Duration::from_secs(2);
    let t = std::time::Instant::now();
    let out2 = run_script(
        &mut b2,
        s2,
        "/usr/bin/python3",
        &silent,
        short,
        &Transport::Local,
    )
    .expect("run");
    assert_eq!(out2.exit, Exit::TimedOut);
    assert!(t.elapsed() < Duration::from_secs(10));
    // the owner's stop ends a served worker at once
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut held = limits();
    held.wall = Duration::from_secs(60);
    held.stop = Some(flag.clone());
    let setter = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(700));
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let mut b3 = broker(vec![]);
    let s3 = open(&mut b3);
    let t = std::time::Instant::now();
    let out3 = run_script(
        &mut b3,
        s3,
        "/usr/bin/python3",
        &silent,
        held,
        &Transport::Local,
    )
    .expect("run");
    assert_eq!(out3.exit, Exit::Stopped);
    assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
    setter.join().unwrap();
}
