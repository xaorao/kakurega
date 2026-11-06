//! Excel workbooks (`.xlsx`). Every non-empty cell is read with its address, so a number can be traced to the cell it came
//! from. Dates are written as dates, long numbers keep every digit, and formulas give the value Excel last calculated.
//! Hidden sheets, hidden rows and hidden columns are left out and counted.

use crate::archive::{open, part};
use crate::xml::{attr, walk, Ev};
use crate::{text, Ctx, ParseError};
use std::collections::HashMap;

const MAX_EVENTS: usize = 12_000_000;
const ROWS_PER_PASSAGE: usize = 25;

pub(crate) fn read(ctx: &mut Ctx, prefix: &str, bytes: &[u8]) -> Result<(), ParseError> {
    let mut z = open(bytes)?;
    let book = part(ctx, &mut z, "xl/workbook.xml")?
        .ok_or_else(|| ParseError::Damaged("no xl/workbook.xml".into()))?;
    let (sheets, date1904) = workbook(&book)?;
    let rels = match part(ctx, &mut z, "xl/_rels/workbook.xml.rels")? {
        Some(b) => relationships(&b)?,
        None => HashMap::new(),
    };
    let shared = match part(ctx, &mut z, "xl/sharedStrings.xml")? {
        Some(b) => shared_strings(&b)?,
        None => Vec::new(),
    };
    let date_styles = match part(ctx, &mut z, "xl/styles.xml")? {
        Some(b) => date_styles(&b)?,
        None => Vec::new(),
    };
    let (mut hidden_sheets, mut hidden_rows, mut hidden_cols, mut read_sheets) =
        (0usize, 0usize, 0usize, 0usize);
    for (i, s) in sheets.iter().enumerate() {
        if s.hidden {
            hidden_sheets += 1;
            continue;
        }
        if read_sheets >= ctx.limits.max_sheets {
            ctx.note(format!(
                "Only the first {} sheets were read.",
                ctx.limits.max_sheets
            ));
            break;
        }
        let target = rels
            .get(&s.rid)
            .cloned()
            .unwrap_or_else(|| format!("worksheets/sheet{}.xml", i + 1));
        let path = match target.strip_prefix('/') {
            Some(abs) => abs.to_string(),
            None => format!("xl/{target}"),
        };
        let Some(xml) = part(ctx, &mut z, &path)? else {
            ctx.note(format!("Sheet {} could not be found in the file.", s.name));
            continue;
        };
        read_sheet(
            ctx,
            prefix,
            &s.name,
            &xml,
            &shared,
            &date_styles,
            date1904,
            &mut hidden_rows,
            &mut hidden_cols,
        )?;
        read_sheets += 1;
        if ctx.truncated {
            break;
        }
    }
    if hidden_sheets > 0 {
        ctx.note(format!("{hidden_sheets} hidden sheet(s) were left out."));
    }
    if hidden_rows > 0 {
        ctx.note(format!("{hidden_rows} hidden row(s) were left out."));
    }
    if hidden_cols > 0 {
        ctx.note(format!("{hidden_cols} hidden column(s) were left out."));
    }
    Ok(())
}

struct Sheet {
    name: String,
    rid: String,
    hidden: bool,
}

fn damaged(what: &str, e: String) -> ParseError {
    ParseError::Damaged(format!("{what}: {e}"))
}

fn workbook(xml: &[u8]) -> Result<(Vec<Sheet>, bool), ParseError> {
    let mut sheets = Vec::new();
    let mut date1904 = false;
    walk(xml, MAX_EVENTS, |ev| {
        if let Ev::Start(n, a) = ev {
            match n.as_str() {
                "workbookPr" => date1904 = matches!(attr(&a, "date1904"), Some("1" | "true")),
                "sheet" => sheets.push(Sheet {
                    name: attr(&a, "name").unwrap_or("").to_string(),
                    rid: attr(&a, "id").unwrap_or("").to_string(),
                    hidden: matches!(attr(&a, "state"), Some("hidden" | "veryHidden")),
                }),
                _ => {}
            }
        }
        true
    })
    .map_err(|e| damaged("workbook", e))?;
    Ok((sheets, date1904))
}

fn relationships(xml: &[u8]) -> Result<HashMap<String, String>, ParseError> {
    let mut map = HashMap::new();
    walk(xml, MAX_EVENTS, |ev| {
        if let Ev::Start(n, a) = ev {
            if n == "Relationship" {
                if let (Some(id), Some(t)) = (attr(&a, "Id"), attr(&a, "Target")) {
                    map.insert(id.to_string(), t.to_string());
                }
            }
        }
        true
    })
    .map_err(|e| damaged("relationships", e))?;
    Ok(map)
}

