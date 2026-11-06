//! Plain text: decoded the way people expect (UTF-8, UTF-16 with a byte-order mark, then Windows-1252) and cut into blocks
//! of lines, each anchored by its line numbers.

use crate::{Ctx, ParseError};

const BLOCK_LINES: usize = 40;

/// True for bytes that read as text: no NUL bytes and few control characters in the first 8,000 bytes (or valid UTF-16).
pub(crate) fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        return true;
    }
    let head = &bytes[..bytes.len().min(8000)];
    if head.contains(&0) {
        return false;
    }
    let odd = head
        .iter()
        .filter(|&&b| b < 0x20 && !matches!(b, b'\n' | b'\r' | b'\t' | 0x0c | 0x1b))
        .count();
    odd * 100 <= head.len().max(1)
}

/// UTF-16 by byte-order mark, else UTF-8 when valid, else Windows-1252.
pub(crate) fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| windows_1252(b)).collect(),
    }
}

fn windows_1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '\u{20ac}', '\u{81}', '\u{201a}', '\u{192}', '\u{201e}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{2c6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8d}', '\u{17d}',
        '\u{8f}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}',
        '\u{2014}', '\u{2dc}', '\u{2122}', '\u{161}', '\u{203a}', '\u{153}', '\u{9d}', '\u{17e}',
        '\u{178}',
    ];
    match b {
        0x80..=0x9f => HIGH[(b - 0x80) as usize],
        _ => b as char,
    }
}

/// Characters that must not reach a model or a screen from a file: control characters other than line breaks and tabs, and
/// the characters that reorder or hide text.
pub(crate) fn clean(s: &str) -> String {
    s.chars()
        .filter(|&c| {
            (!c.is_control() || matches!(c, '\n' | '\t'))
                && !matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
        })
        .collect()
}

pub(crate) fn read(ctx: &mut Ctx, prefix: &str, bytes: &[u8]) -> Result<(), ParseError> {
    let text = decode(bytes);
    let stripped = clean(&text.replace("\r\n", "\n").replace('\r', "\n"));
    if stripped.chars().count() + 50 < text.chars().count() / 2 {
        return Err(ParseError::Unsupported(
            "it is mostly control characters".into(),
        ));
    }
    let lines: Vec<&str> = stripped.trim_end_matches('\n').split('\n').collect();
    blocks(ctx, prefix, &lines);
    Ok(())
}

/// Adds lines as passages of up to 40 lines each, anchored `lines A-B`.
pub(crate) fn blocks(ctx: &mut Ctx, prefix: &str, lines: &[&str]) {
    let mut start = 0;
    while start < lines.len() {
        let end = (start + BLOCK_LINES).min(lines.len());
        let body = lines[start..end].join("\n");
        let anchor = if end - start == 1 {
            format!("line {}", start + 1)
        } else {
            format!("lines {}-{}", start + 1, end)
        };
        if !ctx.push(prefix, anchor, body) {
            return;
        }
        start = end;
    }
}
