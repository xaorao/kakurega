//! A saved web page: its visible text. Scripts, styles, comments and anything the page hides (the `hidden` attribute,
//! `display:none`, `visibility:hidden`) are left out and counted, so the assistant reads what a person would see.

use crate::{text, Ctx, ParseError};

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];
const BLOCK: &[&str] = &[
    "p",
    "div",
    "br",
    "li",
    "tr",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "table",
    "ul",
    "ol",
    "section",
    "article",
    "header",
    "footer",
    "blockquote",
    "pre",
    "dt",
    "dd",
];

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" | "#39" => Some('\''),
        "nbsp" => Some(' '),
        n if n.starts_with("#x") || n.starts_with("#X") => u32::from_str_radix(&n[2..], 16)
            .ok()
            .and_then(char::from_u32),
        n if n.starts_with('#') => n[1..].parse::<u32>().ok().and_then(char::from_u32),
        _ => None,
    }
}

pub(crate) fn read(ctx: &mut Ctx, prefix: &str, bytes: &[u8]) -> Result<(), ParseError> {
    let src = text::decode(bytes);
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    // while inside an element that is not shown: the tag name being skipped and how deep the same name is nested
    let mut skip: Option<(String, usize)> = None;
    let mut hidden = 0usize;
    while i < chars.len() {
        if chars[i] != '<' {
            let start = i;
            while i < chars.len() && chars[i] != '<' {
                i += 1;
            }
            if skip.is_none() {
                let raw: String = chars[start..i].iter().collect();
                out.push_str(&decode_entities(&raw));
            }
            continue;
        }
        // a comment
        if chars[i..].starts_with(&['<', '!', '-', '-']) {
            let rest: String = chars[i + 4..].iter().take(200_000).collect();
            match rest.find("-->") {
                Some(p) => {
                    i += 4 + rest[..p].chars().count() + 3;
                    if skip.is_none() {
                        hidden += 1;
                    }
                }
                None => break,
            }
            continue;
        }
        // a tag
        let mut j = i + 1;
        let mut quote: Option<char> = None;
        while j < chars.len() {
            match (quote, chars[j]) {
                (Some(q), c) if c == q => quote = None,
                (None, '"') | (None, '\'') => quote = Some(chars[j]),
                (None, '>') => break,
                _ => {}
            }
            j += 1;
        }
        let tag: String = chars[i + 1..j.min(chars.len())].iter().collect();
        i = j + 1;
        let closing = tag.starts_with('/');
        let body = tag.trim_start_matches('/').trim_end_matches('/');
        let name: String = body
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_lowercase();
        if name.is_empty() {
            continue;
        }
        if let Some((skipping, depth)) = skip.as_mut() {
            if &name == skipping {
                if closing {
                    if *depth == 0 {
                        skip = None;
                    } else {
                        *depth -= 1;
                    }
                } else if !VOID.contains(&name.as_str()) && !tag.ends_with('/') {
                    *depth += 1;
                }
            }
            continue;
        }
        if closing {
            if BLOCK.contains(&name.as_str()) {
                out.push('\n');
            }
            continue;
        }
        let lower = body.to_lowercase();
        let compact: String = lower.chars().filter(|c| !c.is_whitespace()).collect();
        let hides = compact.contains("display:none")
            || compact.contains("visibility:hidden")
            || lower
                .split_whitespace()
                .skip(1)
                .any(|a| a == "hidden" || a.starts_with("hidden=") || a == "aria-hidden=\"true\"");
        if matches!(name.as_str(), "script" | "style" | "template" | "noscript") || hides {
            if !VOID.contains(&name.as_str()) && !tag.ends_with('/') {
                skip = Some((name.clone(), 0));
            }
            hidden += 1;
            continue;
        }
        if name == "td" || name == "th" {
            out.push_str(" | ");
        }
        if BLOCK.contains(&name.as_str()) {
            out.push('\n');
        }
    }
    if hidden > 0 {
        ctx.note(format!(
            "{hidden} hidden or non-text element(s) of the page (scripts, styles, comments, hidden content) were left out."
        ));
    }
    let cleaned = text::clean(&out);
    let lines: Vec<String> = cleaned
        .split('\n')
        .map(|l| {
            l.split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .trim_matches(&[' ', '|'][..])
                .trim()
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    text::blocks(ctx, prefix, &refs);
    Ok(())
}

fn decode_entities(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        let tail = &rest[p + 1..];
        match tail.find(';').filter(|&e| e <= 10) {
            Some(e) => match entity(&tail[..e]) {
                Some(c) => {
                    out.push(c);
                    rest = &tail[e + 1..];
                }
                None => {
                    out.push('&');
                    rest = tail;
                }
            },
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}
