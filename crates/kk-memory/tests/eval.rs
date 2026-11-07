//! Small labelled evaluation sets for the parts of memory that are about being right, not about being fast: which notes apply at a
//! given moment, what a correction leaves behind, and what is still findable after a note is forgotten. Each suite is a table of
//! cases with the expected answer written next to them, so a change that breaks one names the case.

use kk_memory::{Memories, NewMemory, Remembered, Scope, Source};
use kk_types::label::{Label, Level};
use kk_vault::{Vault, VaultKey};
use std::sync::atomic::{AtomicU64, Ordering};

static N: AtomicU64 = AtomicU64::new(0);

fn mem() -> Memories {
    let d = std::env::temp_dir().join(format!(
        "kk-eval-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    Memories::open(
        Vault::create(&d.join("m.db"), &VaultKey::from_bytes([7; 32])).unwrap(),
        [9; 32],
    )
    .unwrap()
}

fn note(text: &str, from: Option<i64>, to: Option<i64>) -> NewMemory {
    NewMemory {
        text: text.into(),
        label: Label::new(Level::Internal),
        subject: None,
        project: None,
        reason: "evaluation".into(),
        source: Source::User,
        valid_from: from,
        valid_to: to,
    }
}

fn id(r: Remembered) -> String {
    match r {
        Remembered::Added(i) | Remembered::AlreadyKnown(i) => i,
    }
}

fn internal() -> Label {
    Label::new(Level::Internal)
}

fn texts(m: &Memories, q: &str, at: i64) -> Vec<String> {
    let mut t: Vec<String> = m
        .recall(q, &internal(), at, 10)
        .unwrap()
        .into_iter()
        .map(|r| r.memory.text)
        .collect();
    t.sort();
    t
}

/// Which notes apply at a moment: a note with a start applies from it, a note with an end stops applying at it (the end is exclusive,
/// the start inclusive), and a note with neither always applies.
#[test]
fn ev01_as_of_correctness() {
    let m = mem();
    id(m.remember(
        1,
        note("Price list A: seat costs five euros", None, Some(100)),
    )
    .unwrap());
    id(m.remember(
        1,
        note("Price list B: seat costs six euros", Some(100), Some(200)),
    )
    .unwrap());
    id(m.remember(
        1,
        note("Price list C: seat costs seven euros", Some(200), None),
    )
    .unwrap());
    id(m.remember(
        1,
        note("Price rule: seat prices are quoted in euros", None, None),
    )
    .unwrap());
    // (moment, expected notes among those that mention a seat)
    let cases: [(i64, &[&str]); 8] = [
        (
            50,
            &[
                "Price list A: seat costs five euros",
                "Price rule: seat prices are quoted in euros",
            ],
        ),
        (
            99,
            &[
                "Price list A: seat costs five euros",
                "Price rule: seat prices are quoted in euros",
            ],
        ),
        (
            100,
            &[
                "Price list B: seat costs six euros",
                "Price rule: seat prices are quoted in euros",
            ],
        ),
        (
            150,
            &[
                "Price list B: seat costs six euros",
                "Price rule: seat prices are quoted in euros",
            ],
        ),
        (
            199,
            &[
                "Price list B: seat costs six euros",
                "Price rule: seat prices are quoted in euros",
            ],
        ),
        (
            200,
            &[
                "Price list C: seat costs seven euros",
                "Price rule: seat prices are quoted in euros",
            ],
        ),
        (
            9_000,
            &[
                "Price list C: seat costs seven euros",
                "Price rule: seat prices are quoted in euros",
            ],
        ),
        (
            0,
            &[
                "Price list A: seat costs five euros",
                "Price rule: seat prices are quoted in euros",
            ],
        ),
    ];
    for (at, want) in cases {
        let mut want: Vec<String> = want.iter().map(|s| s.to_string()).collect();
        want.sort();
        assert_eq!(texts(&m, "seat price euros", at), want, "as of {at}");
    }
    // a note that has not started and one that has ended are not listed either
    let listed = |at| m.list(&internal(), at, 20).unwrap().len();
    assert_eq!((listed(50), listed(150), listed(250)), (2, 2, 2));
}

/// A correction replaces what is recalled and listed, and keeps the old text in the note's history and nowhere else.
#[test]
fn ev02_corrections_replace_without_leaking_the_old_text() {
    let m = mem();
    let old = id(m
        .remember(10, note("The review meeting is on Tuesday", None, None))
        .unwrap());
    let new = m
        .correct(
            20,
            &old,
            note("The review meeting is on Thursday", None, None),
        )
        .unwrap();
    assert_eq!(
        texts(&m, "review meeting", 30),
        ["The review meeting is on Thursday"]
    );
    assert!(m
        .list(&internal(), 30, 10)
        .unwrap()
        .iter()
        .all(|r| !r.memory.text.contains("Tuesday")));
    assert!(m
        .similar_notes(&internal(), 30, Scope::Global)
        .unwrap()
        .groups
        .iter()
        .all(|g| g.iter().all(|r| !r.memory.text.contains("Tuesday"))));
    let hist: Vec<String> = m
        .history(&new, &internal())
        .unwrap()
        .into_iter()
        .map(|x| x.text)
        .collect();
    assert!(
        hist.iter().any(|t| t.contains("Tuesday")) && hist.iter().any(|t| t.contains("Thursday")),
        "the history keeps both: {hist:?}"
    );
    // correcting twice leaves exactly one live note
    let newer = m
        .correct(
            40,
            &new,
            note("The review meeting is on Friday", None, None),
        )
        .unwrap();
    assert_eq!(
        texts(&m, "review meeting", 50),
        ["The review meeting is on Friday"]
    );
    assert_eq!(m.list(&internal(), 50, 10).unwrap().len(), 1);
    assert!(m.history(&newer, &internal()).unwrap().len() >= 3);
}

/// After a note is forgotten, none of the ways of finding a note finds it: not by its words, not by words it shares with a
/// neighbour, not by meaning, not in a listing, a similarity scan, a conflict scan or the expiring list.
#[test]
fn ev03_forget_completeness() {
    let m = mem();
    let secret_word = "zephyrquartz";
    let keep = id(m
        .remember(
            1,
            note("The ledger closes on the last day of the month", None, None),
        )
        .unwrap());
    let gone = id(m
        .remember(
            1,
            note(
                &format!("The ledger password reminder code is {secret_word} for audit"),
                None,
                Some(5_000),
            ),
        )
        .unwrap());
    let twin = id(m
        .remember(
            1,
            note(
                &format!("The ledger closes on the last day of the month {secret_word}"),
                None,
                None,
            ),
        )
        .unwrap());
    assert_eq!(
        m.recall(secret_word, &internal(), 2, 10).unwrap().len(),
        2,
        "before: both notes with the word are found"
    );
    m.forget(3, &gone, "no longer true").unwrap();
    // the other notes are untouched
    let live: Vec<String> = m
        .list(&internal(), 4, 10)
        .unwrap()
        .into_iter()
        .map(|r| r.memory.id)
        .collect();
    assert!(
        live.contains(&keep) && live.contains(&twin) && !live.contains(&gone),
        "{live:?}"
    );
    let probes = [
        secret_word,
        "password reminder code audit",
        "ledger password",
        "reminder code",
        "closes month ledger",
    ];
    for q in probes {
        for r in m.recall(q, &internal(), 4, 10).unwrap() {
            assert_ne!(r.memory.id, gone, "found by `{q}` after being forgotten");
            assert!(
                !r.memory.text.contains("password reminder"),
                "`{q}` returned the forgotten text"
            );
        }
    }
    assert!(
        m.expiring(&internal(), 4, 10_000, 10, Scope::Global)
            .unwrap()
            .is_empty(),
        "its end date is not listed"
    );
    let simil = m.similar_notes(&internal(), 4, Scope::Global).unwrap();
    assert!(simil
        .groups
        .iter()
        .all(|g| g.iter().all(|r| r.memory.id != gone)));
    let conf = m.conflicts(&internal(), 4, Scope::Global).unwrap();
    assert!(!format!("{conf:?}").contains("password reminder"));
    // forgetting it again, or something that never existed, says only that there is no such note, and its history is empty
    assert_eq!(
        m.forget(5, &gone, "again").unwrap_err().to_string(),
        "no such memory"
    );
    assert_eq!(
        m.forget(5, "no-such-note", "x").unwrap_err().to_string(),
        "no such memory"
    );
    assert!(
        m.history(&gone, &internal()).unwrap().is_empty(),
        "the history of a forgotten note is gone with it"
    );
    assert_eq!(
        m.list(&internal(), 6, 10).unwrap().len(),
        2,
        "and nothing else changed"
    );
}
