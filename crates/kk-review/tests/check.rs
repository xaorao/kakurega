use kk_review::check::{
    apply_all, apply_fix, clean_text, compare_test_runs, consolidate, is_test_path, js_closure,
    missing_package, names_sandbox_path, node_major, parse_finding, parse_fix, parse_node_test,
    parse_unittest, python_closure, run_project_tests, run_project_tests_for, run_reproducer,
    unified_diff, unified_diff_all, verify_fix, verify_fix_with_tests, CheckEnv, CheckStatus,
    ClosureError, Edit, Finding, Fix, FixStatus, Language, TestRun, TestVerdict,
};
use kk_review::import::{scan_tree, ImportedFile};
use kk_sandbox::{Exit, Transport};
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
fn f06_a_fix_may_carry_up_to_three_more_files_and_nothing_odd() {
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

const JS_REPRO: &str = "import { createRequire } from 'node:module';\nconst require = createRequire(process.cwd() + '/');\nconst session = require('./src/auth/session.js');\nconst t = session.login('alice');\nif (!session.validate(t)) throw new Error('control');\nconsole.log('CONTROL_OK');\nsession.logout(t);\nconsole.log(session.validate(t) ? 'REPRODUCED' : 'NOT REPRODUCED');\n";

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
    let decoy = "import { createRequire } from 'node:module';\nconst require = createRequire(process.cwd() + '/');\nconst tokens = require('./src/auth/tokens.js');\nconsole.log('CONTROL_OK');\nconsole.log(tokens.make('a') === 'zzz' ? 'REPRODUCED' : 'NOT REPRODUCED');\n";
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
        ("import fs from 'node:fs';\nfs.writeFileSync(process.cwd() + '/x', 'y');\nconsole.log('CONTROL_OK');\nconsole.log('REPRODUCED');\n", "ended"),
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

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn f10_a_fix_that_just_ends_the_program_before_the_verdict_is_not_a_fix() {
    let t = transport();
    let env = CheckEnv {
        transport: &t,
        scratch: &scratch(),
        python: "/usr/bin/python3",
        node: "/usr/bin/node",
    };
    // Python: the "fix" makes logout end the whole program cleanly, after CONTROL_OK and before any verdict
    let files = fixture_files();
    let idx = index(&files);
    let f = finding(REPRO);
    let before = run_reproducer(&f, &idx, &env);
    assert_eq!(before.status, CheckStatus::Reproduced);
    let quits = fix(GOOD_OLD, "    import sys\n    sys.exit(0)\n");
    match verify_fix(&f, &before, &quits, &idx, &env) {
        FixStatus::NotChecked(m) => assert!(m.contains("verdict"), "{m}"),
        other => panic!("ending the program was accepted as a fix: {other:?}"),
    }
    // the honest fix is still accepted
    assert_eq!(
        verify_fix(&f, &before, &fix(GOOD_OLD, GOOD_NEW), &idx, &env),
        FixStatus::FixesTheReproducer
    );
    // JavaScript, same trick
    let jsfiles = js_files();
    let jidx = index(&jsfiles);
    let jf = js_finding(JS_REPRO);
    let jbefore = run_reproducer(&jf, &jidx, &env);
    assert_eq!(jbefore.status, CheckStatus::Reproduced, "{jbefore:?}");
    let old = "function logout(t) { let copy = t; copy = null; return copy; }";
    let mk = |new: &str| Fix {
        title: "x".into(),
        file: "src/auth/session.js".into(),
        old_text: old.into(),
        new_text: new.into(),
        also: vec![],
        explanation: String::new(),
    };
    match verify_fix(
        &jf,
        &jbefore,
        &mk("function logout(t) { process.exit(0); }"),
        &jidx,
        &env,
    ) {
        FixStatus::NotChecked(m) => assert!(m.contains("verdict"), "{m}"),
        other => panic!("ending the program was accepted as a fix: {other:?}"),
    }
    assert_eq!(
        verify_fix(
            &jf,
            &jbefore,
            &mk("function logout(t) { ACTIVE.delete(t); return null; }"),
            &jidx,
            &env
        ),
        FixStatus::FixesTheReproducer
    );
}

fn tap(tests: u32, pass: u32, fail: u32, cancelled: u32, skipped: u32, todo: u32) -> String {
    format!("# tests {tests}\n# suites 0\n# pass {pass}\n# fail {fail}\n# cancelled {cancelled}\n# skipped {skipped}\n# todo {todo}\n")
}

#[test]
fn n01_node_test_output_is_read_strictly_and_anything_unclear_is_not_a_pass() {
    let ok = Exit::Code(0);
    let body = |lines: &str, summary: String| format!("TAP version 13\n{lines}1..9\n{summary}");
    // formats captured from Node 22.22.1
    assert_eq!(
        parse_node_test(&ok, &body("ok 1 - a\nok 2 - b\n", tap(4, 4, 0, 0, 0, 0))),
        TestRun::Passed(4)
    );
    // skipped and todo tests are not passes: 3 tests, 1 pass
    let skip = body(
        "ok 1 - ran\nok 2 - skipped # SKIP\nnot ok 3 - todo # TODO\n",
        tap(3, 1, 0, 0, 1, 1),
    );
    assert_eq!(parse_node_test(&ok, &skip), TestRun::Passed(1));
    // nothing ran, or everything was skipped: no evidence
    assert_eq!(
        parse_node_test(&ok, &body("", tap(0, 0, 0, 0, 0, 0))),
        TestRun::NoTests
    );
    assert_eq!(
        parse_node_test(&ok, &body("ok 1 - s # SKIP\n", tap(2, 0, 0, 0, 2, 0))),
        TestRun::NoTests
    );
    // failures
    let failed = |t: TestRun| matches!(t, TestRun::Failed(_));
    assert!(failed(parse_node_test(
        &Exit::Code(1),
        &body("not ok 2 - bad\n", tap(2, 1, 1, 0, 0, 0))
    )));
    assert!(
        failed(parse_node_test(
            &ok,
            &body("not ok 2 - bad\n", tap(2, 1, 1, 0, 0, 0))
        )),
        "a failure with exit 0 is still a failure"
    );
    assert!(
        failed(parse_node_test(&ok, &body("", tap(2, 1, 0, 1, 0, 0)))),
        "cancelled is not a pass"
    );
    // SWE-bench's rule: the exit status must agree with the summary
    assert!(failed(parse_node_test(
        &Exit::Code(1),
        &body("ok 1 - a\n", tap(4, 4, 0, 0, 0, 0))
    )));
    assert!(failed(parse_node_test(
        &Exit::Signal(9),
        &body("ok 1 - a\n", tap(4, 4, 0, 0, 0, 0))
    )));
    assert!(failed(parse_node_test(
        &Exit::TimedOut,
        &body("", tap(4, 4, 0, 0, 0, 0))
    )));
    // a summary that does not add up, or is incomplete, or missing (a run killed by the time limit prints none)
    assert!(failed(parse_node_test(
        &ok,
        &body("", tap(9, 4, 0, 0, 0, 0))
    )));
    assert!(failed(parse_node_test(
        &ok,
        "TAP version 13\n# tests 4\n# pass 4\n"
    )));
    assert!(failed(parse_node_test(&ok, "TAP version 13\n")));
    assert!(failed(parse_node_test(&ok, "")));
    // a failing test that the summary does not count
    assert!(failed(parse_node_test(
        &ok,
        &body("not ok 1 - sneaky\n", tap(1, 1, 0, 0, 0, 0))
    )));
    // text printed by a test file is escaped by Node (`# \# tests 99`) and indented text is not a summary
    let forged = body(
        "# \\# tests 99\n# \\# pass 99\n  # tests 99\n  # pass 99\nnot ok 1 - real\n",
        tap(1, 0, 1, 0, 0, 0),
    );
    assert!(failed(parse_node_test(&ok, &forged)));
    // indented or escaped look-alikes AFTER the runner's summary change nothing either
    let trailing = format!(
        "{}  # tests 99\n  # pass 99\n  # fail 0\n  # cancelled 0\n  # skipped 0\n  # todo 0\n# \\# tests 99\n",
        body("ok 1 - a\n", tap(2, 2, 0, 0, 0, 0))
    );
    assert_eq!(parse_node_test(&ok, &trailing), TestRun::Passed(2));
    // a test that prints plain summary words gets them prefixed by Node as `# tests 5`, which looks like a summary: with no
    // runner summary after it (the plan line `1..N` must sit at column 0 right before `# tests`) it is not accepted
    let printed = "TAP version 13\n# tests 5\n# pass 5\n# fail 0\n# cancelled 0\n# skipped 0\n# todo 0\n# 1..5\n";
    assert!(failed(parse_node_test(&ok, printed)));
    assert!(failed(parse_node_test(
        &ok,
        &format!("{printed}1..5\nnot a summary\n")
    )));
    // the plan line must be digits and directly before the summary
    let summary = tap(2, 2, 0, 0, 0, 0);
    for plan in ["1..x", "1..", "1..2x", "x1..2", " 1..2", "1...2"] {
        assert!(
            failed(parse_node_test(
                &ok,
                &format!("TAP version 13\n{plan}\n{summary}")
            )),
            "{plan:?}"
        );
    }
    assert!(failed(parse_node_test(
        &ok,
        &format!("TAP version 13\n1..2\nunrelated line\n{summary}")
    )));
    assert!(
        failed(parse_node_test(&ok, &format!("TAP version 13\n{summary}"))),
        "a summary with no plan line before it"
    );
    assert_eq!(
        parse_node_test(&ok, &format!("TAP version 13\n1..2\n{summary}")),
        TestRun::Passed(2)
    );
    // fields far from the `# tests` line belong to something else
    let spread = format!("TAP version 13\n1..2\n# tests 4\n{}# pass 4\n# fail 0\n# cancelled 0\n# skipped 0\n# todo 0\n", "# filler\n".repeat(12));
    assert!(failed(parse_node_test(&ok, &spread)));
    // the last complete summary is the runner's own
    let early = format!(
        "# tests 50\n# pass 50\n# fail 0\n# cancelled 0\n# skipped 0\n# todo 0\n{}",
        body("ok 1 - a\n", tap(2, 2, 0, 0, 0, 0))
    );
    assert_eq!(parse_node_test(&ok, &early), TestRun::Passed(2));
    // a missing package is named, but never turns a failure into "unavailable" (a fix could print that text)
    let missing = body(
        "# Error: Cannot find module 'left-pad'\nnot ok 1 - a.test.js\n",
        tap(1, 0, 1, 0, 0, 0),
    );
    match parse_node_test(&Exit::Code(1), &missing) {
        TestRun::Failed(m) => assert!(m.contains("'left-pad'"), "{m}"),
        other => panic!("{other:?}"),
    }
}

const SESSION_JS_TESTS: &str = "const test = require('node:test');\nconst assert = require('node:assert');\nconst s = require('./session');\ntest('login makes a valid token', () => { const t = s.login('a'); assert.ok(s.validate(t)); });\ntest('unknown tokens are invalid', () => assert.equal(s.validate('nope'), false));\ntest('other users stay logged in', () => { const a = s.login('a'); const b = s.login('b'); s.logout(a); assert.ok(s.validate(b)); });\n";

fn with_js_tests(mut files: Vec<ImportedFile>, path: &str, text: &str) -> Vec<ImportedFile> {
    let bytes = text.as_bytes().to_vec();
    files.push(ImportedFile {
        path: path.into(),
        sha256: kk_evidence::capsule::sha256_hex(&bytes),
        bytes,
        label: files[0].label.clone(),
        notes: vec![],
    });
    files
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn n02_the_projects_javascript_tests_are_run_in_the_sandbox_and_a_fix_is_held_to_them() {
    let t = transport();
    let env = CheckEnv {
        transport: &t,
        scratch: &scratch(),
        python: "/usr/bin/python3",
        node: "/usr/bin/node",
    };
    let none = js_files();
    assert_eq!(
        run_project_tests_for(Language::JavaScript, &index(&none), &env),
        TestRun::NoTests
    );
    let files = with_js_tests(js_files(), "src/auth/session.test.js", SESSION_JS_TESTS);
    let idx = index(&files);
    assert_eq!(
        run_project_tests_for(Language::JavaScript, &idx, &env),
        TestRun::Passed(3)
    );
    let jf = js_finding(JS_REPRO);
    let before = run_reproducer(&jf, &idx, &env);
    assert_eq!(before.status, CheckStatus::Reproduced, "{before:?}");
    let old = "function logout(t) { let copy = t; copy = null; return copy; }";
    let mk = |new: &str| Fix {
        title: "x".into(),
        file: "src/auth/session.js".into(),
        old_text: old.into(),
        new_text: new.into(),
        also: vec![],
        explanation: String::new(),
    };
    let (good, note) = verify_fix_with_tests(
        &jf,
        &before,
        &mk("function logout(t) { ACTIVE.delete(t); return null; }"),
        &idx,
        &env,
    );
    assert_eq!(good, FixStatus::FixesTheReproducer, "{note}");
    assert!(note.contains("3 test(s) passed before and after"), "{note}");
    let (bad, note) = verify_fix_with_tests(
        &jf,
        &before,
        &mk("function logout(t) { ACTIVE.clear(); return null; }"),
        &idx,
        &env,
    );
    assert!(matches!(bad, FixStatus::BreaksTests(_)), "{bad:?} {note}");
    // a module that ends the program while the test file loads: the file "passes" as one test, so fewer tests ran
    let exits = {
        let mut m = files.clone();
        let i = m
            .iter()
            .position(|f| f.path == "src/auth/session.js")
            .unwrap();
        m[i].bytes =
            format!("process.exit(0);\n{}", String::from_utf8_lossy(&m[i].bytes)).into_bytes();
        m
    };
    let after = run_project_tests_for(Language::JavaScript, &index(&exits), &env);
    assert_eq!(after, TestRun::Passed(1), "{after:?}");
    assert!(matches!(
        compare_test_runs(&TestRun::Passed(3), &after),
        TestVerdict::Broken(_)
    ));
    // a test that needs a package the sandbox does not have
    let needs = with_js_tests(js_files(), "src/x.test.js", "const test = require('node:test');\nconst x = require('left-pad');\ntest('t', () => {});\n");
    match run_project_tests_for(Language::JavaScript, &index(&needs), &env) {
        TestRun::Failed(m) => assert!(m.contains("'left-pad'"), "{m}"),
        other => panic!("{other:?}"),
    }
    // a test file that never ends is stopped by the time limit and is a failure, never a pass
    let hang = with_js_tests(
        js_files(),
        "src/h.test.js",
        "setInterval(() => {}, 1000);\n",
    );
    assert!(matches!(
        run_project_tests_for(Language::JavaScript, &index(&hang), &env),
        TestRun::Failed(_)
    ));
    // a test file that prints a whole fake summary does not make a failing suite pass
    let forged = with_js_tests(
        js_files(),
        "src/forge.test.js",
        "const test = require('node:test');\nfor (const l of ['tests 5', 'pass 5', 'fail 0', 'cancelled 0', 'skipped 0', 'todo 0', '1..5']) console.log(l);\ntest('really fails', () => { throw new Error('no'); });\n",
    );
    assert!(matches!(
        run_project_tests_for(Language::JavaScript, &index(&forged), &env),
        TestRun::Failed(_)
    ));
    // Python is untouched by all of this
    let py = fixture_files();
    assert_eq!(
        run_project_tests_for(Language::Python, &index(&py), &env),
        run_project_tests(&index(&py), &env)
    );
}

#[test]
fn c25_a_reproducer_may_not_name_the_sandboxs_own_paths_because_no_one_else_could_re_run_it() {
    for bad in [
        "const r = createRequire('/sample/');",
        "require('/sample/src/a.js')",
        "open('/sample/x')",
        "path = \"/sample\"",
        "x=/sample/y",
        "import('/repro/repro.mjs')",
        "(\"/repro/repro.py\")",
        "/sample",
        "see /sample/y",
        "\t/sample/a",
        // not told apart from the sandbox path without parsing the language, so it fails closed
        "process.cwd() + '/sample'",
    ] {
        assert!(names_sandbox_path(bad), "{bad:?}");
    }
    for ok in [
        "",
        "require('./src/sample/a.js')",
        "from sample import x",
        "open('data/sample.txt')",
        "path = '/samples/x'",
        "path = '/sample_data/x'",
        "require('../sample/x.js')",
        "https://example.com/sample/page",
        "https://example.com/repro/page",
        "samples = 3",
        "x = a/sample/b",
        "~/sample/x",
    ] {
        assert!(!names_sandbox_path(ok), "{ok:?}");
    }
    // a script that does is not run and says why; this needs no sandbox
    let files = fixture_files();
    let idx = index(&files);
    let t = Transport::Local;
    let scratch = std::env::temp_dir();
    let env = CheckEnv {
        transport: &t,
        scratch: &scratch,
        python: "/nonexistent/python",
        node: "/nonexistent/node",
    };
    let mut f = finding(REPRO);
    f.reproducer = format!("{}\nopen('/sample/payments/auth/session.py')\n", REPRO);
    match run_reproducer(&f, &idx, &env).status {
        CheckStatus::Unavailable(m) => {
            assert!(m.contains("/sample") && m.contains("current folder"), "{m}")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn e01_what_the_sandbox_confirmed_is_what_the_recipients_own_machine_reproduces_in_both_languages()
{
    use kk_review::release::assemble;
    let t = transport();
    let scratch_dir = scratch();
    let env = CheckEnv {
        transport: &t,
        scratch: &scratch_dir,
        python: "/usr/bin/python3",
        node: "/usr/bin/node",
    };
    let mut files = fixture_files();
    files.extend(js_files());
    let idx = index(&files);
    let py = finding(REPRO);
    let js = js_finding(JS_REPRO);
    let py_check = run_reproducer(&py, &idx, &env);
    let js_check = run_reproducer(&js, &idx, &env);
    assert_eq!(py_check.status, CheckStatus::Reproduced, "{py_check:?}");
    assert_eq!(js_check.status, CheckStatus::Reproduced, "{js_check:?}");
    let draft = assemble(
        &[(py, py_check), (js, js_check)],
        &idx,
        "acme-audit",
        "policy-1",
        1_700_000_000,
        86_400,
    )
    .expect("assemble");
    let dir = scratch_dir.join(format!("e01-release-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for (n, b) in &draft.files {
        fs::write(dir.join(n), b).unwrap();
    }
    // the recipient's side: plain python3 and node, outside our sandbox, on the release folder alone
    let run = |script: &str| -> (bool, String) {
        let out = if cfg!(target_os = "linux") {
            std::process::Command::new("python3")
                .arg(dir.join(script))
                .output()
                .unwrap()
        } else {
            std::process::Command::new("limactl")
                .args(["shell", "--tty=false", "kak", "--", "python3"])
                .arg(dir.join(script))
                .output()
                .unwrap()
        };
        (
            out.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    };
    let (ok, text) = run("run-check.py");
    assert!(ok, "{text}");
    assert_eq!(text.matches("OK   ").count(), 2, "{text}");
}

#[test]
fn rd01_the_node_version_is_read_from_its_own_answer_and_nothing_else_counts() {
    assert_eq!(node_major("v22.22.1\n"), Some(22));
    assert_eq!(node_major("v22.0.0"), Some(22));
    assert_eq!(node_major("v25.5.0"), Some(25));
    assert_eq!(node_major("v8.17.0\n"), Some(8));
    assert_eq!(node_major("v100.1.2"), Some(100));
    for bad in [
        "",
        "22.1.0",
        "v",
        "vx.1.2",
        "v.1.2",
        "node v22",
        "v22",
        "v22.1",
        " v22.1.1",
        "v22.1.1 extra",
        "v-22.1.1",
        "v022.1.1x",
        "v2e2.1.1",
        "bash: node: command not found",
        "v99999999999.0.0",
        "v22.1.1.1",
        "v22.1.1.",
        "v22.1.1\n\n",
        "v22..1",
    ] {
        assert_eq!(node_major(bad), None, "{bad:?}");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// several changes in one file

fn hunk(file: &str, old: &str, new: &str) -> Edit {
    Edit {
        file: file.into(),
        old_text: old.into(),
        new_text: new.into(),
    }
}

fn fix_of(edits: Vec<Edit>) -> Fix {
    let mut it = edits.into_iter();
    let first = it.next().unwrap();
    Fix {
        title: "t".into(),
        file: first.file,
        old_text: first.old_text,
        new_text: first.new_text,
        also: it.collect(),
        explanation: String::new(),
    }
}

fn index_of(files: Vec<&ImportedFile>) -> BTreeMap<String, &ImportedFile> {
    files.into_iter().map(|f| (f.path.clone(), f)).collect()
}

/// Applies a unified diff made by `unified_diff_all` for one file to its text. Panics if a hunk does not fit: the check that
/// the diff people read is the change that would be made.
fn apply_unified(original: &str, diff: &str) -> String {
    let lines: Vec<&str> = original
        .strip_suffix('\n')
        .unwrap_or(original)
        .split('\n')
        .collect();
    let mut out: Vec<String> = Vec::new();
    let mut at = 0usize;
    let mut it = diff.lines().peekable();
    while let Some(l) = it.next() {
        if l.starts_with("--- ") || l.starts_with("+++ ") {
            continue;
        }
        let Some(rest) = l.strip_prefix("@@ -") else {
            panic!("unexpected line outside a hunk: {l:?}");
        };
        let (old_part, tail) = rest.split_once(" +").unwrap();
        let (new_part, _) = tail.split_once(" @@").unwrap();
        let (old_start, old_len) = old_part.split_once(',').map_or_else(
            || (old_part.parse::<usize>().unwrap(), 1),
            |(a, b)| (a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()),
        );
        let (_, new_len) = new_part.split_once(',').map_or_else(
            || (new_part.parse::<usize>().unwrap(), 1),
            |(a, b)| (a.parse::<usize>().unwrap(), b.parse::<usize>().unwrap()),
        );
        let start = if old_len == 0 {
            old_start
        } else {
            old_start - 1
        };
        while at < start {
            out.push(lines[at].to_string());
            at += 1;
        }
        let (mut seen_old, mut seen_new) = (0, 0);
        while seen_old < old_len || seen_new < new_len {
            let h = it.next().expect("the hunk ends before its counts say");
            match h.chars().next() {
                Some(' ') => {
                    assert_eq!(&h[1..], lines[at], "context line differs");
                    out.push(lines[at].to_string());
                    at += 1;
                    seen_old += 1;
                    seen_new += 1;
                }
                Some('-') => {
                    assert_eq!(&h[1..], lines[at], "removed line differs");
                    at += 1;
                    seen_old += 1;
                }
                Some('+') => {
                    out.push(h[1..].to_string());
                    seen_new += 1;
                }
                other => panic!("bad hunk line {other:?}: {h:?}"),
            }
        }
    }
    while at < lines.len() {
        out.push(lines[at].to_string());
        at += 1;
    }
    let mut text = out.join("\n");
    if original.ends_with('\n') {
        text.push('\n');
    }
    text
}

#[test]
fn f20_several_changes_in_one_file_become_one_exact_change_that_applies_and_diffs_correctly() {
    let text = "import os\n\ndef a():\n    return 1\n\ndef b():\n    return 2\n\ndef c():\n    return 3\n\nMAIN = a()\n";
    let (m, other) = (
        imported("m.py", text),
        imported("n.py", "name = old\nrest\n"),
    );
    let files = index_of(vec![&m, &other]);
    let fix = fix_of(vec![
        hunk("m.py", "    return 1\n", "    return 10\n"),
        hunk("n.py", "old", "new"),
        hunk("m.py", "    return 3\n", "    return 30\n"),
    ]);
    let merged = consolidate(&files, fix).expect("consolidates");
    let edits = merged.edits();
    assert_eq!(
        edits.len(),
        2,
        "one change per file after consolidation: {edits:?}"
    );
    assert_eq!(edits[0].file, "m.py");
    assert_eq!(edits[1].file, "n.py");
    let done = apply_all(&files, &merged).expect("applies");
    let want = "import os\n\ndef a():\n    return 10\n\ndef b():\n    return 2\n\ndef c():\n    return 30\n\nMAIN = a()\n";
    assert_eq!(String::from_utf8_lossy(&done["m.py"]), want);
    assert_eq!(done["n.py"], b"name = new\nrest\n");
    // the diff shows only the two changed lines of m.py, not the unchanged function between them
    let diff = unified_diff_all(&files, &merged);
    let m_diff: String = diff.split("--- a/n.py").next().unwrap().to_string();
    assert!(
        !m_diff.contains("-def b()") && !m_diff.contains("+def b()"),
        "{m_diff}"
    );
    assert!(
        m_diff.contains("-    return 1\n+    return 10\n")
            && m_diff.contains("-    return 3\n+    return 30\n"),
        "{m_diff}"
    );
    assert_eq!(
        apply_unified(text, &m_diff),
        want,
        "the diff is the change:\n{m_diff}"
    );
    // the hunks may be listed in any order and may touch
    let reversed = fix_of(vec![
        hunk("m.py", "    return 3\n", "    return 30\n"),
        hunk("m.py", "    return 1\n", "    return 10\n"),
    ]);
    let again = consolidate(&files, reversed).expect("any order");
    assert_eq!(apply_all(&files, &again).unwrap()["m.py"], want.as_bytes());
    let touching = imported("t.py", "aaa\nbbb\nccc\n");
    let tf = index_of(vec![&touching]);
    let t = consolidate(
        &tf,
        fix_of(vec![
            hunk("t.py", "aaa\n", "AAA\n"),
            hunk("t.py", "bbb\n", "BBB\n"),
        ]),
    )
    .expect("touching hunks are fine");
    assert_eq!(apply_all(&tf, &t).unwrap()["t.py"], b"AAA\nBBB\nccc\n");
}

#[test]
fn f21_a_set_of_changes_that_could_be_ambiguous_overlapping_or_missing_is_not_a_fix() {
    let f = imported("m.py", "x = 1\nx = 1\ny = 2\naaa\nz = 3\nabc abc\n");
    let g = imported("n.py", "q = 1\n");
    let files = index_of(vec![&f, &g]);
    let bad: Vec<(&str, Fix)> = vec![
        (
            "a hunk that is not there",
            fix_of(vec![
                hunk("m.py", "y = 2", "y = 3"),
                hunk("m.py", "nope", "x"),
            ]),
        ),
        (
            "a hunk that is there twice",
            fix_of(vec![
                hunk("m.py", "y = 2", "y = 3"),
                hunk("m.py", "x = 1", "x = 9"),
            ]),
        ),
        (
            "two hunks on the same text",
            fix_of(vec![
                hunk("m.py", "y = 2", "y = 3"),
                hunk("m.py", "y = 2", "y = 4"),
            ]),
        ),
        (
            "overlapping hunks",
            fix_of(vec![
                hunk("m.py", "y = 2\naaa", "y = 3\naaa"),
                hunk("m.py", "aaa\nz = 3", "bbb\nz = 3"),
            ]),
        ),
        (
            "a hunk inside another",
            fix_of(vec![
                hunk("m.py", "y = 2\naaa\nz", "Y\nz"),
                hunk("m.py", "aaa", "bbb"),
            ]),
        ),
        (
            "a text that overlaps itself",
            fix_of(vec![
                hunk("m.py", "aa", "bb"),
                hunk("m.py", "y = 2", "y = 3"),
            ]),
        ),
        (
            "a missing file",
            fix_of(vec![
                hunk("m.py", "y = 2", "y = 3"),
                hunk("zzz.py", "a", "b"),
                hunk("m.py", "z = 3", "z = 4"),
            ]),
        ),
        (
            "one bad file among good ones",
            fix_of(vec![
                hunk("m.py", "y = 2", "y = 3"),
                hunk("m.py", "z = 3", "z = 4"),
                hunk("n.py", "absent", "x"),
            ]),
        ),
    ];
    for (label, fix) in bad {
        // joined, and then made: a fix is only a fix if both steps work
        let made = consolidate(&files, fix).and_then(|f| {
            apply_all(&files, &f)
                .map(|_| ())
                .ok_or_else(|| "cannot apply".to_string())
        });
        assert!(made.is_err(), "{label}");
    }
    // a file that is not text cannot take several hunks
    let bin = ImportedFile {
        path: "b.bin".into(),
        sha256: String::new(),
        bytes: vec![0xff, 0xfe, b'x', b'y', 0xff, b'z'],
        label: kk_types::label::Label::new(Level::Internal),
        notes: vec![],
    };
    let bf = index_of(vec![&bin]);
    assert!(consolidate(
        &bf,
        fix_of(vec![hunk("b.bin", "x", "q"), hunk("b.bin", "z", "w")])
    )
    .is_err());
    // too far apart to be one span
    let big = imported(
        "big.txt",
        &format!("first\n{}last\n", "filler line\n".repeat(30_000)),
    );
    let bg = index_of(vec![&big]);
    assert!(
        consolidate(
            &bg,
            fix_of(vec![
                hunk("big.txt", "first", "FIRST"),
                hunk("big.txt", "last", "LAST")
            ])
        )
        .is_err(),
        "the span between two hunks is bounded"
    );
    // a fix of one change per file is returned as it is
    let single = fix_of(vec![
        hunk("m.py", "y = 2", "y = 3"),
        hunk("n.py", "q = 1", "q = 2"),
    ]);
    assert_eq!(consolidate(&files, single.clone()).unwrap(), single);
}

#[test]
fn f22_exactly_once_means_once_even_when_the_text_overlaps_itself() {
    // "aa" occurs twice in "aaa" (at 0 and at 1); counting only non-overlapping matches calls it once
    let single = hunk("m.py", "aa", "bb");
    assert!(apply_fix(b"aaa\n", &fix_of(vec![single.clone()])).is_none());
    assert!(apply_fix(b"xaay\n", &fix_of(vec![single])).is_some());
    assert!(
        apply_fix(b"abab\n", &fix_of(vec![hunk("m.py", "aba", "x")])).is_some(),
        "one real occurrence"
    );
    assert!(
        apply_fix(b"ababa\n", &fix_of(vec![hunk("m.py", "aba", "x")])).is_none(),
        "two overlapping occurrences"
    );
}

#[test]
fn f23_for_any_set_of_disjoint_changes_the_diff_applied_to_the_file_is_the_changed_file() {
    // a small fixed generator, so a failure can be repeated
    let mut state: u64 = 0x2545f4914f6cdd1d;
    let mut next = move |m: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % m
    };
    for round in 0..300 {
        let n_lines = 5 + next(60) as usize;
        let lines: Vec<String> = (0..n_lines)
            .map(|i| format!("line {i} of the file"))
            .collect();
        let text = lines.join("\n") + "\n";
        let f = imported("r.py", &text);
        let files = index_of(vec![&f]);
        // pick up to 5 disjoint line ranges; each replaced by 0..3 new lines
        let mut starts: Vec<usize> = Vec::new();
        for _ in 0..(1 + next(5)) {
            let s = next(n_lines as u64) as usize;
            if starts.iter().all(|x| x.abs_diff(s) > 3) {
                starts.push(s);
            }
        }
        let mut edits = Vec::new();
        for &s in &starts {
            let len = 1 + next(2) as usize;
            let len = len.min(n_lines - s);
            let old = lines[s..s + len].join("\n") + "\n";
            let new_count = next(4) as usize;
            let new: String = (0..new_count)
                .map(|k| format!("changed {round}-{s}-{k}\n"))
                .collect();
            edits.push(hunk("r.py", &old, &new));
        }
        let fix = fix_of(edits.clone());
        let merged = consolidate(&files, fix).unwrap_or_else(|e| panic!("round {round}: {e}"));
        let done =
            apply_all(&files, &merged).unwrap_or_else(|| panic!("round {round} does not apply"));
        // what the changes mean, done by hand from the end of the file backwards
        let mut want = text.clone();
        let mut ordered = edits.clone();
        ordered.sort_by_key(|e| text.find(e.old_text.as_str()).unwrap());
        for e in ordered.iter().rev() {
            want = want.replacen(e.old_text.as_str(), &e.new_text, 1);
        }
        assert_eq!(
            String::from_utf8_lossy(&done["r.py"]),
            want,
            "round {round}"
        );
        let diff = unified_diff_all(&files, &merged);
        if want == text {
            continue;
        }
        assert_eq!(
            apply_unified(&text, &diff),
            want,
            "round {round}: the diff is not the change\n{diff}"
        );
    }
}

#[test]
fn f24_a_fix_has_at_most_eight_changes_in_at_most_four_files_and_each_text_is_bounded() {
    let known: BTreeSet<String> = ["a.py", "b.py", "c.py", "d.py", "e.py"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let doc = |n: usize, files: &[&str], size: usize| {
        let also: Vec<serde_json::Value> = (1..n)
            .map(|i| serde_json::json!({"file": files[i % files.len()], "old_text": format!("old {i}"), "new_text": "n".repeat(size)}))
            .collect();
        serde_json::json!({"title": "t", "file": files[0], "old_text": "old 0", "new_text": "n".repeat(size), "also": also}).to_string()
    };
    assert!(
        parse_fix(doc(8, &["a.py", "b.py"], 3).as_bytes(), &known).is_some(),
        "eight changes"
    );
    assert!(
        parse_fix(doc(9, &["a.py", "b.py"], 3).as_bytes(), &known).is_none(),
        "nine changes"
    );
    assert!(
        parse_fix(
            doc(4, &["a.py", "b.py", "c.py", "d.py"], 3).as_bytes(),
            &known
        )
        .is_some(),
        "four files"
    );
    assert!(
        parse_fix(
            doc(5, &["a.py", "b.py", "c.py", "d.py", "e.py"], 3).as_bytes(),
            &known
        )
        .is_none(),
        "five files"
    );
    assert!(
        parse_fix(doc(2, &["a.py"], 8000).as_bytes(), &known).is_some(),
        "8,000 characters of new text"
    );
    assert!(
        parse_fix(doc(2, &["a.py"], 8001).as_bytes(), &known).is_none(),
        "8,001 characters of new text"
    );
    // eight changes that each add the most they may still make one file that grows by 8 x 8,000
    let mut text = String::new();
    for i in 0..8 {
        text.push_str(&format!("marker {i}\nfiller\n"));
    }
    let f = imported("m.py", &text);
    let files = index_of(vec![&f]);
    let edits: Vec<Edit> = (0..8)
        .map(|i| {
            hunk(
                "m.py",
                &format!("marker {i}\n"),
                &format!("{}\n", "x".repeat(7999)),
            )
        })
        .collect();
    let merged = consolidate(&files, fix_of(edits)).expect("joins");
    let done = apply_all(&files, &merged).expect("a file may grow by what eight changes may add");
    assert!(done["m.py"].len() > text.len() + 60_000);
}

#[test]
fn f25_a_very_large_rewrite_still_gets_a_correct_diff() {
    let lines: Vec<String> = (0..6000).map(|i| format!("line {i}")).collect();
    let text = lines.join("\n") + "\n";
    let f = imported("big.py", &text);
    let files = index_of(vec![&f]);
    // 3,000 lines in the middle replaced by 3,000 different ones: too large for the table, shown as one replacement
    let old = lines[1000..4000].join("\n") + "\n";
    let new: String = (0..3000).map(|i| format!("different {i}\n")).collect();
    let fix = fix_of(vec![hunk("big.py", &old, &new)]);
    let diff = unified_diff_all(&files, &fix);
    let want =
        String::from_utf8(apply_all(&files, &fix).unwrap().remove("big.py").unwrap()).unwrap();
    assert_eq!(apply_unified(&text, &diff), want);
    // a change that makes no difference has no diff
    assert_eq!(
        unified_diff(b"a\nb\n", &fix_of(vec![hunk("x", "zzz", "yyy")])),
        ""
    );
}

#[test]
fn f26_changes_that_share_even_one_byte_are_refused_and_ones_that_only_touch_are_not() {
    let f = imported("o.txt", "abcde");
    let files = index_of(vec![&f]);
    assert!(
        consolidate(
            &files,
            fix_of(vec![hunk("o.txt", "abc", "X"), hunk("o.txt", "cde", "Y")])
        )
        .is_err(),
        "they share the c"
    );
    let ok = consolidate(
        &files,
        fix_of(vec![hunk("o.txt", "abc", "X"), hunk("o.txt", "de", "Y")]),
    )
    .expect("they only touch");
    assert_eq!(apply_all(&files, &ok).unwrap()["o.txt"], b"XY");
}

#[test]
fn f27_changes_close_together_share_a_hunk_and_changes_apart_do_not() {
    let lines: Vec<String> = (0..40).map(|i| format!("L{i}")).collect();
    let text = lines.join("\n") + "\n";
    let f = imported("h.txt", &text);
    let files = index_of(vec![&f]);
    let hunks_for = |a: usize, b: usize| {
        let fix = fix_of(vec![
            hunk("h.txt", &format!("L{a}\n"), "A\n"),
            hunk("h.txt", &format!("L{b}\n"), "B\n"),
        ]);
        let merged = consolidate(&files, fix).unwrap();
        let diff = unified_diff_all(&files, &merged);
        let want = String::from_utf8(apply_all(&files, &merged).unwrap().remove("h.txt").unwrap())
            .unwrap();
        assert_eq!(apply_unified(&text, &diff), want, "{a},{b}\n{diff}");
        diff.matches("\n@@ ").count()
    };
    assert_eq!(hunks_for(10, 11), 1);
    assert_eq!(
        hunks_for(10, 17),
        1,
        "six unchanged lines between: the contexts meet"
    );
    assert_eq!(
        hunks_for(10, 18),
        2,
        "seven unchanged lines between: two hunks"
    );
    assert_eq!(hunks_for(3, 35), 2);
}

#[test]
fn f28_when_a_rewrite_is_too_large_to_compare_it_is_shown_whole_and_still_correct() {
    let lines: Vec<String> = (0..6000).map(|i| format!("line {i}")).collect();
    let text = lines.join("\n") + "\n";
    let f = imported("big.py", &text);
    let files = index_of(vec![&f]);
    // every second line of 3,000 stays the same
    let old = lines[1000..4000].join("\n") + "\n";
    let new: String = (1000..4000)
        .map(|i| {
            if i % 2 == 0 {
                format!("line {i}\n")
            } else {
                format!("changed {i}\n")
            }
        })
        .collect();
    let fix = fix_of(vec![hunk("big.py", &old, &new)]);
    let diff = unified_diff_all(&files, &fix);
    let want =
        String::from_utf8(apply_all(&files, &fix).unwrap().remove("big.py").unwrap()).unwrap();
    assert_eq!(apply_unified(&text, &diff), want);
    assert_eq!(
        diff.matches("\n-line").count(),
        2999,
        "shown as one replacement, not as the 1,500 changed lines"
    );
    // a rewrite that is small enough is compared line by line
    let small: Vec<String> = (0..200).map(|i| format!("line {i}")).collect();
    let stext = small.join("\n") + "\n";
    let sf = imported("s.py", &stext);
    let sfiles = index_of(vec![&sf]);
    let sold = small[20..120].join("\n") + "\n";
    let snew: String = (20..120)
        .map(|i| {
            if i % 2 == 0 {
                format!("line {i}\n")
            } else {
                format!("changed {i}\n")
            }
        })
        .collect();
    let sdiff = unified_diff_all(&sfiles, &fix_of(vec![hunk("s.py", &sold, &snew)]));
    assert_eq!(
        sdiff.matches("\n-line").count(),
        50,
        "only the 50 lines that differ: {sdiff}"
    );
}

#[test]
fn f29_a_fix_that_names_a_file_twice_is_never_applied_as_if_it_named_it_once() {
    let f = imported("m.py", "a = 1\nb = 2\n");
    let files = index_of(vec![&f]);
    let unjoined = fix_of(vec![
        hunk("m.py", "a = 1", "a = 9"),
        hunk("m.py", "b = 2", "b = 8"),
    ]);
    assert!(
        apply_all(&files, &unjoined).is_none(),
        "the second change would overwrite the first"
    );
    let joined = consolidate(&files, unjoined).unwrap();
    assert_eq!(
        apply_all(&files, &joined).unwrap()["m.py"],
        b"a = 9\nb = 8\n"
    );
}
