//! Fields in a saved prompt, the same rules as the web page's (`apps/web/src/promptVars.ts`; both are checked against
//! `apps/web/test/promptvars-cases.json`): `{{name}}`, `{{name:one|two}}` (the choices start after the first colon), one
//! field per name (every place with that name takes the one value), and `{{current_date}}`, `{{current_datetime}}`,
//! `{{iso_datetime}}` filled from the clock whatever follows a colon (in UTC here; the web page uses the computer's own time
//! zone). A prompt is filled only when every field has a value.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// As first written between the braces.
    pub raw: String,
    pub name: String,
    pub options: Vec<String>,
}

const AUTO: [&str; 3] = ["current_date", "current_datetime", "iso_datetime"];
const MAX: usize = 80;

fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

fn name_of(raw: &str) -> &str {
    trim(raw.split_once(':').map_or(raw, |(n, _)| n))
}

fn is_auto(name: &str) -> bool {
    AUTO.contains(&name.to_lowercase().as_str())
}

/// Each `{{...}}` in the text with where it starts and ends (the braces included), in order: up to 80 characters with no
/// brace and no line break between the pairs; the innermost pair counts, so `{{{x}}}` is the field `x` inside braces.
fn spots(text: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find("{{").map(|i| from + i) {
        let inner_start = open + 2;
        let Some(close) = text[inner_start..].find("}}").map(|i| inner_start + i) else {
            break;
        };
        let inner = &text[inner_start..close];
        if inner.contains(['{', '}', '\n']) || inner.is_empty() {
            from = open + 1;
            continue;
        }
        if inner.chars().count() <= MAX {
            out.push((open, close + 2, inner));
        }
        from = close + 2;
    }
    out
}

pub fn fields(text: &str) -> Vec<Field> {
    let mut out: Vec<Field> = Vec::new();
    for (_, _, raw) in spots(text) {
        let name = name_of(raw);
        if name.is_empty() || is_auto(name) {
            continue;
        }
        let options: Vec<String> = raw
            .split_once(':')
            .map(|(_, o)| {
                o.split('|')
                    .map(trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        if let Some(seen) = out.iter_mut().find(|f| f.name == name) {
            if seen.options.is_empty() && !options.is_empty() {
                seen.options = options;
            }
            continue;
        }
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
    let missing: Vec<String> = fields(text)
        .iter()
        .filter(|f| values.get(&f.name).is_none_or(|v| trim(v).is_empty()))
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
        let name = name_of(raw);
        match name.to_lowercase().as_str() {
            "current_date" => out.push_str(&format!("{y:04}-{m:02}-{d:02}")),
            "current_datetime" => {
                out.push_str(&format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02} UTC"))
            }
            "iso_datetime" => {
                out.push_str(&format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z"))
            }
            _ => match values.get(name).filter(|_| !name.is_empty()) {
                Some(v) => out.push_str(trim(v)),
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

#[cfg(test)]
mod shared_cases {
    use serde_json::Value;
    use std::collections::BTreeMap;

    #[test]
    fn the_same_fields_and_the_same_filled_text_as_the_web_page() {
        let cases: Value =
            serde_json::from_str(include_str!("../../../apps/web/test/promptvars-cases.json"))
                .unwrap();
        for c in cases["fields"].as_array().unwrap() {
            let text = c["text"].as_str().unwrap();
            let got: Vec<Value> = super::fields(text)
                .into_iter()
                .map(|f| serde_json::json!({"name": f.name, "options": f.options}))
                .collect();
            assert_eq!(Value::Array(got), c["want"], "{text:?}");
        }
        for c in cases["fill"].as_array().unwrap() {
            let text = c["text"].as_str().unwrap();
            let values: BTreeMap<String, String> = c["values"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
                .collect();
            let got = match super::fill(text, &values, 1_767_603_600) {
                Ok(t) => serde_json::json!({"text": t}),
                Err(m) => serde_json::json!({"missing": m}),
            };
            assert_eq!(got, c["want"], "{text:?}");
        }
        let r = super::fill(
            "{{current_date:x}} {{ ISO_DATETIME }}",
            &BTreeMap::new(),
            1_767_604_023,
        )
        .unwrap();
        assert!(
            !r.contains("{{") && r.ends_with("2026-01-05T09:07:03Z"),
            "{r}"
        );
    }
}
