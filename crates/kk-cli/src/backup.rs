use std::io::{BufRead, Write};
use std::os::unix::fs::OpenOptionsExt;

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn make(args: &[String]) -> u8 {
    let data = crate::data_dir(args);
    let out =
        crate::flag(args, "--out").unwrap_or_else(|| format!("kakurega-backup-{}.kkbak", now()));
    let made = match kk_service::backup::make(&data) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("backup failed: {e}");
            return 1;
        }
    };
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&out)
        .and_then(|mut f| f.write_all(&made.file));
    if let Err(e) = written {
        eprintln!("cannot write {out}: {e} (an existing file is never overwritten)");
        return 1;
    }
    println!(
        "Saved {out} ({} databases, {} bytes).",
        made.databases,
        made.file.len()
    );
    println!();
    println!("Your recovery key. It is shown once and is not stored anywhere:");
    println!();
    println!("  {}", made.recovery_key);
    println!();
    println!("Keep it away from the backup file (a password manager or paper). Anyone with both can read everything;");
    println!("without the key the file cannot be opened by anyone, including us.");
    println!(
        "Restore into an empty folder with: kaku restore {} --data-dir NEWDIR",
        crate::shell_word(&out.to_string())
    );
    0
}

pub fn restore(args: &[String]) -> u8 {
    let Some(file) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("usage: kaku restore FILE [--key-file PATH] [--data-dir DIR]   (the recovery key is read from KAKU_RECOVERY_KEY, the key file, or typed when asked)");
        return 2;
    };
    let bytes = match std::fs::read(file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {file}: {e}");
            return 2;
        }
    };
    let key = if let Ok(k) = std::env::var("KAKU_RECOVERY_KEY") {
        k
    } else if let Some(path) = crate::flag(args, "--key-file") {
        match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("cannot read {path}: {e}");
                return 2;
            }
        }
    } else {
        // the key opens everything in the backup: it is not shown as it is typed, and not left in the terminal's scrollback
        match read_hidden("Recovery key (KKRK-...)") {
            Hidden::Line(l) => l,
            Hidden::Cancelled => {
                eprintln!("Cancelled. Nothing was restored.");
                return 1;
            }
            Hidden::Failed => return 2,
        }
    };
    let data = crate::data_dir(args);
    match kk_service::backup::restore(&bytes, &key, &data) {
        Ok(r) => {
            println!(
                "Restored {} databases into {} (the backup was made {}).",
                r.databases,
                data.display(),
                ago_words(r.created_at as i64)
            );
            println!(
                "Start with: kaku serve --data-dir {}",
                crate::shell_word(&data.display().to_string())
            );
            0
        }
        Err(e) => {
            eprintln!("restore failed: {e}");
            1
        }
    }
}

/// How long ago a moment was, for a person.
fn ago_words(at: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let secs = (now - at).max(0);
    match secs {
        0..=119 => "just now".into(),
        120..=7199 => format!("{} minutes ago", secs / 60),
        7200..=172_799 => format!("{} hours ago", secs / 3600),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// What came of asking for a line that is not shown as it is typed.
enum Hidden {
    Line(String),
    Cancelled,
    Failed,
}

/// Asks for a line without showing what is typed. In a terminal the keys are read one at a time with echo and the
/// interrupt key turned off, so Ctrl-C ends the question here, as a cancel, and the terminal is always put back as it was
/// (a Ctrl-C that killed the program while echo was off would leave the terminal showing nothing typed). Away from a
/// terminal the line is read as it comes.
fn read_hidden(prompt: &str) -> Hidden {
    use std::io::{Read, Write};
    let stty = |args: &[&str]| {
        std::process::Command::new("stty")
            .args(args)
            .stdin(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let saved = if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        stty(&["-g"])
            .filter(|_| stty(&["-echo", "-icanon", "-isig", "min", "1", "time", "0"]).is_some())
    } else {
        None
    };
    eprint!(
        "{prompt}{}: ",
        if saved.is_some() {
            ", not shown as you type"
        } else {
            ""
        }
    );
    let _ = std::io::stderr().flush();
    let Some(saved) = saved else {
        let mut line = String::new();
        return match std::io::stdin().lock().read_line(&mut line) {
            Ok(_) => Hidden::Line(line),
            Err(_) => Hidden::Failed,
        };
    };
    let mut typed: Vec<u8> = Vec::new();
    let mut stdin = std::io::stdin().lock();
    let mut byte = [0u8; 1];
    let outcome = loop {
        match stdin.read(&mut byte) {
            Ok(0) | Err(_) => {
                break if typed.is_empty() {
                    Hidden::Cancelled
                } else {
                    Hidden::Line(String::from_utf8_lossy(&typed).into_owned())
                }
            }
            Ok(_) => match byte[0] {
                b'\r' | b'\n' => break Hidden::Line(String::from_utf8_lossy(&typed).into_owned()),
                // Ctrl-C, Ctrl-D on an empty line, or Esc: the owner changed their mind
                0x03 | 0x1b => break Hidden::Cancelled,
                0x04 if typed.is_empty() => break Hidden::Cancelled,
                0x7f | 0x08 => {
                    typed.pop();
                }
                0x15 => typed.clear(),
                b => typed.push(b),
            },
        }
    };
    let _ = stty(&[saved.as_str()]);
    eprintln!();
    outcome
}
