use kk_eventlog::{verify_export, Anchor, Datum, EventLog, Failure, GENESIS};
use kk_vault::{Vault, VaultKey};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static N: AtomicU64 = AtomicU64::new(0);
const SEED: [u8; 32] = [11; 32];

fn path() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "kk-log-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d.join("log.db")
}

fn key() -> VaultKey {
    VaultKey::from_bytes([4; 32])
}

fn log() -> (EventLog, PathBuf) {
    let p = path();
    (
        EventLog::open(Vault::create(&p, &key()).unwrap(), SEED).unwrap(),
        p,
    )
}

fn fill(l: &EventLog, n: i64) {
    for i in 0..n {
        l.append(
            100 + i,
            "job.finished",
            &format!("job-{i}"),
            &[("state", "ready".into()), ("findings", Datum::Int(i))],
        )
        .unwrap();
    }
}

fn edit(p: &std::path::Path, sql: &str) {
    let v = Vault::open(p, &key()).unwrap();
    v.connection().execute_batch(sql).unwrap();
}

fn pk() -> [u8; 32] {
    kk_evidence::keys::Signer::from_seed(SEED).public()
}

#[test]
fn el01_an_untouched_log_verifies_and_reports_its_head_and_signer() {
    let (l, _) = log();
    assert_eq!(
        verify_export(&l.export().unwrap(), None, None)
            .unwrap()
            .count,
        0,
        "an empty log is valid"
    );
    fill(&l, 5);
    let r = verify_export(&l.export().unwrap(), Some(&pk()), None).unwrap();
    assert_eq!(r.count, 5);
    assert!(r.signer_pinned && r.head.len() == 64);
    assert_ne!(r.head, GENESIS);
}

#[test]
fn el02_changing_any_recorded_value_is_detected() {
    let (l, p) = log();
    fill(&l, 4);
    edit(
        &p,
        "UPDATE log SET entry = replace(entry, '\"findings\":2', '\"findings\":9') WHERE seq = 2",
    );
    assert_eq!(
        verify_export(&l.export().unwrap(), None, None),
        Err(Failure::HashMismatch)
    );
}

#[test]
fn el03_removing_an_entry_from_the_middle_or_the_start_is_detected() {
    let (l, p) = log();
    fill(&l, 5);
    edit(&p, "DELETE FROM log WHERE seq = 2");
    assert_eq!(
        verify_export(&l.export().unwrap(), None, None),
        Err(Failure::SeqGap)
    );
    let (l2, p2) = log();
    fill(&l2, 5);
    edit(&p2, "DELETE FROM log WHERE seq = 0");
    assert_eq!(
        verify_export(&l2.export().unwrap(), None, None),
        Err(Failure::SeqGap)
    );
}

#[test]
fn el04_reordering_or_splicing_in_a_recomputed_entry_is_detected() {
    let (l, p) = log();
    fill(&l, 4);
    edit(&p, "UPDATE log SET seq = 100 WHERE seq = 1; UPDATE log SET seq = 1 WHERE seq = 2; UPDATE log SET seq = 2 WHERE seq = 100");
    assert!(matches!(
        verify_export(&l.export().unwrap(), None, None),
        Err(Failure::SeqGap) | Err(Failure::PrevMismatch)
    ));
}

#[test]
fn el05_an_attacker_who_recomputes_hashes_still_cannot_forge_the_signed_head_or_use_their_own_key()
{
    let (l, p) = log();
    fill(&l, 3);
    let export = l.export().unwrap();
    let v = Vault::open(&p, &key()).unwrap();
    let entry: String = v
        .connection()
        .query_row("SELECT entry FROM log WHERE seq = 1", [], |r| r.get(0))
        .unwrap();
    let forged = entry.replace("\"findings\":1", "\"findings\":7");
    let new_hash = kk_evidence::capsule::sha256_hex(forged.as_bytes());
    v.connection()
        .execute(
            "UPDATE log SET entry = ?1, hash = ?2 WHERE seq = 1",
            kk_vault::rusqlite::params![forged, new_hash],
        )
        .unwrap();
    assert!(
        matches!(
            verify_export(&l.export().unwrap(), None, None),
            Err(Failure::PrevMismatch) | Err(Failure::HeadMismatch)
        ),
        "the next entry's link and the signed head both expose it"
    );
    assert!(
        verify_export(&export, None, None).is_ok(),
        "control: the original export is still valid"
    );
    let other = kk_evidence::keys::Signer::from_seed([99; 32]).public();
    assert_eq!(
        verify_export(&export, Some(&other), None),
        Err(Failure::UntrustedSigner),
        "a log signed by another key is not the key you pinned"
    );
}

