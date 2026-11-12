//! Fields in a saved prompt, the same rules as the web page's: `{{name}}`, `{{name:one|two}}` (the choices start after the
//! first colon), each asked once, and `{{current_date}}`, `{{current_datetime}}`, `{{iso_datetime}}` filled from the clock
//! (in UTC here; the web page uses the computer's own time zone). A prompt is filled only when every field has a value.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub raw: String,
    pub name: String,
    pub options: Vec<String>,
}

const AUTO: [&str; 3] = ["current_date", "current_datetime", "iso_datetime"];

/// Each `{{...}}` in the text with where it starts and ends (the braces included), in order.
fn spots(text: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find("{{").map(|i| from + i) {
        let inner_start = open + 2;
        let Some(close) = text[inner_start..].find("}}").map(|i| inner_start + i) else {
            break;
        };
        let inner = &text[inner_start..close];
        if !inner.is_empty() && inner.chars().count() <= 80 && !inner.contains(['{', '}', '\n']) {
            out.push((open, close + 2, inner));
            from = close + 2;
        } else {
            from = inner_start;
        }
    }
    out
}

pub fn fields(text: &str) -> Vec<Field> {
    let mut out: Vec<Field> = Vec::new();
    for (_, _, raw) in spots(text) {
        let (name, opts) = match raw.split_once(':') {
            Some((n, o)) => (n.trim(), Some(o)),
            None => (raw.trim(), None),
        };
        if name.is_empty()
            || AUTO.contains(&name.to_lowercase().as_str())
            || out.iter().any(|f| f.raw == raw)
        {
            continue;
        }
        let options = opts
            .map(|o| {
                o.split('|')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        out.push(Field {
            raw: raw.to_string(),
            name: name.to_string(),
            options,
        });
    }
    out
}

/// The prompt with its fields filled from `values` (by field name), or the names still without a value.
pub fn fill(
    text: &str,
    values: &BTreeMap<String, String>,
    now_secs: i64,
) -> Result<String, Vec<String>> {
    let fs = fields(text);
    let missing: Vec<String> = fs
        .iter()
        .filter(|f| values.get(&f.name).is_none_or(|v| v.trim().is_empty()))
        .map(|f| f.name.clone())
        .collect();
    if !missing.is_empty() {
        return Err(missing);
    }
    let (y, m, d) = crate::picker::civil(now_secs.div_euclid(86_400));
    let secs = now_secs.rem_euclid(86_400);
    let (hh, mm, ss) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (start, end, raw) in spots(text) {
        out.push_str(&text[last..start]);
        let key = raw.trim().to_lowercase();
        let name = raw.split_once(':').map_or(raw, |(n, _)| n).trim();
        match key.as_str() {
            "current_date" => out.push_str(&format!("{y:04}-{m:02}-{d:02}")),
            "current_datetime" => {
                out.push_str(&format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02} UTC"))
            }
            "iso_datetime" => {
                out.push_str(&format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z"))
            }
            _ => match values.get(name) {
                Some(v) => out.push_str(v.trim()),
                None => out.push_str(&text[start..end]),
            },
        }
        last = end;
    }
    out.push_str(&text[last..]);
    Ok(out)
}

/// `name=value` pairs, a value in double quotes when it holds spaces.
pub fn pairs(words: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut rest = words.trim();
    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else {
            return Err(format!(
                "give each field as name=value; \"{}\" is not one",
                rest.split_whitespace().next().unwrap_or(rest)
            ));
        };
        let name = rest[..eq].trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err("give each field as name=value".into());
        }
        let after = &rest[eq + 1..];
        let (value, tail) = if let Some(q) = after.strip_prefix('"') {
            match q.find('"') {
                Some(close) => (&q[..close], &q[close + 1..]),
                None => return Err(format!("the value of {name} has no closing quote")),
            }
        } else {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            (&after[..end], &after[end..])
        };
        out.insert(name.to_string(), value.to_string());
        rest = tail.trim_start();
    }
    Ok(out)
}