fn shared_strings(xml: &[u8]) -> Result<Vec<String>, ParseError> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut in_si, mut in_t, mut in_phonetic) = (false, false, 0usize);
    walk(xml, MAX_EVENTS, |ev| {
        match ev {
            Ev::Start(n, _) => match n.as_str() {
                "si" => {
                    in_si = true;
                    cur.clear();
                }
                "rPh" => in_phonetic += 1,
                "t" if in_si && in_phonetic == 0 => in_t = true,
                _ => {}
            },
            Ev::End(n) => match n.as_str() {
                "si" => {
                    in_si = false;
                    out.push(text::clean(&cur));
                }
                "rPh" => in_phonetic = in_phonetic.saturating_sub(1),
                "t" => in_t = false,
                _ => {}
            },
            Ev::Text(t) => {
                if in_t {
                    cur.push_str(&t);
                }
            }
        }
        true
    })
    .map_err(|e| damaged("shared strings", e))?;
    Ok(out)
}

/// For each cell style number, whether it shows a date or time.
fn date_styles(xml: &[u8]) -> Result<Vec<bool>, ParseError> {
    let mut custom: HashMap<u32, bool> = HashMap::new();
    let mut xfs: Vec<bool> = Vec::new();
    let mut in_cell_xfs = false;
    walk(xml, MAX_EVENTS, |ev| {
        match ev {
            Ev::Start(n, a) => match n.as_str() {
                "numFmt" => {
                    if let (Some(id), Some(code)) = (
                        attr(&a, "numFmtId").and_then(|v| v.parse().ok()),
                        attr(&a, "formatCode"),
                    ) {
                        custom.insert(id, code_is_date(code));
                    }
                }
                "cellXfs" => in_cell_xfs = true,
                "xf" if in_cell_xfs => {
                    let id: u32 = attr(&a, "numFmtId")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    xfs.push(
                        custom
                            .get(&id)
                            .copied()
                            .unwrap_or_else(|| builtin_is_date(id)),
                    );
                }
                _ => {}
            },
            Ev::End(n) => {
                if n == "cellXfs" {
                    in_cell_xfs = false;
                }
            }
            Ev::Text(_) => {}
        }
        true
    })
    .map_err(|e| damaged("styles", e))?;
    Ok(xfs)
}

fn builtin_is_date(id: u32) -> bool {
    matches!(id, 14..=22 | 27..=36 | 45..=47 | 50..=58)
}

fn code_is_date(code: &str) -> bool {
    let mut in_quote = false;
    let mut in_bracket = false;
    let mut found = false;
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => in_quote = !in_quote,
            '[' if !in_quote => in_bracket = true,
            ']' if !in_quote => in_bracket = false,
            '\\' | '_' | '*' if !in_quote => {
                chars.next();
            }
            'y' | 'Y' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' | 'm' | 'M'
                if !in_quote && !in_bracket =>
            {
                found = true
            }
            _ => {}
        }
    }
    found
}

