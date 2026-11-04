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

fn vm_home() -> String {
    std::env::var("KK_VM_HOME")
        .expect("KK_VM_HOME is the VM user's home directory (scripts/test-lima sets it)")
}

fn transport() -> Transport {
    Transport::Lima {
        vm: "kak".into(),
        launcher: PathBuf::from(format!("{}/kk-target/debug/kk-sandbox-launch", vm_home())),
    }
}

fn job() -> JobKey {
    JobKey::new("tenant", "lima")
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

fn session(b: &mut Broker) -> kk_broker::SessionId {
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

fn limits() -> Limits {
    Limits {
        wall: Duration::from_secs(60),
        ..Limits::default()
    }
}

fn call(items: Vec<(&str, serde_json::Value)>) -> ChatMessage {
    let mut m = ChatMessage::new("assistant", "");
    m.tool_calls = items
        .into_iter()
        .enumerate()
        .map(|(i, (n, a))| ToolCall {
            id: format!("c{i}"),
            name: n.into(),
            arguments: a,
        })
        .collect();
    m
}

#[test]
#[ignore]
fn l01_the_real_worker_runs_in_the_vm_sandbox_with_the_broker_on_the_host() {
    let mut b = broker(vec![
        call(vec![("read_file", json!({"path": "src/auth.rs"}))]),
        call(vec![(
            "report_finding",
            json!({"title": "Token not revoked", "file": "src/auth.rs", "line": 1, "explanation": "x"}),
        )]),
        ChatMessage::new("assistant", "done"),
    ]);
    let s = session(&mut b);
    let launch = WorkerLaunch {
        binary: PathBuf::from(format!("{}/kk-target/debug/kk-worker", vm_home())),
        system_prompt: "You review code.".into(),
        task: "Review.".into(),
        tools_json:
            json!([{"name": "read_file", "description": "", "parameters": {"type": "object"}}])
                .to_string(),
        max_turns: 8,
        limits: limits(),
        transport: transport(),
    };
    let out = run_worker(&mut b, s, &launch).expect("lima run");
    assert_eq!(
        out.exit,
        Exit::Code(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        b.kernel.label_of(&ArtifactHandle::of(&job(), 2)).is_some(),
        "the finding was written through the broker"
    );
    assert!(b.audit.iter().all(|e| e.outcome == "ok"), "{:?}", b.audit);
}

#[test]
#[ignore]
fn l02_a_hostile_worker_in_the_vm_cannot_reach_the_hosts_network_or_files() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/lima-tmp")
        .join(format!("hostile-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("host-secret.txt");
    std::fs::write(&marker, "HOST SECRET").unwrap();
    let script = dir.join("w.py");
    std::fs::write(
        &script,
        format!(
            r#"
import json, os, socket, sys
res = {{}}
for host in ("192.168.5.2", "host.lima.internal", "127.0.0.1"):
    try:
        s = socket.create_connection((host, 11434), timeout=1); res[host] = True
    except Exception:
        res[host] = False
res["host_file"] = os.path.exists({marker:?})
res["users_dir"] = os.path.exists("/Users")
if any(res.values()):
    sys.stderr.write("ESCAPE " + json.dumps(res)); sys.exit(7)
sys.stdout.write(json.dumps({{"id": 1, "op": "log.note", "code": "net_" + "".join("1" if v else "0" for v in res.values())}}) + "\n"); sys.stdout.flush()
sys.stdin.readline()
"#,
            marker = marker.to_string_lossy()
        ),
    )
    .unwrap();
    let mut b = broker(vec![]);
    let s = session(&mut b);
    let out = run_script(
        &mut b,
        s,
        "/usr/bin/python3",
        &script,
        limits(),
        &transport(),
    )
    .expect("run");
    assert_eq!(
        out.exit,
        Exit::Code(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(b.audit.len(), 1);
    assert!(b.audit[0].op == "log.note", "{:?}", b.audit);
}
