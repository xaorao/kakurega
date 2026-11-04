use kk_review::check::{
    clean_text, parse_finding, python_closure, run_reproducer, CheckEnv, CheckStatus, ClosureError,
    Finding,
};
use kk_review::import::{scan_tree, ImportedFile};
use kk_sandbox::Transport;
use kk_types::label::Level;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn fixture_files() -> Vec<ImportedFile> {
    let dst = std::env::temp_dir().join(format!("kk-check-fixture-{}", std::process::id()));
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

const REPRO: &str = "from payments.auth import session\nt = session.login('alice')\nassert session.validate(t)\nsession.logout(t)\nprint('REPRODUCED' if session.validate(t) else 'NOT REPRODUCED')\n";

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
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/check-tmp");
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
