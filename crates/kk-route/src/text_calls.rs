//! Tool calls a model wrote as text. Some local models (qwen2.5-coder under Ollama, for one) answer a tool request with the call
//! written in the reply instead of as a structured call. These are recovered here, but only for tools that were offered in that
//! request: any other name, or JSON that merely has a "name" field, stays text. The idea and the accepted shapes follow goose's
//! native tool parsing (Apache-2.0); see NOTICE.md.

use kk_types::chat::ToolCall;
use serde_json::{json, Value};

/// Replies longer than this are not scanned: a call written as text is short, and the scan below looks at every brace.
pub const MAX_SCAN: usize = 64 * 1024;
const MAX_CANDIDATES: usize = 64;
/// The most bytes the brace scan reads in all. Each opening brace starts its own scan, so text built of nothing but openers would
/// otherwise cost the square of its length; past this the text is left as text.
const MAX_STEPS: usize = 2_000_000;
const MAX_CALLS: usize = 8;
/// A reply longer than this (in characters) never yields a call to an outside tool.
const OUTSIDE_PROSE_LIMIT: usize = 600;

/// The calls written in `text` to tools named in `offered`, in order. Empty when there are none, the reply is too long, or a
/// recognised call names a tool that was not offered (a reply that mixes offered and unoffered calls yields only the offered ones).
pub fn calls_in_text(text: &str, offered: &[&str]) -> Vec<ToolCall> {
    let mut calls = calls_anywhere(text, offered);
    // a call to an outside program is taken only from a short reply: in a long one it is more likely a quotation of text the model
    // read (a page, a tool answer, a project file) than something it meant to do
    if text.chars().count() > OUTSIDE_PROSE_LIMIT {
        calls.retain(|c| !c.name.starts_with("mcp__"));
    }
    calls
}

fn calls_anywhere(text: &str, offered: &[&str]) -> Vec<ToolCall> {
    if text.len() > MAX_SCAN || offered.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let push = |v: &Value, out: &mut Vec<ToolCall>| {
        if out.len() >= MAX_CALLS {
            return;
        }
        if let Some(c) = one_call(v, offered, out.len()) {
            out.push(c);
        }
    };
    // tags first: <tool_call>{...}</tool_call> (the qwen chat format) and <function=NAME><parameter=K>V</parameter></function>
    for body in between(text, "<tool_call>", "</tool_call>") {
        if let Ok(v) = serde_json::from_str::<Value>(body.trim()) {
            push(&v, &mut out);
        }
    }
    for (name, body) in function_tags(text) {
        let mut args = serde_json::Map::new();
        for (k, v) in parameter_tags(body) {
            args.insert(k.trim().to_string(), Value::String(v.trim().to_string()));
        }
        push(&json!({"name": name.trim(), "arguments": args}), &mut out);
    }
    if !out.is_empty() {
        return out;
    }
    for v in json_candidates(text) {
        let list: Vec<Value> = match &v {
            Value::Array(items) => items.clone(),
            Value::Object(o) if o.get("tool_calls").is_some_and(Value::is_array) => {
                o["tool_calls"].as_array().cloned().unwrap_or_default()
            }
            _ => vec![v.clone()],
        };
        let before = out.len();
        for item in &list {
            push(item, &mut out);
        }
        // the first candidate that holds calls is the answer; later braces are usually inside its arguments
        if out.len() > before {
            break;
        }
    }
    out
}

