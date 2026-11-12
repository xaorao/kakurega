//! The diagnostics report as plain text. It holds versions, the machine checks and how they came out, counts, and the kinds of
//! the last events. It never holds a file name, a path, the text of a note, a conversation or a file, or a key: the service
//! builds the data that way, and the owner reads it all before sharing it.

use serde_json::Value;

/// The report as text.
pub fn render(v: &Value) -> String {
    let mut out = String::new();
    let s = |x: &Value| x.as_str().unwrap_or("?").to_string();
    out.push_str(&format!(
        "Kakurega diagnostics\nversion {}   {} {}   {}\n",
        s(&v["version"]),
        s(&v["os"]),
        s(&v["arch"]),
        if v["test_mode"] == true {
            "TEST MODE"
        } else {
            "normal mode"
        }
    ));
    let r = &v["route"];
    out.push_str(&format!(
        "model route: {} ({}), {}\n",
        s(&r["model"]).replace("?", "none"),
        s(&r["class"]),
        if r["stays_here"] == true {
            "runs on this computer"
        } else {
            "does not run on this computer"
        }
    ));
    out.push_str("\nChecks\n");
    for c in v["checks"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  [{}] {}{}: {}\n",
            if c["ok"] == true { "ok" } else { "FAILED" },
            s(&c["id"]),
            if c["required"] == false {
                " (optional)"
            } else {
                ""
            },
            s(&c["detail"])
        ));
    }
    out.push_str(&format!(
        "\nCounts\n  reviews {}\n  kept conversations {}\n  signed log entries {}\n",
        v["counts"]["reviews"], v["counts"]["kept_conversations"], v["counts"]["log_entries"]
    ));
    out.push_str("\nKinds of the last 200 events\n");
    if let Some(k) = v["recent_event_kinds"].as_object() {
        for (name, n) in k {
            out.push_str(&format!("  {name}: {n}\n"));
        }
    }
    out.push_str("\nCrashes recorded on this computer\n");
    let crashes: Vec<&Value> = v["crashes"].as_array().into_iter().flatten().collect();
    if crashes.is_empty() {
        out.push_str("  none\n");
    }
    for c in crashes {
        out.push_str(&format!(
            "  unix time {}, version {}, thread {}, in {}\n",
            c["at"],
            s(&c["version"]),
            s(&c["thread"]),
            s(&c["place"])
        ));
    }
    if v["crashes"].as_array().is_some_and(|a| !a.is_empty()) {
        out.push_str("  (A crash report holds no message and no personal part of a path; it is shown here and sent nowhere.)\n");
    }
    out.push_str("\nThis report holds no file names, paths, note, conversation or file text, keys or tokens.\n");
    out
}

