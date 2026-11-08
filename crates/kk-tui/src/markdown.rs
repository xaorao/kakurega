//! A small, safe Markdown reader for answers in the terminal, the same subset and the same rules as the web page's reader
//! (`apps/web/src/markdown.ts`): paragraphs, headings, lists, quotes, fenced code, inline code, bold, italic, and links. It makes a
//! tree of plain data and then styled lines; nothing a model writes can become a control sequence, a hidden link or an image
//! request. Links are never clickable: the text is shown with its whole address next to it, so the words cannot hide where a link
//! goes. Tables are not understood and stay as the text the model wrote.
//!
//! The grammar is checked against the web reader by one shared set of cases (`tests/fixtures/markdown_cases.json`).
//!
//! Codex (Apache-2.0) `tui/src/markdown_render.rs` was read in part (the header, the styles, the link rules and the entry points,
//! about 370 of 3,267 lines) for what a terminal reader decides; it is built on `pulldown-cmark` with table layout, theme-based
//! colours and hyperlink escapes, none of which is used here.

use crate::ui::Theme;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use serde_json::{json, Value};
use unicode_width::UnicodeWidthChar;

pub const MAX_INPUT: usize = 200_000;

/// The farthest a label or an address is looked for: a bracket with no end in sight is text, so a line of nothing but brackets cannot
/// make the reader rescan the rest of the line from every one of them.
const MAX_SPAN: usize = 2200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Code(String),
    Bold(Vec<Inline>),
    Em(Vec<Inline>),
    Link { text: String, href: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    P(Vec<Inline>),
    H(Vec<Inline>),
    Code { lang: String, text: String },
    Ul(Vec<Vec<Inline>>),
    Ol(Vec<Vec<Inline>>),
    Quote(Vec<Inline>),
}

/// Only plain https addresses with a real host and no credentials, spaces or markup characters count as links.
pub fn safe_href(raw: &str) -> Option<String> {
    if raw.chars().count() > 2000
        || raw
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "<>\"'`\\".contains(c))
    {
        return None;
    }
    let rest = raw.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let host = authority.rsplit_once(':').map_or(authority, |(h, p)| {
        if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
            h
        } else {
            authority
        }
    });
    let ok_host = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        && !host.starts_with('.')
        && !host.ends_with('.');
    ok_host.then(|| raw.to_string())
}

fn starts_at(chars: &[char], i: usize, s: &str) -> bool {
    s.chars()
        .enumerate()
        .all(|(k, c)| chars.get(i + k) == Some(&c))
}

/// The index just after `needle` in `chars` from `from`, stopping at a line break; None when it does not occur on this line.
fn find_on_line(chars: &[char], from: usize, needle: char) -> Option<usize> {
    let mut j = from;
    while j < chars.len() && chars[j] != '\n' {
        if chars[j] == needle {
            return Some(j);
        }
        j += 1;
    }
    None
}

pub fn inline(src: &str) -> Vec<Inline> {
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<Inline> = Vec::new();
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut Vec<Inline>| {
        if !text.is_empty() {
            out.push(Inline::Text(std::mem::take(text)));
        }
    };
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // `code`
        if c == '`' {
            if let Some(end) = find_on_line(&chars, i + 1, '`') {
                if end > i + 1 {
                    flush(&mut text, &mut out);
                    out.push(Inline::Code(chars[i + 1..end].iter().collect()));
                    i = end + 1;
                    continue;
                }
            }
        }
        // **bold**
        if starts_at(&chars, i, "**") {
            let mut j = i + 2;
            while j < chars.len() && chars[j] != '*' && chars[j] != '\n' {
                j += 1;
            }
            if j > i + 2 && starts_at(&chars, j, "**") {
                flush(&mut text, &mut out);
                out.push(Inline::Bold(inline(
                    &chars[i + 2..j].iter().collect::<String>(),
                )));
                i = j + 2;
                continue;
            }
        }
        // *italic*: the first character after the star is not a space or another star
        if c == '*' && i + 1 < chars.len() && !chars[i + 1].is_whitespace() && chars[i + 1] != '*' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '*' && chars[j] != '\n' {
                j += 1;
            }
            if j < chars.len() && chars[j] == '*' {
                flush(&mut text, &mut out);
                out.push(Inline::Em(inline(
                    &chars[i + 1..j].iter().collect::<String>(),
                )));
                i = j + 1;
                continue;
            }
        }
        // ![image](url): never loaded, kept as words
        if starts_at(&chars, i, "![") {
            if let Some((label, end)) = image_pair(&chars, i + 1) {
                text.push_str(&format!("[image: {label}]"));
                i = end;
                continue;
            }
            text.push(c);
            i += 1;
            continue;
        }
        // [text](url)
        if c == '[' {
            if let Some((label, url, end)) = bracket_pair(&chars, i) {
                match safe_href(url.trim()) {
                    Some(href) => {
                        flush(&mut text, &mut out);
                        out.push(Inline::Link { text: label, href });
                    }
                    None => text.push_str(&format!("{label} ({url})")),
                }
                i = end;
                continue;
            }
        }
        text.push(c);
        i += 1;
    }
    flush(&mut text, &mut out);
    out
}

