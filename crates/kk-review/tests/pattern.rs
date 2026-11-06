use kk_review::import::ImportedFile;
use kk_review::pattern::{from_setting, scan, PatternMatch, PatternScan};
use kk_types::label::{Label, Level};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn file(path: &str, text: &str) -> ImportedFile {
    let bytes = text.as_bytes().to_vec();
    ImportedFile {
        path: path.into(),
        sha256: kk_evidence::capsule::sha256_hex(&bytes),
        bytes,
        label: Label::new(Level::Internal),
        notes: vec![],
    }
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kk-pattern-test-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A stand-in for semgrep: a shell script with the given body.
fn fake(d: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = d.join("fake-semgrep");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn leftovers(d: &Path) -> usize {
    std::fs::read_dir(d)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("kk-pattern-"))
        .count()
}

#[test]
fn pp01_semgrep_is_run_offline_in_a_clean_folder_with_our_rules_and_only_the_files_it_was_given() {
    let d = dir("probe");
    let probe = d.join("probe.txt");
    std::env::set_var("KK_PATTERN_TEST_SECRET", "hunter2");
    let body = format!(
        "{{ echo ARGS \"$@\"; echo @@ENV; env | sort; echo @@PWD; pwd; echo @@FILES; find . -type f | sort; echo @@IGNORE; wc -c < .semgrepignore; }} > '{}'\nprintf '{{\"results\":[{{\"check_id\":\"x.kk-py-eval-exec\",\"path\":\"a.py\",\"start\":{{\"line\":2}}}}]}}'",
        probe.display()
    );
    let sg = fake(&d, &body);
    let files = [
        file("a.py", "x\neval(y)\n"),
        file("sub/b.js", "1"),
        file("../escape.py", "evil"),
        file("/abs.py", "evil"),
        file(".semgrepignore", "*\n"),
    ];
    let refs: Vec<&ImportedFile> = files.iter().collect();
    let r = scan(&sg, &refs, &d, Duration::from_secs(20));
    let PatternScan::Ran {
        matches, files: n, ..
    } = r
    else {
        panic!("{r:?}")
    };
    assert_eq!(
        matches,
        vec![PatternMatch {
            rule: "kk-py-eval-exec".into(),
            file: "a.py".into(),
            line: 2
        }]
    );
    assert_eq!(n, 3, "the two unsafe paths were never given");
    let seen = std::fs::read_to_string(&probe).unwrap();
    for must in [
        "--metrics=off",
        "--disable-version-check",
        "--disable-nosem",
        "--no-git-ignore",
        "--json",
        "--config",
        "rules.yml",
    ] {
        assert!(seen.contains(must), "missing {must}: {seen}");
    }
    let env_part = seen
        .split("@@ENV")
        .nth(1)
        .unwrap()
        .split("@@PWD")
        .next()
        .unwrap();
    assert!(
        env_part.contains("SEMGREP_SEND_METRICS=off")
            && env_part.contains("SEMGREP_ENABLE_VERSION_CHECK=0")
    );
    assert!(
        !env_part.contains("hunter2") && !env_part.contains("KK_PATTERN_TEST_SECRET"),
        "the environment was emptied: {env_part}"
    );
    let files_part = seen
        .split("@@FILES")
        .nth(1)
        .unwrap()
        .split("@@IGNORE")
        .next()
        .unwrap();
    assert!(files_part.contains("./a.py") && files_part.contains("./sub/b.js"));
    assert!(!files_part.contains("escape") && !files_part.contains("abs.py"));
    assert!(
        seen.trim_end().ends_with('0'),
        "the project's own ignore file was replaced by an empty one: {seen}"
    );
    assert_eq!(leftovers(&d), 0, "the working folder is removed");
}

#[test]
fn pp02_a_scanner_that_hangs_is_stopped_with_its_helpers_and_reported_as_a_failure() {
    let d = dir("hang");
    let pidfile = d.join("pid");
    let sg = fake(
        &d,
        &format!("sleep 60 &\necho $! > '{}'\nsleep 60", pidfile.display()),
    );
    let t = std::time::Instant::now();
    let r = scan(&sg, &[&file("a.py", "x")], &d, Duration::from_secs(1));
    assert_eq!(
        r,
        PatternScan::Failed("semgrep did not finish in time".into())
    );
    assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
    std::thread::sleep(Duration::from_millis(300));
    let helper: i32 = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let alive = std::process::Command::new("kill")
        .args(["-0", &helper.to_string()])
        .status()
        .unwrap()
        .success();
    assert!(!alive, "the helper process was stopped too");
    assert_eq!(leftovers(&d), 0);
}

#[test]
fn pp03_too_much_output_a_nonzero_exit_and_a_missing_program_are_failures_never_results() {
    let d = dir("fail");
    let flood = fake(&d, "head -c 20000000 /dev/zero");
    assert_eq!(
        scan(&flood, &[&file("a.py", "x")], &d, Duration::from_secs(20)),
        PatternScan::Failed("semgrep wrote more output than is accepted".into())
    );
    let bad_exit = fake(&d, "printf '{\"results\":[]}'; exit 2");
    assert_eq!(
        scan(
            &bad_exit,
            &[&file("a.py", "x")],
            &d,
            Duration::from_secs(20)
        ),
        PatternScan::Failed("semgrep ended with code 2".into())
    );
    let missing = d.join("nope");
    assert!(
        matches!(scan(&missing, &[&file("a.py", "x")], &d, Duration::from_secs(5)), PatternScan::Failed(m) if m.contains("could not be started"))
    );
    let garbage = fake(&d, "printf 'not json'");
    assert!(matches!(
        scan(&garbage, &[&file("a.py", "x")], &d, Duration::from_secs(20)),
        PatternScan::Failed(_)
    ));
    assert_eq!(leftovers(&d), 0);
}

#[test]
fn pp04_big_and_numerous_files_are_bounded_and_nothing_given_is_a_clean_run() {
    let d = dir("bounds");
    let sg = fake(&d, "printf '{\"results\":[]}'");
    let big = file("big.py", &"x".repeat(1_000_001));
    let r = scan(
        &sg,
        &[&big, &file("ok.py", "1")],
        &d,
        Duration::from_secs(20),
    );
    assert!(matches!(r, PatternScan::Ran { files: 1, .. }), "{r:?}");
    let none = scan(&sg, &[], &d, Duration::from_secs(20));
    assert!(matches!(none, PatternScan::Ran { files: 0, ref matches, .. } if matches.is_empty()));
    assert!(from_setting(None).unwrap_err().contains("KAKU_SEMGREP"));
}

/// With the real program: set KAKU_TEST_SEMGREP to the full path of a semgrep to run it. Without it this test says so and
/// does nothing, so a run that includes it is not evidence about Semgrep.
#[test]
fn pp05_with_the_real_semgrep_bad_code_matches_good_code_and_the_fixture_do_not_and_hiding_tricks_fail(
) {
    let Some(path) = std::env::var_os("KAKU_TEST_SEMGREP") else {
        eprintln!("NOT RUN: KAKU_TEST_SEMGREP is not set");
        return;
    };
    let d = dir("real");
    let sg = from_setting(path.to_str()).expect("a usable path");
    let here = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pattern_samples");
    let read = |n: &str| file(n, &std::fs::read_to_string(here.join(n)).unwrap());
    let all = [
        read("bad.py"),
        read("good.py"),
        read("bad.js"),
        read("good.js"),
    ];
    let refs: Vec<&ImportedFile> = all.iter().collect();
    let PatternScan::Ran { matches, files, .. } = scan(&sg, &refs, &d, Duration::from_secs(120))
    else {
        panic!()
    };
    assert_eq!(files, 4);
    let by = |f: &str| matches.iter().filter(|m| m.file == f).count();
    assert_eq!((by("bad.py"), by("bad.js")), (10, 4), "{matches:?}");
    assert_eq!((by("good.py"), by("good.js")), (0, 0), "{matches:?}");
    // the project cannot hide a line with a comment, or a file with its own ignore file
    let sneaky = file("s.py", "import os\nos.system(cmd)  # nosem\n");
    let ignore = file(".semgrepignore", "*\n");
    let PatternScan::Ran { matches, .. } =
        scan(&sg, &[&sneaky, &ignore], &d, Duration::from_secs(120))
    else {
        panic!()
    };
    assert_eq!(matches.len(), 1, "{matches:?}");
    // and a test folder is scanned like any other
    let t = file("tests/test_x.py", "eval(user)\n");
    let PatternScan::Ran { matches, .. } = scan(&sg, &[&t], &d, Duration::from_secs(120)) else {
        panic!()
    };
    assert_eq!(matches.len(), 1);
}
