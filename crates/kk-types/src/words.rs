//! Small helpers for the sentences the program itself writes.

/// "1 file(s)" becomes "1 file", "3 file(s)" becomes "3 files", and "1 note(s) were" becomes "1 note was". The count is
/// the nearest number within the three words before the counted word; with no number in reach the word is made plural.
pub fn counted(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("(s)") {
        let before = &rest[..at];
        let one = before
            .split_whitespace()
            .rev()
            .take(4)
            .find_map(|w| {
                let digits: String = w
                    .chars()
                    .filter(|c| c.is_ascii_digit() || *c == ',')
                    .collect();
                let plain = w.trim_matches(|c: char| !c.is_ascii_alphanumeric());
                (!digits.is_empty() && plain.chars().all(|c| c.is_ascii_digit() || c == ','))
                    .then(|| digits == "1")
            })
            .unwrap_or(false);
        out.push_str(before);
        if !one {
            out.push('s');
        }
        let mut after = &rest[at + 3..];
        if one {
            for (many, single) in [
                (" were ", " was "),
                (" are ", " is "),
                (" hold ", " holds "),
            ] {
                if let Some(tail) = after.strip_prefix(many) {
                    out.push_str(single);
                    after = tail;
                    break;
                }
            }
        }
        rest = after;
    }
    out.push_str(rest);
    out
}
