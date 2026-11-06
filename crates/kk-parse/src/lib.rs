//! Reading a file the owner attaches, on this computer.
//!
//! Text, Word, Excel, PDF, web pages and archives become plain text split into passages, each with an anchor the owner can
//! open (a page, a sheet cell, a table cell, a line range). Nothing is sent anywhere and nothing is written to disk.
//!
//! A file is hostile until shown otherwise, so every reader works inside hard limits: the size of the file, the size of
//! everything an archive would unpack to, the depth of archives inside archives, the number of pages, sheets, rows and
//! characters. What a person cannot see in the application that made the file (hidden text, hidden sheets, deleted tracked
//! changes) is left out and counted, so the assistant reads what the owner would read. What could not be read at all
//! (pictures, scans, password-protected files) is said in plain words and never silently dropped.

mod archive;
mod docx;
mod html;
mod pdf;
mod text;
mod xlsx;
mod xml;

use std::fmt;

/// Hard limits for one attached file. The defaults are what the product uses; tests use smaller ones.
#[derive(Debug, Clone)]
pub struct Limits {
    /// The file as given.
    pub max_input: usize,
    /// Characters of text kept from the whole file; more is cut and said so.
    pub max_chars: usize,
    /// One member of an archive, or one part of an Office file, after unpacking.
    pub max_member: u64,
    /// Everything an archive or Office file unpacks to, added together.
    pub max_unpacked: u64,
    /// Files in one archive.
    pub max_entries: usize,
    /// Archives inside archives (an Office file inside an archive counts as one level).
    pub max_depth: usize,
    pub max_pages: usize,
    pub max_sheets: usize,
    /// Cells read from all sheets of one workbook.
    pub max_cells: usize,
    /// A part may not unpack to more than this many times its packed size.
    pub max_ratio: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_input: 8 * 1024 * 1024,
            max_chars: 400_000,
            max_member: 16 * 1024 * 1024,
            max_unpacked: 48 * 1024 * 1024,
            max_entries: 200,
            max_depth: 2,
            max_pages: 400,
            max_sheets: 40,
            max_cells: 200_000,
            max_ratio: 200,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Html,
    Word,
    Excel,
    Pdf,
    Archive,
    Picture,
}

impl Kind {
    /// What this kind is called to the owner.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Text => "text file",
            Kind::Html => "web page file",
            Kind::Word => "Word document",
            Kind::Excel => "Excel workbook",
            Kind::Pdf => "PDF",
            Kind::Archive => "archive",
            Kind::Picture => "picture",
        }
    }
}

impl Kind {
    /// A short stable name, for storage and the wire.
    pub fn id(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Html => "html",
            Kind::Word => "word",
            Kind::Excel => "excel",
            Kind::Pdf => "pdf",
            Kind::Archive => "archive",
            Kind::Picture => "picture",
        }
    }

    pub fn from_id(id: &str) -> Option<Kind> {
        [
            Kind::Text,
            Kind::Html,
            Kind::Word,
            Kind::Excel,
            Kind::Pdf,
            Kind::Archive,
            Kind::Picture,
        ]
        .into_iter()
        .find(|k| k.id() == id)
    }
}

/// A piece of text and where in the file it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    /// For example `page 3`, `Q1!B7`, `table 2, row 1, column 3`, `lines 41-80`, `notes.docx > paragraph 4`.
    pub anchor: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Parsed {
    pub name: String,
    pub kind: Kind,
    pub passages: Vec<Passage>,
    /// What was not read or was left out, in plain words, so the owner is never told less than the truth.
    pub notes: Vec<String>,
    /// True when text was cut because it reached the limit.
    pub truncated: bool,
}

impl Parsed {
    pub fn chars(&self) -> usize {
        self.passages.iter().map(|p| p.text.chars().count()).sum()
    }

    /// The text as it is shown to a model: each passage under its anchor.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for p in &self.passages {
            out.push('[');
            out.push_str(&p.anchor);
            out.push_str("]\n");
            out.push_str(&p.text);
            out.push_str("\n\n");
        }
        out.trim_end().to_string()
    }
}

