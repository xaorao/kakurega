//! Pictures and scanned pages when a recogniser is available: what is read, how it is labelled, and what a recogniser that
//! lies, fails or says too much can and cannot do. The engine is a stand-in here; the real one is tested in kk-cli.

use kk_parse::{parse, parse_with, Limits, Medium, Recognised, Recogniser};
use std::cell::RefCell;
use std::path::Path;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n and some more bytes that only need to look like a picture";

struct Fixed {
    answer: Result<Vec<Recognised>, String>,
    asked: RefCell<Vec<(Medium, Vec<u32>)>>,
}

impl Fixed {
    fn new(answer: Result<Vec<Recognised>, String>) -> Self {
        Fixed {
            answer,
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl Recogniser for Fixed {
    fn recognise(
        &self,
        medium: Medium,
        _bytes: &[u8],
        pages: &[u32],
    ) -> Result<Vec<Recognised>, String> {
        self.asked.borrow_mut().push((medium, pages.to_vec()));
        self.answer.clone()
    }
}

fn page(n: u32, text: &str) -> Recognised {
    Recognised {
        page: n,
        text: text.into(),
        confidence: 0.9,
    }
}

#[test]
fn rc01_a_picture_is_read_when_a_recogniser_exists_and_says_so() {
    let r = Fixed::new(Ok(vec![page(1, "Invoice 4471 total 1,280.00")]));
    let p = parse_with("scan.png", PNG, &Limits::default(), Some(&r)).unwrap();
    assert_eq!(p.passages.len(), 1);
    assert_eq!(p.passages[0].anchor, "recognised text");
    assert_eq!(p.passages[0].text, "Invoice 4471 total 1,280.00");
    assert!(
        p.notes
            .iter()
            .any(|n| n.contains("recognised from a picture") && n.contains("mistakes")),
        "{:?}",
        p.notes
    );
    assert_eq!(r.asked.borrow().as_slice(), &[(Medium::Picture, vec![])]);
}

#[test]
fn rc02_without_a_recogniser_nothing_changes() {
    let p = parse("scan.png", PNG, &Limits::default()).unwrap();
    assert!(p.passages.is_empty());
    assert!(
        p.notes
            .iter()
            .any(|n| n.contains("no text recognition on this computer yet")),
        "{:?}",
        p.notes
    );
}

#[test]
fn rc03_a_failing_or_silent_recogniser_is_said_in_words_and_invents_nothing() {
    let failing = Fixed::new(Err("the picture could not be read".into()));
    let p = parse_with("scan.png", PNG, &Limits::default(), Some(&failing)).unwrap();
    assert!(p.passages.is_empty());
    assert!(
        p.notes
            .iter()
            .any(|n| n.contains("could not be recognised") && n.contains("could not be read")),
        "{:?}",
        p.notes
    );
    let silent = Fixed::new(Ok(vec![page(1, "  \n \n")]));
    let p = parse_with("scan.png", PNG, &Limits::default(), Some(&silent)).unwrap();
    assert!(p.passages.is_empty());
    assert!(
        p.notes
            .iter()
            .any(|n| n.contains("no text could be recognised")),
        "{:?}",
        p.notes
    );
}

#[test]
fn rc04_a_scanned_pdf_is_recognised_page_by_page_and_the_pages_asked_are_the_blank_ones() {
    let r = Fixed::new(Ok(vec![page(1, "Delivery on 14 March")]));
    let p = parse_with(
        "s.pdf",
        &fixture("shapes_only.pdf"),
        &Limits::default(),
        Some(&r),
    )
    .unwrap();
    assert_eq!(p.passages.len(), 1);
    assert_eq!(p.passages[0].anchor, "page 1 (recognised)");
    assert_eq!(r.asked.borrow().as_slice(), &[(Medium::Pdf, vec![1])]);
    assert!(
        p.notes
            .iter()
            .any(|n| n.contains("no text layer") && n.contains("recognised")),
        "{:?}",
        p.notes
    );
}

#[test]
fn rc05_a_pdf_with_a_text_layer_never_asks_the_recogniser() {
    let r = Fixed::new(Ok(vec![page(1, "should never appear")]));
    let p = parse_with(
        "r.pdf",
        &fixture("report.pdf"),
        &Limits::default(),
        Some(&r),
    )
    .unwrap();
    assert!(!p.passages.is_empty());
    assert!(p
        .passages
        .iter()
        .all(|x| !x.text.contains("should never appear") && !x.anchor.contains("recognised")));
    assert!(r.asked.borrow().is_empty());
}

#[test]
fn rc06_text_for_a_page_that_was_not_asked_for_is_dropped() {
    // a recogniser that answers for pages it was not asked about (the text layer page, or one that does not exist)
    let r = Fixed::new(Ok(vec![
        page(1, "asked"),
        page(7, "not asked"),
        page(0, "page zero"),
    ]));
    let p = parse_with(
        "s.pdf",
        &fixture("shapes_only.pdf"),
        &Limits::default(),
        Some(&r),
    )
    .unwrap();
    let all: Vec<&str> = p.passages.iter().map(|x| x.text.as_str()).collect();
    assert_eq!(all, vec!["asked"]);
}

#[test]
fn rc07_recognised_text_is_cleaned_like_any_other_and_cut_at_the_character_budget() {
    let r = Fixed::new(Ok(vec![page(
        1,
        &format!("ok\u{202e}evil\u{0}{}", "x".repeat(500)),
    )]));
    let limits = Limits {
        max_chars: 100,
        ..Limits::default()
    };
    let p = parse_with("scan.png", PNG, &limits, Some(&r)).unwrap();
    assert!(p.truncated);
    let text = &p.passages[0].text;
    assert!(text.chars().count() <= 100, "{}", text.chars().count());
    assert!(
        !text.contains('\u{202e}') && !text.contains('\u{0}'),
        "{text:?}"
    );
}

#[test]
fn rc08_a_picture_inside_an_archive_is_recognised_under_its_own_name() {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(&mut buf);
        z.start_file("scans/page1.png", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(PNG).unwrap();
        z.finish().unwrap();
    }
    let r = Fixed::new(Ok(vec![page(1, "inside the archive")]));
    let p = parse_with(
        "bundle.zip",
        &buf.into_inner(),
        &Limits::default(),
        Some(&r),
    )
    .unwrap();
    assert_eq!(p.passages.len(), 1);
    assert!(
        p.passages[0].anchor.contains("scans/page1.png"),
        "{}",
        p.passages[0].anchor
    );
}
