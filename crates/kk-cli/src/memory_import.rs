//! `kaku memory import DIR`: bring notes from another assistant's markdown memory files into Kakurega as untrusted notes.
//!
//! The file shape handled is the one Claude Code's auto-memory writes (a small frontmatter with `name`, `description` and a
//! `type`, possibly under `metadata:`), and plain markdown files. The frontmatter reading follows the grammar documented
//! in Anamnesis's `adapter-claude-code` (Apache-2.0); this code is written for this project.

use serde_json::{json, Value};
use std::path::Path;

pub const MAX_FILES: usize = 500;
pub const MAX_FILE_BYTES: u64 = 20_000;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Note {
    pub name: String,
    pub kind: String,
    pub text: String,
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    let v = v
        .strip_prefix('"')
        .and_then(|x| x.strip_suffix('"'))
        .or_else(|| v.strip_prefix('\'').and_then(|x| x.strip_suffix('\'')))
        .unwrap_or(v);
    v.to_string()
}

/// Reads one markdown note. Returns `None` for an empty body.
pub fn parse_note(file_stem: &str, input: &str) -> Option<Note> {
    let t = input.trim_start_matches('\u{feff}');
    let (mut name, mut description, mut kind, mut body) =
        (String::new(), String::new(), String::new(), t);
    if let Some(rest) = t.strip_prefix("---") {
        if let Some(after_open) = rest.find('\n').map(|i| &rest[i + 1..]) {
            let mut offset = 0;
            let mut close = None;
            for line in after_open.split_inclusive('\n') {
                if line.trim_end_matches(['\n', '\r']) == "---" {
                    close = Some(offset);
                    break;
                }
                offset += line.len();
            }
            if let Some(c) = close {
                let yaml = &after_open[..c];
                let after_close = &after_open[c..];
                body = after_close.find('\n').map_or("", |i| &after_close[i + 1..]);
                let mut in_metadata = false;
                for line in yaml.lines() {
                    let indented = line.starts_with(' ') || line.starts_with('\t');
                    let line_t = line.trim();
                    if line_t.is_empty() || line_t.starts_with('#') {
                        continue;
                    }
                    if !indented {
                        in_metadata = line_t == "metadata:";
                    }
                    if let Some((k, v)) = line_t.split_once(':') {
                        let (k, v) = (k.trim(), unquote(v));
                        match (k, indented && in_metadata) {
                            ("name", false) => name = v,
                            ("description", false) => description = v,
                            ("type", true) => kind = v,
                            ("type", false) if kind.is_empty() => kind = v,
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    let body = body.trim();
    let text = match (description.is_empty(), body.is_empty()) {
        (_, true) if description.is_empty() => return None,
        (true, _) => body.to_string(),
        (false, true) => description.clone(),
        (false, false) => format!("{description}\n\n{body}"),
    };
    Some(Note {
        name: if name.is_empty() {
            file_stem.to_string()
        } else {
            name
        },
        kind,
        text,
    })
}

/// Hermes Agent keeps its curated memory in `MEMORY.md` (what it learned) and `USER.md` (about the user) as entries joined by
/// a line holding only `§`. Claude Code's `MEMORY.md` is something else: an index of links to the notes beside it.
pub fn is_link_index(text: &str) -> bool {
    let mut any = false;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        any = true;
        let linked =
            line.trim_start_matches(['-', '*', ' ']).starts_with('[') && line.contains("](");
        if !linked && !line.starts_with('#') {
            return false;
        }
    }
    any
}

/// The entries of a `§`-delimited memory file, without empty ones.
pub fn split_entries(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        if line.trim() == "§" {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push_str(line);
            cur.push('\n');
        }
    }
    out.push(cur);
    out.into_iter()
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty())
        .collect()
}

/// All notes under `dir` (two levels deep), skipping the `MEMORY.md` index, hidden files, links and oversized files.
pub fn read_dir(dir: &Path) -> Result<(Vec<Note>, usize), String> {
    let mut notes = Vec::new();
    let mut skipped = 0usize;
    let mut stack = vec![(dir.to_path_buf(), 0u8)];
    while let Some((d, depth)) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&d)
            .map_err(|e| format!("cannot read {}: {e}", d.display()))?
            .flatten()
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(meta) = std::fs::symlink_metadata(e.path()) else {
                continue;
            };
            if name.starts_with('.') || meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                if depth < 2 {
                    stack.push((e.path(), depth + 1));
                }
                continue;
            }
            if !(name.ends_with(".md") || name.ends_with(".mdc")) {
                continue;
            }
            if meta.len() > MAX_FILE_BYTES || notes.len() >= MAX_FILES {
                skipped += 1;
                continue;
            }
            let hermes_file =
                name.eq_ignore_ascii_case("MEMORY.md") || name.eq_ignore_ascii_case("USER.md");
            if hermes_file {
                match std::fs::read_to_string(e.path()) {
                    Ok(text) if name.eq_ignore_ascii_case("MEMORY.md") && is_link_index(&text) => {}
                    Ok(text) => {
                        let kind = if name.eq_ignore_ascii_case("USER.md") {
                            "preference"
                        } else {
                            "reference"
                        };
                        let stem = name.trim_end_matches(".md");
                        for (i, entry) in split_entries(&text).into_iter().enumerate() {
                            if notes.len() >= MAX_FILES {
                                skipped += 1;
                            } else {
                                notes.push(Note {
                                    name: format!("{stem} entry {}", i + 1),
                                    kind: kind.into(),
                                    text: entry,
                                });
                            }
                        }
                    }
                    Err(_) => skipped += 1,
                }
                continue;
            }
            match std::fs::read_to_string(e.path()) {
                Ok(text) => {
                    match parse_note(name.trim_end_matches(".mdc").trim_end_matches(".md"), &text) {
                        Some(n) => notes.push(n),
                        None => skipped += 1,
                    }
                }
                Err(_) => skipped += 1,
            }
        }
    }
    Ok((notes, skipped))
}

