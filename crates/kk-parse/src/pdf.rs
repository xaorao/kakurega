//! PDF files: the text layer, page by page. A page whose text could not be read is named, and a file with no text layer at
//! all (a scan) is said to be one: this program has no text recognition, and never makes up text it did not read.

use crate::{text, Ctx, Medium, ParseError};

pub(crate) fn read(ctx: &mut Ctx, prefix: &str, bytes: &[u8]) -> Result<(), ParseError> {
    let doc =
        lopdf::Document::load_mem(bytes).map_err(|e| ParseError::Damaged(format!("pdf: {e}")))?;
    // a file locked only against copying opens with the empty password and is already decrypted on load; one that
    // still reads as encrypted needs a real password, which this program never guesses
    if doc.is_encrypted() {
        return Err(ParseError::Protected);
    }
    let pages: Vec<u32> = doc.get_pages().keys().copied().collect();
    if pages.is_empty() {
        return Err(ParseError::Damaged("pdf: no pages".into()));
    }
    let limit = ctx.limits.max_pages;
    if pages.len() > limit {
        ctx.note(format!(
            "The PDF has {} pages; only the first {limit} were read.",
            pages.len()
        ));
    }
    let (mut with_text, mut unreadable) = (0usize, Vec::new());
    let (mut found, mut blank): (Vec<(u32, bool, String)>, Vec<u32>) = (Vec::new(), Vec::new());
    for &n in pages.iter().take(limit) {
        match doc.extract_text(&[n]) {
            Ok(t) => {
                let cleaned = tidy(&text::clean(&t));
                if cleaned.trim().is_empty() {
                    blank.push(n);
                } else {
                    with_text += 1;
                    found.push((n, false, cleaned));
                }
            }
            Err(_) => unreadable.push(n),
        }
    }
    // pages with no text layer are scans or pictures: they are recognised when this computer can, otherwise said to be unread
    let mut recognised = 0usize;
    if let (Some(ocr), false) = (ctx.ocr, blank.is_empty()) {
        match ocr.recognise(Medium::Pdf, bytes, &blank) {
            Ok(list) => {
                for r in list {
                    if blank.contains(&r.page) {
                        let t = text::clean(&r.text);
                        if !t.trim().is_empty() {
                            recognised += 1;
                            found.push((r.page, true, t));
                        }
                    }
                }
                if recognised > 0 {
                    ctx.note(format!(
                        "{recognised} page(s) had no text layer; their text was recognised from the page picture by this computer and may contain mistakes."
                    ));
                }
            }
            Err(why) => ctx.note(format!(
                "Text could not be recognised on the pages without a text layer ({why})."
            )),
        }
    }
    found.sort_by_key(|(n, _, _)| *n);
    for (n, was_recognised, body) in found {
        let anchor = if was_recognised {
            format!("page {n} (recognised)")
        } else {
            format!("page {n}")
        };
        if !ctx.push(prefix, anchor, body) {
            break;
        }
    }
    if !unreadable.is_empty() {
        let shown: Vec<String> = unreadable.iter().take(20).map(|n| n.to_string()).collect();
        ctx.note(format!(
            "The text of page(s) {} could not be read.",
            shown.join(", ")
        ));
    }
    if with_text == 0 && recognised == 0 && !ctx.truncated {
        ctx.note(if ctx.ocr.is_some() {
            "The PDF has no text layer: it looks like a scan or a set of pictures, and no text could be recognised in it.".to_string()
        } else {
            "The PDF has no text layer: it looks like a scan or a set of pictures. Text recognition is not available on this computer yet, so nothing was read from it.".to_string()
        });
    }
    Ok(())
}

/// Collapses runs of blank lines and trailing spaces the extractor leaves behind.
fn tidy(s: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in s.lines() {
        let l = line.trim_end();
        if l.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(l);
        out.push('\n');
    }
    out.trim().to_string()
}