/// `[label](url)` of an image, where the label and the address may be empty; returns the label and the index after the paren.
fn image_pair(chars: &[char], from: usize) -> Option<(String, usize)> {
    if chars.get(from) != Some(&'[') {
        return None;
    }
    let mut j = from + 1;
    while j < chars.len() && chars[j] != ']' && chars[j] != '\n' && j - from < MAX_SPAN {
        j += 1;
    }
    if j >= chars.len() || chars[j] != ']' || chars.get(j + 1) != Some(&'(') {
        return None;
    }
    let mut k = j + 2;
    while k < chars.len() && chars[k] != ')' && chars[k] != '\n' && k - j < MAX_SPAN {
        k += 1;
    }
    if k >= chars.len() || chars[k] != ')' {
        return None;
    }
    Some((chars[from + 1..j].iter().collect(), k + 1))
}

/// `[label](url)` starting at `from` (which must be the `[`): the label, the address, and the index after the closing paren.
fn bracket_pair(chars: &[char], from: usize) -> Option<(String, String, usize)> {
    if chars.get(from) != Some(&'[') {
        return None;
    }
    let mut j = from + 1;
    while j < chars.len() && chars[j] != ']' && chars[j] != '\n' && j - from < MAX_SPAN {
        j += 1;
    }
    if j >= chars.len() || chars[j] != ']' || j == from + 1 || chars.get(j + 1) != Some(&'(') {
        return None;
    }
    let mut k = j + 2;
    while k < chars.len() && chars[k] != ')' && chars[k] != '\n' && k - j < MAX_SPAN {
        k += 1;
    }
    if k >= chars.len() || chars[k] != ')' || k == j + 2 {
        return None;
    }
    Some((
        chars[from + 1..j].iter().collect(),
        chars[j + 2..k].iter().collect(),
        k + 1,
    ))
}

fn list_item(line: &str, ordered: bool) -> Option<&str> {
    let t = line.trim_start();
    if ordered {
        let digits = t.chars().take_while(char::is_ascii_digit).count();
        if !(1..=3).contains(&digits) {
            return None;
        }
        let rest = &t[digits..];
        let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))?;
        rest.strip_prefix([' ', '\t']).map(str::trim_start)
    } else {
        let rest = t.strip_prefix(['-', '*', '+'])?;
        rest.strip_prefix([' ', '\t']).map(str::trim_start)
    }
}

fn fence_open(line: &str) -> Option<String> {
    let t = line.trim();
    let rest = t.strip_prefix("```")?;
    let lang = rest.trim();
    lang.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '+' || c == '-')
        .then(|| lang.chars().take(20).collect())
}

fn fence_close(line: &str) -> bool {
    line.trim() == "```"
}

pub fn parse(input: &str) -> Vec<Block> {
    let clipped: String = input.chars().take(MAX_INPUT).collect();
    let normal = clipped.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normal.split('\n').collect();
    let mut blocks = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let end_para = |para: &mut Vec<String>, blocks: &mut Vec<Block>| {
        if !para.is_empty() {
            blocks.push(Block::P(inline(&para.join(" "))));
            para.clear();
        }
    };
    let mut n = 0;
    while n < lines.len() {
        let line = lines[n];
        if let Some(lang) = fence_open(line) {
            end_para(&mut para, &mut blocks);
            let mut body: Vec<&str> = Vec::new();
            n += 1;
            while n < lines.len() && !fence_close(lines[n]) {
                body.push(lines[n]);
                n += 1;
            }
            blocks.push(Block::Code {
                lang,
                text: body.join("\n"),
            });
            n += 1;
            continue;
        }
        if line.trim().is_empty() {
            end_para(&mut para, &mut blocks);
            n += 1;
            continue;
        }
        let hashes = line.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && line[hashes..].starts_with([' ', '\t']) {
            end_para(&mut para, &mut blocks);
            blocks.push(Block::H(inline(line[hashes..].trim_start())));
            n += 1;
            continue;
        }
        let ul = list_item(line, false).is_some();
        let ol = list_item(line, true).is_some();
        if ul || ol {
            end_para(&mut para, &mut blocks);
            let mut items = Vec::new();
            let mut k = n;
            while k < lines.len() {
                match list_item(lines[k], ol && !ul) {
                    Some(rest) => items.push(inline(rest)),
                    None => break,
                }
                k += 1;
            }
            blocks.push(if ol && !ul {
                Block::Ol(items)
            } else {
                Block::Ul(items)
            });
            n = k;
            continue;
        }
        if let Some(rest) = line.strip_prefix('>') {
            end_para(&mut para, &mut blocks);
            blocks.push(Block::Quote(inline(rest.strip_prefix(' ').unwrap_or(rest))));
            n += 1;
            continue;
        }
        para.push(line.trim().to_string());
        n += 1;
    }
    end_para(&mut para, &mut blocks);
    blocks
}