/// Replaces the user name in any home-folder path (`/home/NAME/`, `/Users/NAME/`, `C:\Users\NAME\`, in either case) with `~`,
/// whoever's home it is: a check that ran in a virtual machine or under another account names that user, not only the owner.
pub fn scrub_home_paths(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let lower = text.to_ascii_lowercase();
    let mut i = 0;
    while i < text.len() {
        let rest = &lower[i..];
        let hit = ["/home/", "/users/", "c:\\users\\", "c:/users/"]
            .iter()
            .find(|m| rest.starts_with(**m));
        if let Some(m) = hit {
            let start = i + m.len();
            // a name runs to the next separator when one comes before any quote or bracket (names may hold spaces); otherwise to
            // the first quote, bracket or space
            let window: String = text[start..].chars().take(64).collect();
            let hard = window.find(['"', '\'', ',', ';', ')', ']']);
            let slash = window.find(['/', '\\']);
            let name_len = match (slash, hard) {
                _ if window.starts_with(' ') => 0,
                (Some(sl), Some(h)) if sl < h => sl,
                (Some(sl), None) => sl,
                (_, Some(h)) => h,
                (None, None) => window.find(' ').unwrap_or(window.len()),
            };
            if name_len > 0 {
                out.push('~');
                i = start + name_len;
                continue;
            }
        }
        let ch = text[i..].chars().next().expect("a character");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Every path in the text (`/…`, `~/…`, `~`, or a drive path like `C:\…`) as `<path>`, so a report that says it holds no
/// paths holds none: a path outside the home folder can name a person or a project too. Words after a path that carry
/// a slash are taken as the rest of a path with spaces in it.
pub fn scrub_paths(text: &str) -> String {
    let opening: &[char] = &['(', '"', '\'', '[', '`'];
    let closing: &[char] = &[')', '"', '\'', ',', ';', ':', '.', ']', '`'];
    // words with the white space after each, so the text keeps its own spaces, tabs and line breaks
    let mut words: Vec<(&str, &str)> = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let w_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let after = &rest[w_end..];
        let s_end = after
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(after.len());
        words.push((&rest[..w_end], &after[..s_end]));
        rest = &after[s_end..];
    }
    let split = |token: &str| -> (usize, usize) {
        let lead = token.len() - token.trim_start_matches(opening).len();
        let end = token.trim_end_matches(closing).len().max(lead);
        (lead, end)
    };
    let starts_path = |token: &str| {
        let (lead, end) = split(token);
        let core = &token[lead..end];
        let b = core.as_bytes();
        let drive = b.len() >= 3
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'\\' | b'/');
        (core.starts_with('/') && core.len() > 1) || core.starts_with("~/") || core == "~" || drive
    };
    let slashed = |token: &str| token.contains(['/', '\\']) && !token.contains("://");
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < words.len() {
        let (word, space) = words[i];
        if !starts_path(word) {
            out.push_str(word);
            out.push_str(space);
            i += 1;
            continue;
        }
        // a path may hold spaces: it runs on, within its line and before the next path, to the last word with a slash
        let mut last = i;
        let mut j = i;
        while j + 1 < words.len() && !words[j].1.contains('\n') {
            j += 1;
            if starts_path(words[j].0) {
                break;
            }
            if slashed(words[j].0) {
                last = j;
            }
        }
        let (lead, _) = split(word);
        let (_, end) = split(words[last].0);
        out.push_str(&word[..lead]);
        out.push_str("<path>");
        out.push_str(&words[last].0[end..]);
        out.push_str(words[last].1);
        i = last + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::scrub_home_paths as scrub;

    #[test]
    fn rp02_no_path_of_any_kind_is_left() {
        use super::scrub_paths as paths;
        assert_eq!(
            paths("worker: /private/tmp/alice/userbuild/bin/kk-worker is missing"),
            "worker: <path> is missing"
        );
        assert_eq!(
            paths("(see ~/notes/a.md), then ~"),
            "(see <path>), then <path>"
        );
        assert_eq!(paths("at C:\\Users\\x\\a and D:/b"), "at <path> and <path>");
        assert_eq!(paths("in /Volumes/My Disk/x: gone"), "in <path>: gone");
        assert_eq!(
            paths("at ~/Acme Corp Merger/notes.txt: gone"),
            "at <path>: gone"
        );
        assert_eq!(
            paths("first\n/private/tmp/bob/x\tand ~/y here"),
            "first\n<path>\tand <path> here"
        );
        assert_eq!(
            paths("worker ~/kk-target/debug/kk-worker, ~/x"),
            "worker <path>, <path>"
        );
        // addresses, versions and plain words stay
        assert_eq!(
            paths("http://127.0.0.1:11434 answers; rustc 1.93, 1/2 and / alone"),
            "http://127.0.0.1:11434 answers; rustc 1.93, 1/2 and / alone"
        );
    }

    #[test]
    fn rp01_the_user_name_of_any_home_path_is_replaced() {
        assert_eq!(
            scrub("worker: /home/alice.guest/kk-target/debug/kk-worker"),
            "worker: ~/kk-target/debug/kk-worker"
        );
        assert_eq!(scrub("in /Users/Bob Smith/x here"), "in ~/x here");
        assert_eq!(scrub("at C:\\Users\\carol\\AppData\\x"), "at ~\\AppData\\x");
        assert_eq!(
            scrub("see c:/users/dave/p and /HOME/erin/q"),
            "see ~/p and ~/q"
        );
        assert_eq!(scrub("(/home/frank), '/home/gina'"), "(~), '~'");
        assert_eq!(scrub("home is /home/hal and more"), "home is ~ and more");
        // text with no home path, and an empty name, are left alone
        assert_eq!(
            scrub("rustc 1.93 and gcc; editions up to 2024"),
            "rustc 1.93 and gcc; editions up to 2024"
        );
        assert_eq!(scrub("/home/ and /users/"), "/home/ and /users/");
        assert_eq!(scrub("håndbok /home/zoë/ü"), "håndbok ~/ü");
    }
}
