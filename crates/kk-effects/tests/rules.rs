mod support;

use kk_effects::{
    Class, Clock, Grant, Kind, Ledger, LedgerError, NewIntent, Resolution, State, SystemClock,
};
use kk_vault::Vault;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Barrier};
use support::*;

#[derive(Clone)]
struct ManualClock(Rc<Cell<i64>>);

impl Clock for ManualClock {
    fn now(&self) -> i64 {
        self.0.get()
    }
}

fn ledger_at(dir: &std::path::Path, key: [u8; 32]) -> Ledger<SystemClock> {
    let path = dir.join("ledger.db");
    let vault = if path.exists() {
        Vault::open(&path, &vault_key(key)).expect("open")
    } else {
        Vault::create(&path, &vault_key(key)).expect("create")
    };
    Ledger::open_owner(vault, SystemClock, &dir.join("ledger.lock")).expect("ledger")
}

fn prepare(ledger: &mut Ledger<SystemClock>, class: Class, id: &str) -> Kind {
    let (intent, kind) = intent_for(class, id);
    ledger.record_intent(&intent).expect("intent");
    ledger
        .authorize(id, kind, &grant_for(class, kind))
        .expect("authorize");
    kind
}

#[test]
fn ef02_executing_a_finished_intent_again_is_a_lookup_not_a_new_call() {
    let dir = fresh_dir("replay");
    let key = random_bytes();
    let remote = FileRemote::new(&dir, false, false);
    let mut l = ledger_at(&dir, key);
    prepare(&mut l, Class::E3, "i1");
    assert_eq!(l.execute("i1", &remote, b"x").unwrap(), State::Confirmed);
    for _ in 0..3 {
        assert_eq!(l.execute("i1", &remote, b"x").unwrap(), State::Confirmed);
    }
    assert_eq!(remote.applied("i1"), 1);
    assert_eq!(l.get("i1").unwrap().attempts, 1);
}

#[test]
fn ef03_unknown_outcome_is_uncertain_and_reconciled_per_class() {
    let dir = fresh_dir("unknown");
    let key = random_bytes();
    let mut remote = FileRemote::new(&dir, true, false);
    remote.unknown_outcome_on_send = true;
    let mut l = ledger_at(&dir, key);
    prepare(&mut l, Class::E1, "e1");
    assert_eq!(l.execute("e1", &remote, b"x").unwrap(), State::Uncertain);
    let quiet = FileRemote {
        unknown_outcome_on_send: false,
        ..FileRemote::new(&dir, true, false)
    };
    assert_eq!(l.execute("e1", &quiet, b"x").unwrap(), State::Confirmed);
    assert_eq!(quiet.applied("e1"), 1, "same key reused, one effect");

    let dir3 = fresh_dir("unknown3");
    let mut remote3 = FileRemote::new(&dir3, false, false);
    remote3.unknown_outcome_on_send = true;
    let mut l3 = ledger_at(&dir3, key);
    prepare(&mut l3, Class::E3, "e3");
    assert_eq!(l3.execute("e3", &remote3, b"x").unwrap(), State::Uncertain);
    assert!(matches!(
        l3.execute("e3", &remote3, b"x"),
        Err(LedgerError::NoAutomaticRetry)
    ));
    assert_eq!(remote3.applied("e3"), 1);
    l3.resolve_uncertain("e3", Resolution::ConfirmedByUser, "recipient replied")
        .unwrap();
    assert_eq!(l3.get("e3").unwrap().state, State::Confirmed);
}

#[test]
fn ef04_a_second_authorisation_and_a_racing_worker_are_refused() {
    let dir = fresh_dir("race");
    let key: [u8; 32] = random_bytes();
    {
        let mut l = ledger_at(&dir, key);
        let kind = prepare(&mut l, Class::E3, "r1");
        assert!(matches!(
            l.authorize("r1", kind, &grant_for(Class::E3, kind)),
            Err(LedgerError::AlreadyAuthorized)
        ));
    }
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let dir = dir.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            let remote = FileRemote::new(&dir, false, false);
            let vault = Vault::open(&dir.join("ledger.db"), &vault_key(key)).expect("open");
            let mut l = Ledger::open_shared(vault, SystemClock).expect("shared");
            barrier.wait();
            l.execute("r1", &remote, b"x")
        }));
    }
    let results: Vec<_> = handles
        .into_iter()
        .map(|h| h.join().expect("thread"))
        .collect();
    let remote = FileRemote::new(&dir, false, false);
    assert_eq!(
        remote.applied("r1"),
        1,
        "exactly one worker may send: {results:?}"
    );
    assert!(
        results
            .iter()
            .filter(|r| matches!(r, Ok(State::Confirmed)))
            .count()
            >= 1
    );
}

#[test]
fn ef06_spending_and_permission_changes_need_a_grant_naming_them() {
    let dir = fresh_dir("grant");
    let key = random_bytes();
    let mut l = ledger_at(&dir, key);
    for kind in [Kind::Funds, Kind::PermissionChange] {
        let id = format!("g-{}", kind as u8);
        l.record_intent(&NewIntent {
            intent_id: id.clone(),
            kind,
            class: Class::E4,
            idempotency_key: None,
            key_window_secs: None,
            cooldown_secs: 60,
        })
        .unwrap();
        let mut general = Grant::general("step-capability");
        general.exact_amount = Some("1".into());
        general.recipient = Some("x".into());
        general.purpose = Some("y".into());
        assert!(matches!(
            l.authorize(&id, kind, &general),
            Err(LedgerError::GrantInsufficient(_))
        ));
        let mut specific = Grant::specific("own-grant", kind);
        assert!(
            matches!(
                l.authorize(&id, kind, &specific),
                Err(LedgerError::GrantInsufficient(_))
            ),
            "E4 needs amount, recipient, purpose"
        );
        specific.exact_amount = Some("1".into());
        specific.recipient = Some("x".into());
        specific.purpose = Some("y".into());
        l.authorize(&id, kind, &specific).unwrap();
    }
}

