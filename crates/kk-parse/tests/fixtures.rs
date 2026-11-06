//! The same 95 items the offline-parsing experiment (X8) was scored on, read by this crate from files made by independent
//! tools (python-docx, openpyxl, reportlab): every sentence and table cell must be found, with the right anchor.

use kk_parse::{parse, Kind, Limits, Parsed};
use serde_json::Value;
use std::path::Path;

fn load(name: &str) -> Parsed {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let bytes = std::fs::read(&p).unwrap();
    parse(name, &bytes, &Limits::default()).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn col(n: usize) -> String {
    ((b'A' + n as u8) as char).to_string()
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn fx01_every_sentence_and_cell_of_the_experiment_is_found_with_its_anchor() {
    let truth: Vec<Value> = serde_json::from_slice(
        &std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/truth.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(truth.len(), 95);
    let mut found = 0;
    let mut missed = Vec::new();
    for item in &truth {
        let doc = item["doc"].as_str().unwrap();
        let parsed = load(doc);
        let want = item["text"].as_str().unwrap();
        let ok = match (doc, item["kind"].as_str().unwrap()) {
            ("report.pdf" | "small_table.pdf", _) => {
                let page = item["page"].as_u64().unwrap();
                parsed
                    .passages
                    .iter()
                    .any(|p| p.anchor == format!("page {page}") && squash(&p.text).contains(want))
            }
            ("contract.docx", "text") => parsed
                .passages
                .iter()
                .any(|p| p.anchor.starts_with("paragraph") && squash(&p.text) == want),
            ("contract.docx", _) => {
                let (r, c) = (item["row"].as_u64().unwrap(), item["col"].as_u64().unwrap());
                parsed.passages.iter().any(|p| {
                    p.anchor == format!("table 1, row {}, column {}", r + 1, c + 1)
                        && p.text == want
                })
            }
            ("figures.xlsx", _) => {
                let sheet = item["sheet"].as_str().unwrap();
                let (r, c) = (
                    item["row"].as_u64().unwrap() as usize,
                    item["col"].as_u64().unwrap() as usize,
                );
                let cell = format!("{}{}: {}", col(c), r + 1, want);
                parsed.passages.iter().any(|p| {
                    p.anchor.starts_with(&format!("{sheet}!"))
                        && p.text
                            .lines()
                            .flat_map(|l| l.split(" | "))
                            .any(|x| x.trim() == cell)
                })
            }
            ("page.html", _) => parsed
                .passages
                .iter()
                .any(|p| squash(&p.text).contains(want)),
            other => panic!("unexpected {other:?}"),
        };
        if ok {
            found += 1;
        } else {
            missed.push(item.to_string());
        }
    }
    assert_eq!(found, 95, "missed: {missed:#?}");
}

#[test]
fn fx02_kinds_are_known_from_the_bytes_and_nothing_is_left_unsaid() {
    assert_eq!(load("report.pdf").kind, Kind::Pdf);
    assert_eq!(load("contract.docx").kind, Kind::Word);
    assert_eq!(load("figures.xlsx").kind, Kind::Excel);
    assert_eq!(load("page.html").kind, Kind::Html);
    // a file named for one kind and holding another is read as what it holds
    let bytes =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/report.pdf"))
            .unwrap();
    assert_eq!(
        parse("notes.txt", &bytes, &Limits::default()).unwrap().kind,
        Kind::Pdf
    );
}

#[test]
fn fx03_the_rendered_text_shows_each_passage_under_its_anchor() {
    let t = load("figures.xlsx").render();
    assert!(
        t.contains(
            "[Q1!A1:C4]\nA1: Region | B1: Revenue | C1: Cost\nA2: North | B2: 1200 | C2: 800"
        ),
        "{t}"
    );
    assert!(t.contains("[Q2!A1:C3]"), "{t}");
    let p = load("report.pdf").render();
    assert!(p.contains("[page 1]") && p.contains("[page 3]"), "{p}");
}

#[test]
fn fx04_a_reading_and_an_error_survive_the_trip_through_json() {
    let p = load("figures.xlsx");
    let back = Parsed::from_json(&p.to_json()).unwrap();
    assert_eq!(
        (back.name.as_str(), back.kind, back.truncated),
        (p.name.as_str(), p.kind, p.truncated)
    );
    assert_eq!(back.render(), p.render());
    assert_eq!(back.notes, p.notes);
    for e in [
        kk_parse::ParseError::TooLarge,
        kk_parse::ParseError::Empty,
        kk_parse::ParseError::Unsupported("x".into()),
        kk_parse::ParseError::Damaged("y".into()),
        kk_parse::ParseError::Protected,
        kk_parse::ParseError::Bomb("z".into()),
        kk_parse::ParseError::BadName,
    ] {
        assert_eq!(kk_parse::ParseError::from_json(&e.to_json()), Some(e));
    }
    assert!(Parsed::from_json(&serde_json::json!({"name": "a"})).is_none());
    assert!(Parsed::from_json(&serde_json::json!({"name": "a", "kind": "nope", "passages": [], "notes": [], "truncated": false})).is_none());
    assert!(
        kk_parse::ParseError::from_json(&serde_json::json!({"error": {"code": "nope"}})).is_none()
    );
}
