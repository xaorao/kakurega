//! The reader the service starts for an attached file: the real program, the real fixtures.

use std::io::Write;
use std::process::{Command, Stdio};

fn run(name: &str, bytes: &[u8]) -> serde_json::Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kaku"))
        .args(["parse-attachment", "--name", name])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = bytes.to_vec();
    let w = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let out = child.wait_with_output().unwrap();
    w.join().unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        text.lines().count(),
        1,
        "exactly one line of JSON: {text:.200}"
    );
    serde_json::from_str(&text).unwrap()
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../kk-parse/tests/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn pa01_the_reader_gives_the_same_answer_as_reading_in_process() {
    for f in ["contract.docx", "figures.xlsx", "report.pdf", "page.html"] {
        let bytes = fixture(f);
        let got = run(f, &bytes);
        let here = kk_parse::parse(f, &bytes, &kk_parse::Limits::default()).unwrap();
        assert_eq!(
            kk_parse::Parsed::from_json(&got).unwrap().render(),
            here.render(),
            "{f}"
        );
    }
}

#[test]
fn pa02_a_file_that_cannot_be_read_comes_back_as_its_reason() {
    let e = |name: &str, bytes: &[u8]| kk_parse::ParseError::from_json(&run(name, bytes)).unwrap();
    assert_eq!(e("a.txt", b""), kk_parse::ParseError::Empty);
    assert_eq!(
        e("locked.pdf", &fixture("locked.pdf")),
        kk_parse::ParseError::Protected
    );
    assert_eq!(e("../x", b"hello"), kk_parse::ParseError::BadName);
    assert!(matches!(
        e("b.bin", &[0, 1, 2, 3, 0, 0, 9, 9]),
        kk_parse::ParseError::Unsupported(_)
    ));
}

#[test]
fn pa03_a_file_over_the_limit_is_refused_without_being_read_in_full() {
    let big = vec![b'a'; 9 * 1024 * 1024];
    assert_eq!(
        kk_parse::ParseError::from_json(&run("big.txt", &big)).unwrap(),
        kk_parse::ParseError::TooLarge
    );
}
