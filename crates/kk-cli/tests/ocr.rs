//! Reading pictures and scanned pages with the real helper through the real reader program: the operating system's own
//! recogniser (macOS Vision), the reader process, the confinement and the fixtures. Fixture pictures are drawn text that was
//! then rotated, blurred and speckled; a photographed paper scan was not available (NOT VERIFIED on one).

#![cfg(target_os = "macos")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Instant;

fn helper() -> &'static Path {
    static H: OnceLock<PathBuf> = OnceLock::new();
    H.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let out = Path::new(env!("CARGO_BIN_EXE_kaku")).with_file_name("kaku-ocr");
        let source = root.join("helpers/ocr/kaku-ocr.swift");
        let stale = std::fs::metadata(&out).and_then(|o| o.modified()).ok()
            < std::fs::metadata(&source).and_then(|s| s.modified()).ok();
        if stale {
            let status = Command::new("sh")
                .arg(root.join("helpers/ocr/build.sh"))
                .arg(&out)
                .stdout(Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "the recognition helper must build");
        }
        out
    })
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ocr")
            .join(name),
    )
    .unwrap()
}

fn run(name: &str, bytes: &[u8], with_helper: bool) -> serde_json::Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kaku"));
    cmd.args(["parse-attachment", "--name", name]);
    if with_helper {
        cmd.arg("--ocr").arg(helper());
    }
    let mut child = cmd
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
    serde_json::from_slice(&out.stdout).unwrap()
}

fn passages(v: &serde_json::Value) -> Vec<(String, String)> {
    v["passages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["anchor"].as_str().unwrap().to_string(),
                p["text"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn notes(v: &serde_json::Value) -> String {
    v["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn pa04_a_rotated_speckled_picture_is_read() {
    let v = run("scan.jpg", &fixture("scan.jpg"), true);
    let p = passages(&v);
    assert_eq!(p.len(), 1, "{v}");
    assert_eq!(p[0].0, "recognised text");
    for needle in [
        "Master Services Agreement",
        "Invoice 4471",
        "1,280.00 EUR",
        "Priya Natarajan",
        "Acme Audit Ltd",
    ] {
        assert!(
            p[0].1.contains(needle),
            "missing {needle:?} in {:?}",
            p[0].1
        );
    }
    assert!(notes(&v).contains("may contain mistakes"));
}

#[test]
fn pa05_a_pdf_that_is_only_a_scan_is_read_page_by_page() {
    let v = run("scan_only.pdf", &fixture("scan_only.pdf"), true);
    let p = passages(&v);
    assert_eq!(p.len(), 1, "{v}");
    assert_eq!(p[0].0, "page 1 (recognised)");
    assert!(
        p[0].1.contains("Invoice 4471") && p[0].1.contains("Priya Natarajan"),
        "{:?}",
        p[0].1
    );
}

#[test]
fn pa06_a_pdf_with_a_text_layer_page_and_a_scanned_page_keeps_both_in_page_order() {
    let v = run("mixed.pdf", &fixture("mixed.pdf"), true);
    let p = passages(&v);
    assert_eq!(p.len(), 2, "{v}");
    assert_eq!(p[0].0, "page 1");
    assert!(p[0].1.contains("real text layer"));
    assert_eq!(p[1].0, "page 2 (recognised)");
    assert!(
        p[1].1.contains("Delivery on 14 March") && p[1].1.contains("Penalty 2 percent"),
        "{:?}",
        p[1].1
    );
}

#[test]
fn pa07_a_picture_bomb_and_a_broken_picture_cost_one_attachment_and_little_time() {
    let started = Instant::now();
    let bomb = run("bomb.png", &fixture("bomb.png"), true);
    assert!(passages(&bomb).is_empty());
    assert!(
        notes(&bomb).contains("too large to recognise"),
        "{}",
        notes(&bomb)
    );
    let junk = run("junk.png", &fixture("junk.png"), true);
    assert!(passages(&junk).is_empty());
    assert!(
        notes(&junk).contains("could not be read"),
        "{}",
        notes(&junk)
    );
    assert!(started.elapsed().as_secs() < 20, "{:?}", started.elapsed());
}

#[test]
fn pa08_without_the_helper_or_with_a_wrong_path_a_picture_is_named_not_read() {
    let v = run("scan.jpg", &fixture("scan.jpg"), false);
    assert!(passages(&v).is_empty());
    assert!(
        notes(&v).contains("no text recognition on this computer yet"),
        "{}",
        notes(&v)
    );
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kaku"));
    cmd.args([
        "parse-attachment",
        "--name",
        "scan.jpg",
        "--ocr",
        "/nonexistent/kaku-ocr",
    ]);
    let mut child = cmd
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&fixture("scan.jpg"))
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(notes(&v).contains("no text recognition on this computer yet"));
}

#[test]
fn pa09_a_picture_that_tells_the_assistant_what_to_do_is_read_as_data_with_the_words_intact() {
    // the reader only reads; leaving instruction-like lines out is the kernel's job at the next step (tested in the service)
    let v = run("inject.png", &fixture("inject.png"), true);
    let p = passages(&v);
    assert!(
        p[0].1
            .to_lowercase()
            .contains("ignore all previous instructions"),
        "{:?}",
        p[0].1
    );
}

#[test]
fn pa10_a_scanned_first_page_stays_first_when_the_second_page_has_a_text_layer() {
    let v = run("scan_then_text.pdf", &fixture("scan_then_text.pdf"), true);
    let p = passages(&v);
    assert_eq!(p.len(), 2, "{v}");
    assert_eq!(p[0].0, "page 1 (recognised)");
    assert!(
        p[0].1.contains("Termination notice 60 days"),
        "{:?}",
        p[0].1
    );
    assert_eq!(p[1].0, "page 2");
    assert!(p[1].1.contains("real text layer"));
}
