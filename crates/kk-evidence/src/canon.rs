use sha2::{Digest, Sha256};
use std::fmt;

pub const MAX_DEPTH: usize = 128;
const SAFE_MAX: i128 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidUtf8,
    Syntax,
    DuplicateKey,
    FloatForbidden,
    NegativeZero,
    UnsafeInteger,
    LoneSurrogate,
    TrailingData,
    TooDeep,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::InvalidUtf8 => "invalid_utf8",
            ErrorCode::Syntax => "syntax",
            ErrorCode::DuplicateKey => "duplicate_key",
            ErrorCode::FloatForbidden => "float_forbidden",
            ErrorCode::NegativeZero => "negative_zero",
            ErrorCode::UnsafeInteger => "unsafe_integer",
            ErrorCode::LoneSurrogate => "lone_surrogate",
            ErrorCode::TrailingData => "trailing_data",
            ErrorCode::TooDeep => "too_deep",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonError {
    pub code: ErrorCode,
    pub at: usize,
}

impl fmt::Display for CanonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.code.as_str(), self.at)
    }
}

impl std::error::Error for CanonError {}

struct Parser<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn err<T>(&self, code: ErrorCode) -> Result<T, CanonError> {
        Err(CanonError { code, at: self.pos })
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, CanonError> {
        if depth > MAX_DEPTH {
            return self.err(ErrorCode::TooDeep);
        }
        self.ws();
        match self.peek() {
            None => self.err(ErrorCode::Syntax),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => {
                for (lit, v) in [
                    ("true", Value::Bool(true)),
                    ("false", Value::Bool(false)),
                    ("null", Value::Null),
                ] {
                    if self.text[self.pos..].starts_with(lit) {
                        self.pos += lit.len();
                        return Ok(v);
                    }
                }
                self.err(ErrorCode::Syntax)
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, CanonError> {
        self.pos += 1;
        let mut members: Vec<(String, Value)> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Object(members));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return self.err(ErrorCode::Syntax);
            }
            let key = self.string()?;
            if members.iter().any(|(k, _)| *k == key) {
                return self.err(ErrorCode::DuplicateKey);
            }
            self.ws();
            if self.peek() != Some(b':') {
                return self.err(ErrorCode::Syntax);
            }
            self.pos += 1;
            let v = self.value(depth + 1)?;
            members.push((key, v));
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Object(members));
                }
                _ => return self.err(ErrorCode::Syntax),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, CanonError> {
        self.pos += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value(depth + 1)?);
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Array(items));
                }
                _ => return self.err(ErrorCode::Syntax),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, CanonError> {
        let end = self.pos + 4;
        let slice = match self.text.as_bytes().get(self.pos..end) {
            Some(s) => s,
            None => return self.err(ErrorCode::Syntax),
        };
        if !slice.iter().all(|b| b.is_ascii_hexdigit()) {
            return self.err(ErrorCode::Syntax);
        }
        let v =
            u32::from_str_radix(std::str::from_utf8(slice).unwrap_or("zz"), 16).map_err(|_| {
                CanonError {
                    code: ErrorCode::Syntax,
                    at: self.pos,
                }
            })?;
        self.pos = end;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, CanonError> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let rest = &self.text[self.pos..];
            let c = match rest.chars().next() {
                Some(c) => c,
                None => return self.err(ErrorCode::Syntax),
            };
            match c {
                '"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                '\\' => {
                    self.pos += 1;
                    let e = match self.peek() {
                        Some(e) => e,
                        None => return self.err(ErrorCode::Syntax),
                    };
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            if (0xd800..=0xdbff).contains(&hi) {
                                if !self.text[self.pos..].starts_with("\\u") {
                                    return self.err(ErrorCode::LoneSurrogate);
                                }
                                self.pos += 2;
                                let lo = self.hex4()?;
                                if !(0xdc00..=0xdfff).contains(&lo) {
                                    return self.err(ErrorCode::LoneSurrogate);
                                }
                                let cp = 0x10000 + ((hi - 0xd800) << 10) + (lo - 0xdc00);
                                out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                            } else if (0xdc00..=0xdfff).contains(&hi) {
                                return self.err(ErrorCode::LoneSurrogate);
                            } else {
                                out.push(char::from_u32(hi).unwrap_or('\u{fffd}'));
                            }
                        }
                        _ => return self.err(ErrorCode::Syntax),
                    }
                }
                c if (c as u32) < 0x20 => return self.err(ErrorCode::Syntax),
                c => {
                    out.push(c);
                    self.pos += c.len_utf8();
                }
            }
        }
    }

    fn number(&mut self) -> Result<Value, CanonError> {
        let start = self.pos;
        let b = self.text.as_bytes();
        let mut p = self.pos;
        if b.get(p) == Some(&b'-') {
            p += 1;
        }
        match b.get(p) {
            Some(b'0') => p += 1,
            Some(b'1'..=b'9') => {
                while matches!(b.get(p), Some(b'0'..=b'9')) {
                    p += 1;
                }
            }
            _ => return self.err(ErrorCode::Syntax),
        }
        let int_end = p;
        let mut is_float = false;
        if b.get(p) == Some(&b'.') {
            let digits_from = p + 1;
            let mut q = digits_from;
            while matches!(b.get(q), Some(b'0'..=b'9')) {
                q += 1;
            }
            if q == digits_from {
                return self.err(ErrorCode::Syntax);
            }
            p = q;
            is_float = true;
        }
        if matches!(b.get(p), Some(b'e' | b'E')) {
            let mut q = p + 1;
            if matches!(b.get(q), Some(b'+' | b'-')) {
                q += 1;
            }
            let digits_from = q;
            while matches!(b.get(q), Some(b'0'..=b'9')) {
                q += 1;
            }
            if q == digits_from {
                return self.err(ErrorCode::Syntax);
            }
            p = q;
            is_float = true;
        }
        self.pos = p;
        if matches!(b.get(p), Some(c) if c.is_ascii_alphanumeric() || matches!(c, b'.' | b'+' | b'-'))
        {
            return self.err(ErrorCode::Syntax);
        }
        if is_float {
            self.pos = start;
            return self.err(ErrorCode::FloatForbidden);
        }
        let lit = &self.text[start..int_end];
        if lit == "-0" {
            self.pos = start;
            return self.err(ErrorCode::NegativeZero);
        }
        match lit.parse::<i128>() {
            Ok(n) if (-SAFE_MAX..=SAFE_MAX).contains(&n) => Ok(Value::Int(n as i64)),
            _ => {
                self.pos = start;
                self.err(ErrorCode::UnsafeInteger)
            }
        }
    }
}

