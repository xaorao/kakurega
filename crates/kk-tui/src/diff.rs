//! Line numbers for a unified diff. Each changed or context line keeps its `+`, `-` or space first, so the change reads the same
//! with no colour; the old and new line numbers follow, then the text.

/// `lines` as a service prints them (`--- a`, `+++ b`, `@@ -1,3 +1,4 @@`, then the hunk). Lines that are not part of a hunk, and a
/// hunk header that cannot be read, are passed through unchanged, so nothing is ever dropped or invented. The lengths in the hunk
/// header say where a hunk ends, so a removed line that itself starts with `--` is not taken for a file header.
pub fn number(lines: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut at: Option<Hunk> = None;
    let width = lines
        .iter()
        .filter_map(|l| header(l))
        .map(|h| h.old.max(h.new).saturating_add(lines.len() as u64))
        .max()
        .unwrap_or(0)
        .to_string()
        .len()
        .max(3);
    for l in lines {
        let inside = at
            .as_ref()
            .is_some_and(|h| h.old_left > 0 || h.new_left > 0);
        // a hunk line always starts with +, - or a space, so an `@@` line is a new hunk header whatever the counts said
        if !inside || l.starts_with("@@") {
            at = header(l);
            out.push(l.clone());
            continue;
        }
        let Some(h) = at.as_mut() else { unreachable!() };
        let (mark, text) = match l.chars().next() {
            Some(c @ ('+' | '-' | ' ')) => (c, &l[1..]),
            _ => {
                out.push(l.clone());
                continue;
            }
        };
        let (o, n) = match mark {
            '+' => (String::new(), h.new.to_string()),
            '-' => (h.old.to_string(), String::new()),
            _ => (h.old.to_string(), h.new.to_string()),
        };
        if mark != '+' {
            h.old = h.old.saturating_add(1);
            h.old_left = h.old_left.saturating_sub(1);
        }
        if mark != '-' {
            h.new = h.new.saturating_add(1);
            h.new_left = h.new_left.saturating_sub(1);
        }
        out.push(format!("{mark}{o:>width$} {n:>width$} {text}"));
    }
    out
}

struct Hunk {
    old: u64,
    new: u64,
    old_left: u64,
    new_left: u64,
}

/// A hunk header `@@ -a[,b] +c[,d] @@`: where it starts in the old and new file and how many lines of each it covers (1 if not stated).
fn header(l: &str) -> Option<Hunk> {
    let rest = l.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let new = rest.split(" @@").next()?;
    let part = |s: &str| -> Option<(u64, u64)> {
        let mut it = s.split(',');
        let start = it.next()?.parse::<u64>().ok()?;
        let len = match it.next() {
            Some(n) => n.parse::<u64>().ok()?,
            None => 1,
        };
        Some((start, len))
    };
    let ((os, ol), (ns, nl)) = (part(old)?, part(new)?);
    Some(Hunk {
        old: os,
        new: ns,
        old_left: ol,
        new_left: nl,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn df01_changed_and_context_lines_get_old_and_new_numbers_and_keep_their_marker_first() {
        let d = v(&[
            "--- a/s.py",
            "+++ b/s.py",
            "@@ -10,4 +10,4 @@",
            " keep",
            "-old line",
            "+new line",
            " tail",
            "@@ -40,2 +40,3 @@",
            " x",
            "+added",
            " y",
        ]);
        let n = number(&d);
        assert_eq!(n[..3], d[..3], "file and hunk headers are untouched");
        assert_eq!(n[3], "  10  10 keep");
        assert_eq!(n[4], "- 11     old line");
        assert_eq!(n[5], "+     11 new line");
        assert_eq!(n[6], "  12  12 tail");
        assert_eq!(n[7], "@@ -40,2 +40,3 @@");
        assert_eq!(n[8], "  40  40 x");
        assert_eq!(n[9], "+     41 added");
        assert_eq!(n[10], "  41  42 y");
        for (a, b) in d.iter().zip(&n) {
            assert_eq!(
                a.chars().next(),
                b.chars().next(),
                "the first character (the marker) never changes"
            );
        }
    }

    #[test]
    fn df02_anything_that_is_not_a_readable_hunk_is_passed_through_unchanged() {
        for odd in [
            v(&[]),
            v(&["no diff here", "+not in a hunk"]),
            v(&["@@ nonsense @@", "+x", "-y"]),
            v(&["@@ -a,b +c,d @@", "+x"]),
            v(&["--- a", "+++ b"]),
        ] {
            assert_eq!(number(&odd), odd);
        }
        // a line inside a hunk with an unexpected first character is kept as it is
        let n = number(&v(&[
            "@@ -1,1 +1,1 @@",
            "\\ No newline at end of file",
            "+x",
        ]));
        assert_eq!(n[1], "\\ No newline at end of file");
        assert_eq!(n[2], "+      1 x");
    }

    #[test]
    fn df04_a_removed_line_that_starts_with_dashes_is_not_taken_for_a_file_header() {
        let d = v(&[
            "--- a/q.sql",
            "+++ b/q.sql",
            "@@ -5,3 +5,2 @@",
            " select 1;",
            "--- drop table x; -- gone",
            "+-- kept",
            " select 2;",
            "--- a/r.sql",
            "+++ b/r.sql",
            "@@ -1,1 +1,1 @@",
            "-a",
            "+b",
        ]);
        let n = number(&d);
        assert_eq!(n[4], "-  6     -- drop table x; -- gone");
        assert_eq!(n[5], "+      6 -- kept");
        assert_eq!(n[6], "   7   7 select 2;");
        assert_eq!(
            n[7..10],
            d[7..10],
            "the next file's headers are still headers"
        );
        assert_eq!(n[10], "-  1     a");
        assert_eq!(n[11], "+      1 b");
    }

    #[test]
    fn df05_a_hunk_header_without_lengths_covers_one_line_each() {
        let n = number(&v(&["@@ -3 +3 @@", "-a", "+b", " not part of the hunk"]));
        assert_eq!(n[1], "-  3     a");
        assert_eq!(n[2], "+      3 b");
        assert_eq!(
            n[3], " not part of the hunk",
            "the hunk ended after one line of each"
        );
    }

    #[test]
    fn df03_a_hostile_header_cannot_overflow_or_panic() {
        let n = number(&v(&[
            "@@ -18446744073709551615,1 +18446744073709551615,1 @@",
            " a",
            " b",
            "-c",
        ]));
        assert_eq!(n.len(), 4);
        let n = number(&v(&["@@ -99999999999999999999999,1 +1,1 @@", "+x"]));
        assert_eq!(n, v(&["@@ -99999999999999999999999,1 +1,1 @@", "+x"]));
    }
}
