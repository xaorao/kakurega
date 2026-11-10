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

#[cfg(test)]
mod tests {
    use super::scrub_home_paths as scrub;

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
