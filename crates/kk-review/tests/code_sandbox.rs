//! The coding task's tester against the real sandbox. These run in the Linux test machine (scripts/test-lima), where the
//! sandbox is the local one; on a Mac they are ignored.

use kk_review::code::*;
use kk_review::import::ImportedFile;
use kk_route::{RouteClass, ScriptedRoute};
use kk_sandbox::Transport;
use kk_types::chat::{ChatMessage, ToolCall};
use kk_types::label::{Label, Level};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn transport() -> Transport {
    if cfg!(target_os = "linux") {
        Transport::Local
    } else {
        let home = std::env::var("KK_VM_HOME")
            .expect("KK_VM_HOME is the VM user's home (scripts/test-lima sets it)");
        Transport::Lima {
            vm: "kak".into(),
            launcher: PathBuf::from(format!("{home}/kk-target/debug/kk-sandbox-launch")),
        }
    }
}

fn scratch() -> PathBuf {
    let d = if cfg!(target_os = "linux") {
        std::env::temp_dir().join("kk-code-scratch")
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/code-tmp")
    };
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn map(files: &[(&str, &str)]) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|(p, t)| (p.to_string(), t.to_string()))
        .collect()
}

const APP: &str = "def add(a, b):\n    return a - b\n";
const TEST: &str = "from app import add\n\ndef test_add():\n    assert add(1, 2) == 3\n";

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn cs01_python_tests_fail_on_the_bug_and_pass_on_the_fix_in_the_sandbox() {
    let t = transport();
    let s = scratch();
    let tester = SandboxTester {
        transport: &t,
        scratch: &s,
        runner: Runner::Python,
        interpreter: "/usr/bin/python3".into(),
    };
    let bad = tester.run(&map(&[("app.py", APP), ("test_app.py", TEST)]));
    assert!(!bad.passed, "{}", bad.output);
    assert!(
        bad.output.contains("FAILED test_app.py::test_add")
            && bad.output.contains("0 passed, 1 failed"),
        "{}",
        bad.output
    );
    let good = tester.run(&map(&[
        ("app.py", "def add(a, b):\n    return a + b\n"),
        ("test_app.py", TEST),
    ]));
    assert!(good.passed, "{}", good.output);
    assert!(
        good.output.contains("1 passed, 0 failed"),
        "{}",
        good.output
    );
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn cs02_no_tests_or_only_fixture_tests_never_count_as_passing() {
    let t = transport();
    let s = scratch();
    let tester = SandboxTester {
        transport: &t,
        scratch: &s,
        runner: Runner::Python,
        interpreter: "/usr/bin/python3".into(),
    };
    let none = tester.run(&map(&[("app.py", APP)]));
    assert!(
        !none.passed && none.output.contains("0 passed, 0 failed"),
        "{}",
        none.output
    );
    let fixture = tester.run(&map(&[
        ("app.py", APP),
        ("test_x.py", "def test_needs(tmp_path):\n    assert True\n"),
    ]));
    assert!(
        !fixture.passed && fixture.output.contains("SKIPPED"),
        "{}",
        fixture.output
    );
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn cs03_a_test_cannot_reach_the_network_or_write_to_the_project() {
    let t = transport();
    let s = scratch();
    let tester = SandboxTester {
        transport: &t,
        scratch: &s,
        runner: Runner::Python,
        interpreter: "/usr/bin/python3".into(),
    };
    let net =
        "import socket\n\ndef test_net():\n    socket.create_connection(('1.1.1.1', 80), 2)\n";
    let r = tester.run(&map(&[("test_net.py", net)]));
    assert!(
        !r.passed && r.output.contains("FAILED test_net.py::test_net"),
        "{}",
        r.output
    );
    let write = "def test_write():\n    open('/project/owned.txt', 'w').write('x')\n";
    let w = tester.run(&map(&[("test_write.py", write)]));
    assert!(!w.passed && w.output.contains("FAILED"), "{}", w.output);
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn cs04_node_tests_run_with_nodes_own_runner_and_an_empty_run_is_not_a_pass() {
    let t = transport();
    let s = scratch();
    let tester = SandboxTester {
        transport: &t,
        scratch: &s,
        runner: Runner::Node,
        interpreter: "/usr/bin/node".into(),
    };
    let lib = "export function add(a, b) { return a - b; }\n";
    let test = "import test from 'node:test';\nimport assert from 'node:assert/strict';\nimport { add } from './lib.mjs';\ntest('add', () => assert.equal(add(1, 2), 3));\n";
    let bad = tester.run(&map(&[("lib.mjs", lib), ("lib.test.mjs", test)]));
    assert!(!bad.passed, "{}", bad.output);
    let good = tester.run(&map(&[
        ("lib.mjs", "export function add(a, b) { return a + b; }\n"),
        ("lib.test.mjs", test),
    ]));
    assert!(good.passed, "{}", good.output);
    let empty = tester.run(&map(&[("lib.mjs", lib)]));
    assert!(!empty.passed, "{}", empty.output);
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn cs05_a_whole_task_with_the_real_sandbox_ends_with_passing_tests() {
    let t = transport();
    let s = scratch();
    let tester = SandboxTester {
        transport: &t,
        scratch: &s,
        runner: Runner::Python,
        interpreter: "/usr/bin/python3".into(),
    };
    let file = |p: &str, text: &str| {
        let bytes = text.as_bytes().to_vec();
        ImportedFile {
            path: p.into(),
            sha256: kk_evidence::capsule::sha256_hex(&bytes),
            bytes,
            label: Label::new(Level::Internal),
            notes: vec![],
        }
    };
    let (mut w, _) = Workspace::from_imported(&[file("app.py", APP), file("test_app.py", TEST)]);
    let call = |n: &str, a: serde_json::Value| {
        let mut m = ChatMessage::new("assistant", "");
        m.tool_calls = vec![ToolCall {
            id: "1".into(),
            name: n.into(),
            arguments: a,
        }];
        m
    };
    let r = ScriptedRoute::new(
        "s",
        RouteClass::DeviceOnly,
        false,
        vec![
            call(
                "edit_file",
                json!({"path": "app.py", "old_text": "a - b", "new_text": "a + b"}),
            ),
            call("run_tests", json!({})),
            call("finish", json!({"summary": "fixed add"})),
        ],
    );
    let rep = run_task(&r, &mut w, "fix add", Some(&tester), 10);
    assert_eq!(rep.ending, Ending::Finished);
    assert!(
        rep.last_test.as_ref().is_some_and(|t| t.passed),
        "{:?}",
        rep.last_test
    );
    // the kernel's own run after the task, on the final files, agrees
    assert!(tester.run(w.files()).passed);
}

#[test]
fn cs06_the_runner_is_chosen_from_the_projects_own_files() {
    assert_eq!(
        detect_runner(&map(&[("app.py", APP), ("tests/test_app.py", TEST)])),
        Some(Runner::Python)
    );
    assert_eq!(
        detect_runner(&map(&[("app_test.py", TEST)])),
        Some(Runner::Python)
    );
    assert_eq!(
        detect_runner(&map(&[("src/lib.mjs", "x"), ("src/lib.test.mjs", "x")])),
        Some(Runner::Node)
    );
    assert_eq!(
        detect_runner(&map(&[("test/run.js", "x")])),
        Some(Runner::Node)
    );
    assert_eq!(
        detect_runner(&map(&[("app.py", APP), ("README.md", "x")])),
        None
    );
    assert_eq!(
        detect_runner(&map(&[("contest.py", "x"), ("latest.js", "x")])),
        None,
        "a name that only contains 'test' is not a test file"
    );
}
