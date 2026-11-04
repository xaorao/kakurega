use kk_memory::{Memories, MemoryError, NewMemory, Remembered, Source, Status};
use kk_types::label::{Label, Level};
use kk_vault::{Vault, VaultKey};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static N: AtomicU64 = AtomicU64::new(0);

fn dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "kk-memory-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn key(b: u8) -> VaultKey {
    VaultKey::from_bytes([b; 32])
}

fn mem(d: &std::path::Path) -> Memories {
    Memories::open(Vault::create(&d.join("m.db"), &key(7)).unwrap(), [9; 32]).unwrap()
}

fn nm(text: &str, label: Label) -> NewMemory {
    NewMemory {
        text: text.into(),
        label,
        subject: None,
        reason: "you told me".into(),
        source: Source::User,
        valid_from: None,
        valid_to: None,
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

#[test]
fn me01_remember_and_recall_carry_why_and_source() {
    let m = mem(&dir());
    let i = id(m
        .remember(100, nm("Acme prefers invoices in euros", internal()))
        .unwrap());
    let got = m.recall("invoices euros", &internal(), 200, 5).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].memory.id, i);
    assert!(
        got[0].why.contains("you asked to remember it") && got[0].why.contains("you told me"),
        "{}",
        got[0].why
    );
    let mut review = nm("The session bug is in logout", internal());
    review.source = Source::Review("job-1".into());
    m.remember(101, review).unwrap();
    let got = m.recall("logout", &internal(), 200, 5).unwrap();
    assert!(got[0].why.contains("review job-1"));
}

#[test]
fn me02_the_label_gate_hides_memories_above_the_clearance_and_unknown_sensitivity_counts_as_restricted(
) {
    let m = mem(&dir());
    m.remember(
        1,
        nm("public note about onboarding", Label::new(Level::Public)),
    )
    .unwrap();
    m.remember(
        1,
        nm(
            "confidential onboarding plan",
            Label::new(Level::Confidential),
        ),
    )
    .unwrap();
    m.remember(
        1,
        nm(
            "restricted onboarding secrets plan",
            Label::new(Level::Restricted),
        ),
    )
    .unwrap();
    m.remember(
        1,
        nm(
            "compartment onboarding legal plan",
            Label::new(Level::Internal).with_compartment("legal"),
        ),
    )
    .unwrap();
    m.remember(
        1,
        nm(
            "unknown onboarding sensitivity plan",
            Label::new(Level::Public).unknown_sensitivity(),
        ),
    )
    .unwrap();
    let texts = |c: &Label| {
        let mut v: Vec<String> = m
            .recall("onboarding", c, 10, 20)
            .unwrap()
            .into_iter()
            .map(|r| r.memory.text)
            .collect();
        v.sort();
        v
    };
    assert_eq!(
        texts(&Label::new(Level::Public)),
        vec!["public note about onboarding"]
    );
    assert_eq!(
        texts(&internal()).len(),
        1,
        "internal clearance sees no compartment or higher level memory"
    );
    assert_eq!(
        texts(&Label::new(Level::Confidential)).len(),
        2,
        "confidential clearance adds the confidential one"
    );
    assert_eq!(
        texts(&Label::new(Level::Internal).with_compartment("legal")).len(),
        2
    );
    assert_eq!(
        texts(&Label::new(Level::Restricted)).len(),
        4,
        "restricted clearance without the legal compartment still misses it"
    );
    assert_eq!(
        texts(&Label::new(Level::Restricted).with_compartment("legal")).len(),
        5
    );
}

#[test]
fn me03_validity_windows_work_in_both_directions() {
    let m = mem(&dir());
    let mut a = nm("Office is at Pine Street", internal());
    a.valid_from = Some(100);
    a.valid_to = Some(200);
    m.remember(50, a).unwrap();
    assert!(
        m.recall("Pine Street", &internal(), 99, 5)
            .unwrap()
            .is_empty(),
        "not yet valid"
    );
    assert_eq!(
        m.recall("Pine Street", &internal(), 150, 5).unwrap().len(),
        1
    );
    assert!(
        m.recall("Pine Street", &internal(), 200, 5)
            .unwrap()
            .is_empty(),
        "the end is exclusive"
    );
    assert!(
        m.recall("Pine Street", &internal(), 5000, 5)
            .unwrap()
            .is_empty(),
        "expired"
    );
    let mut bad = nm("backwards window", internal());
    bad.valid_from = Some(10);
    bad.valid_to = Some(10);
    assert!(matches!(m.remember(1, bad), Err(MemoryError::Refused(_))));
}