#[test]
fn el06_an_anchor_catches_a_log_that_was_cut_short_or_replaced() {
    let (l, p) = log();
    fill(&l, 6);
    let before = verify_export(&l.export().unwrap(), None, None).unwrap();
    let anchor = Anchor {
        count: before.count,
        head: before.head.clone(),
    };
    assert!(verify_export(&l.export().unwrap(), None, Some(&anchor)).is_ok());
    fill(&l, 2);
    assert!(
        verify_export(&l.export().unwrap(), None, Some(&anchor)).is_ok(),
        "growth after the anchor is fine"
    );
    edit(&p, "DELETE FROM log WHERE seq >= 4");
    assert_eq!(
        verify_export(&l.export().unwrap(), None, None)
            .unwrap()
            .count,
        4,
        "without an anchor a cut tail is not visible"
    );
    assert_eq!(
        verify_export(&l.export().unwrap(), None, Some(&anchor)),
        Err(Failure::RolledBack)
    );
    let (other, _) = log();
    fill(&other, 6);
    other.append(999, "x.y", "different", &[]).unwrap();
    let different = Anchor {
        count: 6,
        head: "f".repeat(64),
    };
    assert_eq!(
        verify_export(&other.export().unwrap(), None, Some(&different)),
        Err(Failure::AnchorMismatch)
    );
}

