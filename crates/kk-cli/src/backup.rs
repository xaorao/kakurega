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
    println!("Restore into an empty folder with: kaku restore {out} --data-dir NEWDIR");
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
        eprint!("Recovery key (KKRK-...): ");
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        if std::io::stdin().lock().read_line(&mut line).is_err() {
            return 2;
        }
        line
    };
    let data = crate::data_dir(args);
    match kk_service::backup::restore(&bytes, &key, &data) {
        Ok(r) => {
            println!(
                "Restored {} databases into {} (backup made at unix time {}).",
                r.databases,
                data.display(),
                r.created_at
            );
            println!("Start with: kaku serve --data-dir {}", data.display());
            0
        }
        Err(e) => {
            eprintln!("restore failed: {e}");
            1
        }
    }
}