#[test]
fn ef07_the_idempotency_key_retention_window_is_respected() {
    let dir = fresh_dir("window");
    let key = random_bytes();
    let now = Rc::new(Cell::new(1_000_000));
    let path = dir.join("ledger.db");
    let vault = Vault::create(&path, &vault_key(key)).unwrap();
    let mut l =
        Ledger::open_owner(vault, ManualClock(now.clone()), &dir.join("ledger.lock")).unwrap();
    let (mut intent, kind) = intent_for(Class::E1, "w1");
    intent.key_window_secs = Some(60);
    l.record_intent(&intent).unwrap();
    l.authorize("w1", kind, &grant_for(Class::E1, kind))
        .unwrap();
    let mut remote = FileRemote::new(&dir, true, false);
    remote.unknown_outcome_on_send = true;
    assert_eq!(l.execute("w1", &remote, b"x").unwrap(), State::Uncertain);
    now.set(1_000_000 + 61);
    let quiet = FileRemote::new(&dir, true, false);
    assert!(matches!(
        l.execute("w1", &quiet, b"x"),
        Err(LedgerError::KeyWindowExpired)
    ));
    assert_eq!(l.get("w1").unwrap().state, State::Uncertain);
}

#[test]
fn ef08_e2_retries_exactly_once_when_read_back_finds_nothing() {
    let dir = fresh_dir("e2once");
    let key = random_bytes();
    let mut remote = FileRemote::new(&dir, false, true);
    remote.unknown_outcome_on_send = true;
    let mut l = ledger_at(&dir, key);
    prepare(&mut l, Class::E2, "m1");
    let other = FileRemote {
        log: dir.join("elsewhere.log"),
        ..FileRemote::new(&dir, false, true)
    };
    assert_eq!(l.execute("m1", &other, b"x").unwrap(), State::Confirmed);
    let dir2 = fresh_dir("e2twice");
    let mut l2 = ledger_at(&dir2, key);
    prepare(&mut l2, Class::E2, "m2");
    struct Lossy;
    impl kk_effects::Remote for Lossy {
        fn send(&self, _: Option<&str>, _: &str, _: &[u8]) -> kk_effects::SendResult {
            kk_effects::SendResult::OutcomeUnknown
        }
        fn read_back(&self, _: &str) -> Result<bool, String> {
            Ok(false)
        }
    }
    assert_eq!(l2.execute("m2", &Lossy, b"x").unwrap(), State::Uncertain);
    assert_eq!(l2.execute("m2", &Lossy, b"x").unwrap(), State::Uncertain);
    assert!(matches!(
        l2.execute("m2", &Lossy, b"x"),
        Err(LedgerError::NoAutomaticRetry)
    ));
    assert_eq!(
        l2.get("m2").unwrap().attempts,
        2,
        "first attempt plus exactly one retry"
    );
}

#[test]
fn ef10_e4_cooldown_blocks_a_second_attempt_and_everything_is_logged() {
    let dir = fresh_dir("e4");
    let key = random_bytes();
    let now = Rc::new(Cell::new(5_000));
    let vault = Vault::create(&dir.join("ledger.db"), &vault_key(key)).unwrap();
    let mut l =
        Ledger::open_owner(vault, ManualClock(now.clone()), &dir.join("ledger.lock")).unwrap();
    let (intent, kind) = intent_for(Class::E4, "p1");
    l.record_intent(&intent).unwrap();
    l.authorize("p1", kind, &grant_for(Class::E4, kind))
        .unwrap();
    let mut remote = FileRemote::new(&dir, false, false);
    remote.unknown_outcome_on_send = true;
    assert_eq!(l.execute("p1", &remote, b"pay").unwrap(), State::Uncertain);
    assert!(matches!(
        l.allow_new_attempt_after("p1"),
        Err(LedgerError::NoAutomaticRetry)
    ));
    l.resolve_uncertain("p1", Resolution::FailedByUser, "bank shows no payment")
        .unwrap();
    assert!(matches!(
        l.allow_new_attempt_after("p1"),
        Err(LedgerError::CooldownActive)
    ));
    now.set(5_000 + 601);
    assert!(l.allow_new_attempt_after("p1").is_ok());
    let rec = l.get("p1").unwrap();
    assert_eq!(rec.state, State::Failed);
    assert_eq!(remote.applied("p1"), 1);
    assert!(l.log_len().unwrap() >= 5);
}

#[test]
fn terminal_states_are_written_once_and_the_log_is_append_only() {
    let dir = fresh_dir("terminal");
    let key = random_bytes();
    let remote = FileRemote::new(&dir, false, false);
    let mut l = ledger_at(&dir, key);
    prepare(&mut l, Class::E3, "t1");
    l.execute("t1", &remote, b"x").unwrap();
    assert!(matches!(
        l.resolve_uncertain("t1", Resolution::FailedByUser, "late"),
        Err(LedgerError::IllegalTransition { .. })
    ));
    assert_eq!(l.get("t1").unwrap().state, State::Confirmed);
    let vault = l.into_vault();
    assert!(vault
        .connection()
        .execute("UPDATE effect_log SET to_state = 'FAILED'", [])
        .is_err());
    assert!(vault
        .connection()
        .execute("DELETE FROM effect_log", [])
        .is_err());
}
