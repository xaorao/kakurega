//! Word documents (`.docx`). What the owner sees in Word is what is read: body paragraphs, tables, headers, footers,
//! footnotes, endnotes and comments. Hidden text and deleted tracked changes are left out and counted.

use crate::archive::{names, open, part, Pack};
use crate::xml::{attr, walk, Ev};
use crate::{text, Ctx, ParseError};

const MAX_EVENTS: usize = 6_000_000;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Body,
    /// A part made of `<footnote>`, `<endnote>` or `<comment>` elements.
    Notes(&'static str),
    /// A header or footer: all of it is one passage with this name.
    Whole(&'static str),
}

#[derive(Default)]
struct Counts {
    hidden_runs: usize,
    deleted: usize,
}

pub(crate) fn read(ctx: &mut Ctx, prefix: &str, bytes: &[u8]) -> Result<(), ParseError> {
    let mut z = open(bytes)?;
    let body = part(ctx, &mut z, "word/document.xml")?
        .ok_or_else(|| ParseError::Damaged("no word/document.xml".into()))?;
    let mut counts = Counts::default();
    run(ctx, prefix, &body, Mode::Body, &mut counts)?;
    for (file, mode) in [
        ("word/footnotes.xml", Mode::Notes("footnote")),
        ("word/endnotes.xml", Mode::Notes("endnote")),
        ("word/comments.xml", Mode::Notes("comment")),
    ] {
        if ctx.truncated {
            break;
        }
        if let Some(b) = part(ctx, &mut z, file)? {
            run(ctx, prefix, &b, mode, &mut counts)?;
        }
    }
    extra_parts(ctx, prefix, &mut z, &mut counts)?;
    if counts.hidden_runs > 0 {
        ctx.note(format!(
            "{} hidden piece(s) of text in the document were left out.",
            counts.hidden_runs
        ));
    }
    if counts.deleted > 0 {
        ctx.note(format!(
            "{} deleted tracked change(s) were left out.",
            counts.deleted
        ));
    }
    Ok(())
}

fn extra_parts(
    ctx: &mut Ctx,
    prefix: &str,
    z: &mut Pack<'_>,
    counts: &mut Counts,
) -> Result<(), ParseError> {
    let all = names(z);
    for (stem, label) in [("word/header", "header"), ("word/footer", "footer")] {
        for n in all
            .iter()
            .filter(|n| n.starts_with(stem) && n.ends_with(".xml"))
        {
            if ctx.truncated {
                return Ok(());
            }
            if let Some(b) = part(ctx, z, n)? {
                run(ctx, prefix, &b, Mode::Whole(label), counts)?;
            }
        }
    }
    Ok(())
}

fn run(
    ctx: &mut Ctx,
    prefix: &str,
    xml: &[u8],
    mode: Mode,
    counts: &mut Counts,
) -> Result<(), ParseError> {
    // paragraphs being written (a text box holds paragraphs inside a paragraph)
    let mut paras: Vec<String> = Vec::new();
    // tables: (table number, current row, current column) per nesting level, and the cell text being collected
    let mut tables: Vec<(usize, usize, usize)> = Vec::new();
    let mut cells: Vec<String> = Vec::new();
    let mut table_no = 0usize;
    let mut para_no = 0usize;
    let mut run_hidden = false;
    let mut in_rpr = false;
    let mut in_del = 0usize;
    let mut in_text = false;
    // for notes: the element being collected
    let mut note: Option<(String, String)> = None;
    let mut whole = String::new();
    let mut room = true;

    let res = walk(xml, MAX_EVENTS, |ev| {
        match ev {
            Ev::Start(n, a) => match n.as_str() {
                "p" => paras.push(String::new()),
                "r" => run_hidden = false,
                "rPr" => in_rpr = true,
                "vanish" | "specVanish" if in_rpr => {
                    let off = matches!(attr(&a, "val"), Some("0" | "false" | "off"));
                    if !off && !run_hidden {
                        run_hidden = true;
                        counts.hidden_runs += 1;
                    }
                }
                "del" | "moveFrom" => {
                    in_del += 1;
                    counts.deleted += 1;
                }
                "t" => in_text = true,
                "tab" | "br" | "cr" | "noBreakHyphen" => {
                    let shown = !run_hidden && in_del == 0;
                    if let (Some(p), true) = (paras.last_mut(), shown) {
                        match n.as_str() {
                            "tab" => p.push('\t'),
                            "noBreakHyphen" => p.push('-'),
                            _ if attr(&a, "type") != Some("page") => p.push('\n'),
                            _ => {}
                        }
                    }
                }
                "tbl" => {
                    table_no += 1;
                    tables.push((table_no, 0, 0));
                    cells.push(String::new());
                }
                "tr" => {
                    if let Some(t) = tables.last_mut() {
                        t.1 += 1;
                        t.2 = 0;
                    }
                }
                "tc" => {
                    if let Some(t) = tables.last_mut() {
                        t.2 += 1;
                    }
                    if let Some(c) = cells.last_mut() {
                        c.clear();
                    }
                }
                "footnote" | "endnote" | "comment" => {
                    if let Mode::Notes(kind) = mode {
                        let id = attr(&a, "id").unwrap_or("?").to_string();
                        // separators and continuation notices carry a type and no text of the author's
                        note = attr(&a, "type")
                            .is_none()
                            .then(|| (format!("{kind} {id}"), String::new()));
                    }
                }
                _ => {}
            },
            Ev::End(n) => match n.as_str() {
                "p" => {
                    let text = text::clean(&paras.pop().unwrap_or_default());
                    if let Some(outer) = paras.last_mut() {
                        // a paragraph inside another (a text box): its text belongs to the outer paragraph
                        outer.push_str(&text);
                        outer.push('\n');
                    } else if !tables.is_empty() {
                        if let Some(c) = cells.last_mut() {
                            if !c.is_empty() {
                                c.push('\n');
                            }
                            c.push_str(&text);
                        }
                    } else {
                        match mode {
                            Mode::Body => {
                                para_no += 1;
                                if !text.trim().is_empty() {
                                    room = ctx.push(prefix, format!("paragraph {para_no}"), text);
                                }
                            }
                            Mode::Notes(_) => {
                                if let Some((_, buf)) = note.as_mut() {
                                    if !buf.is_empty() {
                                        buf.push('\n');
                                    }
                                    buf.push_str(&text);
                                }
                            }
                            Mode::Whole(_) => {
                                if !whole.is_empty() {
                                    whole.push('\n');
                                }
                                whole.push_str(&text);
                            }
                        }
                    }
                }
                "rPr" => in_rpr = false,
                "del" | "moveFrom" => in_del = in_del.saturating_sub(1),
                "t" => in_text = false,
                "tc" => {
                    if let (Some(t), Some(c)) = (tables.last(), cells.last()) {
                        if !c.trim().is_empty() {
                            room = ctx.push(
                                prefix,
                                format!("table {}, row {}, column {}", t.0, t.1, t.2),
                                c.clone(),
                            );
                        }
                    }
                }
                "tbl" => {
                    tables.pop();
                    cells.pop();
                }
                "footnote" | "endnote" | "comment" => {
                    if let Some((anchor, text)) = note.take() {
                        room = ctx.push(prefix, anchor, text);
                    }
                }
                _ => {}
            },
            Ev::Text(t) => {
                if in_text && !run_hidden && in_del == 0 {
                    if let Some(p) = paras.last_mut() {
                        p.push_str(&t);
                    }
                }
            }
        }
        room
    });
    if let Mode::Whole(label) = mode {
        if !whole.trim().is_empty() {
            ctx.push(prefix, label.to_string(), whole);
        }
    }
    res.map_err(|e| ParseError::Damaged(format!("word document: {e}")))
}
