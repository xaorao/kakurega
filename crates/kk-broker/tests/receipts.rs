use kk_broker::{Broker, BrokerConfig, SessionId};
use kk_kernel::JobKey;
use kk_policy::{Engine, WorkerFacts};
use kk_route::{
    ChatMessage, Completion, CompletionRequest, Route, RouteClass, RouteError, RouteEvidence,
    RouteSet, ScriptedRoute,
};
use kk_types::label::{Label, Level};
use serde_json::{json, Value};
use std::sync::Mutex;

/// A route that behaves like a verified model route: each answer leaves a receipt on its evidence.
struct Receipting {
    n: Mutex<usize>,
    last: Mutex<Option<Value>>,
    with_receipts: bool,
}

impl Route for Receipting {
    fn id(&self) -> &str {
        "near:test"
    }
    fn class(&self) -> RouteClass {
        RouteClass::VerifiedTee
    }
    fn tee_verified(&self) -> bool {
        true
    }
    fn reply_key_bound(&self) -> bool {
        true
    }
    fn evidence(&self) -> RouteEvidence {
        RouteEvidence {
            route_id: "near:test".into(),
            class: "verified_tee".into(),
            model: "m".into(),
            model_digest: None,
            endpoint: "in-process".into(),
            tee_verified: true,
            simulated: true,
            receipt: self.last.lock().unwrap().clone(),
        }
    }
    fn complete(&self, _r: &CompletionRequest) -> Result<Completion, RouteError> {
        let mut n = self.n.lock().unwrap();
        *n += 1;
        if self.with_receipts {
            *self.last.lock().unwrap() = Some(
                json!({"completion_id": format!("chat-{n}"), "signature_kind": "provider_tee"}),
            );
        }
        Ok(Completion {
            message: ChatMessage::new("assistant", "ok"),
            prompt_tokens: 1,
            completion_tokens: 1,
            cut_short: false,
        })
    }
}

fn session(routes: RouteSet) -> (Broker, SessionId) {
    let mut b = Broker::new(Engine::default_policy(), routes);
    let j = JobKey::new("tenant", "job-1");
    let clearance = Label::new(Level::Restricted).with_compartment("secrets");
    let s = b.open_session(
        &j,
        WorkerFacts {
            id: "w".into(),
            kind: "AGENT".into(),
            clearance,
            allowed_tools: vec![],
        },
        BrokerConfig::default(),
    );
    (b, s)
}

fn ask(b: &mut Broker, s: SessionId, id: u64) -> Value {
    let line = json!({"id": id, "op": "model.complete", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 8}).to_string();
    serde_json::from_str(&b.handle_line(s, &line)).unwrap()
}

#[test]
fn rc01_each_answer_of_a_verified_route_leaves_its_receipt_with_the_job() {
    let route = Receipting {
        n: Mutex::new(0),
        last: Mutex::new(None),
        with_receipts: true,
    };
    let (mut b, s) = session(RouteSet::new(vec![Box::new(route) as Box<dyn Route>]));
    for i in 1..=3 {
        let r = ask(&mut b, s, i);
        assert_eq!(r["ok"], true, "{r}");
    }
    let ids: Vec<&str> = b
        .receipts
        .iter()
        .map(|r| r["completion_id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["chat-1", "chat-2", "chat-3"]);
}

#[test]
fn rc02_a_local_route_has_no_receipts_and_a_route_with_none_adds_none() {
    let local = ScriptedRoute::new(
        "local",
        RouteClass::DeviceOnly,
        false,
        vec![
            ChatMessage::new("assistant", "x"),
            ChatMessage::new("assistant", "y"),
        ],
    );
    let (mut b, s) = session(RouteSet::new(vec![Box::new(local) as Box<dyn Route>]));
    assert_eq!(ask(&mut b, s, 1)["ok"], true);
    assert_eq!(ask(&mut b, s, 2)["ok"], true);
    assert!(b.receipts.is_empty());
    let silent = Receipting {
        n: Mutex::new(0),
        last: Mutex::new(None),
        with_receipts: false,
    };
    let (mut b2, s2) = session(RouteSet::new(vec![Box::new(silent) as Box<dyn Route>]));
    assert_eq!(ask(&mut b2, s2, 1)["ok"], true);
    assert!(b2.receipts.is_empty());
}

#[test]
fn rc03_a_refused_or_failed_answer_leaves_no_receipt_and_the_list_is_capped() {
    // no eligible route: nothing is asked, nothing is kept
    let unverified = ScriptedRoute::new("tee", RouteClass::VerifiedTee, false, vec![]);
    let (mut b, s) = session(RouteSet::new(vec![Box::new(unverified) as Box<dyn Route>]));
    let r = ask(&mut b, s, 1);
    assert_eq!(r["error"]["code"], "NO_ELIGIBLE_ROUTE", "{r}");
    assert!(b.receipts.is_empty());
    // many answers: at most 64 are kept
    let many = Receipting {
        n: Mutex::new(0),
        last: Mutex::new(None),
        with_receipts: true,
    };
    let (mut b2, s2) = session(RouteSet::new(vec![Box::new(many) as Box<dyn Route>]));
    let mut cfg_ok = 0;
    for i in 1..=80 {
        if ask(&mut b2, s2, i)["ok"] == true {
            cfg_ok += 1;
        }
    }
    assert!(cfg_ok >= 65, "the broker refused answers early: {cfg_ok}");
    assert_eq!(b2.receipts.len(), 64);
}

/// A local route whose evidence is costly (it asks the model service): the broker must not ask for it on every answer.
struct CountingLocal(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl Route for CountingLocal {
    fn id(&self) -> &str {
        "local:test"
    }
    fn class(&self) -> RouteClass {
        RouteClass::DeviceOnly
    }
    fn tee_verified(&self) -> bool {
        false
    }
    fn evidence(&self) -> RouteEvidence {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        RouteEvidence {
            route_id: "local:test".into(),
            class: "device_only".into(),
            model: "m".into(),
            model_digest: None,
            endpoint: "in-process".into(),
            tee_verified: false,
            simulated: true,
            receipt: None,
        }
    }
    fn complete(&self, _r: &CompletionRequest) -> Result<Completion, RouteError> {
        Ok(Completion {
            message: ChatMessage::new("assistant", "ok"),
            prompt_tokens: 1,
            completion_tokens: 1,
            cut_short: false,
        })
    }
}

#[test]
fn rc04_the_broker_never_asks_a_local_route_for_its_evidence_after_an_answer() {
    let asked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (mut b, s) = session(RouteSet::new(vec![
        Box::new(CountingLocal(asked.clone())) as Box<dyn Route>
    ]));
    for i in 1..=3 {
        assert_eq!(ask(&mut b, s, i)["ok"], true);
    }
    assert_eq!(asked.load(std::sync::atomic::Ordering::SeqCst), 0);
}