/// `A`, `B`, ... `AA` from a column number starting at 1.
fn col_name(mut n: usize) -> String {
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push((b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    s.iter().rev().collect()
}

/// The column number (from 1) and row of an address like `B7`.
fn split_ref(r: &str) -> Option<(usize, usize)> {
    let letters: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let digits = &r[letters.len()..];
    if letters.is_empty() || letters.len() > 3 || digits.is_empty() {
        return None;
    }
    let col = letters
        .to_ascii_uppercase()
        .bytes()
        .fold(0usize, |a, b| a * 26 + (b - b'A' + 1) as usize);
    Some((col, digits.parse().ok()?))
}

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// An Excel serial date as `YYYY-MM-DD`, with the time when the number has a fraction.
fn serial_to_text(serial: f64, date1904: bool) -> Option<String> {
    if !serial.is_finite() || !(0.0..2_958_466.0).contains(&serial) {
        return None;
    }
    let whole = serial.floor() as i64;
    let secs = ((serial - serial.floor()) * 86_400.0).round() as i64;
    // 1900 system: serial 1 is 1900-01-01 and Excel counts a 29 February 1900 that never existed
    let days_from_epoch = if date1904 {
        whole - 24_107
    } else if whole >= 61 {
        whole - 25_569
    } else {
        whole - 25_568
    };
    let (y, m, d) = civil(days_from_epoch);
    let date = format!("{y:04}-{m:02}-{d:02}");
    if secs == 0 {
        Some(date)
    } else if whole == 0 && !date1904 {
        Some(format!(
            "{:02}:{:02}:{:02}",
            secs / 3600,
            secs / 60 % 60,
            secs % 60
        ))
    } else {
        Some(format!(
            "{date} {:02}:{:02}:{:02}",
            secs / 3600,
            secs / 60 % 60,
            secs % 60
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn read_sheet(
    ctx: &mut Ctx,
    prefix: &str,
    name: &str,
    xml: &[u8],
    shared: &[String],
    date_styles: &[bool],
    date1904: bool,
    hidden_rows: &mut usize,
    hidden_cols: &mut usize,
) -> Result<(), ParseError> {
    let quoted = if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    };
    let mut hidden_col_ranges: Vec<(usize, usize)> = Vec::new();
    // the rows waiting to become one passage: (lines, first row, last row, first column, last column)
    let mut lines: Vec<String> = Vec::new();
    let (mut first_row, mut last_row, mut min_col, mut max_col) =
        (0usize, 0usize, usize::MAX, 0usize);
    let mut room = true;
    let mut over_cells = false;

    // the cell being read
    let mut cell: Option<(usize, usize, String, Option<usize>)> = None; // (col, row, type, style)
    let (mut value, mut inline, mut in_v, mut in_is, mut in_t, mut in_rph) =
        (String::new(), String::new(), false, false, false, 0usize);
    let mut row_hidden = false;
    let mut row_cells: Vec<String> = Vec::new();
    let mut row_no = 0usize;
    let mut row_cols: (usize, usize) = (usize::MAX, 0);

    let flush = |ctx: &mut Ctx,
                 lines: &mut Vec<String>,
                 fr: usize,
                 lr: usize,
                 mc: usize,
                 xc: usize,
                 room: &mut bool| {
        if lines.is_empty() {
            return;
        }
        let anchor = format!("{quoted}!{}{}:{}{}", col_name(mc), fr, col_name(xc), lr);
        let body = lines.join("\n");
        lines.clear();
        *room = ctx.push(prefix, anchor, body);
    };

    let res = walk(xml, MAX_EVENTS, |ev| {
        match ev {
            Ev::Start(n, a) => match n.as_str() {
                "col" => {
                    if matches!(attr(&a, "hidden"), Some("1" | "true")) {
                        let lo = attr(&a, "min").and_then(|v| v.parse().ok()).unwrap_or(0);
                        let hi = attr(&a, "max")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(lo)
                            .min(lo + 16_384);
                        if lo > 0 {
                            hidden_col_ranges.push((lo, hi));
                            *hidden_cols += hi - lo + 1;
                        }
                    }
                }
                "row" => {
                    row_no = attr(&a, "r")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(row_no + 1);
                    row_hidden = matches!(attr(&a, "hidden"), Some("1" | "true"));
                    if row_hidden {
                        *hidden_rows += 1;
                    }
                    row_cells.clear();
                    row_cols = (usize::MAX, 0);
                }
                "c" => {
                    let (col, row) = attr(&a, "r").and_then(split_ref).unwrap_or((0, row_no));
                    cell = Some((
                        col,
                        row,
                        attr(&a, "t").unwrap_or("n").to_string(),
                        attr(&a, "s").and_then(|v| v.parse().ok()),
                    ));
                    value.clear();
                    inline.clear();
                }
                "v" => in_v = true,
                "is" => in_is = true,
                "rPh" => in_rph += 1,
                "t" if in_is && in_rph == 0 => in_t = true,
                _ => {}
            },
            Ev::End(n) => match n.as_str() {
                "v" => in_v = false,
                "is" => in_is = false,
                "rPh" => in_rph = in_rph.saturating_sub(1),
                "t" => in_t = false,
                "c" => {
                    if let Some((col, row, ty, style)) = cell.take() {
                        let hidden_col = hidden_col_ranges
                            .iter()
                            .any(|&(lo, hi)| col >= lo && col <= hi);
                        let shown = match ty.as_str() {
                            "s" => value
                                .trim()
                                .parse::<usize>()
                                .ok()
                                .and_then(|i| shared.get(i).cloned()),
                            "inlineStr" => Some(text::clean(&inline)),
                            "str" | "e" => Some(text::clean(&value)),
                            "b" => {
                                Some(if value.trim() == "1" { "TRUE" } else { "FALSE" }.to_string())
                            }
                            _ => {
                                let v = value.trim();
                                if v.is_empty() {
                                    None
                                } else if style
                                    .and_then(|s| date_styles.get(s))
                                    .copied()
                                    .unwrap_or(false)
                                {
                                    v.parse::<f64>()
                                        .ok()
                                        .and_then(|n| serial_to_text(n, date1904))
                                        .or_else(|| Some(v.to_string()))
                                } else {
                                    Some(v.to_string())
                                }
                            }
                        };
                        if let Some(t) = shown.filter(|t| !t.trim().is_empty()) {
                            if !row_hidden && !hidden_col && col > 0 {
                                ctx.cells += 1;
                                if ctx.cells > ctx.limits.max_cells {
                                    over_cells = true;
                                    return false;
                                }
                                row_cells.push(format!(
                                    "{}{}: {}",
                                    col_name(col),
                                    row,
                                    t.replace('\n', " ")
                                ));
                                row_cols = (row_cols.0.min(col), row_cols.1.max(col));
                            }
                        }
                    }
                }
                "row" => {
                    if !row_cells.is_empty() {
                        if lines.is_empty() {
                            first_row = row_no;
                            min_col = usize::MAX;
                            max_col = 0;
                        }
                        lines.push(row_cells.join(" | "));
                        last_row = row_no;
                        min_col = min_col.min(row_cols.0);
                        max_col = max_col.max(row_cols.1);
                        row_cells.clear();
                        if lines.len() >= ROWS_PER_PASSAGE {
                            flush(
                                ctx, &mut lines, first_row, last_row, min_col, max_col, &mut room,
                            );
                        }
                    }
                }
                _ => {}
            },
            Ev::Text(t) => {
                if in_v && cell.is_some() {
                    value.push_str(&t);
                } else if in_t {
                    inline.push_str(&t);
                }
            }
        }
        room
    });
    flush(
        ctx, &mut lines, first_row, last_row, min_col, max_col, &mut room,
    );
    res.map_err(|e| damaged("sheet", e))?;
    if over_cells {
        ctx.truncated = true;
        ctx.note(format!(
            "Only the first {} cells were read; the rest of the workbook was left out.",
            ctx.limits.max_cells
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xl01_columns_and_addresses() {
        assert_eq!(col_name(1), "A");
        assert_eq!(col_name(26), "Z");
        assert_eq!(col_name(27), "AA");
        assert_eq!(col_name(702), "ZZ");
        assert_eq!(col_name(703), "AAA");
        assert_eq!(split_ref("B7"), Some((2, 7)));
        assert_eq!(split_ref("AA10"), Some((27, 10)));
        assert_eq!(split_ref("7"), None);
        assert_eq!(split_ref("B"), None);
    }

    #[test]
    fn xl02_serial_dates_follow_excels_own_rules() {
        assert_eq!(serial_to_text(1.0, false).as_deref(), Some("1900-01-01"));
        assert_eq!(serial_to_text(61.0, false).as_deref(), Some("1900-03-01"));
        assert_eq!(
            serial_to_text(25_569.0, false).as_deref(),
            Some("1970-01-01")
        );
        assert_eq!(
            serial_to_text(45_000.0, false).as_deref(),
            Some("2023-03-15")
        );
        assert_eq!(
            serial_to_text(45_000.5, false).as_deref(),
            Some("2023-03-15 12:00:00")
        );
        assert_eq!(serial_to_text(0.25, false).as_deref(), Some("06:00:00"));
        assert_eq!(serial_to_text(0.0, true).as_deref(), Some("1904-01-01"));
        assert_eq!(serial_to_text(-1.0, false), None);
        assert_eq!(serial_to_text(f64::NAN, false), None);
        assert_eq!(serial_to_text(1e12, false), None);
    }

    #[test]
    fn xl03_a_format_is_a_date_by_its_letters_outside_quotes_and_brackets() {
        for yes in [
            "yyyy-mm-dd",
            "dd/mm/yy",
            "h:mm:ss",
            "[$-409]d mmm yyyy",
            "d\\ mmmm",
        ] {
            assert!(code_is_date(yes), "{yes}");
        }
        for no in ["General", "0.00", "#,##0", "\"day\" 0", "[Red]0.0", "0%"] {
            assert!(!code_is_date(no), "{no}");
        }
        assert!(
            builtin_is_date(14)
                && builtin_is_date(22)
                && !builtin_is_date(0)
                && !builtin_is_date(2)
                && !builtin_is_date(49)
        );
    }
}
