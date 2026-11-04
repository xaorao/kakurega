use std::path::Path;

fn walk(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if matches!(
            name.as_str(),
            ".git" | "node_modules" | "target" | "dist" | ".venv"
        ) {
            continue;
        }
        let Ok(m) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if m.is_dir() {
            walk(&p, files);
        } else if m.is_file() && m.len() < 1_000_000 {
            files.push(p);
        }
    }
}

fn main() {
    let root = std::env::args().nth(1).expect("directory");
    let mut files = Vec::new();
    walk(Path::new(&root), &mut files);
    let (mut scanned, mut flagged) = (0, 0);
    for f in &files {
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        scanned += 1;
        let hits = kk_secrets::scan(&text);
        if let Some(h) = hits.first() {
            flagged += 1;
            let snippet: String = text[h.start..h.end.min(h.start + 60)]
                .chars()
                .filter(|c| !c.is_control())
                .collect();
            println!(
                "{}  [{}{}]  {}",
                f.display(),
                h.rule,
                if h.decoded { ", decoded" } else { "" },
                snippet
            );
        }
    }
    println!("scanned {scanned} text files, flagged {flagged}");
}
