use kk_types::chat::ToolCall;
use kk_worker::{run_local_tool, Link, WorkerError};
use serde_json::{json, Value};

/// A broker that answers every write with the given result.
struct Canned(Value, Vec<Value>);

impl Link for Canned {
    fn call(&mut self, request: Value) -> Result<Value, WorkerError> {
        self.1.push(request);
        Ok(json!({"id": 1, "ok": true, "result": self.0.clone()}))
    }
}

fn call(name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        id: "c1".into(),
        name: name.into(),
        arguments,
    }
}

const FINDING: fn() -> Value = || json!({"title": "t", "file": "src/a.py", "line": 3, "explanation": "e", "reproducer": "print(1)"});
const FIX: fn() -> Value =
    || json!({"title": "t", "file": "src/a.py", "old_text": "a", "new_text": "b"});

#[test]
fn wf01_the_brokers_verdict_on_a_finding_or_a_fix_reaches_the_model_as_check() {
    let verdict = json!({"status": "confirmed", "note": "n"});
    let mut link = Canned(json!({"handle": 7, "feedback": verdict}), vec![]);
    let r = run_local_tool(&mut link, &call("report_finding", FINDING())).unwrap();
    assert_eq!(
        r,
        json!({"recorded": true, "handle": 7, "check": {"status": "confirmed", "note": "n"}})
    );
    let r = run_local_tool(&mut link, &call("propose_fix", FIX())).unwrap();
    assert_eq!(
        r,
        json!({"proposed": true, "handle": 7, "check": {"status": "confirmed", "note": "n"}})
    );
    // the write itself is unchanged
    assert_eq!(link.1[0]["op"], "write.artifact");
    assert_eq!(link.1[0]["kind"], "finding");
    assert_eq!(link.1[1]["kind"], "fix");
}

#[test]
fn wf02_without_a_verdict_the_tool_result_is_exactly_what_it_was() {
    let mut link = Canned(json!({"handle": 3}), vec![]);
    assert_eq!(
        run_local_tool(&mut link, &call("report_finding", FINDING())).unwrap(),
        json!({"recorded": true, "handle": 3})
    );
    assert_eq!(
        run_local_tool(&mut link, &call("propose_fix", FIX())).unwrap(),
        json!({"proposed": true, "handle": 3})
    );
    // a verdict of null is no verdict
    let mut link = Canned(json!({"handle": 3, "feedback": null}), vec![]);
    assert_eq!(
        run_local_tool(&mut link, &call("report_finding", FINDING())).unwrap(),
        json!({"recorded": true, "handle": 3})
    );
}

#[test]
fn wf03_an_incomplete_call_is_refused_before_anything_is_written_and_gets_no_verdict() {
    let mut link = Canned(json!({"handle": 1, "feedback": {"status": "x"}}), vec![]);
    let r = run_local_tool(&mut link, &call("report_finding", json!({"title": "t"}))).unwrap();
    assert!(r["error"].is_string() && r.get("check").is_none(), "{r}");
    let r = run_local_tool(&mut link, &call("propose_fix", json!({"title": "t"}))).unwrap();
    assert!(r["error"].is_string() && r.get("check").is_none(), "{r}");
    assert!(link.1.is_empty(), "nothing was sent to the broker");
}
