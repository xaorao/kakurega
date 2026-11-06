//! Measures how often the reader flags real documents. Not part of the normal run:
//!   KK_INJECTION_CORPUS=/path/to/docs cargo test -p kk-types --test injection_corpus -- --ignored --nocapture
//! Every markdown and text file under the folder (up to 4000 files of at most 300 KB) is scanned; the flagged ones are listed
//! with their reasons so the false alarms can be read, not only counted.
use kk_types::injection::scan;
use std::collections::BTreeMap;
use std::path::Path;

/// The phrase list the memory crate used before the shared reader, kept here only to compare the two on the same files.
const OLD: &[&str] = &[
    "ignore all previous",
    "ignore previous",
    "ignore the previous",
    "ignore prior",
    "ignore the above",
    "ignore all prior",
    "ignore your instructions",
    "ignore any instructions",
    "disregard all",
    "disregard previous",
    "disregard the previous",
    "disregard your",
    "forget your instructions",
    "forget all previous",
    "forget previous instructions",
    "new instructions:",
    "system prompt",
    "you are now",
    "you must now",
    "from now on you",
    "act as ",
    "pretend to be",
    "pretend you are",
    "reply only with",
    "respond only with",
    "answer only with",
    "do not tell the user",
    "don't tell the user",
    "without telling the user",
    "override your",
    "jailbreak",
    "developer mode",
];
fn old_flags(text: &str) -> bool {
    let t = text.to_lowercase();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    OLD.iter().any(|p| t.contains(p))
}

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name == "node_modules" || name == ".git" || name == "target" || name == "vendor" {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk(&p, out);
        } else if ft.is_file()
            && matches!(
                p.extension().and_then(|x| x.to_str()),
                Some("md" | "txt" | "rst" | "mdx")
            )
        {
            out.push(p);
        }
        if out.len() >= 4000 {
            return;
        }
    }
}

#[test]
#[ignore]
fn corpus_flag_rate() {
    let Ok(root) = std::env::var("KK_INJECTION_CORPUS") else {
        println!("set KK_INJECTION_CORPUS to a folder of documents");
        return;
    };
    let mut files = Vec::new();
    walk(Path::new(&root), &mut files);
    files.sort();
    let (mut total, mut flagged, mut old) = (0usize, 0usize, 0usize);
    let mut by_reason: BTreeMap<&str, usize> = BTreeMap::new();
    for f in &files {
        let Ok(meta) = std::fs::metadata(f) else {
            continue;
        };
        if meta.len() > 300_000 {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        total += 1;
        let s = scan(&text);
        if old_flags(&text) {
            old += 1;
        }
        if s.suspicious() {
            flagged += 1;
            let names: Vec<&str> = s.reasons().iter().map(|r| r.name()).collect();
            for n in &names {
                *by_reason.entry(n).or_default() += 1;
            }
            println!("FLAG {} {:?}", f.display(), names);
            if let Ok(want) = std::env::var("KK_INJECTION_REASON") {
                // the lines that alone give that reason, so the false alarms can be read
                let mut shown = 0;
                for (n, line) in text.lines().enumerate() {
                    if shown < 2 && scan(line).reasons().iter().any(|r| r.name() == want) {
                        println!(
                            "LINE {}:{} {}",
                            f.display(),
                            n + 1,
                            line.chars().take(220).collect::<String>()
                        );
                        shown += 1;
                    }
                }
            }
        }
    }
    println!(
        "documents {total}, flagged {flagged} ({:.1}%); the old phrase list flagged {old} ({:.1}%)",
        100.0 * flagged as f64 / total.max(1) as f64,
        100.0 * old as f64 / total.max(1) as f64
    );
    for (r, n) in by_reason {
        println!("  {r}: {n}");
    }
}
