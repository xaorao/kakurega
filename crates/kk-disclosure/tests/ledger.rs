use kk_disclosure::{canonical_recipient, Ledger, Plan, Policy, Verdict};
use kk_vault::{Vault, VaultKey};
use std::sync::atomic::{AtomicU64, Ordering};

static N: AtomicU64 = AtomicU64::new(0);

fn open(p: Policy) -> (Ledger, std::path::PathBuf) {
    let d = std::env::temp_dir().join(format!(
        "kk-disc-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&d).unwrap();
    let path = d.join("d.db");
    (
        Ledger::open(
            Vault::create(&path, &VaultKey::from_bytes([3; 32])).unwrap(),
            p,
        )
        .unwrap(),
        path,
    )
}

fn plan(who: &str, files: &[&str], total: usize) -> Plan {
    Plan {
        recipient: who.into(),
        source_paths: files.iter().map(|s| s.to_string()).collect(),
        project_files: total,
        project: "payments".into(),
    }
}

#[test]
fn dc01_recipient_spellings_that_reduce_to_the_same_name_are_one_recipient() {
    assert_eq!(
        canonical_recipient("Acme-Audit"),
        canonical_recipient("acme audit")
    );
    assert_eq!(canonical_recipient("ACME_audit "), "acmeaudit");
    assert_ne!(
        canonical_recipient("acme-audit"),
        canonical_recipient("acme-audits")
    );
    let (l, _) = open(Policy::default());
    l.record_delivered(1, &plan("Acme-Audit", &["a.py", "b.py", "c.py"], 10), "d1")
        .unwrap();
    let a = l.assess(2, &plan("acme_audit", &["d.py"], 10)).unwrap();
    assert_eq!(
        a.files_after, 4,
        "a different spelling is linked to the same ledger"
    );
}

#[test]
fn dc02_coverage_escalates_then_blocks_as_releases_add_up() {
    let (l, _) = open(Policy::default());
    assert_eq!(
        l.assess(1, &plan("acme", &["a", "b"], 10)).unwrap().verdict,
        Verdict::Allow
    );
    l.record_delivered(1, &plan("acme", &["a", "b"], 10), "d1")
        .unwrap();
    let second = l.assess(2, &plan("acme", &["c", "d"], 10)).unwrap();
    assert_eq!(second.verdict, Verdict::Escalate, "{:?}", second.reasons);
    assert!(second.reasons[0].contains("4 of this project's 10 files"));
    l.record_delivered(2, &plan("acme", &["c", "d"], 10), "d2")
        .unwrap();
    let third = l.assess(3, &plan("acme", &["e", "f", "g"], 10)).unwrap();
    assert_eq!(third.verdict, Verdict::Block, "{:?}", third.reasons);
    let other = l
        .assess(3, &plan("other-recipient", &["e", "f", "g"], 10))
        .unwrap();
    assert_eq!(
        (other.files_after, other.verdict),
        (3, Verdict::Allow),
        "another recipient has its own account"
    );
}

#[test]
fn dc03_showing_the_same_files_again_does_not_increase_coverage() {
    let (l, _) = open(Policy::default());
    l.record_delivered(1, &plan("acme", &["a", "b"], 10), "d1")
        .unwrap();
    let again = l.assess(2, &plan("acme", &["a", "b"], 10)).unwrap();
    assert_eq!(again.files_after, 2);
    assert_eq!(again.verdict, Verdict::Allow);
}

#[test]
fn dc04_a_release_counts_only_once_delivered_and_only_once() {
    let (l, _) = open(Policy::default());
    let p = plan("acme", &["a", "b", "c", "d"], 10);
    assert_eq!(
        l.assess(1, &p).unwrap().releases_before,
        0,
        "assessing records nothing"
    );
    l.record_delivered(1, &p, "dg").unwrap();
    l.record_delivered(2, &p, "dg").unwrap();
    assert_eq!(
        l.history("acme").unwrap().len(),
        1,
        "a repeated delivery record is ignored"
    );
    assert_eq!(
        l.history("acme").unwrap()[0].2,
        4,
        "and its files are not counted twice"
    );
    assert_eq!(
        l.assess(3, &plan("acme", &["a"], 10))
            .unwrap()
            .releases_before,
        1
    );
}

#[test]
fn dc05_too_many_releases_in_a_day_are_blocked_but_not_after_the_day() {
    let (l, _) = open(Policy::default());
    for i in 0..5 {
        l.record_delivered(100 + i, &plan("acme", &["a"], 100), &format!("d{i}"))
            .unwrap();
    }
    let a = l.assess(200, &plan("acme", &["a"], 100)).unwrap();
    assert_eq!(a.verdict, Verdict::Block);
    assert!(a.reasons.iter().any(|r| r.contains("last 24 hours")));
    assert_eq!(
        l.assess(100 + 86_401 + 5, &plan("acme", &["a"], 100))
            .unwrap()
            .verdict,
        Verdict::Allow
    );
}

#[test]
fn dc06_every_assessment_says_what_was_checked_and_what_was_not() {
    let (l, _) = open(Policy::default());
    let a = l.assess(1, &plan("acme", &["a"], 10)).unwrap();
    assert!(a.checked.len() >= 2 && a.not_checked.len() >= 3);
    assert!(a
        .not_checked
        .iter()
        .any(|n| n.contains("combined excerpts")));
}

#[test]
fn dc07_the_ledger_survives_a_reopen_and_tiny_projects_are_not_judged_by_percentage() {
    let (l, path) = open(Policy::default());
    l.record_delivered(1, &plan("acme", &["a", "b"], 10), "d1")
        .unwrap();
    drop(l);
    let l = Ledger::open(
        Vault::open(&path, &VaultKey::from_bytes([3; 32])).unwrap(),
        Policy::default(),
    )
    .unwrap();
    assert_eq!(
        l.assess(2, &plan("acme", &["c", "d"], 10)).unwrap().verdict,
        Verdict::Escalate
    );
    let tiny = l.assess(2, &plan("acme", &["x", "y"], 2)).unwrap();
    assert_eq!(
        tiny.verdict,
        Verdict::Allow,
        "below the minimum project size a percentage means nothing"
    );
}