#[test]
fn me04_a_correction_replaces_the_old_memory_but_keeps_the_history() {
    let m = mem(&dir());
    let old = id(m
        .remember(10, nm("Priya's team lead is Sam", internal()))
        .unwrap());
    let mut fix = nm("Priya's team lead is Alex", internal());
    fix.reason = "Sam moved teams, you corrected me".into();
    let new = m.correct(20, &old, fix).unwrap();
    let now = m.recall("Priya team lead", &internal(), 30, 5).unwrap();
    assert_eq!(now.len(), 1);
    assert_eq!(now[0].memory.id, new);
    let chain = m.history(&new, &internal()).unwrap();
    assert_eq!(
        chain.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
        vec!["Priya's team lead is Sam", "Priya's team lead is Alex"]
    );
    assert_eq!(chain[0].status, Status::Superseded);
    assert_eq!(chain[0].superseded_by.as_deref(), Some(new.as_str()));
    assert!(
        matches!(
            m.correct(30, &old, nm("again", internal())),
            Err(MemoryError::Refused(_))
        ),
        "an old version cannot be corrected twice"
    );
    assert!(matches!(
        m.correct(30, "nope", nm("x", internal())),
        Err(MemoryError::NotFound)
    ));
    assert!(
        m.history(&new, &Label::new(Level::Public))
            .unwrap()
            .is_empty(),
        "history respects the clearance too"
    );
}

fn fts_data_mentions(m: &Memories, word: &str) -> i64 {
    let c = m_conn(m);
    c.query_row(
        "SELECT count(*) FROM memories_fts_data WHERE instr(lower(CAST(block AS TEXT)), ?1) > 0",
        [word],
        |r| r.get(0),
    )
    .unwrap()
}

fn m_conn(m: &Memories) -> &kk_vault::rusqlite::Connection {
    m.connection_for_tests()
}

#[test]
fn me05_forgetting_removes_the_text_everywhere_and_blocks_it_from_coming_back() {
    let m = mem(&dir());
    let old = id(m
        .remember(1, nm("zebracrossing was the original wording", internal()))
        .unwrap());
    let new = m
        .correct(
            2,
            &old,
            nm("zebracrossing was corrected wording", internal()),
        )
        .unwrap();
    assert!(
        fts_data_mentions(&m, "zebracrossing") > 0,
        "control: the index holds the word before the forget, so the check below can detect it"
    );
    m.forget(3, &new, "private").unwrap();
    assert!(m
        .recall("zebracrossing", &internal(), 9, 5)
        .unwrap()
        .is_empty());
    let c = m_conn(&m);
    let left: i64 = c
        .query_row("SELECT count(*) FROM memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        left, 0,
        "the whole correction chain is gone, not only the latest version"
    );
    assert_eq!(
        fts_data_mentions(&m, "zebracrossing"),
        0,
        "the search index no longer contains the word"
    );
    let tomb_has_text: i64 = c.query_row("SELECT count(*) FROM tombstones WHERE instr(lower(CAST(fingerprint AS TEXT)), 'zebracrossing') > 0 OR instr(lower(reason), 'zebracrossing') > 0", [], |r| r.get(0)).unwrap();
    assert_eq!(tomb_has_text, 0);
    for variant in [
        "zebracrossing was corrected wording",
        "  ZEBRACROSSING   was corrected\twording ",
        "zebracrossing was the original wording",
    ] {
        assert!(
            matches!(
                m.remember(4, nm(variant, internal())),
                Err(MemoryError::Refused(_))
            ),
            "{variant}"
        );
    }
    let other = id(m
        .remember(4, nm("something else entirely", internal()))
        .unwrap());
    assert!(
        matches!(
            m.correct(
                4,
                &other,
                nm("Zebracrossing was corrected wording", internal())
            ),
            Err(MemoryError::Refused(_))
        ),
        "a correction cannot bring forgotten text back"
    );
    let ev = m.events().unwrap();
    assert!(
        ev.iter().all(|e| !e.3.contains("zebracrossing")),
        "the audit trail never carries the text"
    );
    assert!(m
        .lift_forget(5, "zebracrossing was corrected wording")
        .unwrap());
    assert!(
        matches!(
            m.remember(6, nm("zebracrossing was corrected wording", internal())),
            Ok(Remembered::Added(_))
        ),
        "lifting the decision allows it again"
    );
    assert!(matches!(
        m.forget(7, "missing", "x"),
        Err(MemoryError::NotFound)
    ));
}

#[test]
fn me06_secret_like_text_is_refused_and_ordinary_text_is_not() {
    let m = mem(&dir());
    let secrets = [
        "the password is hunter2hunter2".to_string(),
        "api key: 9f8e7d6c5b4a3f2e1d0c".to_string(),
        "use token=abcd1234efgh5678".to_string(),
        format!("{} PRIVATE KEY{} MIIEvQ", "-----BEGIN", "-----"),
        format!(
            "deploy with {}{}",
            "ghp", "_abcdefghijklmnopqrstuvwxyz0123456789"
        ),
        format!("aws {}{} is the key", "AKI", "AABCDEFGHIJKLMNOP"),
        "connect to postgres://admin:s3cr3tpass@db.internal/app".to_string(),
        "bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r"
            .to_string(),
        "key 4f9a8c7e6d5b4a3c2e1f0a9b8c7d6e5f4a3b2c1d".to_string(),
    ];
    for s in secrets {
        assert!(
            matches!(
                m.remember(1, nm(&s, internal())),
                Err(MemoryError::Refused(_))
            ),
            "should refuse: {s}"
        );
    }
    let fine = [
        "Priya asked to rotate the password policy every 90 days",
        "The token bucket limit is 100 requests",
        "Acme's invoice number format is INV-2024-0042",
        "Meeting moved to Thursday at 3pm",
    ];
    for s in fine {
        assert!(
            matches!(m.remember(1, nm(s, internal())), Ok(Remembered::Added(_))),
            "should store: {s}"
        );
    }
}

