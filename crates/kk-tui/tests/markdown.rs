use kk_tui::markdown::{block_json, inline, parse, render, safe_href, Inline};
use kk_tui::ui::Theme;
use serde_json::Value;

fn plain(lines: &[ratatui::text::Line<'static>]) -> Vec<String> {
    lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

fn th() -> Theme {
    Theme { color: true }
}

/// The links' `host` field is the web reader's own bookkeeping; the terminal shows the whole address and keeps no host.
fn without_host(v: &mut Value) {
    match v {
        Value::Object(m) => {
            m.remove("host");
            // the browser writes an address in its normal form (a bare host gains a final slash); the terminal shows what was written
            if let Some(Value::String(h)) = m.get_mut("href") {
                *h = h.trim_end_matches('/').to_string();
            }
            m.values_mut().for_each(without_host);
        }
        Value::Array(a) => a.iter_mut().for_each(without_host),
        _ => {}
    }
}

#[test]
fn md01_the_terminal_reader_makes_the_same_tree_as_the_web_reader_for_every_shared_case() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/markdown_cases.json"
    ))
    .unwrap();
    let cases: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert!(cases.len() >= 40);
    let mut different = Vec::new();
    for c in &cases {
        let input = c["input"].as_str().unwrap();
        let mut got = Value::Array(parse(input).iter().map(block_json).collect());
        without_host(&mut got);
        let mut want = c["blocks"].clone();
        without_host(&mut want);
        if got != want {
            different.push(format!("{input:?}\n  web:      {want}\n  terminal: {got}"));
        }
    }
    assert!(
        different.is_empty(),
        "{} case(s) differ:\n{}",
        different.len(),
        different.join("\n")
    );
}

#[test]
fn md02_links_show_their_whole_address_and_only_plain_https_ones_are_links() {
    assert_eq!(
        safe_href("https://example.com/a?b=1#c").as_deref(),
        Some("https://example.com/a?b=1#c")
    );
    for bad in [
        "http://example.com",
        "javascript:alert(1)",
        "/relative",
        "https://",
        "https://user:pw@example.com",
        "https://exa mple.com",
        "https://example.com/<x>",
        "https://.example.com",
        "file:///etc/passwd",
        "https://a.b/\u{1b}[2J",
    ] {
        assert!(safe_href(bad).is_none(), "{bad}");
    }
    assert!(safe_href(&format!("https://example.com/{}", "a".repeat(2001))).is_none());
    let lines = plain(&render(
        "see [docs](https://example.com/guide) and [https://example.com](https://example.com)",
        80,
        &th(),
    ));
    assert_eq!(
        lines,
        ["see docs (https://example.com/guide) and https://example.com"],
        "the text never hides the address, and an address that is its own text is shown once"
    );
    let lines = plain(&render("[click here](http://evil.test/x)", 80, &th()));
    assert_eq!(
        lines,
        ["click here (http://evil.test/x)"],
        "an unsafe link is only words with the address visible"
    );
    assert!(matches!(
        inline("![alt](https://x.test/p.png)")[0],
        Inline::Text(_)
    ));
    assert_eq!(
        plain(&render("![a cat](https://x.test/p.png)", 80, &th())),
        ["[image: a cat]"],
        "no image is ever fetched or shown as a link"
    );
}

#[test]
fn md03_lists_quotes_headings_and_code_render_in_plain_shapes() {
    let md = "# Title\n\nSome **bold** and `code`.\n\n- one\n- two with a very long line that has to wrap around the narrow screen nicely\n\n1. first\n2. second\n\n> a quote\n\n```python\nprint(1)\n```";
    let lines = plain(&render(md, 30, &th()));
    assert_eq!(lines[0], "Title");
    assert!(lines.contains(&"Some bold and code.".to_string()));
    assert!(lines.contains(&"• one".to_string()));
    let two = lines.iter().position(|l| l.starts_with("• two")).unwrap();
    assert!(
        lines[two + 1].starts_with("  "),
        "a wrapped item hangs under its text: {:?}",
        lines[two + 1]
    );
    assert!(lines.contains(&"1. first".to_string()) && lines.contains(&"2. second".to_string()));
    assert!(lines.contains(&"│ a quote".to_string()));
    assert!(lines.contains(&"  python".to_string()) && lines.contains(&"  print(1)".to_string()));
    for l in &lines {
        assert!(l.chars().count() <= 30, "{l:?} is wider than the screen");
    }
}

#[test]
fn md04_nothing_in_the_text_can_become_a_control_sequence_and_hostile_input_finishes() {
    let hostile = "**bold\u{1b}[2J** `\u{7}code` [l\u{1b}](https://example.com/\u{1b}[31m) ```\n\u{1b}]52;c;AAAA\u{7}\n```\n";
    // text reaches this reader already cleaned of control characters; if one ever does arrive it is not turned into a link or markup
    for l in render(hostile, 40, &th()) {
        assert!(
            l.spans
                .iter()
                .all(|s| !s.content.chars().any(|c| c.is_control())),
            "{l:?}"
        );
    }
    for nasty in [
        "*".repeat(50_000),
        "`".repeat(50_000),
        "[".repeat(200_000),
        "![".repeat(100_000),
        "- ".repeat(30_000),
        "> ".repeat(30_000),
        "#".repeat(60_000),
        format!("{}{}", "**a".repeat(20_000), "*"),
        "```\n".repeat(20_000),
    ] {
        let t = std::time::Instant::now();
        let n = render(&nasty, 80, &th()).len();
        assert!(
            t.elapsed() < std::time::Duration::from_secs(8),
            "{:?}",
            t.elapsed()
        );
        assert!(n < 400_000);
    }
}

#[test]
fn md05_wrapping_never_exceeds_the_width_cuts_long_words_and_keeps_wide_characters_whole() {
    for width in [8usize, 12, 20, 40] {
        for text in [
            "a b c d e f g h i j k l m n o p q r s t u v w x y z",
            "supercalifragilisticexpialidocious_and_more_and_more",
            "日本語の文章はスペースがありません日本語の文章はスペースがありません",
            "🙂 ".repeat(30).as_str(),
        ] {
            for l in plain(&render(text, width, &th())) {
                let w: usize = l
                    .chars()
                    .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(0))
                    .sum();
                assert!(w <= width, "{width}: {l:?} is {w} wide");
            }
        }
    }
    let joined: String = plain(&render("alpha beta gamma delta epsilon zeta", 12, &th())).join(" ");
    assert_eq!(
        joined.split_whitespace().collect::<Vec<_>>(),
        ["alpha", "beta", "gamma", "delta", "epsilon", "zeta"],
        "no word is lost"
    );
}

#[test]
fn md06_code_keeps_its_spaces_and_tabs_and_a_wide_line_is_cut_not_reflowed() {
    let lines = plain(&render("```\n    indented\n\tx = 1\n```", 40, &th()));
    assert_eq!(lines, ["      indented", "      x = 1"]);
    let lines = plain(&render(
        &format!("```\n{}\n```", "x".repeat(100)),
        20,
        &th(),
    ));
    assert_eq!(lines.len(), 1);
    assert!(lines[0].chars().count() <= 20);
}

#[test]
fn md07_blocks_are_separated_by_one_blank_line_and_nothing_else_is() {
    let lines = plain(&render(
        "first paragraph\n\nsecond paragraph\n\n- a\n- b\n\n> q\n\n```\ncode\n```",
        40,
        &th(),
    ));
    assert_eq!(
        lines,
        [
            "first paragraph",
            "",
            "second paragraph",
            "",
            "• a",
            "• b",
            "",
            "│ q",
            "",
            "  code"
        ]
    );
}