fn inline_json(i: &Inline) -> Value {
    match i {
        Inline::Text(v) => json!({"t": "text", "v": v}),
        Inline::Code(v) => json!({"t": "code", "v": v}),
        Inline::Bold(v) => json!({"t": "bold", "v": v.iter().map(inline_json).collect::<Vec<_>>()}),
        Inline::Em(v) => json!({"t": "em", "v": v.iter().map(inline_json).collect::<Vec<_>>()}),
        Inline::Link { text, href } => json!({"t": "link", "v": text, "href": href}),
    }
}

/// The tree in the shape the web reader's tests use, so one set of cases can check both (links carry no `host` here).
pub fn block_json(b: &Block) -> Value {
    let list = |items: &Vec<Vec<Inline>>| {
        items
            .iter()
            .map(|i| i.iter().map(inline_json).collect::<Vec<_>>())
            .collect::<Vec<_>>()
    };
    match b {
        Block::P(v) => json!({"t": "p", "v": v.iter().map(inline_json).collect::<Vec<_>>()}),
        Block::H(v) => json!({"t": "h", "v": v.iter().map(inline_json).collect::<Vec<_>>()}),
        Block::Code { lang, text } => json!({"t": "code", "lang": lang, "v": text}),
        Block::Ul(items) => json!({"t": "ul", "v": list(items)}),
        Block::Ol(items) => json!({"t": "ol", "v": list(items)}),
        Block::Quote(v) => {
            json!({"t": "quote", "v": v.iter().map(inline_json).collect::<Vec<_>>()})
        }
    }
}

// ---- styled lines ----

#[derive(Clone)]
struct Piece {
    text: String,
    style: Style,
}

fn pieces(inl: &[Inline], base: Style, th: &Theme, out: &mut Vec<Piece>) {
    for i in inl {
        match i {
            Inline::Text(t) => out.push(Piece {
                text: t.clone(),
                style: base,
            }),
            Inline::Code(t) => out.push(Piece {
                text: t.clone(),
                style: base.patch(if th.color {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().add_modifier(Modifier::REVERSED)
                }),
            }),
            Inline::Bold(v) => pieces(v, base.add_modifier(Modifier::BOLD), th, out),
            Inline::Em(v) => pieces(v, base.add_modifier(Modifier::ITALIC), th, out),
            Inline::Link { text, href } => {
                let link = base.add_modifier(Modifier::UNDERLINED);
                out.push(Piece {
                    text: text.clone(),
                    style: link,
                });
                if text != href {
                    out.push(Piece {
                        text: format!(" ({href})"),
                        style: base.add_modifier(Modifier::DIM),
                    });
                }
            }
        }
    }
}

