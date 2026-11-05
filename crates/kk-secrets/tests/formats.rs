use kk_secrets::{format_specs, scan, FormatSpec};

/// A body of `n` characters that uses the whole alphabet and never spells a placeholder word.
fn body(spec: &FormatSpec, n: usize, seed: usize) -> String {
    let a: Vec<char> = spec.alphabet.chars().collect();
    (0..n)
        .map(|i| a[(i * 7 + seed * 13 + i * i + i / 3) % a.len()])
        .collect()
}

fn token(spec: &FormatSpec, n: usize, seed: usize) -> String {
    format!("{}{}", spec.prefix, body(spec, n, seed))
}

fn found_as(text: &str, rule: &str) -> bool {
    scan(text).iter().any(|h| h.rule == rule)
}

/// A hit of this rule that begins exactly at `start` (another format's prefix inside the body is a different match).
fn found_at(text: &str, rule: &str, start: usize) -> bool {
    scan(text)
        .iter()
        .any(|h| h.rule == rule && h.start == start)
}

#[test]
fn fm01_every_provider_format_is_found_at_its_shortest_and_longest_allowed_body() {
    let mut bad = Vec::new();
    for (i, spec) in format_specs().iter().enumerate() {
        for n in [spec.min, spec.max.max(spec.min).min(spec.min + 40)] {
            let t = token(spec, n, i + 1);
            if !found_as(&format!("value: {t}\n"), spec.rule) {
                bad.push(format!("{} {} at {n}", spec.rule, spec.prefix));
            }
        }
    }
    assert!(bad.is_empty(), "not found: {bad:?}");
}

#[test]
fn fm02_a_body_one_character_too_short_is_not_that_format() {
    let mut bad = Vec::new();
    for (i, spec) in format_specs().iter().enumerate() {
        if spec.min < 2 {
            continue;
        }
        let t = token(spec, spec.min - 1, i + 1);
        // an assignment-style hit is a different rule; only the format's own rule may not fire
        if found_at(&t, spec.rule, 0) {
            bad.push(format!("{} {}", spec.rule, spec.prefix));
        }
    }
    assert!(bad.is_empty(), "short bodies accepted: {bad:?}");
}

#[test]
fn fm03_fill_characters_and_placeholder_words_are_documentation_not_credentials() {
    let mut bad = Vec::new();
    for spec in format_specs() {
        let first = spec.alphabet.chars().next().unwrap();
        let filled = format!("{}{}", spec.prefix, first.to_string().repeat(spec.min));
        if found_as(&filled, spec.rule) {
            bad.push(format!("{} filled", spec.prefix));
        }
    }
    assert!(bad.is_empty(), "fill bodies accepted: {bad:?}");
    // a body that spells what it is: letters and digits only, so the alphabet does not cut it short
    let spec = format_specs()
        .into_iter()
        .find(|s| s.prefix == "npm_")
        .unwrap();
    let mut text = String::from(spec.prefix);
    let words = "YourTokenGoesHere7";
    while text.len() < spec.prefix.len() + spec.min {
        text.push_str(words);
    }
    text.truncate(spec.prefix.len() + spec.min);
    assert!(
        !found_as(&text, spec.rule),
        "placeholder words accepted: {text}"
    );
    // the same length of ordinary random letters is accepted
    assert!(found_as(&token(&spec, spec.min, 2), spec.rule));
}

#[test]
fn fm04_a_format_inside_a_longer_word_is_not_a_match() {
    for spec in format_specs() {
        let t = token(&spec, spec.min, 3);
        assert!(
            !found_at(&format!("x{t}"), spec.rule, 1),
            "{} glued to a word",
            spec.prefix
        );
    }
}

#[test]
fn fm05_the_exact_shapes_from_the_provider_documents_are_found() {
    let hex = |n: usize| {
        "0123456789abcdef"
            .chars()
            .cycle()
            .skip(3)
            .take(n)
            .collect::<String>()
    };
    let alnum = |n: usize| {
        "aB3dE5gH7jK9mN2pQ4rS6tU8vW1xY0zC"
            .chars()
            .cycle()
            .take(n)
            .collect::<String>()
    };
    let j = |p: &[&str]| p.concat();
    let cases: Vec<(&str, String)> = vec![
        (
            "slack webhook url",
            j(&[
                "https://hooks.slack.",
                "com/services/T0123ABCD/B0456EFGH/",
                &alnum(24),
            ]),
        ),
        (
            "telegram bot token",
            j(&["bot", " 123456789", ":A", &alnum(34)]),
        ),
        ("azure client secret", j(&["abc", "8Q~", &alnum(34)])),
        ("mailchimp api key", j(&[&hex(32), "-us", "18"])),
        (
            "gem server credential",
            j(&["BUNDLE_ENTERPRISE__CONTRIBSYS__COM=", &hex(8), ":", &hex(8)]),
        ),
        (
            "age secret key",
            j(&[
                "AGE-SECRET-",
                "KEY-1",
                &"QPZRY9X8GF2TVDW0S3JN54KHCE6MUA7L"
                    .chars()
                    .cycle()
                    .take(58)
                    .collect::<String>(),
            ]),
        ),
    ];
    for (rule, text) in cases {
        assert!(
            found_as(&text, rule),
            "{rule}: {:?}",
            scan(&text).iter().map(|h| h.rule).collect::<Vec<_>>()
        );
    }
}
