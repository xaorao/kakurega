use kk_review::check::{
    apply_all, apply_fix, clean_text, compare_test_runs, is_test_path, js_closure, missing_package,
    parse_finding, parse_fix, parse_unittest, python_closure, run_project_tests, run_reproducer,
    unified_diff, unified_diff_all, verify_fix, verify_fix_with_tests, CheckEnv, CheckStatus,
    ClosureError, Edit, Finding, Fix, FixStatus, Language, TestRun, TestVerdict,
};
use kk_review::import::{scan_tree, ImportedFile};
use kk_sandbox::Transport;
use kk_types::label::Level;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn fixture_files() -> Vec<ImportedFile> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dst = std::env::temp_dir().join(format!(
        "kk-check-fixture-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dst);
    fn copy(src: &Path, dst: &Path) {
        fs::create_dir_all(dst).unwrap();
        for e in fs::read_dir(src).unwrap().flatten() {
            let n = e.file_name();
            let t = if n == "_env" {
                dst.join(".env")
            } else {
                dst.join(&n)
            };
            if e.path().is_dir() {
                copy(&e.path(), &t);
            } else {
                fs::copy(e.path(), t).unwrap();
            }
        }
    }
    copy(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic-repo"),
        &dst,
    );
    scan_tree(&dst, Level::Internal).files
}

fn index(files: &[ImportedFile]) -> BTreeMap<String, &ImportedFile> {
    files.iter().map(|f| (f.path.clone(), f)).collect()
}

const REPRO: &str = "from payments.auth import session\nt = session.login('alice')\nassert session.validate(t)\nprint('CONTROL_OK')\nsession.logout(t)\nprint('REPRODUCED' if session.validate(t) else 'NOT REPRODUCED')\n";

fn finding(reproducer: &str) -> Finding {
    Finding {
        title: "Token not revoked".into(),
        file: "payments/auth/session.py".into(),
        line: 15,
        explanation: "logout leaves the token valid".into(),
        reproducer: reproducer.into(),
        language: Language::Python,
    }
}

#[test]
fn c01_findings_are_parsed_strictly_and_cleaned() {
    let known: BTreeSet<String> = ["payments/auth/session.py".to_string()].into();
    let ok = serde_json::json!({"title": "T\u{202e}itle\u{0007}", "file": "payments/auth/session.py", "line": 15, "explanation": "why", "reproducer": "print(1)"});
    let f = parse_finding(ok.to_string().as_bytes(), &known).expect("valid");
    assert_eq!(
        f.title, "Title",
        "bidirectional marks and control characters are removed"
    );
    for bad in [
        serde_json::json!({"title": "t", "file": "elsewhere.py", "line": 1, "explanation": "e"}),
        serde_json::json!({"title": "t", "file": "payments/auth/session.py", "line": 0, "explanation": "e"}),
        serde_json::json!({"title": "t", "file": "payments/auth/session.py", "line": "15", "explanation": "e"}),
        serde_json::json!({"title": "t", "file": "payments/auth/session.py", "line": 1, "explanation": "e", "extra": 1}),
        serde_json::json!({"file": "payments/auth/session.py", "line": 1, "explanation": "e"}),
        serde_json::json!([1, 2]),
    ] {
        assert!(
            parse_finding(bad.to_string().as_bytes(), &known).is_none(),
            "{bad}"
        );
    }
    assert!(parse_finding(b"not json", &known).is_none());
    assert_eq!(clean_text(&"x".repeat(500), 10).len(), 10);
}

#[test]
fn c02_the_minimal_sample_follows_imports_and_never_includes_withheld_files() {
    let files = fixture_files();
    let idx = index(&files);
    let closure = python_closure("payments/auth/session.py", &idx).unwrap();
    assert_eq!(
        closure,
        vec![
            "payments/__init__.py",
            "payments/auth/__init__.py",
            "payments/auth/session.py",
            "payments/auth/tokens.py"
        ]
    );
    assert!(!closure
        .iter()
        .any(|p| p.contains("env") || p.contains("config")));
    assert_eq!(
        python_closure("nope.py", &idx),
        Err(ClosureError::EntryMissing)
    );
    let handlers = python_closure("payments/api/handlers.py", &idx).unwrap();
    assert!(handlers.contains(&"payments/auth/session.py".to_string()));
    let mut with_secret: Vec<ImportedFile> = files.clone();
    for f in &mut with_secret {
        if f.path == "payments/auth/tokens.py" {
            f.label = kk_types::label::Label::new(Level::Restricted).with_compartment("secrets");
        }
    }
    let idx2 = index(&with_secret);
    assert_eq!(
        python_closure("payments/auth/session.py", &idx2),
        Err(ClosureError::NeedsRestrictedFile(
            "payments/auth/tokens.py".into()
        ))
    );
}

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
        std::env::temp_dir().join("kk-check-scratch")
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/check-tmp")
    };
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn c03_the_kernel_reproduces_the_real_defect_and_does_not_confirm_a_false_one() {
    let files = fixture_files();
    let idx = index(&files);
    let t = transport();
    let env = CheckEnv {
        transport: &t,
        scratch: &scratch(),
        python: "/usr/bin/python3",
        node: "/usr/bin/node",
    };
    let real = run_reproducer(&finding(REPRO), &idx, &env);
    assert_eq!(real.status, CheckStatus::Reproduced, "{real:?}");
    assert!(real.stdout.contains("REPRODUCED"));
    let decoy_repro = "from payments.util import backoff\nprint('REPRODUCED' if backoff.retry_delay(1) < 0 else 'NOT REPRODUCED')\n";
    let mut decoy = finding(decoy_repro);
    decoy.file = "payments/util/backoff.py".into();
    let r = run_reproducer(&decoy, &idx, &env);
    assert_eq!(r.status, CheckStatus::NotReproduced, "{r:?}");
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn c04_hostile_or_broken_reproducers_never_become_confirmations() {
    let files = fixture_files();
    let idx = index(&files);
    let t = transport();
    let env = CheckEnv {
        transport: &t,
        scratch: &scratch(),
        python: "/usr/bin/python3",
        node: "/usr/bin/node",
    };
    let cases: Vec<(&str, &str)> = vec![
        ("print('REPRODUCED')\nimport sys; sys.exit(3)\n", "ended"),
        ("import time\nprint('REPRODUCED %s' % time.time())\n", "nondeterministic"),
        ("import socket\ns = socket.create_connection(('93.184.216.34', 80), timeout=1)\nprint('REPRODUCED')\n", "ended"),
        ("print(open('/etc/hostname').read())\nprint('REPRODUCED')\n", "ended"),
        ("raise SystemExit('boom')\n", "ended"),
        ("", "unavailable"),
    ];
    for (code, expect) in cases {
        let r = run_reproducer(&finding(code), &idx, &env);
        assert_ne!(r.status, CheckStatus::Reproduced, "{code}: {r:?}");
        match expect {
            "ended" => assert!(matches!(r.status, CheckStatus::Failed(_)), "{code}: {r:?}"),
            "nondeterministic" => {
                assert_eq!(r.status, CheckStatus::Nondeterministic, "{code}: {r:?}")
            }
            _ => assert!(
                matches!(r.status, CheckStatus::Unavailable(_)),
                "{code}: {r:?}"
            ),
        }
    }
}

fn fix(old: &str, new: &str) -> Fix {
    Fix {
        title: "Token not revoked".into(),
        file: "payments/auth/session.py".into(),
        old_text: old.into(),
        new_text: new.into(),
        also: vec![],
        explanation: String::new(),
    }
}

const GOOD_OLD: &str = "    # Clears the browser cookie on the client side; the server keeps the entry.\n    cookie_jar = {\"session\": token}\n    cookie_jar.pop(\"session\", None)\n";
const GOOD_NEW: &str = "    ACTIVE_TOKENS.pop(token, None)\n";

#[test]
fn f01_fixes_are_parsed_strictly_and_applied_only_as_one_exact_replacement() {
    let known: BTreeSet<String> = ["a.py".to_string()].into();
    let ok =
        br#"{"title":"t","file":"a.py","old_text":"x = 1","new_text":"x = 2","explanation":"e"}"#;
    assert!(parse_fix(ok, &known).is_some());
    for bad in [
        br#"{"title":"t","file":"b.py","old_text":"x","new_text":"y"}"#.as_slice(),
        br#"{"title":"t","file":"a.py","old_text":"","new_text":"y"}"#,
        br#"{"title":"t","file":"a.py","old_text":"x","new_text":"x"}"#,
        br#"{"title":"t","file":"a.py","old_text":"x","new_text":"y","extra":1}"#,
        br#"{"title":"t","file":"a.py","old_text":"x","new_text":"y\u0000z"}"#,
        br#"{"title":"t","file":"a.py","old_text":"x","new_text":"a\u202eb"}"#,
        b"not json",
        b"[]",
    ] {
        assert!(
            parse_fix(bad, &known).is_none(),
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
    let big = format!(
        r#"{{"title":"t","file":"a.py","old_text":"x","new_text":"{}"}}"#,
        "y".repeat(9000)
    );
    assert!(parse_fix(big.as_bytes(), &known).is_none());
    let f = Fix {
        title: "t".into(),
        file: "a.py".into(),
        old_text: "x = 1".into(),
        new_text: "x = 2".into(),
        also: vec![],
        explanation: String::new(),
    };
    assert_eq!(apply_fix(b"a\nx = 1\nb\n", &f).unwrap(), b"a\nx = 2\nb\n");
    assert!(apply_fix(b"a\nb\n", &f).is_none(), "not found");
    assert!(apply_fix(b"x = 1\nx = 1\n", &f).is_none(), "ambiguous");
    assert!(apply_fix(&[0xff, 0xfe, 0x00], &f).is_none(), "not text");
    let d = unified_diff(b"a\nx = 1\nb\n", &f);
    assert!(
        d.contains("--- a/a.py")
            && d.contains("-x = 1")
            && d.contains("+x = 2")
            && d.contains("@@ -")
    );
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn f02_a_fix_is_only_called_good_when_the_reproducer_stops_reproducing_and_still_runs() {
    let files = fixture_files();
    let idx = index(&files);
    let t = transport();
    let env = CheckEnv {
        transport: &t,
        scratch: &scratch(),
        python: "/usr/bin/python3",
        node: "/usr/bin/node",
    };
    let f = finding(REPRO);
    let before = run_reproducer(&f, &idx, &env);
    assert_eq!(before.status, CheckStatus::Reproduced);
    assert_eq!(
        verify_fix(&f, &before, &fix(GOOD_OLD, GOOD_NEW), &idx, &env),
        FixStatus::FixesTheReproducer
    );
    let cosmetic = fix(
        "    cookie_jar = {\"session\": token}\n",
        "    cookie_jar = {\"session\": token}  # unchanged behaviour\n",
    );
    assert_eq!(
        verify_fix(&f, &before, &cosmetic, &idx, &env),
        FixStatus::StillReproduced,
        "a change that does nothing is not a fix"
    );
    let disables = fix("    return token in ACTIVE_TOKENS\n", "    return False\n");
    assert!(
        matches!(
            verify_fix(&f, &before, &disables, &idx, &env),
            FixStatus::NotChecked(_)
        ),
        "making validate always fail breaks the reproducer's own login check and is not accepted"
    );
    let broken = fix(
        "    ACTIVE_TOKENS[token] = user_id\n",
        "    ACTIVE_TOKENS[token] = user_id +\n",
    );
    assert!(
        matches!(
            verify_fix(&f, &before, &broken, &idx, &env),
            FixStatus::NotChecked(_)
        ),
        "code that no longer runs is not a fix"
    );
    assert!(matches!(
        verify_fix(&f, &before, &fix("no such text", "x"), &idx, &env),
        FixStatus::CouldNotApply(_)
    ));
    assert!(
        matches!(
            verify_fix(&f, &before, &fix("token", "tok"), &idx, &env),
            FixStatus::CouldNotApply(_)
        ),
        "ambiguous text"
    );
    let mut other_file = fix(GOOD_OLD, GOOD_NEW);
    other_file.file = "payments/util/backoff.py".into();
    assert!(matches!(
        verify_fix(&f, &before, &other_file, &idx, &env),
        FixStatus::CouldNotApply(_)
    ));
    let unreproduced = run_reproducer(&finding("print('NOT REPRODUCED')"), &idx, &env);
    assert!(matches!(
        verify_fix(&f, &unreproduced, &fix(GOOD_OLD, GOOD_NEW), &idx, &env),
        FixStatus::NotChecked(_)
    ));
    let original = idx["payments/auth/session.py"].bytes.clone();
    assert_eq!(
        files
            .iter()
            .find(|x| x.path == "payments/auth/session.py")
            .unwrap()
            .bytes,
        original,
        "verification never changes the project files"
    );
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn c05_a_script_that_prints_the_marker_without_touching_the_cited_code_is_not_a_confirmation() {
    // Audit P0: confirmation must depend on the cited code, not on a script that merely prints the success word.
    let files = fixture_files();
    let idx = index(&files);
    let t = transport();
    let env = CheckEnv {
        transport: &t,
        scratch: &scratch(),
        python: "/usr/bin/python3",
        node: "/usr/bin/node",
    };
    for (name, code) in [
        ("unconditional marker", "print('REPRODUCED')\n"),
        ("marker after an import that proves nothing", "import os\nprint('REPRODUCED')\n"),
        ("marker after importing a different module", "from payments.util import backoff\nprint('REPRODUCED')\n"),
        ("both markers printed unconditionally", "print('CONTROL_OK')\nprint('REPRODUCED')\n"),
        ("both markers after importing the cited module", "from payments.auth import session\nprint('CONTROL_OK')\nprint('REPRODUCED')\n"),
        ("the real script without its control line", "from payments.auth import session\nt = session.login('alice')\nassert session.validate(t)\nsession.logout(t)\nprint('REPRODUCED' if session.validate(t) else 'NOT REPRODUCED')\n"),
    ] {
        let r = run_reproducer(&finding(code), &idx, &env);
        assert_ne!(r.status, CheckStatus::Reproduced, "{name} must not confirm: {r:?}");
    }
    let real = run_reproducer(&finding(REPRO), &idx, &env);
    assert_eq!(
        real.status,
        CheckStatus::Reproduced,
        "a reproducer that really exercises the cited file still confirms: {real:?}"
    );
}

#[test]
fn f03_the_diff_shows_context_a_header_and_correct_counts() {
    let original = b"l1\nl2\nl3\nl4\nold a\nold b\nl7\nl8\nl9\nl10\nl11\n";
    let fix = Fix {
        title: "t".into(),
        file: "a.py".into(),
        old_text: "old a\nold b\n".into(),
        new_text: "new\n".into(),
        also: vec![],
        explanation: String::new(),
    };
    let d = unified_diff(original, &fix);
    assert!(
        d.starts_with("--- a/a.py\n+++ b/a.py\n@@ -2,8 +2,7 @@\n"),
        "{d}"
    );
    assert!(
        d.contains(" l2\n l3\n l4\n-old a\n-old b\n+new\n l7\n l8\n l9\n"),
        "{d}"
    );
    assert!(
        !d.contains("l1") && !d.contains("l10"),
        "only three lines of context"
    );
    let at_start = unified_diff(
        b"x\ny\n",
        &Fix {
            title: "t".into(),
            file: "b".into(),
            old_text: "x\n".into(),
            new_text: "z\n".into(),
            also: vec![],
            explanation: String::new(),
        },
    );
    assert!(
        at_start.contains("@@ -1,2 +1,2 @@") && at_start.contains("-x\n+z\n y\n"),
        "{at_start}"
    );
}

#[test]
fn f04_the_project_test_run_is_read_strictly() {
    let ok = "..\n----------------------------------------------------------------------\nRan 2 tests in 0.001s\n\nOK\n";
    assert_eq!(parse_unittest(true, ok), TestRun::Passed(2));
    assert_eq!(
        parse_unittest(false, ok),
        TestRun::Failed("Ran 2 tests in 0.001s  OK".into()),
        "a non-zero exit is never a pass"
    );
    let failed = "F.\nRan 2 tests in 0.002s\n\nFAILED (failures=1)\n";
    assert!(
        matches!(parse_unittest(false, failed), TestRun::Failed(m) if m.contains("FAILED (failures=1)"))
    );
    assert_eq!(
        parse_unittest(false, "Ran 0 tests in 0.000s\n\nNO TESTS RAN\n"),
        TestRun::NoTests
    );
    assert!(
        matches!(parse_unittest(false, "Traceback (most recent call last):\nImportError: boom\n"), TestRun::Failed(m) if m.contains("no test summary"))
    );
    assert!(
        matches!(parse_unittest(true, ""), TestRun::Failed(_)),
        "no output is not a pass"
    );
    // output that merely contains the word OK earlier does not pass
    assert!(matches!(
        parse_unittest(true, "OK\nRan 1 test in 0.1s\n\nFAILED (errors=1)\n"),
        TestRun::Failed(_)
    ));
}

const SESSION_TESTS: &str = "import unittest\nfrom payments.auth import session\n\n\nclass SessionTests(unittest.TestCase):\n    def test_login_validates(self):\n        self.assertTrue(session.validate(session.login('alice')))\n\n    def test_logging_one_user_out_keeps_the_other_logged_in(self):\n        a = session.login('alice')\n        b = session.login('bob')\n        session.logout(a)\n        self.assertTrue(session.validate(b))\n";

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn f05_a_fix_that_stops_the_reproducer_but_breaks_the_projects_passing_tests_is_not_accepted() {
    let mut files = fixture_files();
    let bytes = SESSION_TESTS.as_bytes().to_vec();
    files.push(ImportedFile {
        path: "payments/auth/test_session.py".into(),
        sha256: kk_evidence::capsule::sha256_hex(&bytes),
        bytes,
        label: files[0].label.clone(),
        notes: vec![],
    });
    let idx = index(&files);
    let t = transport();
    let env = CheckEnv {
        transport: &t,
        scratch: &scratch(),
        python: "/usr/bin/python3",
        node: "/usr/bin/node",
    };
    assert_eq!(
        run_project_tests(&idx, &env),
        TestRun::Passed(2),
        "the tests pass on the original code"
    );
    let f = finding(REPRO);
    let before = run_reproducer(&f, &idx, &env);
    assert_eq!(before.status, CheckStatus::Reproduced);
    let (good, note) = verify_fix_with_tests(&f, &before, &fix(GOOD_OLD, GOOD_NEW), &idx, &env);
    assert_eq!(good, FixStatus::FixesTheReproducer);
    assert!(note.contains("2 test(s) passed before and after"), "{note}");
    let sledgehammer = fix(GOOD_OLD, "    ACTIVE_TOKENS.clear()\n");
    let (bad, note) = verify_fix_with_tests(&f, &before, &sledgehammer, &idx, &env);
    assert!(matches!(bad, FixStatus::BreaksTests(_)), "{bad:?} {note}");
    assert!(
        note.contains("passed before this change and fail after it"),
        "{note}"
    );
    assert_eq!(
        verify_fix(&f, &before, &sledgehammer, &idx, &env),
        FixStatus::FixesTheReproducer,
        "without the project's tests the sledgehammer looks fine"
    );

    // when the project's tests already fail, there is nothing to compare, and the note says so
    let mut broken_files = fixture_files();
    let tests_bytes = format!("{SESSION_TESTS}\n    def test_that_always_fails(self):\n        self.fail('already broken')\n").into_bytes();
    broken_files.push(ImportedFile {
        path: "payments/auth/test_session.py".into(),
        sha256: kk_evidence::capsule::sha256_hex(&tests_bytes),
        bytes: tests_bytes,
        label: broken_files[0].label.clone(),
        notes: vec![],
    });
    let bidx = index(&broken_files);
    let bbefore = run_reproducer(&f, &bidx, &env);
    let (status, note) = verify_fix_with_tests(&f, &bbefore, &sledgehammer, &bidx, &env);
    assert_eq!(status, FixStatus::FixesTheReproducer, "{note}");
    assert!(note.contains("before the change: tests failed"), "{note}");
}

#[test]
fn f06_a_fix_may_carry_up_to_three_more_changes_in_other_files_and_nothing_odd() {
    let known: BTreeSet<String> = ["a.py", "b.py", "c.py", "d.py", "e.py"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let edit =
        |file: &str| serde_json::json!({"file": file, "old_text": "x = 1", "new_text": "x = 2"});
    let doc = |also: serde_json::Value| {
        serde_json::json!({"title": "t", "file": "a.py", "old_text": "x = 1", "new_text": "x = 2", "also": also}).to_string()
    };
    let ok = parse_fix(
        doc(serde_json::json!([edit("b.py"), edit("c.py")])).as_bytes(),
        &known,
    )
    .expect("two more files");
    assert_eq!(
        ok.edits()
            .iter()
            .map(|e| e.file.as_str())
            .collect::<Vec<_>>(),
        ["a.py", "b.py", "c.py"]
    );
    assert!(
        parse_fix(
            doc(serde_json::json!([
                edit("b.py"),
                edit("c.py"),
                edit("d.py")
            ]))
            .as_bytes(),
            &known
        )
        .is_some(),
        "three more is the limit"
    );
    for (label, bad) in [
        (
            "four more",
            serde_json::json!([edit("b.py"), edit("c.py"), edit("d.py"), edit("e.py")]),
        ),
        (
            "the same file as the first",
            serde_json::json!([edit("a.py")]),
        ),
        (
            "the same file twice",
            serde_json::json!([edit("b.py"), edit("b.py")]),
        ),
        ("an unknown file", serde_json::json!([edit("zzz.py")])),
        ("not a list", serde_json::json!({"file": "b.py"})),
        (
            "an extra key",
            serde_json::json!([{"file": "b.py", "old_text": "x = 1", "new_text": "x = 2", "extra": 1}]),
        ),
        (
            "a no-op change",
            serde_json::json!([{"file": "b.py", "old_text": "x = 1", "new_text": "x = 1"}]),
        ),
        (
            "a hidden character",
            serde_json::json!([{"file": "b.py", "old_text": "x = 1", "new_text": "x = 2\u{202e}"}]),
        ),
        (
            "a path outside the project",
            serde_json::json!([edit("../outside.py")]),
        ),
    ] {
        assert!(parse_fix(doc(bad).as_bytes(), &known).is_none(), "{label}");
    }
}

fn imported(path: &str, text: &str) -> ImportedFile {
    let bytes = text.as_bytes().to_vec();
    ImportedFile {
        path: path.into(),
        sha256: kk_evidence::capsule::sha256_hex(&bytes),
        bytes,
        label: kk_types::label::Label::new(Level::Internal),
        notes: vec![],
    }
}

#[test]
fn f07_every_change_applies_or_none_does_and_the_diff_names_every_file_with_whole_lines() {
    let (a, b) = (
        imported("a.py", "keep\nvalue = 1\nend\n"),
        imported("b.py", "name = old_name\nother\n"),
    );
    let files: BTreeMap<String, &ImportedFile> =
        [("a.py".to_string(), &a), ("b.py".to_string(), &b)].into();
    let fix = Fix {
        title: "t".into(),
        file: "a.py".into(),
        old_text: "value = 1".into(),
        new_text: "value = 2".into(),
        also: vec![Edit {
            file: "b.py".into(),
            old_text: "old_name".into(),
            new_text: "new_name".into(),
        }],
        explanation: String::new(),
    };
    let done = apply_all(&files, &fix).expect("both apply");
    assert_eq!(done["a.py"], b"keep\nvalue = 2\nend\n");
    assert_eq!(done["b.py"], b"name = new_name\nother\n");
    let diff = unified_diff_all(&files, &fix);
    assert!(
        diff.contains("--- a/a.py") && diff.contains("--- a/b.py"),
        "{diff}"
    );
    assert!(
        diff.contains("-value = 1\n+value = 2\n")
            && diff.contains("-name = old_name\n+name = new_name\n"),
        "a change inside a line is shown as the whole line:\n{diff}"
    );
    let mut broken = fix.clone();
    broken.also[0].old_text = "not there".into();
    assert!(
        apply_all(&files, &broken).is_none(),
        "one change that cannot apply means no change applies"
    );
    let mut ambiguous = fix.clone();
    ambiguous.old_text = "e".into();
    assert!(
        apply_all(&files, &ambiguous).is_none(),
        "text that appears more than once is not a fix"
    );
}

#[test]
fn c20_a_finding_names_its_language_python_by_default_and_unknown_names_are_refused() {
    let known: BTreeSet<String> = [
        "payments/auth/session.py".to_string(),
        "web/app.js".to_string(),
    ]
    .into();
    let base = |extra: &str| {
        format!(
            r#"{{"title":"t","file":"payments/auth/session.py","line":3,"explanation":"e","reproducer":"print(1)"{extra}}}"#
        )
    };
    assert_eq!(
        parse_finding(base("").as_bytes(), &known).unwrap().language,
        Language::Python,
        "no language: python"
    );
    assert_eq!(
        parse_finding(base(r#","language":"python""#).as_bytes(), &known)
            .unwrap()
            .language,
        Language::Python
    );
    assert_eq!(
        parse_finding(base(r#","language":"javascript""#).as_bytes(), &known)
            .unwrap()
            .language,
        Language::JavaScript
    );
    for bad in [
        r#","language":"ruby""#,
        r#","language":"JavaScript""#,
        r#","language":""#.to_string().as_str(),
        r#","language":7"#,
        r#","language":null"#,
        r#","language":["python"]"#,
    ] {
        let text = if bad == r#","language":""# {
            base(r#","language":"""#)
        } else {
            base(bad)
        };
        assert!(
            parse_finding(text.as_bytes(), &known).is_none(),
            "refused: {bad}"
        );
    }
    assert_eq!(Language::parse("python"), Some(Language::Python));
    assert_eq!(Language::JavaScript.as_str(), "javascript");
    assert_eq!(Language::parse("node"), None);
}

fn js_files() -> Vec<ImportedFile> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dst = std::env::temp_dir().join(format!(
        "kk-js-fixture-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dst);
    fs::create_dir_all(dst.join("src/auth")).unwrap();
    fs::create_dir_all(dst.join("src/util")).unwrap();
    fs::write(dst.join("src/auth/session.js"), "const tokens = require('./tokens');\nconst ACTIVE = new Set();\nfunction login(user) { const t = tokens.make(user); ACTIVE.add(t); return t; }\n// logout only clears a local copy, so the token stays valid\nfunction logout(t) { let copy = t; copy = null; return copy; }\nfunction validate(t) { return ACTIVE.has(t); }\nmodule.exports = { login, logout, validate };\n").unwrap();
    fs::write(
        dst.join("src/auth/tokens.js"),
        "let n = 0;\nexports.make = (user) => `${user}-${++n}`;\n",
    )
    .unwrap();
    fs::write(
        dst.join("src/util/backoff.mjs"),
        "export function retryDelay(n) { return 2 ** n; }\n",
    )
    .unwrap();
    fs::write(dst.join("src/index.js"), "const s = require('./auth/session');\nimport('./util/backoff.mjs');\nconst lodash = require('lodash');\nmodule.exports = { s, lodash };\n").unwrap();
    fs::write(dst.join(".env"), "SECRET=1\n").unwrap();
    scan_tree(&dst, Level::Internal).files
}

const JS_REPRO: &str = "import { createRequire } from 'node:module';\nconst require = createRequire('/sample/');\nconst session = require('/sample/src/auth/session.js');\nconst t = session.login('alice');\nif (!session.validate(t)) throw new Error('control');\nconsole.log('CONTROL_OK');\nsession.logout(t);\nconsole.log(session.validate(t) ? 'REPRODUCED' : 'NOT REPRODUCED');\n";

fn js_finding(code: &str) -> Finding {
    Finding {
        title: "Token not revoked".into(),
        file: "src/auth/session.js".into(),
        line: 5,
        explanation: "logout leaves the token valid".into(),
        reproducer: code.into(),
        language: Language::JavaScript,
    }
}

#[test]
fn c21_the_javascript_sample_follows_relative_imports_only() {
    let files = js_files();
    let idx = index(&files);
    assert_eq!(
        js_closure("src/auth/session.js", &idx).unwrap(),
        vec!["src/auth/session.js", "src/auth/tokens.js"]
    );
    let top = js_closure("src/index.js", &idx).unwrap();
    assert!(
        top.contains(&"src/util/backoff.mjs".to_string()),
        "dynamic import followed: {top:?}"
    );
    assert!(top.contains(&"src/auth/tokens.js".to_string()));
    assert!(!top.iter().any(|p| p.contains("env")), "{top:?}");
    assert_eq!(js_closure("nope.js", &idx), Err(ClosureError::EntryMissing));
    let mut withheld = files.clone();
    for f in &mut withheld {
        if f.path == "src/auth/tokens.js" {
            f.label = kk_types::label::Label::new(Level::Restricted).with_compartment("secrets");
        }
    }
    let idx2 = index(&withheld);
    assert_eq!(
        js_closure("src/auth/session.js", &idx2),
        Err(ClosureError::NeedsRestrictedFile(
            "src/auth/tokens.js".into()
        ))
    );
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn c22_javascript_reproducers_confirm_a_real_defect_and_hostile_ones_never_pass() {
    let files = js_files();
    let idx = index(&files);
    let t = transport();
    let env = CheckEnv {
        transport: &t,
        scratch: &scratch(),
        python: "/usr/bin/python3",
        node: "/usr/bin/node",
    };
    let real = run_reproducer(&js_finding(JS_REPRO), &idx, &env);
    assert_eq!(real.status, CheckStatus::Reproduced, "{real:?}");
    let decoy = "import { createRequire } from 'node:module';\nconst require = createRequire('/sample/');\nconst tokens = require('/sample/src/auth/tokens.js');\nconsole.log('CONTROL_OK');\nconsole.log(tokens.make('a') === 'zzz' ? 'REPRODUCED' : 'NOT REPRODUCED');\n";
    let r = run_reproducer(&js_finding(decoy), &idx, &env);
    assert_eq!(r.status, CheckStatus::NotReproduced, "{r:?}");
    let no_control = "console.log('REPRODUCED');\n";
    let r = run_reproducer(&js_finding(no_control), &idx, &env);
    assert!(matches!(r.status, CheckStatus::Failed(_)), "{r:?}");
    let independent = "console.log('CONTROL_OK');\nconsole.log('REPRODUCED');\n";
    let r = run_reproducer(&js_finding(independent), &idx, &env);
    assert!(
        matches!(r.status, CheckStatus::Failed(_)),
        "emptied file still reproduces: {r:?}"
    );
    let cases: Vec<(&str, &str)> = vec![
        ("console.log('REPRODUCED'); process.exit(3);\n", "ended"),
        ("console.log('REPRODUCED ' + Date.now());\n", "nondeterministic"),
        ("import net from 'node:net';\nconst s = net.connect(80, '93.184.216.34');\ns.on('connect', () => { console.log('CONTROL_OK'); console.log('REPRODUCED'); });\ns.on('error', () => process.exit(4));\nsetTimeout(() => process.exit(5), 3000);\n", "ended"),
        ("import fs from 'node:fs';\nconsole.log(fs.readFileSync('/etc/hostname','utf8'));\nconsole.log('REPRODUCED');\n", "ended"),
        ("import fs from 'node:fs';\nfs.writeFileSync('/sample/x', 'y');\nconsole.log('CONTROL_OK');\nconsole.log('REPRODUCED');\n", "ended"),
        ("throw new Error('boom');\n", "ended"),
        ("", "unavailable"),
    ];
    for (code, expect) in cases {
        let r = run_reproducer(&js_finding(code), &idx, &env);
        assert_ne!(r.status, CheckStatus::Reproduced, "{code}: {r:?}");
        match expect {
            "ended" => assert!(matches!(r.status, CheckStatus::Failed(_)), "{code}: {r:?}"),
            "nondeterministic" => {
                assert_eq!(r.status, CheckStatus::Nondeterministic, "{code}: {r:?}")
            }
            _ => assert!(
                matches!(r.status, CheckStatus::Unavailable(_)),
                "{code}: {r:?}"
            ),
        }
    }
}

#[test]
fn f08_a_fix_may_not_change_a_test_file_and_is_refused_before_anything_runs() {
    for t in [
        "tests/test_a.py",
        "test_a.py",
        "pkg/a_test.py",
        "pkg/tests/helpers.py",
        "pkg/test/data.py",
        "TESTS/x.py",
        "conftest.py",
        "web/__tests__/a.js",
        "web/a.test.js",
        "web/a.spec.mjs",
        "web/a.test.ts",
        "test/helper.js",
        "web/a_test.js",
    ] {
        assert!(is_test_path(t), "{t} is a test file");
    }
    for n in [
        "payments/auth/session.py",
        "latest.py",
        "contest.py",
        "attestation.py",
        "protest/a.py",
        "web/contest.js",
        "docs/tests.md",
        "src/a.testing.js",
    ] {
        // names that merely contain the word are ordinary source
        assert!(!is_test_path(n), "{n} is not a test file");
    }
    let mut files = fixture_files();
    let tests_bytes = b"import unittest\n\nclass T(unittest.TestCase):\n    def test_it(self):\n        self.fail('real failure')\n".to_vec();
    files.push(ImportedFile {
        path: "payments/auth/test_session.py".into(),
        sha256: kk_evidence::capsule::sha256_hex(&tests_bytes),
        bytes: tests_bytes,
        label: files[0].label.clone(),
        notes: vec![],
    });
    let idx = index(&files);
    let t = Transport::Local;
    let scratch = std::env::temp_dir();
    let env = CheckEnv {
        transport: &t,
        scratch: &scratch,
        python: "/nonexistent/python",
        node: "/nonexistent/node",
    };
    let f = finding(REPRO);
    let reproduced = kk_review::check::CheckResult {
        status: CheckStatus::Reproduced,
        stdout: String::new(),
        sample_paths: vec![],
    };
    let mut hide_failure = fix("self.fail('real failure')", "pass");
    hide_failure.file = "payments/auth/test_session.py".into();
    let refused = |fx: &Fix| match verify_fix(&f, &reproduced, fx, &idx, &env) {
        FixStatus::CouldNotApply(m) => m,
        other => panic!("not refused: {other:?}"),
    };
    assert!(refused(&hide_failure).contains("test file"));
    // the same change tucked into the second place of an otherwise ordinary fix
    let mut sneaky = fix(GOOD_OLD, GOOD_NEW);
    sneaky.also = vec![Edit {
        file: "payments/auth/test_session.py".into(),
        old_text: "self.fail('real failure')".into(),
        new_text: "pass".into(),
    }];
    assert!(refused(&sneaky).contains("payments/auth/test_session.py"));
    // an ordinary fix is not refused for this reason (it goes on to run the reproducer, which this machine does not have)
    let ordinary = verify_fix(&f, &reproduced, &fix(GOOD_OLD, GOOD_NEW), &idx, &env);
    assert!(
        !matches!(&ordinary, FixStatus::CouldNotApply(m) if m.contains("test file")),
        "{ordinary:?}"
    );
}

#[test]
fn f09_a_fix_must_keep_every_passing_test_running_not_only_passing() {
    let n = TestRun::Passed(5);
    let note = |v: TestVerdict| match v {
        TestVerdict::Maintained(m)
        | TestVerdict::Broken(m)
        | TestVerdict::Inconclusive(m)
        | TestVerdict::NotComparable(m) => m,
    };
    assert!(matches!(
        compare_test_runs(&n, &TestRun::Passed(5)),
        TestVerdict::Maintained(_)
    ));
    assert!(note(compare_test_runs(&n, &TestRun::Passed(5)))
        .contains("5 test(s) passed before and after"));
    assert!(
        matches!(
            compare_test_runs(&n, &TestRun::Passed(7)),
            TestVerdict::Maintained(_)
        ),
        "more tests is fine"
    );
    let fewer = compare_test_runs(&n, &TestRun::Passed(4));
    assert!(matches!(fewer, TestVerdict::Broken(_)));
    assert!(
        note(fewer).contains("5") && note(compare_test_runs(&n, &TestRun::Passed(4))).contains("4")
    );
    assert!(matches!(
        compare_test_runs(&n, &TestRun::NoTests),
        TestVerdict::Broken(_)
    ));
    assert!(matches!(
        compare_test_runs(&n, &TestRun::Failed("boom".into())),
        TestVerdict::Broken(_)
    ));
    assert!(note(compare_test_runs(&n, &TestRun::Failed("boom".into())))
        .contains("passed before this change and fail after it: boom"));
    assert!(matches!(
        compare_test_runs(&n, &TestRun::Unavailable("no sandbox".into())),
        TestVerdict::Inconclusive(_)
    ));
    for before in [
        TestRun::NoTests,
        TestRun::Failed("x".into()),
        TestRun::Unavailable("y".into()),
    ] {
        let v = compare_test_runs(&before, &TestRun::Passed(3));
        assert!(matches!(v, TestVerdict::NotComparable(_)), "{before:?}");
        assert!(note(v).starts_with("before the change: "));
    }
}

#[test]
fn c23_a_missing_package_is_read_from_the_error_and_nothing_else_is() {
    let py = |s: &str| missing_package(Language::Python, s);
    let js = |s: &str| missing_package(Language::JavaScript, s);
    assert_eq!(py("Traceback (most recent call last):\n  File \"/repro/repro.py\", line 1\nModuleNotFoundError: No module named 'requests'\n"), Some("requests".into()));
    assert_eq!(
        py("ModuleNotFoundError: No module named 'yaml.constructor'"),
        Some("yaml".into())
    );
    assert_eq!(
        py("ModuleNotFoundError: No module named 'numpy'; 'numpy' is not a package"),
        Some("numpy".into())
    );
    assert_eq!(py("ImportError: cannot import name 'x' from 'y'"), None);
    assert_eq!(py("AssertionError"), None);
    assert_eq!(py(""), None);
    // hostile text: only a plain name is ever repeated back
    assert_eq!(py("ModuleNotFoundError: No module named 'a\nb'"), None);
    assert_eq!(
        py("ModuleNotFoundError: No module named '../../etc/passwd'"),
        None
    );
    assert_eq!(
        py(&format!(
            "ModuleNotFoundError: No module named '{}'",
            "x".repeat(200)
        )),
        None
    );
    assert_eq!(py("ModuleNotFoundError: No module named 'ünï'"), None);
    assert_eq!(py("ModuleNotFoundError: No module named ''"), None);
    assert_eq!(py("ModuleNotFoundError: No module named 'a/../etc'"), None);
    assert_eq!(py("ModuleNotFoundError: No module named 'x..y'"), None);
    assert_eq!(js("Error: Cannot find package 'a/../b'"), None);
    assert_eq!(py("ModuleNotFoundError: No module named 'a/b'"), None);
    assert_eq!(
        js("Error: Cannot find package 'a/b'"),
        None,
        "an unscoped name has one part"
    );
    assert_eq!(js("Error: Cannot find package '@scope'"), None);
    assert_eq!(js("Error: Cannot find package '@a/b/c'"), None);
    assert_eq!(js("Error: Cannot find package '.hidden'"), None);
    assert_eq!(js("Error: Cannot find package '@/x'"), None);
    assert_eq!(
        js(&format!("Error: Cannot find package '{}'", "x".repeat(100))),
        None
    );
    assert_eq!(
        js("Error: Cannot find package 'lodash"),
        None,
        "no closing quote"
    );
    assert_eq!(js("Error: Cannot find module 'lodash"), None);
    assert_eq!(js("Error: Cannot find package 'a b'"), None);
    assert_eq!(js("Error [ERR_MODULE_NOT_FOUND]: Cannot find package 'lodash' imported from /repro/repro.mjs"), Some("lodash".into()));
    assert_eq!(
        js("Error: Cannot find module 'left-pad'\nRequire stack:\n- /repro/repro.mjs"),
        Some("left-pad".into())
    );
    assert_eq!(js("Error [ERR_MODULE_NOT_FOUND]: Cannot find package '@scope/pkg' imported from /repro/repro.mjs"), Some("@scope/pkg".into()));
    assert_eq!(js("Error [ERR_MODULE_NOT_FOUND]: Cannot find module '/sample/lib/gone.js' imported from /repro/repro.mjs"), None, "a missing project file is not a package");
    assert_eq!(js("Error: Cannot find module './gone.js'"), None);
    assert_eq!(js("Error: Cannot find package 'node:nonsense'"), None);
    assert_eq!(js("Error: Cannot find package '../up'"), None);
    assert_eq!(js("TypeError: x is not a function"), None);
    assert_eq!(
        py("Error: Cannot find module 'left-pad'"),
        None,
        "each language reads its own error"
    );
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn c24_a_missing_third_party_package_is_unavailable_and_a_mistake_about_the_projects_own_code_is_not(
) {
    let files = fixture_files();
    let idx = index(&files);
    let t = transport();
    let env = CheckEnv {
        transport: &t,
        scratch: &scratch(),
        python: "/usr/bin/python3",
        node: "/usr/bin/node",
    };
    let run = |f: &Finding| run_reproducer(f, &idx, &env).status;
    let py = |code: &str| finding(code);
    // a package that is not installed in the sandbox: nothing can be said about the code
    match run(&py(
        "import requests_not_installed_here\nprint('CONTROL_OK')\n",
    )) {
        CheckStatus::Unavailable(m) => assert!(m.contains("'requests_not_installed_here'"), "{m}"),
        other => panic!("{other:?}"),
    }
    // a mistake about the project's own package is a bad reproducer, never "unavailable"
    for code in [
        "import payments.no_such_module\nprint('CONTROL_OK')\n",
        "from payments.auth import no_such_name\nprint('CONTROL_OK')\n",
        "raise SystemExit(3)\n",
    ] {
        assert!(matches!(run(&py(code)), CheckStatus::Failed(_)), "{code}");
    }
    // a script that merely prints the error text is not a failure of any kind
    assert_eq!(
        run(&py("print(\"ModuleNotFoundError: No module named 'x'\")\n")),
        CheckStatus::NotReproduced
    );
    let jsfiles = js_files();
    let jidx = index(&jsfiles);
    let jrun = |code: &str| run_reproducer(&js_finding(code), &jidx, &env).status;
    match jrun("import leftPad from 'left-pad';\nconsole.log('CONTROL_OK');\n") {
        CheckStatus::Unavailable(m) => assert!(m.contains("'left-pad'"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(
        matches!(
            jrun("import x from './no_such_file.js';\nconsole.log('CONTROL_OK');\n"),
            CheckStatus::Failed(_)
        ),
        "a missing project file is a bad reproducer"
    );
    assert!(matches!(jrun("process.exit(3);\n"), CheckStatus::Failed(_)));
}
