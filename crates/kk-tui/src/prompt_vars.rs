//! Fields in a saved prompt, the same rules as the web page's (`apps/web/src/promptVars.ts`; both are checked against
//! `apps/web/test/promptvars-cases.json`): `{{name}}`, `{{name:one|two}}` (the choices start after the first colon), one
//! field per name (every place with that name takes the one value), and `{{current_date}}`, `{{current_datetime}}`,
//! `{{iso_datetime}}` filled from the clock whatever follows a colon (the date and the date and time in this computer's time
//! zone, as on the web page; the ISO form in UTC). A prompt is filled only when every field has a value.

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
    // white space as the web page's JavaScript `\s` reads it: Unicode white space except U+0085, and U+FEFF
    s.trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
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
    offset_secs: i64,
) -> Result<String, Vec<String>> {
    let missing: Vec<String> = fields(text)
        .iter()
        .filter(|f| values.get(&f.name).is_none_or(|v| trim(v).is_empty()))
        .map(|f| f.name.clone())
        .collect();
    if !missing.is_empty() {
        return Err(missing);
    }
    let parts = |at: i64| {
        let (y, m, d) = crate::picker::civil(at.div_euclid(86_400));
        let secs = at.rem_euclid(86_400);
        (y, m, d, secs / 3600, (secs % 3600) / 60, secs % 60)
    };
    let (y, m, d, hh, mm, _) = parts(now_secs + offset_secs);
    let (uy, um, ud, uh, umin, us) = parts(now_secs);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (start, end, raw) in spots(text) {
        out.push_str(&text[last..start]);
        let name = name_of(raw);
        match name.to_lowercase().as_str() {
            "current_date" => out.push_str(&format!("{y:04}-{m:02}-{d:02}")),
            "current_datetime" => out.push_str(&format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")),
            "iso_datetime" => out.push_str(&format!(
                "{uy:04}-{um:02}-{ud:02}T{uh:02}:{umin:02}:{us:02}Z"
            )),
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

/// One word: in double quotes (where \" and \\ stand for a quote and a backslash), or bare up to the first character that
/// `stop` accepts. Returns the word and what follows it.
fn word(s: &str, stop: impl Fn(char) -> bool) -> Result<(String, &str), String> {
    let Some(q) = s.strip_prefix('"') else {
        let end = s.find(stop).unwrap_or(s.len());
        return Ok((s[..end].to_string(), &s[end..]));
    };
    let mut out = String::new();
    let mut chars = q.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some((_, n)) => out.push(n),
                None => return Err("a backslash ends it".into()),
            },
            '"' => return Ok((out, &q[i + 1..])),
            c => out.push(c),
        }
    }
    Err("its quote is not closed".into())
}

/// The way to write a field name on the command line: bare when it can be, else in quotes.
pub fn quoted_name(name: &str) -> String {
    if name.is_empty()
        || name.contains(|c: char| c.is_whitespace() || c == '=' || c == '"' || c == '\\')
    {
        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        name.to_string()
    }
}

/// `name=value` pairs. A name or a value with spaces, `=` or quotes is written in double quotes: `"due date"="next \"big\" day"`.
pub fn pairs(words: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    let mut rest = words.trim();
    while !rest.is_empty() {
        let (name, after) = word(rest, |c| c == '=' || c.is_whitespace())
            .map_err(|e| format!("a field name: {e}"))?;
        let Some(after) = after.strip_prefix('=') else {
            return Err(format!(
                "give each field as name=value (a name with spaces in double quotes, like \"due date\"=Friday); {} is not one",
                quoted_name(&name)
            ));
        };
        if trim(&name).is_empty() {
            return Err("give each field as name=value".into());
        }
        let (value, tail) = word(after, char::is_whitespace)
            .map_err(|e| format!("the value of {}: {e}", quoted_name(&name)))?;
        out.insert(trim(&name).to_string(), value);
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
            let got = match super::fill(text, &values, 1_767_603_600, 0) {
                Ok(t) => serde_json::json!({"text": t}),
                Err(m) => serde_json::json!({"missing": m}),
            };
            assert_eq!(got, c["want"], "{text:?}");
        }
        let r = super::fill(
            "{{current_date:x}} {{ ISO_DATETIME }}",
            &BTreeMap::new(),
            1_767_604_023,
            0,
        )
        .unwrap();
        assert!(
            !r.contains("{{") && r.ends_with("2026-01-05T09:07:03Z"),
            "{r}"
        );
    }
}

#[cfg(test)]
mod local_time {
    #[test]
    fn the_date_follows_this_computers_zone_and_the_iso_form_stays_utc() {
        // 2026-01-05 23:30 UTC is already 2026-01-06 05:00 at +05:30
        let r = super::fill(
            "{{current_date}} | {{current_datetime}} | {{iso_datetime}}",
            &Default::default(),
            1_767_655_800,
            5 * 3600 + 1800,
        )
        .unwrap();
        assert_eq!(r, "2026-01-06 | 2026-01-06 05:00 | 2026-01-05T23:30:00Z");
    }
}

#[cfg(test)]
mod quoted_pairs {
    #[test]
    fn names_and_values_with_spaces_equals_and_quotes_can_be_given() {
        let p =
            super::pairs(r#""due date"=Friday "a=b"=1 "say \"hi\""="a \"b\" c" plain=x"#).unwrap();
        assert_eq!(p.get("due date").map(String::as_str), Some("Friday"));
        assert_eq!(p.get("a=b").map(String::as_str), Some("1"));
        assert_eq!(p.get("say \"hi\"").map(String::as_str), Some("a \"b\" c"));
        assert_eq!(p.get("plain").map(String::as_str), Some("x"));
        assert!(
            super::pairs(r#""due date"#).is_err()
                && super::pairs("x=\"open").is_err()
                && super::pairs("lonely").is_err()
        );
        assert_eq!(super::quoted_name("due date"), r#""due date""#);
        assert_eq!(super::quoted_name(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(super::quoted_name("tone"), "tone");
        // a name written the way the hint writes it reads back as itself
        for n in ["due date", "a=b", r#"say "hi""#, "back\\slash"] {
            let p = super::pairs(&format!("{}=v", super::quoted_name(n))).unwrap();
            assert_eq!(p.get(n).map(String::as_str), Some("v"), "{n}");
        }
    }
}