fn one_call(v: &Value, offered: &[&str], n: usize) -> Option<ToolCall> {
    let f = v.get("function").filter(|f| f.is_object()).unwrap_or(v);
    let name = f.get("name")?.as_str()?.trim();
    if !offered.contains(&name) {
        return None;
    }
    // a bare {"name": ...} counts only when it also says what to call it with
    let args = f
        .get("arguments")
        .or_else(|| f.get("parameters"))
        .or_else(|| v.get("arguments"));
    if args.is_none() && v.get("function").is_none() {
        return None;
    }
    let arguments = match args.cloned().unwrap_or(Value::Null) {
        Value::Null => json!({}),
        Value::String(s) if s.trim().is_empty() => json!({}),
        Value::String(s) => serde_json::from_str::<Value>(&s)
            .ok()
            .filter(Value::is_object)?,
        o @ Value::Object(_) => o,
        _ => return None,
    };
    Some(ToolCall {
        id: format!("text-call-{n}"),
        name: name.to_string(),
        arguments,
    })
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(open) {
        let after = &rest[i + open.len()..];
        let Some(j) = after.find(close) else { break };
        out.push(&after[..j]);
        rest = &after[j + close.len()..];
    }
    out
}

fn function_tags(text: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("<function=") {
        let after = &rest[i + "<function=".len()..];
        let Some(gt) = after.find('>') else { break };
        let name = &after[..gt];
        let body_all = &after[gt + 1..];
        let Some(end) = body_all.find("</function>") else {
            break;
        };
        out.push((name, &body_all[..end]));
        rest = &body_all[end + "</function>".len()..];
    }
    out
}

fn parameter_tags(body: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("<parameter=") {
        let after = &rest[i + "<parameter=".len()..];
        let Some(gt) = after.find('>') else { break };
        let key = &after[..gt];
        let val_all = &after[gt + 1..];
        let Some(end) = val_all.find("</parameter>") else {
            break;
        };
        out.push((key, &val_all[..end]));
        rest = &val_all[end + "</parameter>".len()..];
    }
    out
}

