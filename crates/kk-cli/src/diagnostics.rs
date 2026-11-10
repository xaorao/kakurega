//! `kaku diagnostics [--out FILE]`: a plain-text report the owner can read before sharing it. It holds versions, the machine
//! checks and how they came out, counts, and the kinds of the last events. It never holds a file name, a path, the text of
//! a note, a conversation or a file, or a key: the service builds it that way, and the owner reads it all here first.


pub use kk_service::report::render;

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

    #[test]
    fn dg02_crashes_are_listed_in_plain_words_or_said_to_be_none() {
        let none = render(
            &json!({"version":"1","os":"macos","arch":"arm64","test_mode":false,"route":{},"checks":[],"counts":{},"recent_event_kinds":{},"crashes":[]}),
        );
        assert!(
            none.contains("Crashes recorded on this computer\n  none\n"),
            "{none}"
        );
        let some = render(
            &json!({"version":"1","os":"macos","arch":"arm64","test_mode":false,"route":{},"checks":[],"counts":{},"recent_event_kinds":{},"crashes":[{"at":1700000000u64,"version":"0.1.0","thread":"main","place":"crates/a/src/b.rs:3"}]}),
        );
        assert!(
            some.contains(
                "unix time 1700000000, version 0.1.0, thread main, in crates/a/src/b.rs:3"
            ),
            "{some}"
        );
        assert!(
            some.contains("holds no message and no personal part of a path"),
            "{some}"
        );
    }
}
