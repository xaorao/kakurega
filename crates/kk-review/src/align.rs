//! Making a proposed change fit the file it is meant for, when it differs from the file only in ways nobody would call a
//! different text: curly quotes, dashes and odd spaces written as plain ones, line endings, spaces at the end of lines.
//!
//! A model that copies code back often does one of these without meaning to, and a change refused for it is a verified fix
//! lost for nothing. The rule is strict about the rest. The exact text always wins; a looser reading is tried only when the
//! exact text is nowhere in the file, one looseness at a time from the smallest, and only an answer with exactly one place in
//! the file is used. What is applied, shown and checked afterwards is the file's own text at that place, never the model's
//! copy of it, and lines the change leaves alone keep their own bytes.

use crate::check::{occurrences, Edit, Fix};
use crate::import::ImportedFile;
use std::collections::BTreeMap;

const UNICODE_DASHES: &[char] = &[
    '\u{2010}', '\u{2011}', '\u{2012}', '\u{2013}', '\u{2014}', '\u{2015}', '\u{2212}',
];
const UNICODE_SPACES: &[char] = &[
    '\u{a0}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}', '\u{2007}', '\u{2008}',
    '\u{2009}', '\u{200a}', '\u{202f}', '\u{205f}', '\u{3000}',
];

/// One character as a plain one when it is a look-alike, so the comparison is character for character.
fn plain(c: char) -> char {
    match c {
        '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}' => '\'',
        '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{201f}' => '"',
        c if UNICODE_DASHES.contains(&c) => '-',
        c if UNICODE_SPACES.contains(&c) => ' ',
        c => c,
    }
}

fn plain_str(s: &str) -> String {
    s.replace("\r\n", "\n").chars().map(plain).collect()
}

/// Where `old` is in `text` when look-alike characters and line endings are not told apart. Byte ranges of the text itself.
fn loose_matches(text: &str, old: &str) -> Vec<(usize, usize)> {
    let want: Vec<char> = plain_str(old).chars().collect();
    if want.is_empty() {
        return Vec::new();
    }
    // the text as plain characters, each with where it starts and ends in the real text; a CR before an LF is dropped
    let mut view: Vec<(char, usize, usize)> = Vec::new();
    let mut it = text.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if c == '\r' && it.peek().is_some_and(|(_, n)| *n == '\n') {
            continue;
        }
        view.push((plain(c), i, i + c.len_utf8()));
    }
    let mut out = Vec::new();
    let mut from = 0;
    while from + want.len() <= view.len() {
        if view[from..from + want.len()]
            .iter()
            .map(|v| v.0)
            .eq(want.iter().copied())
        {
            // a CRLF at the very end belongs to the match when the wanted text ends with a line break
            let mut end = view[from + want.len() - 1].2;
            if want.last() == Some(&'\n') && text[..end].ends_with('\n') {
                end = view[from + want.len() - 1].2;
            }
            out.push((view[from].1, end));
            if out.len() > 2 {
                break;
            }
        }
        from += 1;
    }
    out
}

/// Where `old` is in `text` line for line when the ends of lines are not compared (spaces and tabs there are ignored).
fn line_matches(text: &str, old: &str) -> Vec<(usize, usize)> {
    let ends_with_break = old.ends_with('\n');
    let want: Vec<String> = {
        let mut v: Vec<String> = plain_str(old)
            .split('\n')
            .map(|l| l.trim_end_matches([' ', '\t']).to_string())
            .collect();
        if ends_with_break {
            v.pop();
        }
        v
    };
    if want.is_empty() || want.iter().all(String::is_empty) {
        return Vec::new();
    }
    // the lines of the text with where each starts and where its content and its break end
    let mut lines: Vec<(String, usize, usize, usize)> = Vec::new();
    let mut at = 0;
    for raw in text.split_inclusive('\n') {
        let content = raw.strip_suffix('\n').unwrap_or(raw);
        let content = content.strip_suffix('\r').unwrap_or(content);
        lines.push((
            plain_str(content).trim_end_matches([' ', '\t']).to_string(),
            at,
            at + content.len(),
            at + raw.len(),
        ));
        at += raw.len();
    }
    let mut out = Vec::new();
    for i in 0..lines.len() {
        if i + want.len() > lines.len() {
            break;
        }
        if lines[i..i + want.len()]
            .iter()
            .map(|l| l.0.as_str())
            .eq(want.iter().map(String::as_str))
        {
            let last = &lines[i + want.len() - 1];
            let end = if ends_with_break { last.3 } else { last.2 };
            if ends_with_break && !text[..end].ends_with('\n') {
                continue;
            }
            out.push((lines[i].1, end));
            if out.len() > 2 {
                break;
            }
        }
    }
    out
}