impl Parsed {
    /// The reading as JSON, for handing it from the isolated process that did the reading.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "kind": self.kind.id(),
            "passages": self.passages.iter().map(|p| serde_json::json!({"anchor": p.anchor, "text": p.text})).collect::<Vec<_>>(),
            "notes": self.notes,
            "truncated": self.truncated,
        })
    }

    /// The reading from JSON written by `to_json`, or `None` when it is not that shape.
    pub fn from_json(v: &serde_json::Value) -> Option<Parsed> {
        let text = |x: &serde_json::Value| x.as_str().map(str::to_string);
        Some(Parsed {
            name: text(v.get("name")?)?,
            kind: Kind::from_id(v.get("kind")?.as_str()?)?,
            passages: v
                .get("passages")?
                .as_array()?
                .iter()
                .map(|p| {
                    Some(Passage {
                        anchor: text(p.get("anchor")?)?,
                        text: text(p.get("text")?)?,
                    })
                })
                .collect::<Option<Vec<_>>>()?,
            notes: v
                .get("notes")?
                .as_array()?
                .iter()
                .map(text)
                .collect::<Option<Vec<_>>>()?,
            truncated: v.get("truncated")?.as_bool()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The file is larger than the limit.
    TooLarge,
    /// Nothing to read: the file is empty.
    Empty,
    /// Not a kind of file this program reads, with what it looked like.
    Unsupported(String),
    /// The file looks like its kind but is damaged or malformed.
    Damaged(String),
    /// Protected by a password.
    Protected,
    /// An archive that would unpack to more than the limits allow.
    Bomb(String),
    /// The name is not a plain file name.
    BadName,
}

impl ParseError {
    /// A short stable code for the wire.
    pub fn code(&self) -> &'static str {
        match self {
            ParseError::TooLarge => "too_large",
            ParseError::Empty => "empty",
            ParseError::Unsupported(_) => "unsupported",
            ParseError::Damaged(_) => "damaged",
            ParseError::Protected => "protected",
            ParseError::Bomb(_) => "bomb",
            ParseError::BadName => "bad_name",
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        let detail = match self {
            ParseError::Unsupported(w) | ParseError::Damaged(w) | ParseError::Bomb(w) => w.clone(),
            _ => String::new(),
        };
        serde_json::json!({"error": {"code": self.code(), "detail": detail}})
    }

    pub fn from_json(v: &serde_json::Value) -> Option<ParseError> {
        let e = v.get("error")?;
        let detail = e
            .get("detail")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_string();
        Some(match e.get("code")?.as_str()? {
            "too_large" => ParseError::TooLarge,
            "empty" => ParseError::Empty,
            "unsupported" => ParseError::Unsupported(detail),
            "damaged" => ParseError::Damaged(detail),
            "protected" => ParseError::Protected,
            "bomb" => ParseError::Bomb(detail),
            "bad_name" => ParseError::BadName,
            _ => return None,
        })
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::TooLarge => f.write_str("the file is larger than the limit"),
            ParseError::Empty => f.write_str("the file is empty"),
            ParseError::Unsupported(w) => write!(f, "this kind of file is not read here ({w})"),
            ParseError::Damaged(w) => {
                write!(f, "the file is damaged or not what it claims to be ({w})")
            }
            ParseError::Protected => f.write_str("the file is protected by a password"),
            ParseError::Bomb(w) => write!(f, "the file would unpack to more than is allowed ({w})"),
            ParseError::BadName => f.write_str("the file name is not a plain name"),
        }
    }
}

impl std::error::Error for ParseError {}

/// A file name that is safe to show and to put in a log: no path, no control or direction characters, at most 120
/// characters. The name is only a label; it is never used to open anything.
pub fn clean_name(name: &str) -> Result<String, ParseError> {
    let n = name.trim();
    let bad = |c: char| {
        c.is_control()
            || matches!(c, '/' | '\\' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}')
    };
    if n.is_empty() || n.chars().count() > 120 || n.chars().any(bad) || n == "." || n == ".." {
        return Err(ParseError::BadName);
    }
    Ok(n.to_string())
}

/// What a file is, from its first bytes first and its name second. A name never makes a file into something it is not.
pub fn detect(name: &str, bytes: &[u8]) -> Option<Kind> {
    if bytes.starts_with(b"%PDF-") {
        return Some(Kind::Pdf);
    }
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return Some(archive::office_or_archive(bytes));
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"\xff\xd8\xff")
        || bytes.starts_with(b"GIF87a")
        || bytes.starts_with(b"GIF89a")
        || (bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP")
        || bytes.starts_with(b"BM") && bytes.len() > 14 && name.to_lowercase().ends_with(".bmp")
    {
        return Some(Kind::Picture);
    }
    if text::looks_like_text(bytes) {
        let lower = name.to_lowercase();
        if lower.ends_with(".html") || lower.ends_with(".htm") || lower.ends_with(".xhtml") {
            return Some(Kind::Html);
        }
        return Some(Kind::Text);
    }
    None
}

/// Reads one file. `name` is only a label.
pub fn parse(name: &str, bytes: &[u8], limits: &Limits) -> Result<Parsed, ParseError> {
    let name = clean_name(name)?;
    if bytes.is_empty() {
        return Err(ParseError::Empty);
    }
    if bytes.len() > limits.max_input {
        return Err(ParseError::TooLarge);
    }
    let mut ctx = Ctx::new(limits);
    let kind = detect(&name, bytes).ok_or_else(|| ParseError::Unsupported(describe(bytes)))?;
    read(&mut ctx, "", &name, kind, bytes, 0)?;
    Ok(Parsed {
        name,
        kind,
        passages: ctx.passages,
        notes: ctx.notes,
        truncated: ctx.truncated,
    })
}

/// What the first bytes look like, for the owner. Never a guess about the content.
fn describe(bytes: &[u8]) -> String {
    let head = &bytes[..bytes.len().min(8)];
    let hex: Vec<String> = head.iter().map(|b| format!("{b:02x}")).collect();
    format!("it starts with the bytes {}", hex.join(" "))
}

/// The state of one reading: the passages so far, what was left out, and what is left of every budget.
pub(crate) struct Ctx<'a> {
    pub limits: &'a Limits,
    pub passages: Vec<Passage>,
    pub notes: Vec<String>,
    pub truncated: bool,
    chars: usize,
    pub unpacked: u64,
    pub cells: usize,
    pub entries: usize,
}

