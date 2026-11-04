mod support;

use kk_effects::{Class, Ledger, LedgerError, State, SystemClock};
use kk_vault::Vault;
use std::path::Path;
use std::process::Command;
use support::*;

const POINTS: [&str; 6] = [
    "none",
    "after_intended",
    "after_authorized",
    "after_attempted",
    "after_send",
    "after_outcome",
];

fn child(
    dir: &Path,
    key: &[u8; 32],
    class: &str,
    remote_mode: &str,
    failpoint: &str,
) -> std::process::ExitStatus {
    Command::new(std::env::current_exe().expect("exe"))
        .args(["--exact", "child_step", "--ignored", "--nocapture"])
        .env("KK_DIR", dir)
        .env("KK_KEY", hex(key))
        .env("KK_CLASS", class)
        .env("KK_REMOTE", remote_mode)
        .env("KK_FAILPOINT", failpoint)
        .status()
        .expect("spawn")
}

fn class_of(s: &str) -> Class {
    match s {
        "E1" => Class::E1,
        "E2" => Class::E2,
        "E3" => Class::E3,
        _ => Class::E4,
    }
}

fn remote_for(dir: &Path, mode: &str) -> FileRemote {
    match mode {
        "idempotent" => FileRemote::new(dir, true, false),
        "readback" => FileRemote::new(dir, false, true),
        _ => FileRemote::new(dir, false, false),
    }
}

#[test]
#[ignore]
fn child_step() {
    let dir = std::path::PathBuf::from(std::env::var("KK_DIR").expect("dir"));
    let key = unhex32(&std::env::var("KK_KEY").expect("key"));
    let class = class_of(&std::env::var("KK_CLASS").expect("class"));
    let remote = remote_for(&dir, &std::env::var("KK_REMOTE").expect("remote"));
    let vault = Vault::create(&dir.join("ledger.db"), &vault_key(key)).expect("create vault");
    let mut ledger =
        Ledger::open_owner(vault, SystemClock, &dir.join("ledger.lock")).expect("ledger");
    let (intent, kind) = intent_for(class, "intent-1");
    ledger.record_intent(&intent).expect("intent");
    ledger
        .authorize("intent-1", kind, &grant_for(class, kind))
        .expect("authorize");
    let _ = ledger.execute("intent-1", &remote, b"payload");
}

fn resume(
    dir: &Path,
    key: &[u8; 32],
    class: Class,
    remote: &FileRemote,
) -> (State, Result<State, LedgerError>) {
    let vault = Vault::open(&dir.join("ledger.db"), &vault_key(*key)).expect("reopen");
    let mut ledger =
        Ledger::open_owner(vault, SystemClock, &dir.join("ledger.lock")).expect("ledger recovery");
    let (_, kind) = intent_for(class, "intent-1");
    let before = match ledger.get("intent-1") {
        Ok(r) => r.state,
        Err(_) => State::Intended,
    };
    if before == State::Intended {
        if ledger.get("intent-1").is_err() {
            let (intent, _) = intent_for(class, "intent-1");
            ledger.record_intent(&intent).expect("re-intend");
        }
        ledger
            .authorize("intent-1", kind, &grant_for(class, kind))
            .expect("authorize on resume");
    }
    let r = ledger.execute("intent-1", remote, b"payload");
    (before, r)
}

#[test]
fn crash_at_every_boundary_for_every_class_never_duplicates_an_effect() {
    let mut report = Vec::new();
    for (class_s, remote_mode) in [
        ("E1", "idempotent"),
        ("E2", "readback"),
        ("E3", "opaque"),
        ("E4", "opaque"),
    ] {
        for point in POINTS {
            let dir = fresh_dir(&format!("{class_s}-{point}"));
            let key: [u8; 32] = random_bytes();
            let status = child(&dir, &key, class_s, remote_mode, point);
            if point == "none" {
                assert!(status.success(), "control run failed");
            } else if !status.success() {
                assert!(dir.join("ledger.db").exists() || point == "none");
            }
            let class = class_of(class_s);
            let remote = remote_for(&dir, remote_mode);
            let applied_before = remote.applied("intent-1");
            let (before, result) = resume(&dir, &key, class, &remote);
            let applied = remote.applied("intent-1");
            let vault = Vault::open(&dir.join("ledger.db"), &vault_key(key)).expect("open");
            let ledger =
                Ledger::open_owner(vault, SystemClock, &dir.join("ledger.lock")).expect("ledger");
            let rec = ledger.get("intent-1").expect("record");
            report.push(format!(
                "{class_s} crash@{point:<16} state-after-recovery={:<10} sends-before-resume={applied_before} sends-after={applied} final={} attempts={}",
                before.as_str(),
                rec.state.as_str(),
                rec.attempts
            ));
            match class {
                Class::E1 | Class::E2 => {
                    assert_eq!(
                        applied, 1,
                        "{class_s}@{point}: exactly one effect expected, got {applied}"
                    );
                    assert_eq!(rec.state, State::Confirmed, "{class_s}@{point}: {result:?}");
                }
                Class::E3 | Class::E4 => {
                    assert!(applied <= 1, "{class_s}@{point}: duplicate effect");
                    match point {
                        "after_attempted" | "after_send" => {
                            assert_eq!(rec.state, State::Uncertain, "{class_s}@{point}");
                            assert!(
                                matches!(result, Err(LedgerError::NoAutomaticRetry)),
                                "{class_s}@{point}: {result:?}"
                            );
                            assert_eq!(applied, if point == "after_send" { 1 } else { 0 });
                        }
                        _ => {
                            assert_eq!(
                                rec.state,
                                State::Confirmed,
                                "{class_s}@{point}: {result:?}"
                            );
                            assert_eq!(applied, 1);
                        }
                    }
                }
            }
            assert_eq!(
                rec.attempts,
                match (class, point) {
                    (Class::E2, "after_attempted") => 2,
                    (Class::E1, "after_attempted" | "after_send") => 2,
                    (_, "after_intended" | "after_authorized" | "none" | "after_outcome") => 1,
                    (Class::E3 | Class::E4, _) => 1,
                    (Class::E2, "after_send") => 1,
                    _ => rec.attempts,
                },
                "{class_s}@{point}: attempts"
            );
        }
    }
    println!("{}", report.join("\n"));
}