/// The change, made to fit the file, or `None` when it already does (the exact text is there) or cannot honestly be made to
/// (the text is nowhere, or in more than one place however loosely it is read).
pub fn align_edit(text: &str, e: &Edit) -> Option<Edit> {
    if !occurrences(text, &e.old_text).is_empty() {
        return None;
    }
    let found = {
        let loose = loose_matches(text, &e.old_text);
        if loose.is_empty() {
            line_matches(text, &e.old_text)
        } else {
            loose
        }
    };
    let [(start, end)] = found.as_slice() else {
        return None;
    };
    let region = &text[*start..*end];
    // lines the change leaves as they are keep the file's own bytes; the others are the model's
    let model_old: Vec<&str> = e
        .old_text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let file_old: Vec<&str> = region
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let key = |l: &str| plain_str(l).trim_end_matches([' ', '\t']).to_string();
    let crlf = region.contains("\r\n") && !e.new_text.contains('\r');
    let same_shape = model_old.len() == file_old.len();
    let new_lines: Vec<String> = e
        .new_text
        .split('\n')
        .map(|l| {
            let l = l.strip_suffix('\r').unwrap_or(l);
            if same_shape {
                if let Some(i) = model_old.iter().position(|m| key(m) == key(l)) {
                    return file_old[i].to_string();
                }
            }
            l.to_string()
        })
        .collect();
    let new_text = new_lines.join(if crlf { "\r\n" } else { "\n" });
    Some(Edit {
        file: e.file.clone(),
        old_text: region.to_string(),
        new_text,
    })
}