pub fn parse_strict(input: &[u8]) -> Result<Value, CanonError> {
    let text = std::str::from_utf8(input).map_err(|e| CanonError {
        code: ErrorCode::InvalidUtf8,
        at: e.valid_up_to(),
    })?;
    if text.starts_with('\u{feff}') {
        return Err(CanonError {
            code: ErrorCode::Syntax,
            at: 0,
        });
    }
    let mut p = Parser { text, pos: 0 };
    let v = p.value(0)?;
    p.ws();
    if p.pos < text.len() {
        return p.err(ErrorCode::TrailingData);
    }
    Ok(v)
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write_value(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Int(n) => out.push_str(&n.to_string()),
        Value::Str(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        Value::Object(members) => {
            let mut sorted: Vec<(Vec<u16>, &String, &Value)> = members
                .iter()
                .map(|(k, v)| (k.encode_utf16().collect(), k, v))
                .collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            out.push('{');
            for (i, (_, k, v)) in sorted.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(k, out);
                out.push(':');
                write_value(v, out);
            }
            out.push('}');
        }
    }
}

pub fn serialize(v: &Value) -> String {
    let mut out = String::new();
    write_value(v, &mut out);
    out
}

pub fn canonicalize(input: &[u8]) -> Result<String, CanonError> {
    Ok(serialize(&parse_strict(input)?))
}

pub fn digest_hex(input: &[u8]) -> Result<String, CanonError> {
    let canonical = canonicalize(input)?;
    let hash = Sha256::digest(canonical.as_bytes());
    Ok(hash.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Accept {
        name: String,
        input: String,
        canonical: String,
        sha256: String,
    }

    #[derive(Deserialize)]
    struct Reject {
        name: String,
        input: Option<String>,
        input_hex: Option<String>,
        code: String,
    }

    #[derive(Deserialize)]
    struct Vectors {
        accept: Vec<Accept>,
        reject: Vec<Reject>,
    }

    fn vectors() -> Vectors {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/canon/vectors.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("vectors file"))
            .expect("vectors parse")
    }

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).expect("hex"))
            .collect()
    }

    #[test]
    fn accepted_vectors_match_byte_for_byte() {
        let v = vectors();
        assert!(v.accept.len() >= 26);
        for a in &v.accept {
            assert_eq!(
                canonicalize(a.input.as_bytes()).expect(&a.name),
                a.canonical,
                "{}",
                a.name
            );
            assert_eq!(
                digest_hex(a.input.as_bytes()).expect(&a.name),
                a.sha256,
                "{}",
                a.name
            );
        }
    }

    #[test]
    fn rejected_vectors_fail_with_the_agreed_code() {
        let v = vectors();
        assert!(v.reject.len() >= 29);
        for r in &v.reject {
            let bytes = match (&r.input, &r.input_hex) {
                (Some(s), _) => s.as_bytes().to_vec(),
                (None, Some(h)) => unhex(h),
                _ => panic!("vector {} has no input", r.name),
            };
            let err = canonicalize(&bytes).expect_err(&r.name);
            assert_eq!(err.code.as_str(), r.code, "{}", r.name);
        }
    }

    #[test]
    fn nesting_beyond_the_limit_is_refused_not_overflowed() {
        let deep = "[".repeat(MAX_DEPTH + 2) + &"]".repeat(MAX_DEPTH + 2);
        assert_eq!(
            canonicalize(deep.as_bytes()).unwrap_err().code,
            ErrorCode::TooDeep
        );
        let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(canonicalize(ok.as_bytes()).is_ok());
    }
}