impl<'a> Ctx<'a> {
    fn new(limits: &'a Limits) -> Self {
        Ctx {
            limits,
            passages: Vec::new(),
            notes: Vec::new(),
            truncated: false,
            chars: 0,
            unpacked: 0,
            cells: 0,
            entries: 0,
        }
    }

    /// Adds a passage if there is room. Returns false once the character budget is used up.
    pub fn push(&mut self, prefix: &str, anchor: String, text: String) -> bool {
        if self.truncated {
            return false;
        }
        let text = text.trim().to_string();
        if text.is_empty() {
            return true;
        }
        let room = self.limits.max_chars.saturating_sub(self.chars);
        let n = text.chars().count();
        let (text, cut) = if n > room {
            (text.chars().take(room).collect::<String>(), true)
        } else {
            (text, false)
        };
        self.chars += text.chars().count();
        let anchor = if prefix.is_empty() {
            anchor
        } else {
            format!("{prefix} > {anchor}")
        };
        if !text.is_empty() {
            self.passages.push(Passage { anchor, text });
        }
        if cut || self.chars >= self.limits.max_chars {
            self.truncated = true;
            self.note(format!(
                "Only the first {} characters were read; the rest of the file was left out.",
                self.limits.max_chars
            ));
            return false;
        }
        true
    }

    pub fn note(&mut self, text: String) {
        if self.notes.len() < 40 && !self.notes.contains(&text) {
            self.notes.push(text);
        }
    }

    /// Takes `n` bytes from the unpacked budget, or says the file would unpack to too much.
    pub fn unpack(&mut self, n: u64) -> Result<(), ParseError> {
        self.unpacked = self.unpacked.saturating_add(n);
        if self.unpacked > self.limits.max_unpacked {
            return Err(ParseError::Bomb(format!(
                "more than {} MiB once unpacked",
                self.limits.max_unpacked / (1024 * 1024)
            )));
        }
        Ok(())
    }
}

/// Reads one file of a known kind into the context. `prefix` names the archive members that lead to it.
pub(crate) fn read(
    ctx: &mut Ctx,
    prefix: &str,
    name: &str,
    kind: Kind,
    bytes: &[u8],
    depth: usize,
) -> Result<(), ParseError> {
    match kind {
        Kind::Text => text::read(ctx, prefix, bytes),
        Kind::Html => html::read(ctx, prefix, bytes),
        Kind::Word => docx::read(ctx, prefix, bytes),
        Kind::Excel => xlsx::read(ctx, prefix, bytes),
        Kind::Pdf => pdf::read(ctx, prefix, bytes),
        Kind::Archive => archive::read(ctx, prefix, name, bytes, depth),
        Kind::Picture => {
            ctx.note(format!(
                "{}: this is a picture. Pictures are not read: there is no text recognition on this computer yet.",
                if prefix.is_empty() { name } else { prefix }
            ));
            Ok(())
        }
    }
}