fn width_of(s: &str) -> usize {
    s.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// Words with their styles, wrapped to `width`; a word wider than the line is cut. Each line also gets the prefix of its block.
fn wrap_pieces(
    ps: &[Piece],
    width: usize,
    first: &str,
    rest: &str,
    prefix_style: Style,
) -> Vec<Line<'static>> {
    let width = width.max(8);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut cur: Vec<Span<'static>> = vec![Span::styled(first.to_string(), prefix_style)];
    let mut w = width_of(first);
    let indent_w = width_of(rest);
    let start_w = |first_line: bool| {
        if first_line {
            width_of(first)
        } else {
            indent_w
        }
    };
    let mut first_line = true;
    let push_line =
        |cur: &mut Vec<Span<'static>>, first_line: &mut bool, lines: &mut Vec<Line<'static>>| {
            lines.push(Line::from(std::mem::take(cur)));
            *first_line = false;
            cur.push(Span::styled(rest.to_string(), prefix_style));
        };
    for p in ps {
        // split into words and the spaces between them, keeping the style
        let mut tokens: Vec<(String, bool)> = Vec::new();
        let mut buf = String::new();
        let mut in_space = false;
        for ch in p.text.chars() {
            let sp = ch == ' ' || ch == '\t';
            if !buf.is_empty() && sp != in_space {
                tokens.push((std::mem::take(&mut buf), in_space));
            }
            in_space = sp;
            buf.push(ch);
        }
        if !buf.is_empty() {
            tokens.push((buf, in_space));
        }
        for (tok, is_space) in tokens {
            if is_space {
                if w > start_w(first_line) && w < width {
                    cur.push(Span::styled(" ".to_string(), p.style));
                    w += 1;
                }
                continue;
            }
            let tw = width_of(&tok);
            if w + tw > width && w > start_w(first_line) {
                // trailing space is not carried to the next line
                if let Some(last) = cur.last() {
                    if last.content.as_ref() == " " {
                        cur.pop();
                    }
                }
                push_line(&mut cur, &mut first_line, &mut lines);
                w = indent_w;
            }
            // a word wider than a whole line is cut at the line's width
            let mut piece = String::new();
            let mut pw = 0;
            for ch in tok.chars() {
                let cw = ch.width().unwrap_or(0);
                if w + pw + cw > width && (w + pw) > start_w(first_line) {
                    cur.push(Span::styled(std::mem::take(&mut piece), p.style));
                    push_line(&mut cur, &mut first_line, &mut lines);
                    w = indent_w;
                    pw = 0;
                }
                piece.push(ch);
                pw += cw;
            }
            if !piece.is_empty() {
                cur.push(Span::styled(piece, p.style));
            }
            w += pw;
        }
    }
    lines.push(Line::from(cur));
    lines
}

/// The answer as styled lines, each at most `width` columns, with no control character from the text in any of them.
/// Characters that show nothing or change the order text is drawn in: zero-width and direction marks, direction overrides and
/// isolates, the byte order mark, and the line and paragraph separators. A model's answer or a file's text must not be able
/// to make one thing look like another on the screen.
fn hidden_format(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}' | '\u{2028}' | '\u{2029}')
}

pub fn render(text: &str, width: usize, th: &Theme) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let dim = Style::default().add_modifier(Modifier::DIM);
    // text reaches here already cleaned; a control character that arrives anyway is dropped, never drawn
    let clean: String = text
        .chars()
        .filter(|c| (!c.is_control() || *c == '\n' || *c == '\t') && !hidden_format(*c))
        .collect();
    let blocks = parse(&clean);
    for (bi, b) in blocks.iter().enumerate() {
        let mut ps: Vec<Piece> = Vec::new();
        match b {
            Block::P(inl) => {
                pieces(inl, Style::default(), th, &mut ps);
                out.extend(wrap_pieces(&ps, width, "", "", Style::default()));
            }
            Block::H(inl) => {
                pieces(
                    inl,
                    Style::default().add_modifier(Modifier::BOLD),
                    th,
                    &mut ps,
                );
                out.extend(wrap_pieces(&ps, width, "", "", Style::default()));
            }
            Block::Quote(inl) => {
                pieces(inl, Style::default(), th, &mut ps);
                out.extend(wrap_pieces(&ps, width, "│ ", "│ ", dim));
            }
            Block::Ul(items) | Block::Ol(items) => {
                let ordered = matches!(b, Block::Ol(_));
                for (k, item) in items.iter().enumerate() {
                    let mut ps: Vec<Piece> = Vec::new();
                    pieces(item, Style::default(), th, &mut ps);
                    let marker = if ordered {
                        format!("{}. ", k + 1)
                    } else {
                        "• ".to_string()
                    };
                    let hang = " ".repeat(width_of(&marker));
                    out.extend(wrap_pieces(&ps, width, &marker, &hang, dim));
                }
            }
            Block::Code { lang, text } => {
                if !lang.is_empty() {
                    out.push(Line::from(Span::styled(format!("  {lang}"), dim)));
                }
                let code = if th.color {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default()
                };
                for raw in text.split('\n') {
                    // code keeps its own line breaks and spaces; a line wider than the screen is cut, never wrapped into new words
                    let mut line = String::new();
                    let mut w = 0;
                    for ch in raw.replace('\t', "    ").chars() {
                        let cw = ch.width().unwrap_or(0);
                        if w + cw > width.saturating_sub(2) {
                            break;
                        }
                        line.push(ch);
                        w += cw;
                    }
                    out.push(Line::from(vec![
                        Span::styled("  ".to_string(), dim),
                        Span::styled(line, code),
                    ]));
                }
            }
        }
        if bi + 1 < blocks.len() {
            out.push(Line::raw(""));
        }
    }
    out
}
