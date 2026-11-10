//! The Rust test runner of coding mode: which projects it takes, and which files count as tests. The run itself is in
//! `code_sandbox.rs` (it needs the Linux test machine).

use kk_review::code::*;
use std::collections::BTreeMap;

fn map(files: &[(&str, &str)]) -> BTreeMap<String, String> {
    files
        .iter()
        .map(|(p, t)| (p.to_string(), t.to_string()))
        .collect()
}

const MANIFEST: &str = "[package]\nname = \"calc\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";
const LIB_WITH_TESTS: &str = "pub fn add(a: i32, b: i32) -> i32 {\n    a - b\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn adds() {\n        assert_eq!(add(1, 2), 3);\n    }\n}\n";
const LIB_PLAIN: &str = "pub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n";

#[test]
fn cr01_a_dependency_free_crate_with_inline_tests_gets_the_rust_runner() {
    let f = map(&[("Cargo.toml", MANIFEST), ("src/lib.rs", LIB_WITH_TESTS)]);
    assert_eq!(detect_runner(&f), Some(Runner::Rust));
    let main = map(&[
        ("Cargo.toml", MANIFEST),
        ("src/main.rs", "fn main() {}\n#[test]\nfn t() {}\n"),
    ]);
    assert_eq!(detect_runner(&main), Some(Runner::Rust));
}

#[test]
fn cr02_a_crate_with_dependencies_or_without_tests_gets_no_runner() {
    let deps = format!("{MANIFEST}\n[dependencies]\nserde = \"1\"\n");
    assert_eq!(
        detect_runner(&map(&[
            ("Cargo.toml", &deps),
            ("src/lib.rs", LIB_WITH_TESTS)
        ])),
        None
    );
    let dev = format!("{MANIFEST}\n[dev-dependencies]\nproptest = \"1\"\n");
    assert_eq!(
        detect_runner(&map(&[
            ("Cargo.toml", &dev),
            ("src/lib.rs", LIB_WITH_TESTS)
        ])),
        None
    );
    assert_eq!(
        detect_runner(&map(&[("Cargo.toml", MANIFEST), ("src/lib.rs", LIB_PLAIN)])),
        None
    );
    // tests with no manifest or no crate root cannot be built
    assert_eq!(detect_runner(&map(&[("src/lib.rs", LIB_WITH_TESTS)])), None);
    assert_eq!(
        detect_runner(&map(&[
            ("Cargo.toml", MANIFEST),
            ("other.rs", LIB_WITH_TESTS)
        ])),
        None
    );
    // an empty dependencies table lists nothing
    let empty = format!("{MANIFEST}\n[dependencies]\n");
    assert_eq!(
        detect_runner(&map(&[
            ("Cargo.toml", &empty),
            ("src/lib.rs", LIB_WITH_TESTS)
        ])),
        Some(Runner::Rust)
    );
}

#[test]
fn cr03_tests_in_a_module_file_count() {
    let f = map(&[
        ("Cargo.toml", MANIFEST),
        ("src/lib.rs", "pub mod math;\n"),
        ("src/math.rs", LIB_WITH_TESTS),
    ]);
    assert_eq!(detect_runner(&f), Some(Runner::Rust));
}

#[test]
fn cr04_python_and_node_tests_still_win() {
    let mut f = map(&[("Cargo.toml", MANIFEST), ("src/lib.rs", LIB_WITH_TESTS)]);
    f.insert("test_app.py".into(), "def test_a():\n    pass\n".into());
    assert_eq!(detect_runner(&f), Some(Runner::Python));
    let mut g = map(&[("Cargo.toml", MANIFEST), ("src/lib.rs", LIB_WITH_TESTS)]);
    g.insert("x.test.js".into(), "".into());
    assert_eq!(detect_runner(&g), Some(Runner::Node));
}

#[test]
fn cr05_a_rust_file_with_inline_tests_is_a_test_file() {
    assert!(is_test_file("src/lib.rs", LIB_WITH_TESTS));
    assert!(!is_test_file("src/lib.rs", LIB_PLAIN));
    assert!(is_test_file("src/m.rs", "#[test]\nfn t() {}\n"));
    // the word in a comment or a string is not a test
    assert!(!is_test_file(
        "src/lib.rs",
        "// #[test]\nfn f() { let _ = \"#[test]\"; }\n"
    ));
    // the rule is for Rust files only
    assert!(!is_test_file("notes.txt", LIB_WITH_TESTS));
    // the path rules for the other runners are unchanged
    assert!(is_test_file("test_app.py", ""));
    assert!(is_test_file("a/test/b.mjs", ""));
    assert!(!is_test_file("tests.py", ""));
}

#[test]
fn cr06_a_patch_that_changes_the_tests_of_a_rust_file_carries_a_warning() {
    use kk_review::import::ImportedFile;
    use kk_types::label::{Label, Level};
    let file = |p: &str, t: &str| ImportedFile {
        path: p.into(),
        bytes: t.as_bytes().to_vec(),
        sha256: kk_evidence::capsule::sha256_hex(t.as_bytes()),
        label: Label::new(Level::Public),
        notes: vec![],
    };
    let (mut ws, _) = Workspace::from_imported(&[
        file("Cargo.toml", MANIFEST),
        file("src/lib.rs", LIB_WITH_TESTS),
        file("src/other.rs", LIB_PLAIN),
    ]);
    ws.call(
        "edit_file",
        &serde_json::json!({"path": "src/lib.rs", "old_text": "assert_eq!(add(1, 2), 3);", "new_text": "assert_eq!(add(1, 2), -1);"}),
    )
    .unwrap();
    let c = concerns(&ws.changes());
    assert!(
        c.iter()
            .any(|x| x.starts_with("src/lib.rs:") && x.contains("test file")),
        "{c:?}"
    );
    let (mut plain, _) = Workspace::from_imported(&[
        file("Cargo.toml", MANIFEST),
        file("src/other.rs", LIB_PLAIN),
    ]);
    plain
        .call(
            "edit_file",
            &serde_json::json!({"path": "src/other.rs", "old_text": "a + b", "new_text": "b + a"}),
        )
        .unwrap();
    assert!(concerns(&plain.changes()).is_empty());
}

#[test]
fn cr07_adding_or_deleting_a_rust_file_with_tests_carries_a_warning() {
    use kk_review::import::ImportedFile;
    use kk_types::label::{Label, Level};
    let file = |p: &str, t: &str| ImportedFile {
        path: p.into(),
        bytes: t.as_bytes().to_vec(),
        sha256: kk_evidence::capsule::sha256_hex(t.as_bytes()),
        label: Label::new(Level::Public),
        notes: vec![],
    };
    let (mut ws, _) = Workspace::from_imported(&[
        file("Cargo.toml", MANIFEST),
        file("src/lib.rs", LIB_PLAIN),
        file("src/old.rs", LIB_WITH_TESTS),
    ]);
    ws.call("delete_file", &serde_json::json!({"path": "src/old.rs"}))
        .unwrap();
    ws.call(
        "create_file",
        &serde_json::json!({"path": "src/new.rs", "content": LIB_WITH_TESTS}),
    )
    .unwrap();
    let c = concerns(&ws.changes());
    assert!(
        c.iter()
            .any(|x| x.starts_with("src/old.rs:") && x.contains("deletes")),
        "{c:?}"
    );
    assert!(
        c.iter()
            .any(|x| x.starts_with("src/new.rs:") && x.contains("is new")),
        "{c:?}"
    );
}