#[test]
fn el07_appending_continues_the_chain_after_a_reopen_and_from_many_threads() {
    let (l, p) = log();
    fill(&l, 2);
    drop(l);
    let l = std::sync::Arc::new(EventLog::open(Vault::open(&p, &key()).unwrap(), SEED).unwrap());
    let handles: Vec<_> = (0..4)
        .map(|t| {
            let l = l.clone();
            std::thread::spawn(move || {
                for i in 0..10 {
                    l.append(
                        1000,
                        "thread.event",
                        &format!("t{t}"),
                        &[("i", Datum::Int(i))],
                    )
                    .unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(l.len().unwrap(), 42);
    assert_eq!(
        verify_export(&l.export().unwrap(), None, None)
            .unwrap()
            .count,
        42
    );
}

#[test]
fn el08_kinds_subjects_and_values_are_limited_so_secrets_and_junk_do_not_get_in() {
    let (l, _) = log();
    assert!(l.append(1, "Bad Kind", "s", &[]).is_err());
    assert!(l.append(1, "", "s", &[]).is_err());
    assert!(l.append(1, "ok.kind", &"s".repeat(200), &[]).is_err());
    assert!(l.append(1, "ok.kind", "s\u{202e}", &[]).is_err());
    assert!(l
        .append(1, "ok.kind", "s", &[("Bad Key", "v".into())])
        .is_err());
    assert!(l
        .append(1, "ok.kind", "s", &[("k", Datum::Str("x".repeat(300)))])
        .is_err());
    let many: Vec<(&str, Datum)> = (0..17).map(|_| ("k", Datum::Int(1))).collect();
    assert!(l.append(1, "ok.kind", "s", &many).is_err());
    assert_eq!(l.len().unwrap(), 0, "a refused entry leaves nothing behind");
    assert!(l.append(1, "ok.kind", "s", &[("k", "v".into())]).is_ok());
}

#[test]
fn el09_the_verifier_refuses_malformed_input_without_panicking() {
    // harmless outer whitespace is allowed: every entry inside is canonical and hashed
    let (probe, _) = log();
    fill(&probe, 1);
    assert!(verify_export(&format!(" {} \n", probe.export().unwrap()), None, None).is_ok());
    let (l, _) = log();
    fill(&l, 2);
    let good = l.export().unwrap();
    for bad in [
        "".to_string(),
        "not json".to_string(),
        "[]".to_string(),
        "{}".to_string(),
        good.replace("\"version\":1", "\"version\":2"),
        good.replace("kakurega-event-log", "other"),
        good.replace("\"entries\":[", "\"entries\":[null,"),
        good.replace(",\"format\"", ",\"format\":\"x\",\"format\""),
        good.replace("\"public_key\":\"", "\"public_key\":\"AAAA"),
        good.replacen("\"hash\":\"", "\"hash\":\"0", 1),
    ] {
        assert!(
            verify_export(&bad, None, None).is_err(),
            "should reject: {}",
            &bad[..bad.len().min(60)]
        );
    }
}

fn rebuild(export: &str, edit: impl Fn(usize, String) -> String, resign: bool) -> String {
    use kk_evidence::canon::{self, Value};
    let Value::Object(mut top) = canon::parse_strict(export.as_bytes()).unwrap() else {
        panic!()
    };
    let signer = kk_evidence::keys::Signer::from_seed(SEED);
    let entries = top.iter_mut().find(|(k, _)| k == "entries").unwrap();
    let Value::Array(list) = &mut entries.1 else {
        panic!()
    };
    let mut prev = GENESIS.to_string();
    for (i, item) in list.iter_mut().enumerate() {
        let Value::Object(m) = item else { panic!() };
        let entry_idx = m.iter().position(|(k, _)| k == "entry").unwrap();
        let Value::Str(text) = &m[entry_idx].1 else {
            panic!()
        };
        let mut text = edit(i, text.clone());
        let Value::Object(mut em) = canon::parse_strict(text.as_bytes()).unwrap() else {
            panic!()
        };
        if resign || i > 0 {
            for (k, v) in em.iter_mut() {
                if k == "prev" && !resign_keep_prev(i) {
                    *v = Value::Str(prev.clone());
                }
            }
        }
        text = canon::serialize(&Value::Object(em));
        let hash = kk_evidence::capsule::sha256_hex(text.as_bytes());
        m[entry_idx].1 = Value::Str(text);
        for (k, v) in m.iter_mut() {
            if k == "hash" {
                *v = Value::Str(hash.clone());
            }
            if k == "head" && resign {
                let payload = canon::serialize(&Value::Object(vec![
                    ("count".into(), Value::Int(i as i64 + 1)),
                    ("head".into(), Value::Str(hash.clone())),
                    ("v".into(), Value::Int(1)),
                ]));
                let env =
                    kk_evidence::dsse::sign(kk_eventlog::HEAD_TYPE, payload.as_bytes(), &signer);
                *v = Value::Str(kk_evidence::dsse::encode(&env));
            }
        }
        prev = hash;
    }
    canon::serialize(&Value::Object(top))
}

fn resign_keep_prev(i: usize) -> bool {
    i == 2
}

#[test]
fn el10_rewriting_the_whole_chain_with_correct_hashes_still_fails_because_the_old_signed_heads_no_longer_match(
) {
    let (l, _) = log();
    fill(&l, 4);
    let export = l.export().unwrap();
    let rewritten = rebuild(
        &export,
        |i, t| {
            if i == 1 {
                t.replace("\"findings\":1", "\"findings\":9")
            } else {
                t
            }
        },
        false,
    );
    assert_ne!(rewritten, export);
    assert_eq!(
        verify_export(&rewritten, None, None),
        Err(Failure::HeadMismatch)
    );
}

#[test]
fn el11_even_a_holder_of_the_signing_key_cannot_slip_in_an_entry_with_a_broken_link() {
    let (l, _) = log();
    fill(&l, 4);
    let export = l.export().unwrap();
    let forged = rebuild(
        &export,
        |i, t| {
            if i != 2 {
                return t;
            }
            let at = t.find("\"prev\":\"").unwrap() + 8;
            format!("{}{}{}", &t[..at], "f".repeat(64), &t[at + 64..])
        },
        true,
    );
    assert_eq!(
        verify_export(&forged, None, None),
        Err(Failure::PrevMismatch),
        "entry 2 keeps a stale prev link while everything else is re-signed"
    );
}

#[test]
fn el12_head_and_recent_describe_the_log_without_exporting_it() {
    let (l, _) = log();
    assert_eq!(l.head().unwrap(), None);
    fill(&l, 3);
    let (count, hash) = l.head().unwrap().unwrap();
    assert_eq!(count, 3);
    assert_eq!(
        hash,
        verify_export(&l.export().unwrap(), None, None)
            .unwrap()
            .head
    );
    let recent = l.recent(2).unwrap();
    assert_eq!(recent.len(), 2);
    assert!(
        recent[0].contains("\"seq\":2") && recent[1].contains("\"seq\":1"),
        "newest first"
    );
}