/// Every balanced JSON object or array in the text that parses, outermost first, the whole text first of all.
fn json_candidates(text: &str) -> Vec<Value> {
    let mut out = Vec::new();
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        if v.is_object() || v.is_array() {
            out.push(v);
            return out;
        }
    }
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut steps = 0usize;
    while i < bytes.len() && out.len() < MAX_CANDIDATES && steps < MAX_STEPS {
        let open = bytes[i];
        if open != b'{' && open != b'[' {
            i += 1;
            continue;
        }
        let close = if open == b'{' { b'}' } else { b']' };
        let (mut depth, mut in_str, mut esc, mut end) = (0i32, false, false, None);
        for (k, &b) in bytes[i..].iter().enumerate() {
            steps += 1;
            if steps >= MAX_STEPS {
                break;
            }
            if esc {
                esc = false;
            } else if in_str {
                match b {
                    b'\\' => esc = true,
                    b'"' => in_str = false,
                    _ => {}
                }
            } else if b == b'"' {
                in_str = true;
            } else if b == open {
                depth += 1;
            } else if b == close {
                depth -= 1;
                if depth == 0 {
                    end = Some(i + k + 1);
                    break;
                }
            }
        }
        match end.and_then(|e| {
            serde_json::from_str::<Value>(&text[i..e])
                .ok()
                .map(|v| (e, v))
        }) {
            // a balanced, parsing span is taken whole; scanning resumes after it, so nested braces are not re-read
            Some((e, v)) => {
                out.push(v);
                i = e;
            }
            None => i += 1,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    const T: &[&str] = &["list_files", "read_file", "edit_file", "finish"];

    fn names(text: &str) -> Vec<String> {
        calls_in_text(text, T).into_iter().map(|c| c.name).collect()
    }

    #[test]
    fn bare_fenced_tagged_and_listed_calls_are_recovered() {
        assert_eq!(
            names(r#"{"name": "list_files", "arguments": {}}"#),
            ["list_files"]
        );
        assert_eq!(names("Sure.\n```json\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.py\"}}\n```"), ["read_file"]);
        assert_eq!(names("<tool_call>\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"a.py\"}}\n</tool_call>"), ["read_file"]);
        assert_eq!(
            names(
                r#"[{"name":"read_file","arguments":{"path":"a"}},{"name":"finish","arguments":{"summary":"x"}}]"#
            ),
            ["read_file", "finish"]
        );
        assert_eq!(
            names(
                r#"{"tool_calls":[{"id":"c1","type":"function","function":{"name":"edit_file","arguments":"{\"path\":\"a\",\"old_text\":\"x\",\"new_text\":\"y\"}"}}]}"#
            ),
            ["edit_file"]
        );
        let c = &calls_in_text(
            "<function=read_file><parameter=path>src/a.py</parameter></function>",
            T,
        )[0];
        assert_eq!(c.arguments, json!({"path": "src/a.py"}));
    }

    #[test]
    fn arguments_holding_braces_and_tags_stay_one_call() {
        let text = r#"{"name": "edit_file", "arguments": {"path": "a.py", "old_text": "d = {'k': [1]}", "new_text": "d = {'k': [2]} # <function=x>"}}"#;
        let calls = calls_in_text(text, T);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].arguments["new_text"],
            "d = {'k': [2]} # <function=x>"
        );
    }

    #[test]
    fn json_that_is_not_a_call_to_an_offered_tool_stays_text() {
        for text in [
            r#"{"name": "Alice", "age": 30}"#,
            r#"{"name": "delete_everything", "arguments": {}}"#,
            r#"{"name": "read_file"}"#,
            r#"["a", "b"]"#,
            r#"{"tool_calls": ["a", "b"]}"#,
            r#"{"name": "read_file", "arguments": "not json"}"#,
            r#"{"name": "read_file", "arguments": [1, 2]}"#,
            "I would call read_file on a.py.",
            "",
        ] {
            assert!(calls_in_text(text, T).is_empty(), "{text}");
        }
        assert!(
            calls_in_text(r#"{"name": "read_file", "arguments": {}}"#, &[]).is_empty(),
            "nothing offered, nothing recovered"
        );
        let long = format!(
            "{} {}",
            "x".repeat(MAX_SCAN),
            r#"{"name": "list_files", "arguments": {}}"#
        );
        assert!(
            calls_in_text(&long, T).is_empty(),
            "an over-long reply is not scanned"
        );
    }

    #[test]
    fn unbalanced_and_hostile_text_ends_quickly_with_nothing() {
        let t = std::time::Instant::now();
        let deep = "{".repeat(60_000);
        assert!(calls_in_text(&deep, T).is_empty());
        let many = "[{\"a\":1} ".repeat(6000);
        assert!(calls_in_text(&many, T).is_empty());
        assert!(
            t.elapsed() < std::time::Duration::from_secs(5),
            "{:?}",
            t.elapsed()
        );
        let capped = format!(
            "[{}]",
            vec![r#"{"name":"finish","arguments":{}}"#; 50].join(",")
        );
        assert_eq!(calls_in_text(&capped, T).len(), MAX_CALLS);
    }

    #[test]
    fn tc_outside_tools_are_not_taken_from_prose_that_merely_quotes_a_call() {
        const O: &[&str] = &["read_file", "mcp__docs__lookup"];
        let call = r#"<tool_call>{"name": "mcp__docs__lookup", "arguments": {}}</tool_call>"#;
        let short = calls_in_text(call, O);
        assert_eq!(short.len(), 1, "a reply that is just the call is taken");
        let prose = format!(
            "{} {call}",
            "The page says the following about the project. ".repeat(20)
        );
        assert!(
            calls_in_text(&prose, O).is_empty(),
            "a quote inside a long reply is not a call to an outside tool"
        );
        let local = format!(
            "{} {}",
            "Reading the file now. ".repeat(40),
            r#"<tool_call>{"name": "read_file", "arguments": {"path": "a.py"}}</tool_call>"#
        );
        assert_eq!(
            calls_in_text(&local, O).len(),
            1,
            "the workspace's own tools keep working from longer replies"
        );
    }
}