pub fn run(args: &[String]) -> u8 {
    let Some(dir) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: kaku memory import DIR [--source NAME] [--data-dir DIR]\n  reads the .md notes in DIR (for example ~/.claude/projects/<project>/memory) and adds them as untrusted notes");
        return 2;
    };
    let source = crate::flag(args, "--source").unwrap_or_else(|| "markdown".into());
    let (notes, skipped) = match read_dir(Path::new(dir)) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    if notes.is_empty() {
        eprintln!(
            "{}",
            kk_types::words::counted(&format!(
                "no notes found in {dir} ({skipped} file(s) skipped)"
            ))
        );
        return 1;
    }
    let http = match kk_tui::http::Http::connect(&crate::data_dir(args)) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let payload: Vec<Value> = notes
        .iter()
        .map(|n| json!({"name": n.name, "kind": n.kind, "text": n.text}))
        .collect();
    let req = kk_tui::app::Req {
        id: 1,
        admin: false,
        method: kk_tui::app::Method::Post,
        path: "/api/memories/import-notes".into(),
        body: Some(
            json!({"source": source, "notes": payload, "from": std::fs::canonicalize(dir).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| dir.to_string()), "project": crate::flag(args, "--project")}),
        ),
        stream: false,
    };
    match http.run(&req).result {
        Ok((200, v)) => {
            println!(
                "{}",
                kk_types::words::counted(&format!(
                    "Read {} note(s) from {dir} ({skipped} file(s) skipped).",
                    notes.len()
                ))
            );
            println!("Added {}, already known {}, refused {} (secrets or empty), {} read like instructions and will never be given to a model.", v["added"], v["already_known"], v["refused"], v["instruction_like"]);
            println!("They are marked untrusted and labelled Internal. Review them with /memory, and correct or forget any.");
            let lost = v["not_saved"].as_u64().unwrap_or(0);
            if lost > 0 {
                eprintln!(
                    "{}",
                    kk_types::words::counted(&format!("{lost} note(s) could NOT be saved: the service could not write its store (is the disk full?). Free some space and import again; notes already saved are recognised and not added twice."))
                );
                return 1;
            }
            0
        }
        Ok((st, v)) => {
            eprintln!(
                "the service refused the import ({st}): {}",
                v["error"]["message"].as_str().unwrap_or("")
            );
            1
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mi01_both_frontmatter_shapes_and_plain_files_are_read() {
        let a = parse_note("x", "---\nname: user_role\ndescription: who the user is\nmetadata:\n  type: user\n---\nThe user builds things.\n").unwrap();
        assert_eq!((a.name.as_str(), a.kind.as_str()), ("user_role", "user"));
        assert_eq!(a.text, "who the user is\n\nThe user builds things.");
        let b = parse_note(
            "x",
            "---\nname: \"fb\"\ntype: feedback\n---\nShort answers.\n",
        )
        .unwrap();
        assert_eq!(
            (b.name.as_str(), b.kind.as_str(), b.text.as_str()),
            ("fb", "feedback", "Short answers.")
        );
        let c = parse_note("plain-file", "Just a note.\nSecond line.").unwrap();
        assert_eq!(
            (c.name.as_str(), c.kind.as_str(), c.text.as_str()),
            ("plain-file", "", "Just a note.\nSecond line.")
        );
        assert!(
            parse_note("e", "---\nname: n\n---\n   \n").is_none(),
            "an empty body with no description is nothing"
        );
        assert_eq!(
            parse_note("d", "---\ndescription: only this\n---\n")
                .unwrap()
                .text,
            "only this"
        );
        let broken = parse_note("u", "---\nname: never closed\nBody here").unwrap();
        assert!(
            broken.text.contains("Body here"),
            "an unclosed frontmatter keeps the text instead of dropping it"
        );
    }

    #[test]
    fn mi02_a_folder_is_read_without_the_index_hidden_files_links_or_huge_files() {
        let d = std::env::temp_dir().join(format!("kk-mi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("MEMORY.md"), "- [a](a.md)\n").unwrap();
        std::fs::write(d.join("a.md"), "---\nname: a\n---\nAlpha\n").unwrap();
        std::fs::write(d.join("sub/b.md"), "Beta").unwrap();
        std::fs::write(d.join(".hidden.md"), "Hidden").unwrap();
        std::fs::write(d.join("notes.txt"), "Not markdown").unwrap();
        std::fs::write(d.join("big.md"), "x".repeat(21_000)).unwrap();
        std::os::unix::fs::symlink("/etc/passwd", d.join("link.md")).unwrap();
        let (notes, skipped) = read_dir(&d).unwrap();
        let mut names: Vec<_> = notes.iter().map(|n| n.name.clone()).collect();
        names.sort();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(skipped, 1, "only the oversized file counts as skipped");
        assert!(read_dir(&d.join("missing")).is_err());
    }

    #[test]
    fn mi03_hermes_memory_files_become_one_note_per_entry_and_claude_code_index_files_are_still_skipped(
    ) {
        assert_eq!(
            split_entries("first lesson\n§\nsecond lesson\nmore\n§\n\n§\n  third  \n"),
            ["first lesson", "second lesson\nmore", "third"]
        );
        assert_eq!(split_entries("only one"), ["only one"]);
        assert!(split_entries("\n§\n").is_empty());
        assert!(is_link_index(
            "# Memory\n- [user role](user_role.md) - who\n- [fb](fb.md)\n"
        ));
        assert!(!is_link_index("The staging box is 10.1.2.3\n§\nUse pnpm\n"));
        assert!(
            !is_link_index("Use pnpm"),
            "a plain sentence is content, not an index"
        );
        assert!(!is_link_index(""));
        let d = std::env::temp_dir().join(format!("kk-mi3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("MEMORY.md"),
            "Deploys go through the blue pipeline\n§\nTests need PAYMENTS_TOKEN_SECRET\n",
        )
        .unwrap();
        std::fs::write(
            d.join("USER.md"),
            "Prefers short answers\n§\nWorks in IST\n",
        )
        .unwrap();
        let (notes, skipped) = read_dir(&d).unwrap();
        assert_eq!(skipped, 0);
        let got: Vec<_> = notes
            .iter()
            .map(|n| (n.name.as_str(), n.kind.as_str(), n.text.as_str()))
            .collect();
        assert!(got.contains(&(
            "MEMORY entry 1",
            "reference",
            "Deploys go through the blue pipeline"
        )));
        assert!(got.contains(&(
            "MEMORY entry 2",
            "reference",
            "Tests need PAYMENTS_TOKEN_SECRET"
        )));
        assert!(got.contains(&("USER entry 1", "preference", "Prefers short answers")));
        assert_eq!(notes.len(), 4);
        // a Claude Code folder keeps its index out
        let c = std::env::temp_dir().join(format!("kk-mi3c-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&c);
        std::fs::create_dir_all(&c).unwrap();
        std::fs::write(c.join("MEMORY.md"), "- [a](a.md)\n").unwrap();
        std::fs::write(c.join("a.md"), "Alpha").unwrap();
        assert_eq!(read_dir(&c).unwrap().0.len(), 1);
    }

    #[test]
    fn mi04_cursor_rule_files_are_read_with_their_description_as_the_first_line() {
        let d = std::env::temp_dir().join(format!("kk-mi4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".cursor/rules")).unwrap();
        std::fs::write(
            d.join(".cursor/rules/no-excessive-comments.mdc"),
            "---\ndescription: Keep comments and docstrings minimal and intentional\nglobs: *,**/*\nalwaysApply: true\n---\n\n# Comments\n\n- Do NOT narrate what the code does.\n",
        )
        .unwrap();
        std::fs::write(d.join(".cursor/rules/too-big.mdc"), "x".repeat(21_000)).unwrap();
        std::fs::write(d.join("AGENTS.md"), "Use pnpm, not npm.\n").unwrap();
        // hidden folders are skipped by the importer, so point it at the rules folder itself
        let (notes, skipped) = read_dir(&d.join(".cursor/rules")).unwrap();
        assert_eq!(
            (notes.len(), skipped),
            (1, 1),
            "the big rule file is skipped and counted"
        );
        assert_eq!(notes[0].name, "no-excessive-comments");
        assert!(
            notes[0]
                .text
                .starts_with("Keep comments and docstrings minimal and intentional\n\n# Comments"),
            "{}",
            notes[0].text
        );
        assert!(
            !notes[0].text.contains("alwaysApply"),
            "the frontmatter is not part of the note"
        );
        let (agents, _) = read_dir(&d).unwrap();
        assert!(agents
            .iter()
            .any(|n| n.name == "AGENTS" && n.text == "Use pnpm, not npm."));
    }
}