/// Every change of a fix made to fit its file. A change whose file is not text or not in the project is left as it is, and the
/// steps after this refuse it as they always did.
pub fn align_fix(files: &BTreeMap<String, &ImportedFile>, fix: Fix) -> Fix {
    let fit = |e: Edit| -> Edit {
        files
            .get(&e.file)
            .and_then(|f| {
                std::str::from_utf8(&f.bytes)
                    .ok()
                    .and_then(|t| align_edit(t, &e))
            })
            .unwrap_or(e)
    };
    let head = fit(Edit {
        file: fix.file.clone(),
        old_text: fix.old_text.clone(),
        new_text: fix.new_text.clone(),
    });
    Fix {
        title: fix.title,
        file: head.file,
        old_text: head.old_text,
        new_text: head.new_text,
        also: fix.also.into_iter().map(fit).collect(),
        explanation: fix.explanation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(old: &str, new: &str) -> Edit {
        Edit {
            file: "f.py".into(),
            old_text: old.into(),
            new_text: new.into(),
        }
    }

    fn fit(text: &str, old: &str, new: &str) -> Option<(String, String)> {
        align_edit(text, &edit(old, new)).map(|e| (e.old_text, e.new_text))
    }

    #[test]
    fn al01_the_exact_text_always_wins_and_is_left_alone() {
        assert_eq!(fit("x = 1\ny = 2\n", "y = 2", "y = 3"), None);
        // even when a looser reading would find it somewhere else too
        assert_eq!(
            fit("say(“hi”)\nsay(\"hi\")\n", "say(\"hi\")", "say(\"yo\")"),
            None
        );
        // an ambiguous exact text is not rescued by a looser reading
        assert_eq!(fit("a = 1\na = 1\n", "a = 1", "a = 2"), None);
    }

    #[test]
    fn al02_look_alike_quotes_dashes_and_spaces_are_read_as_the_plain_ones() {
        let text = "print(‘it’s’ — a\u{a0}test)\nreturn 1\n";
        let got = fit(text, "print('it's' - a test)", "print('it's' - a trial)").unwrap();
        assert_eq!(got.0, "print(‘it’s’ — a\u{a0}test)");
        assert_eq!(
            got.1, "print('it's' - a trial)",
            "the model's changed line is as the model wrote it"
        );
        // multi-byte characters elsewhere do not disturb the offsets
        let emoji = "# 🙂 ünïcode\nprint(“x”)\n";
        let got = fit(emoji, "print(\"x\")", "print(\"y\")").unwrap();
        assert_eq!(got.0, "print(“x”)");
    }

    #[test]
    fn al03_line_endings_are_not_told_apart_and_the_files_endings_are_kept() {
        let text = "a = 1\r\nb = 2\r\nc = 3\r\n";
        let got = fit(text, "b = 2\nc = 3\n", "b = 20\nc = 3\nd = 4\n").unwrap();
        assert_eq!(got.0, "b = 2\r\nc = 3\r\n");
        assert_eq!(
            got.1, "b = 20\r\nc = 3\r\nd = 4\r\n",
            "the new lines use the file's endings"
        );
        // a text that starts or stops in the middle of a line is read across the file's CRLF too
        let mid = fit("a\r\nb = 2\r\nc = 3\r\n", "= 2\nc = 3", "= 2\nc = 4").unwrap();
        assert_eq!(mid.0, "= 2\r\nc = 3");
        assert_eq!(mid.1, "= 2\r\nc = 4");
        // a file with plain endings gets plain endings even if the model wrote CRLF
        let got = fit("a\nb\n", "a\r\nb\r\n", "a\r\nB\r\n").unwrap();
        assert_eq!(got.0, "a\nb\n");
        assert!(
            !got.1.contains('\r') || got.1.contains("\r\n"),
            "{:?}",
            got.1
        );
    }

    #[test]
    fn al04_spaces_at_the_end_of_lines_are_not_compared_and_untouched_lines_keep_their_bytes() {
        let text = "def f():   \n    x = 1  \n    return x\n";
        let got = fit(
            text,
            "def f():\n    x = 1\n    return x\n",
            "def f():\n    x = 2\n    return x\n",
        )
        .unwrap();
        assert_eq!(got.0, "def f():   \n    x = 1  \n    return x\n");
        assert_eq!(
            got.1, "def f():   \n    x = 2\n    return x\n",
            "the unchanged lines keep their trailing spaces; the changed one is the model's"
        );
    }

    #[test]
    fn al05_more_than_one_place_or_none_means_no_change_is_made() {
        assert_eq!(
            fit("a = ‘1’\nb = ‘1’\n", "a = '1'\nb = '1'", "x"),
            Some(("a = ‘1’\nb = ‘1’".into(), "x".into())),
            "one place for the whole text"
        );
        assert_eq!(
            fit("x = ‘1’\ny = 2\nx = ‘1’\n", "x = '1'", "x = '2'"),
            None,
            "two places once read loosely"
        );
        assert_eq!(fit("x = 1\n", "z = '9'", "z = '8'"), None, "nowhere");
        assert_eq!(
            fit("x = 1   \nx = 1\n", "x = 1\n", "x = 2\n"),
            None,
            "two lines equal once the line ends are ignored"
        );
        // indentation is not a look-alike: a different depth is a different text
        assert_eq!(
            fit("if a:\n    b()\n", "if a:\n  b()\n", "if a:\n  c()\n"),
            None
        );
    }

    #[test]
    fn al06_nothing_to_match_or_nothing_to_find_is_refused() {
        assert_eq!(fit("abc", "", "x"), None);
        assert_eq!(fit("", "abc", "x"), None);
        assert_eq!(fit("abc", "\n", "x"), None);
        assert_eq!(
            fit("abc", "   \n  \n", "x"),
            None,
            "blank lines match nothing in particular"
        );
        assert_eq!(
            fit("a\n  \n\t\nb\n", "\n\n", "\nX\n"),
            None,
            "empty lines are not found by their blanks"
        );
    }

    #[test]
    fn al07_a_hostile_text_cannot_make_a_match_that_is_not_there() {
        // zero-width and direction characters are not look-alikes
        assert_eq!(fit("print(1)\n", "pri\u{200b}nt(1)", "x"), None);
        assert_eq!(fit("print(1)\n", "print(\u{202e}1)", "x"), None);
        // a bare CR in the middle of a line is a character, not a line ending
        assert_eq!(fit("a\rb\n", "a\nb\n", "x"), None);
        // a huge text still answers
        let big = "x = ‘1’\n".repeat(20_000);
        assert_eq!(
            fit(&big, "x = '1'", "x = '2'"),
            None,
            "twenty thousand places"
        );
    }

    #[test]
    fn al08_the_aligned_old_text_is_exactly_in_the_file_once() {
        let text = "a = “q”\nb = 2   \n";
        for (old, new) in [
            ("a = \"q\"", "a = 'r'"),
            ("a = \"q\"\nb = 2", "a = 1\nb = 2"),
        ] {
            let got = align_edit(text, &edit(old, new)).unwrap();
            assert_eq!(occurrences(text, &got.old_text).len(), 1, "{old:?}");
        }
    }
}
