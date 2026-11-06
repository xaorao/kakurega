//! PDF files: the text layer, page by page. A page whose text could not be read is named, and a file with no text layer at
//! all (a scan) is said to be one: this program has no text recognition, and never makes up text it did not read.

use crate::{text, Ctx, ParseError};

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
    for &n in pages.iter().take(limit) {
        match doc.extract_text(&[n]) {
            Ok(t) => {
                let cleaned = tidy(&text::clean(&t));
                if !cleaned.trim().is_empty() {
                    with_text += 1;
                    if !ctx.push(prefix, format!("page {n}"), cleaned) {
                        break;
                    }
                }
            }
            Err(_) => unreadable.push(n),
        }
    }
    if !unreadable.is_empty() {
        let shown: Vec<String> = unreadable.iter().take(20).map(|n| n.to_string()).collect();
        ctx.note(format!(
            "The text of page(s) {} could not be read.",
            shown.join(", ")
        ));
    }
    if with_text == 0 && !ctx.truncated {
        ctx.note("The PDF has no text layer: it looks like a scan or a set of pictures. Text recognition is not available on this computer yet, so nothing was read from it.".to_string());
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
