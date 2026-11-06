//! `kaku diagnostics [--out FILE]`: a plain-text report the owner can read before sharing it. It holds versions, the machine
//! checks and how they came out, counts, and the kinds of the last events. It never holds a file name, a path, the text of
//! a note, a conversation or a file, or a key: the service builds it that way, and the owner reads it all here first.

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
    out.push_str("\nThis report holds no file names, paths, note, conversation or file text, keys or tokens.\n");
    out
}

pub fn run(args: &[String]) -> u8 {
    let admin = match crate::approve::connect(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let v = match admin.get("/admin/diagnostics") {
        Ok((200, v)) => v,
        Ok((code, _)) => {
            eprintln!("the service refused ({code})");
            return 1;
        }
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let text = render(&v);
    match crate::flag(args, "--out") {
        Some(path) => match kk_vault::durable::write_atomic(
            std::path::Path::new(&path),
            text.as_bytes(),
            0o600,
        ) {
            Ok(()) => println!("Saved {path}. Read it before you share it."),
            Err(e) => {
                eprintln!("cannot write {path}: {e}");
                return 1;
            }
        },
        None => print!("{text}"),
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dg01_the_report_reads_in_plain_words_and_marks_failures() {
        let v = json!({"version": "0.0.1", "os": "macos", "arch": "aarch64", "test_mode": false,
            "route": {"id": "ollama:m", "class": "device_only", "model": "m", "stays_here": true},
            "checks": [{"id": "ollama", "ok": true, "required": true, "detail": "answers"}, {"id": "vm", "ok": false, "required": false, "detail": "not started"}],
            "counts": {"reviews": 2, "kept_conversations": 1, "log_entries": 30},
            "recent_event_kinds": {"chat.turn": 4, "session.kept": 1}});
        let t = render(&v);
        for want in [
            "version 0.0.1   macos aarch64   normal mode",
            "model route: m (device_only), runs on this computer",
            "[ok] ollama: answers",
            "[FAILED] vm (optional): not started",
            "reviews 2",
            "chat.turn: 4",
            "holds no file names",
        ] {
            assert!(t.contains(want), "{want}\n{t}");
        }
        assert!(
            render(&json!({})).contains("Kakurega diagnostics"),
            "a damaged answer still renders"
        );
    }
}
