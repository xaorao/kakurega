//! Rust reproducers in the real sandbox. Run by `scripts/test-lima` (they need Linux, bubblewrap and `rustc` and `gcc` under /usr).

use kk_review::check::{
    msg, run_project_tests_of, run_reproducer, verify_fix, verify_fix_with_tests, CheckEnv,
    CheckResult, CheckStatus, Finding, Fix, FixStatus, Language, TestRun,
};
use kk_review::import::{scan_tree, ImportedFile};
use kk_sandbox::Transport;
use kk_types::label::Level;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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
        std::env::temp_dir().join("kk-rust-scratch")
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/rust-check-tmp")
    };
    fs::create_dir_all(&d).unwrap();
    d
}

fn index(files: &[ImportedFile]) -> BTreeMap<String, &ImportedFile> {
    files.iter().map(|f| (f.path.clone(), f)).collect()
}

const CARGO: &str =
    "[package]\nname = \"payments-auth\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

/// The session module: `logout` forgets the caller's copy and leaves the token valid. Three unit tests live in the same file.
const SESSION: &str = r#"use super::tokens::make;
use std::cell::RefCell;
use std::collections::HashSet;

thread_local! {
    static ACTIVE: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

pub fn login(user: &str) -> String {
    let token = make(user);
    ACTIVE.with(|a| a.borrow_mut().insert(token.clone()));
    token
}

// logout only drops the caller's copy, so the token stays valid
pub fn logout(token: &str) {
    let _copy = token.to_string();
}

pub fn validate(token: &str) -> bool {
    ACTIVE.with(|a| a.borrow().contains(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_gives_a_valid_token() {
        assert!(validate(&login("a")));
    }

    #[test]
    fn an_unknown_token_is_not_valid() {
        assert!(!validate("nope"));
    }

    #[test]
    fn validating_twice_gives_the_same_answer() {
        let t = login("b");
        assert_eq!(validate(&t), validate(&t));
    }
}
"#;

const TOKENS: &str = "use std::sync::atomic::{AtomicU64, Ordering};\nstatic N: AtomicU64 = AtomicU64::new(0);\npub fn make(user: &str) -> String {\n    format!(\"{}-{}\", user, N.fetch_add(1, Ordering::SeqCst) + 1)\n}\n";

fn project(session: &str) -> Vec<ImportedFile> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dst = std::env::temp_dir().join(format!(
        "kk-rs-fixture-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dst);
    fs::create_dir_all(dst.join("src/auth")).unwrap();
    fs::create_dir_all(dst.join("src/util")).unwrap();
    fs::write(dst.join("Cargo.toml"), CARGO).unwrap();
    fs::write(dst.join("src/lib.rs"), "pub mod auth;\npub mod util;\n").unwrap();
    fs::write(
        dst.join("src/auth/mod.rs"),
        "pub mod session;\npub mod tokens;\n",
    )
    .unwrap();
    fs::write(dst.join("src/auth/session.rs"), session).unwrap();
    fs::write(dst.join("src/auth/tokens.rs"), TOKENS).unwrap();
    fs::write(
        dst.join("src/util/mod.rs"),
        "pub fn retry(n: u32) -> u32 { 2u32.pow(n) }\n",
    )
    .unwrap();
    fs::write(dst.join("src/unused.rs"), "pub fn never() {}\n").unwrap();
    fs::write(dst.join(".env"), "SECRET=1\n").unwrap();
    scan_tree(&dst, Level::Internal).files
}

const REPRO: &str = "use project::auth::session::{login, logout, validate};\nfn main() {\n    let t = login(\"alice\");\n    if !validate(&t) {\n        panic!(\"control\");\n    }\n    println!(\"CONTROL_OK\");\n    logout(&t);\n    println!(\"{}\", if validate(&t) { \"REPRODUCED\" } else { \"NOT REPRODUCED\" });\n}\n";

fn finding(code: &str) -> Finding {
    Finding {
        title: "Token not revoked".into(),
        file: "src/auth/session.rs".into(),
        line: 16,
        explanation: "logout leaves the token valid".into(),
        reproducer: code.into(),
        language: Language::Rust,
    }
}

fn env<'a>(t: &'a Transport, scratch: &'a Path) -> CheckEnv<'a> {
    CheckEnv {
        transport: t,
        scratch,
        python: "/usr/bin/python3",
        node: "/usr/bin/node",
        rustc: "/usr/bin/rustc",
    }
}

/// A script that prints the two required lines and then the verdict `v`.
fn says(v: &str) -> String {
    format!("fn main() {{\n    println!(\"CONTROL_OK\");\n    println!(\"{v}\");\n}}\n")
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn c31_a_real_rust_defect_is_built_reproduced_and_the_sample_is_exactly_the_module_tree() {
    let files = project(SESSION);
    let idx = index(&files);
    let (t, s) = (transport(), scratch());
    let e = env(&t, &s);
    let real = run_reproducer(&finding(REPRO), &idx, &e);
    assert_eq!(real.status, CheckStatus::Reproduced, "{real:?}");
    assert_eq!(real.stdout, "CONTROL_OK\nREPRODUCED\n");
    assert_eq!(
        real.sample_paths,
        vec![
            "src/auth/mod.rs",
            "src/auth/session.rs",
            "src/auth/tokens.rs",
            "src/lib.rs",
            "src/util/mod.rs"
        ],
        "the module tree and nothing else: not the unused file, not the manifest, not .env"
    );
    // the normal behaviour decides: a script that concludes there is no defect is not a confirmation
    let decoy = "use project::auth::session::{login, validate};\nfn main() {\n    let t = login(\"a\");\n    assert!(validate(&t));\n    println!(\"CONTROL_OK\");\n    println!(\"{}\", if validate(\"nope\") { \"REPRODUCED\" } else { \"NOT REPRODUCED\" });\n}\n";
    let r = run_reproducer(&finding(decoy), &idx, &e);
    assert_eq!(r.status, CheckStatus::NotReproduced, "{r:?}");
    // REPRODUCED without CONTROL_OK, and a script that does not use the code at all
    let r = run_reproducer(
        &finding("fn main() { println!(\"REPRODUCED\"); }\n"),
        &idx,
        &e,
    );
    assert!(
        matches!(&r.status, CheckStatus::Failed(m) if m.starts_with(msg::NO_CONTROL)),
        "{r:?}"
    );
    let r = run_reproducer(&finding(&says("REPRODUCED")), &idx, &e);
    assert!(
        matches!(&r.status, CheckStatus::Failed(m) if m.starts_with(msg::INDEPENDENT)),
        "{r:?}"
    );
    // a script whose output changes between runs is not evidence
    let varying = "use std::time::{SystemTime, UNIX_EPOCH};\nfn main() {\n    println!(\"CONTROL_OK\");\n    let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos();\n    println!(\"{n}\");\n    println!(\"REPRODUCED\");\n}\n";
    let r = run_reproducer(&finding(varying), &idx, &e);
    assert_eq!(r.status, CheckStatus::Nondeterministic, "{r:?}");
    // a script that does not compile is the script's fault, and says so without quoting the compiler to the model
    let r = run_reproducer(&finding("fn main() { let x: u32 = \"a\"; }\n"), &idx, &e);
    assert!(
        matches!(&r.status, CheckStatus::Failed(m) if m.starts_with(msg::DOES_NOT_COMPILE)),
        "{r:?}"
    );
    let r = run_reproducer(
        &finding("use project::auth::session::nothing_here;\nfn main() {}\n"),
        &idx,
        &e,
    );
    assert!(
        matches!(&r.status, CheckStatus::Failed(m) if m.starts_with(msg::DOES_NOT_COMPILE)),
        "{r:?}"
    );
    let fb = kk_review::feedback::finding(&r.status);
    assert_eq!(fb["status"], "failed");
    assert!(
        !fb["note"].as_str().unwrap().contains("nothing_here"),
        "{fb}"
    );
}

fn assert_never_reproduced(r: &CheckResult, what: &str) {
    assert_ne!(r.status, CheckStatus::Reproduced, "{what}: {r:?}");
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn c32_hostile_rust_scripts_cannot_reach_the_network_the_project_files_or_the_host() {
    let files = project(SESSION);
    let idx = index(&files);
    let (t, s) = (transport(), scratch());
    let e = env(&t, &s);
    // each script prints REPRODUCED only if the forbidden thing worked
    let outcome = |cond: &str, uses: &str| {
        format!("{uses}\nfn main() {{\n    println!(\"CONTROL_OK\");\n    println!(\"{{}}\", if {cond} {{ \"REPRODUCED\" }} else {{ \"NOT REPRODUCED\" }});\n}}\n")
    };
    let cases = [
        ("a TCP connection", outcome("std::net::TcpStream::connect(\"1.1.1.1:80\").is_ok()", "")),
        ("a UDP socket", outcome("std::net::UdpSocket::bind(\"0.0.0.0:0\").and_then(|s| s.send_to(b\"x\", \"1.1.1.1:53\")).is_ok()", "")),
        ("writing into the project", outcome("std::fs::write(\"src/lib.rs\", \"x\").is_ok()", "")),
        ("creating a file in the project folder", outcome("std::fs::write(\"new.txt\", \"x\").is_ok()", "")),
        ("reading the host's password file", outcome("std::fs::read_to_string(\"/etc/passwd\").is_ok()", "")),
        ("reading the host's home", outcome("std::fs::read_dir(\"/home\").is_ok()", "")),
        ("the unused project file the crate does not reach", outcome("std::fs::read_to_string(\"src/unused.rs\").is_ok()", "")),
        ("the withheld .env file", outcome("std::fs::read_to_string(\".env\").is_ok()", "")),
        ("a path outside the sample", outcome("std::fs::read_to_string(\"../repro/repro.rs\").is_ok()", "")),
    ];
    for (what, code) in cases {
        let r = run_reproducer(&finding(&code), &idx, &e);
        assert_never_reproduced(&r, what);
        if what == "a path outside the sample" {
            // the sandbox shows the script itself at /repro, so this read works; it proves nothing about the project, and is refused
            // as independent of the code it claims to test
            assert!(
                matches!(&r.status, CheckStatus::Failed(m) if m.starts_with(msg::INDEPENDENT)),
                "{what}: {r:?}"
            );
            continue;
        }
        assert_eq!(
            r.status,
            CheckStatus::NotReproduced,
            "{what} should simply not work: {r:?}"
        );
    }
    // a compile-time read of a host file fails to build, so it is the script's mistake and never a confirmation
    let r = run_reproducer(&finding("const S: &str = include_str!(\"/etc/passwd\");\nfn main() { println!(\"CONTROL_OK\"); println!(\"{}\", S.len()); println!(\"REPRODUCED\"); }\n"), &idx, &e);
    assert!(
        matches!(&r.status, CheckStatus::Failed(m) if m.starts_with(msg::DOES_NOT_COMPILE)),
        "{r:?}"
    );
    // a script that names a sandbox path is refused before anything runs
    let r = run_reproducer(
        &finding("fn main() { let _ = std::fs::read_to_string(\"/sample/src/lib.rs\"); }\n"),
        &idx,
        &e,
    );
    assert!(
        matches!(&r.status, CheckStatus::Unavailable(m) if m.starts_with(msg::SANDBOX_PATH)),
        "{r:?}"
    );
    // a panic, an abort, a process that exits non-zero, and a huge output are failures or unproven, never confirmations
    for (what, code) in [
        ("a panic", "fn main() { println!(\"CONTROL_OK\"); println!(\"REPRODUCED\"); panic!(\"x\"); }\n"),
        ("a non-zero exit", "fn main() { println!(\"CONTROL_OK\"); println!(\"REPRODUCED\"); std::process::exit(3); }\n"),
        ("an abort", "fn main() { println!(\"CONTROL_OK\"); println!(\"REPRODUCED\"); std::process::abort(); }\n"),
    ] {
        let r = run_reproducer(&finding(code), &idx, &e);
        assert!(matches!(&r.status, CheckStatus::Failed(m) if m.starts_with(msg::ENDED)), "{what}: {r:?}");
    }
    // 3 MB of output pushes the verdict out of what is kept, so there is nothing to confirm
    let big = "fn main() {\n    println!(\"CONTROL_OK\");\n    for _ in 0..30000 {\n        println!(\"{}\", \"x\".repeat(100));\n    }\n    println!(\"REPRODUCED\");\n}\n";
    let r = run_reproducer(&finding(big), &idx, &e);
    assert_never_reproduced(&r, "a huge output");
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn c33_a_script_that_runs_forever_or_builds_forever_is_stopped_and_never_confirmed() {
    let files = project(SESSION);
    let idx = index(&files);
    let (t, s) = (transport(), scratch());
    let e = env(&t, &s);
    let spin = "fn main() {\n    println!(\"CONTROL_OK\");\n    println!(\"REPRODUCED\");\n    let mut x = 0u64;\n    loop {\n        x = std::hint::black_box(x.wrapping_add(1));\n    }\n}\n";
    let r = run_reproducer(&finding(spin), &idx, &e);
    assert!(
        matches!(&r.status, CheckStatus::Failed(m) if m.starts_with(msg::ENDED)),
        "{r:?}"
    );
    // a compile-time computation that never ends is stopped by the same limits
    let const_loop = "const fn spin() -> u64 {\n    let mut x = 0u64;\n    let mut i = 0u64;\n    while i < 100_000_000_000 {\n        x = x.wrapping_add(i);\n        i += 1;\n    }\n    x\n}\nconst V: u64 = spin();\nfn main() { println!(\"CONTROL_OK\"); println!(\"REPRODUCED\"); println!(\"{V}\"); }\n";
    let r = run_reproducer(&finding(const_loop), &idx, &e);
    assert_never_reproduced(&r, "an endless compile-time loop");
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn c34_what_the_environment_cannot_build_is_unavailable_with_its_reason_and_never_failed_or_reproduced(
) {
    let (t, s) = (transport(), scratch());
    let e = env(&t, &s);
    let check = |files: Vec<ImportedFile>| run_reproducer(&finding(REPRO), &index(&files), &e);
    let with = |edit: &dyn Fn(&mut Vec<ImportedFile>)| {
        let mut f = project(SESSION);
        edit(&mut f);
        f
    };
    let set = |f: &mut Vec<ImportedFile>, path: &str, text: &str, level: Level| {
        let bytes = text.as_bytes().to_vec();
        let new = ImportedFile {
            path: path.into(),
            sha256: kk_evidence::capsule::sha256_hex(&bytes),
            bytes,
            label: kk_types::label::Label::new(level),
            notes: vec![],
        };
        match f.iter_mut().find(|x| x.path == path) {
            Some(slot) => *slot = new,
            None => f.push(new),
        }
    };
    let unavailable = |r: &CheckResult, needle: &str| {
        assert!(
            matches!(&r.status, CheckStatus::Unavailable(m) if m.contains(needle)),
            "wanted '{needle}': {r:?}"
        );
        assert!(r.sample_paths.is_empty() || !matches!(r.status, CheckStatus::Reproduced));
    };
    unavailable(
        &check(with(&|f| {
            set(
                f,
                "Cargo.toml",
                &format!("{CARGO}[dependencies]\nserde = \"1\"\n"),
                Level::Internal,
            )
        })),
        "dependencies",
    );
    unavailable(
        &check(with(&|f| {
            set(f, "build.rs", "fn main() {}\n", Level::Internal)
        })),
        "build script",
    );
    unavailable(
        &check(with(&|f| {
            set(
                f,
                "Cargo.toml",
                "[package]\nname = \"payments-auth\"\nedition = \"2030\"\n",
                Level::Internal,
            )
        })),
        "edition",
    );
    unavailable(
        &check(with(&|f| {
            set(
                f,
                "Cargo.toml",
                &format!("{CARGO}[lib]\nproc-macro = true\n"),
                Level::Internal,
            )
        })),
        "procedural macro",
    );
    unavailable(
        &check(with(&|f| f.retain(|x| x.path != "Cargo.toml"))),
        "Cargo.toml",
    );
    // a crate that has only a program has nothing to link a script against
    unavailable(
        &check(with(&|f| {
            f.retain(|x| x.path != "src/lib.rs");
            set(f, "src/main.rs", "fn main() {}\n", Level::Internal);
        })),
        "only a program",
    );
    // a withheld module in the tree refuses the check and says it is withheld
    let r = check(with(&|f| {
        set(
            f,
            "src/util/mod.rs",
            "pub const K: &str = \"k\";\n",
            Level::Restricted,
        )
    }));
    assert!(
        matches!(&r.status, CheckStatus::Unavailable(m) if m.starts_with(msg::WITHHELD_FILE) && m.contains("src/util/mod.rs")),
        "{r:?}"
    );
    // the project's own crate not compiling (a type error, a feature it needs) is the environment's reason, not the script's
    let r = check(with(&|f| {
        set(
            f,
            "src/util/mod.rs",
            "pub fn retry(n: u32) -> u32 { n + \"x\" }\n",
            Level::Internal,
        )
    }));
    assert!(
        matches!(&r.status, CheckStatus::Unavailable(m) if m.contains("does not build")),
        "{r:?}"
    );
    // a module the crate declares but the project does not have
    let r = check(with(&|f| {
        set(
            f,
            "src/lib.rs",
            "pub mod auth;\npub mod util;\npub mod missing;\n",
            Level::Internal,
        )
    }));
    assert!(
        matches!(&r.status, CheckStatus::Unavailable(m) if m.contains("not in the sample")),
        "{r:?}"
    );
    // an external crate named by the script is a package that is not here
    let files = project(SESSION);
    let r = run_reproducer(&finding("extern crate serde;\nfn main() { println!(\"CONTROL_OK\"); println!(\"REPRODUCED\"); }\n"), &index(&files), &e);
    assert!(
        matches!(&r.status, CheckStatus::Unavailable(m) if m.starts_with(msg::NEEDS_PACKAGE) && m.contains("serde")),
        "{r:?}"
    );
    // the cited file is not part of the crate's modules
    let mut f = finding(REPRO);
    f.file = "src/unused.rs".into();
    let r = run_reproducer(&f, &index(&files), &e);
    assert!(
        matches!(&r.status, CheckStatus::Unavailable(m) if m.contains("module tree")),
        "{r:?}"
    );
    // a compiler that is not there is said, and the other languages are not touched
    let bad = CheckEnv {
        rustc: "/usr/bin/no-such-rustc",
        ..env(&t, &s)
    };
    let r = run_reproducer(&finding(REPRO), &index(&files), &bad);
    assert!(
        matches!(&r.status, CheckStatus::Unavailable(m) if m.contains("cannot build a Rust program")),
        "{r:?}"
    );
}

fn fix(old: &str, new: &str) -> Fix {
    Fix {
        title: "Revoke on logout".into(),
        file: "src/auth/session.rs".into(),
        old_text: old.into(),
        new_text: new.into(),
        also: vec![],
        explanation: "remove the token".into(),
    }
}

const REAL_FIX: (&str, &str) = (
    "// logout only drops the caller's copy, so the token stays valid\npub fn logout(token: &str) {\n    let _copy = token.to_string();\n}",
    "pub fn logout(token: &str) {\n    ACTIVE.with(|a| a.borrow_mut().remove(token));\n}",
);

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn n04_the_crates_own_tests_are_run_and_a_fix_is_held_to_the_reproducer_the_tests_and_the_tests_in_its_own_file(
) {
    let files = project(SESSION);
    let idx = index(&files);
    let (t, s) = (transport(), scratch());
    let e = env(&t, &s);
    let f = finding(REPRO);
    // three unit tests in the same file as the code pass
    assert_eq!(run_project_tests_of(&f, &idx, &e), TestRun::Passed(3));
    let before = run_reproducer(&f, &idx, &e);
    assert_eq!(before.status, CheckStatus::Reproduced, "{before:?}");
    // the real fix: the reproducer stops reproducing and the tests still pass
    let (status, note) = verify_fix_with_tests(&f, &before, &fix(REAL_FIX.0, REAL_FIX.1), &idx, &e);
    assert_eq!(status, FixStatus::FixesTheReproducer, "{note}");
    assert!(note.contains("3 tests passed before and after"), "{note}");
    // a cosmetic change leaves the defect
    let cosmetic = fix(
        "let _copy = token.to_string();",
        "let _copy = token.to_owned();",
    );
    assert_eq!(
        verify_fix(&f, &before, &cosmetic, &idx, &e),
        FixStatus::StillReproduced
    );
    // a fix that makes the defect vanish by breaking another test is not accepted: validating consumes the token
    let breaking = Fix {
        old_text: "ACTIVE.with(|a| a.borrow().contains(token))".into(),
        new_text: "ACTIVE.with(|a| a.borrow_mut().remove(token))".into(),
        ..fix("", "")
    };
    let (status, note) = verify_fix_with_tests(&f, &before, &breaking, &idx, &e);
    assert!(
        matches!(status, FixStatus::BreaksTests(_)),
        "{status:?} {note}"
    );
    // a fix that edits a test in the same file proves nothing, however it is aimed
    let weakens = fix("assert!(!validate(\"nope\"));", "assert!(true);");
    assert!(
        matches!(verify_fix(&f, &before, &weakens, &idx, &e), FixStatus::CouldNotApply(m) if m.starts_with(msg::FIX_TEST_FILE))
    );
    let adds = fix(
        "pub fn validate(token: &str) -> bool {",
        "#[test]\nfn extra() {}\npub fn validate(token: &str) -> bool {",
    );
    assert!(
        matches!(verify_fix(&f, &before, &adds, &idx, &e), FixStatus::CouldNotApply(m) if m.starts_with(msg::FIX_TEST_FILE))
    );
    // a fix that ends the program early is not told apart from a fix, so it is not accepted
    let exits = fix("let _copy = token.to_string();", "std::process::exit(0);");
    assert!(matches!(
        verify_fix(&f, &before, &exits, &idx, &e),
        FixStatus::NotChecked(_) | FixStatus::StillReproduced
    ));
    // a project with no tests has none to run
    let mut none = project(
        &SESSION
            .replace("#[cfg(test)]", "#[allow(dead_code)]")
            .replace("#[test]", ""),
    );
    none.retain(|x| x.path != "x");
    assert_eq!(
        run_project_tests_of(&f, &index(&none), &e),
        TestRun::NoTests
    );
    // a failing test is a failure, not a pass
    let failing = project(&SESSION.replace(
        "assert!(validate(&login(\"a\")));",
        "assert!(!validate(&login(\"a\")));",
    ));
    assert!(
        matches!(run_project_tests_of(&f, &index(&failing), &e), TestRun::Failed(m) if m.contains("failed"))
    );
}

#[test]
#[cfg_attr(not(target_os = "linux"), ignore)]
fn c35_the_crate_is_built_in_the_edition_its_manifest_names_and_the_script_always_in_2021() {
    let (t, s) = (transport(), scratch());
    let e = env(&t, &s);
    // `async` is an ordinary word in the 2015 edition and a keyword in every later one, so this crate builds only as 2015
    let old_style = SESSION.replace(
        "pub fn validate(token: &str) -> bool {",
        "pub fn validate(token: &str) -> bool {\n    let async = 1;\n    let _ = async;",
    );
    let mk = |manifest: &str| {
        let mut f = project(&old_style);
        for x in f.iter_mut() {
            if x.path == "Cargo.toml" {
                x.bytes = manifest.as_bytes().to_vec();
            }
        }
        f
    };
    // no edition key means 2015, as Cargo's reference says: the crate builds, and the 2021 script links against it
    let files = mk("[package]\nname = \"payments-auth\"\n");
    let r = run_reproducer(&finding(REPRO), &index(&files), &e);
    assert_eq!(r.status, CheckStatus::Reproduced, "{r:?}");
    // the same crate declared as 2018 does not build, which is the environment's reason and not the script's
    let files = mk("[package]\nname = \"payments-auth\"\nedition = \"2018\"\n");
    let r = run_reproducer(&finding(REPRO), &index(&files), &e);
    assert!(
        matches!(&r.status, CheckStatus::Unavailable(m) if m.contains("does not build")),
        "{r:?}"
    );
    // and the crate's own tests are built in its edition too
    let files = mk("[package]\nname = \"payments-auth\"\n");
    assert_eq!(
        run_project_tests_of(&finding(REPRO), &index(&files), &e),
        TestRun::Passed(3)
    );
}