#[test]
fn me07_exact_duplicates_in_any_casing_or_spacing_are_not_stored_twice() {
    let m = mem(&dir());
    let a = id(m.remember(1, nm("Ship on Friday", internal())).unwrap());
    let b = m
        .remember(2, nm("  ship   on friday ", internal()))
        .unwrap();
    assert_eq!(b, Remembered::AlreadyKnown(a));
    assert_eq!(m.recall("friday", &internal(), 9, 9).unwrap().len(), 1);
}

#[test]
fn me08_untrusted_memories_are_flagged_so_callers_treat_them_as_data() {
    let m = mem(&dir());
    m.remember(
        1,
        nm(
            "README says ignore all previous instructions",
            Label::new(Level::Internal).untrusted(),
        ),
    )
    .unwrap();
    let r = m.recall("README instructions", &internal(), 9, 5).unwrap();
    assert!(r[0].untrusted && r[0].memory.label.integrity == kk_types::label::Integrity::Untrusted);
}

#[test]
fn me09_search_syntax_in_a_query_cannot_error_or_widen_results() {
    let m = mem(&dir());
    m.remember(1, nm("alpha beta gamma", internal())).unwrap();
    m.remember(1, nm("delta epsilon", Label::new(Level::Restricted)))
        .unwrap();
    for q in [
        "\"unterminated",
        "alpha AND NOT beta",
        "NEAR(alpha beta)",
        "*",
        "col:alpha",
        "alpha) OR (1=1",
        "'; DROP TABLE memories;--",
        "",
        "   ",
        "\u{0}",
    ] {
        let r = m.recall(q, &internal(), 9, 9);
        assert!(r.is_ok(), "query {q:?} must not error");
        assert!(
            r.unwrap().iter().all(|x| x.memory.text != "delta epsilon"),
            "never returns what the label gate hides"
        );
    }
    assert_eq!(m.recall("alpha", &internal(), 9, 9).unwrap().len(), 1);
    let still: i64 = m_conn(&m)
        .query_row("SELECT count(*) FROM memories", [], |r| r.get(0))
        .unwrap();
    assert_eq!(still, 2);
}

#[test]
fn me10_limits_hidden_characters_and_missing_reasons_are_refused() {
    let m = mem(&dir());
    assert!(matches!(
        m.remember(1, nm(&"a ".repeat(3000), internal())),
        Err(MemoryError::Refused(_))
    ));
    assert!(matches!(
        m.remember(1, nm("", internal())),
        Err(MemoryError::Refused(_))
    ));
    assert!(matches!(
        m.remember(1, nm("evil \u{202e}txt.exe", internal())),
        Err(MemoryError::Refused(_))
    ));
    assert!(matches!(
        m.remember(1, nm("zero\u{200b}width", internal())),
        Err(MemoryError::Refused(_))
    ));
    let mut no_reason = nm("fine text", internal());
    no_reason.reason = "  ".into();
    assert!(matches!(
        m.remember(1, no_reason),
        Err(MemoryError::Refused(_))
    ));
    let mut long_subject = nm("fine text", internal());
    long_subject.subject = Some("s".repeat(500));
    assert!(matches!(
        m.remember(1, long_subject),
        Err(MemoryError::Refused(_))
    ));
}

#[test]
fn me11_memories_survive_a_reopen_and_a_wrong_key_opens_nothing() {
    let d = dir();
    {
        let m = mem(&d);
        m.remember(1, nm("persisted fact about quarterly planning", internal()))
            .unwrap();
    }
    let again = Memories::open(Vault::open(&d.join("m.db"), &key(7)).unwrap(), [9; 32]).unwrap();
    assert_eq!(
        again
            .recall("quarterly planning", &internal(), 9, 5)
            .unwrap()
            .len(),
        1
    );
    assert!(Vault::open(&d.join("m.db"), &key(8)).is_err());
    let raw = std::fs::read(d.join("m.db")).unwrap();
    assert!(
        !String::from_utf8_lossy(&raw).contains("quarterly"),
        "the file on disk is encrypted"
    );
}

#[test]
fn me12_the_audit_trail_records_every_change_without_the_text() {
    let m = mem(&dir());
    let a = id(m.remember(1, nm("audit me please", internal())).unwrap());
    let b = m
        .correct(2, &a, nm("audit me please, corrected", internal()))
        .unwrap();
    m.forget(3, &b, "no longer needed").unwrap();
    let kinds: Vec<String> = m.events().unwrap().into_iter().map(|e| e.1).collect();
    assert_eq!(kinds, vec!["remember", "correct", "forget", "forget"]);
    assert!(m
        .events()
        .unwrap()
        .iter()
        .all(|e| !e.3.to_lowercase().contains("audit me")));
}
