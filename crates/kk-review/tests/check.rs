use kk_review::check::{
    apply_fix, clean_text, parse_finding, parse_fix, python_closure, run_reproducer, unified_diff,
    verify_fix, CheckEnv, CheckStatus, ClosureError, Finding, Fix, FixStatus,
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
            && d.contains("line 2")
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
