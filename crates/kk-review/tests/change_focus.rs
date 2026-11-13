use kk_review::job::{ChangeFocus, ChangedFile, JobReport, OUTSIDE_CHANGE};

fn focus() -> ChangeFocus {
    ChangeFocus {
        base: "a".repeat(40),
        head: "b".repeat(40),
        base_name: "main".into(),
        files: vec![
            ChangedFile {
                path: "payments/auth/session.py".into(),
                kind: "modified".into(),
                lines: vec![(15, 18), (30, 30)],
                removed_after: vec![],
            },
            ChangedFile {
                path: "payments/new.py".into(),
                kind: "added".into(),
                lines: vec![(1, 4)],
                removed_after: vec![],
            },
            ChangedFile {
                path: "old.txt".into(),
                kind: "deleted".into(),
                lines: vec![],
                removed_after: vec![],
            },
        ],
        digest: "c".repeat(64),
        left_out: vec![],
    }
}

#[test]
fn cf01_the_agent_is_told_the_change_file_by_file_and_what_counts_as_a_finding_about_it() {
    let t = focus().task();
    assert!(t.contains("the version AFTER the change"), "{t}");
    assert!(
        t.contains("- payments/auth/session.py (modified, lines 15-18, 30)"),
        "{t}"
    );
    assert!(t.contains("- payments/new.py (added, lines 1-4)"), "{t}");
    assert!(t.contains("- old.txt (deleted)"), "{t}");
    assert!(t.contains("A defect in code the change did not touch and that no changed file reaches is not part of this review"), "{t}");
}

#[test]
fn cf02_a_finding_touches_the_change_when_it_cites_a_changed_file_or_its_check_ran_one() {
    let f = focus();
    assert!(f.touches("payments/auth/session.py", &[]));
    // the handler was not changed, but the check that proves the finding runs the changed session code
    assert!(f.touches(
        "payments/api/handlers.py",
        &[
            "payments/api/handlers.py".into(),
            "payments/auth/session.py".into()
        ]
    ));
    // untouched code whose check runs nothing that changed
    assert!(!f.touches(
        "payments/util/backoff.py",
        &["payments/util/backoff.py".into()]
    ));
    // a deleted file is not there to be touched
    assert!(!f.touches("old.txt", &["old.txt".into()]));
    assert!(OUTSIDE_CHANGE.contains("review the whole folder"));
}

#[test]
fn cf03_a_review_that_could_not_begin_says_why_and_offers_nothing() {
    let r = JobReport::not_started("the checked-out commit changes nothing relative to the base");
    assert_eq!(
        r.error.as_deref(),
        Some("the checked-out commit changes nothing relative to the base")
    );
    assert!(
        r.findings.is_empty()
            && r.files.is_empty()
            && r.draft.is_err()
            && r.model_usage.is_empty()
            && r.change.is_none()
    );
}

#[test]
fn cf04_changed_files_the_reviewer_never_saw_are_counted_in_the_release_and_named_in_the_task() {
    let mut f = focus();
    assert!(!f.note(&[]).contains("NOT read"));
    f.left_out = vec![("big.bin".into(), "larger than a file that is read".into())];
    // one changed file was withheld as private (by a list the change itself may have written), one deleted file is not counted
    let note = f.note(&[
        "payments/new.py".to_string(),
        "old.txt".to_string(),
        "unrelated.py".to_string(),
    ]);
    assert!(note.contains("changed files NOT read by the reviewer: 2 (1 could not be read from the repository, 1 withheld as private). This release says nothing about them."), "{note}");
    assert!(f
        .task()
        .contains("1 more changed file(s) could not be read and are not part of this review."));
    // the claim the release makes about the filter is the true one
    assert!(
        note.contains("That is a filter, not a proof of cause")
            && !note.contains("is not covered by this release")
    );
}
