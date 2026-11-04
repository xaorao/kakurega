use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::process::{Command, Stdio};

/// Largest text taken back from an editor.
pub const MAX_EDITED: u64 = 256 * 1024;

/// The editor command the owner has chosen: `$VISUAL`, then `$EDITOR`, then `vi`.
pub fn chosen(visual: Option<String>, editor: Option<String>) -> String {
    [visual, editor]
        .into_iter()
        .flatten()
        .find(|c| !c.trim().is_empty())
        .unwrap_or_else(|| "vi".to_string())
}

/// Opens `text` in the owner's editor and returns what they saved. The text goes through a file only the owner can read,
/// in a folder only the owner can open, removed afterwards. The command is run directly (split on spaces, never through a
/// shell), and the editor is the owner's own program.
pub fn edit_with(command: &str, text: &str) -> Result<String, String> {
    let mut parts = command.split_whitespace();
    let program = parts.next().ok_or("no editor is set; set $EDITOR")?;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    // the counter makes two edits started in the same clock tick (the clock may only tick every microsecond) differ
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("kaku-edit-{}-{nanos}-{n}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| format!("cannot make a private folder for the editor: {e}"))?;
    let file = dir.join("message.md");
    let result = (|| {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&file)
            .and_then(|mut f| f.write_all(text.as_bytes()))
            .map_err(|e| format!("cannot write the editor file: {e}"))?;
        let status = Command::new(program)
            .args(parts)
            .arg(&file)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| format!("cannot start `{program}`: {e}"))?;
        if !status.success() {
            return Err(format!(
                "`{program}` ended with an error, so your text is unchanged"
            ));
        }
        let size = std::fs::metadata(&file).map_err(|e| e.to_string())?.len();
        if size > MAX_EDITED {
            return Err("the edited text is larger than 256 KB, so it was not taken".into());
        }
        let bytes =
            std::fs::read(&file).map_err(|e| format!("cannot read the edited file: {e}"))?;
        String::from_utf8(bytes)
            .map_err(|_| "the edited file is not valid text, so your text is unchanged".to_string())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}
