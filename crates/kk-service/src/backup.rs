//! Backing up and restoring the whole data directory.
//!
//! A backup holds a consistent copy of every encrypted database (taken with SQLite's own copy, so a running service
//! does not matter) and the vault key, sealed under a recovery key that is shown to the owner once. Restoring only ever
//! writes into a data directory that holds no Kakurega data, checks every database opens before anything is moved into
//! place, and never overwrites.

use crate::keys::{open_vault, vault_key};
use kk_vault::{Vault, VaultKey};
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

const DATABASES: &[&str] = &[
    "state.db",
    "ledger.db",
    "jobs.db",
    "events.db",
    "disclosure.db",
    "memory.db",
];
const KEY_FILE: &str = "vault.key";

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub struct Made {
    pub file: Vec<u8>,
    pub recovery_key: String,
    pub databases: usize,
}

/// Makes a backup of `data_dir`. Safe while the service is running.
pub fn make(data_dir: &Path) -> Result<Made, String> {
    vault_key(data_dir).map_err(|e| format!("cannot read the vault key: {e}"))?;
    let tmp = data_dir.join(format!(".backup-tmp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = (|| {
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        for name in DATABASES {
            if !data_dir.join(name).exists() {
                continue;
            }
            let copy = tmp.join(name);
            open_vault(data_dir, name)?
                .backup_to(&copy)
                .map_err(|e| format!("could not copy {name}: {e}"))?;
            entries.push((
                (*name).to_string(),
                fs::read(&copy).map_err(|e| e.to_string())?,
            ));
        }
        if entries.is_empty() {
            return Err("there is no Kakurega data in this folder".to_string());
        }
        let databases = entries.len();
        entries.push((
            KEY_FILE.to_string(),
            fs::read(data_dir.join(KEY_FILE)).map_err(|e| e.to_string())?,
        ));
        let recovery = kk_backup::new_key();
        let file = kk_backup::seal(&entries, &recovery, now()).map_err(|e| e.to_string())?;
        Ok(Made {
            file,
            recovery_key: kk_backup::key_to_text(&recovery),
            databases,
        })
    })();
    let _ = fs::remove_dir_all(&tmp);
    result
}

/// What a restore did.
#[derive(Debug)]
pub struct Restored {
    pub databases: usize,
    pub created_at: u64,
}

/// Restores `file` into `data_dir`, which must hold no Kakurega data.
pub fn restore(file: &[u8], recovery_key_text: &str, data_dir: &Path) -> Result<Restored, String> {
    let key = kk_backup::key_from_text(recovery_key_text).map_err(|e| e.to_string())?;
    for name in DATABASES.iter().chain(std::iter::once(&KEY_FILE)) {
        if data_dir.join(name).exists() {
            return Err(format!(
                "{} already holds Kakurega data ({name}); restore into an empty folder",
                data_dir.display()
            ));
        }
    }
    let (entries, created_at) = kk_backup::open(file, &key).map_err(|e| e.to_string())?;
    let mut key_hex: Option<String> = None;
    for (name, bytes) in &entries {
        if name == KEY_FILE {
            key_hex = Some(
                String::from_utf8(bytes.clone())
                    .map_err(|_| "the vault key in the backup is damaged".to_string())?,
            );
        } else if !DATABASES.contains(&name.as_str()) {
            return Err(format!(
                "the backup holds a file this version does not know: {name}"
            ));
        }
    }
    let key_hex = key_hex.ok_or("the backup holds no vault key")?;
    let raw = parse_key(key_hex.trim()).ok_or("the vault key in the backup is damaged")?;
    fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    let staging: PathBuf = data_dir.join(format!(".restore-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let staged = (|| -> Result<usize, String> {
        let mut dbs = 0;
        for (name, bytes) in &entries {
            if name == KEY_FILE {
                continue;
            }
            let path = staging.join(name);
            write_private(&path, bytes)?;
            let v = Vault::open(&path, &VaultKey::from_bytes(raw))
                .map_err(|e| format!("{name} does not open with the backup's key: {e}"))?;
            v.connection()
                .query_row("SELECT count(*) FROM sqlite_master", [], |r| {
                    r.get::<_, i64>(0)
                })
                .map_err(|e| format!("{name} is damaged: {e}"))?;
            dbs += 1;
        }
        Ok(dbs)
    })();
    let dbs = match staged {
        Ok(n) => n,
        Err(e) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    // everything checked: move into place, key last so a half-finished restore is never mistaken for a working folder
    for (name, _) in entries.iter().filter(|(n, _)| n != KEY_FILE) {
        fs::rename(staging.join(name), data_dir.join(name)).map_err(|e| e.to_string())?;
    }
    kk_vault::durable::write_atomic(&data_dir.join(KEY_FILE), key_hex.trim().as_bytes(), 0o600)
        .map_err(|e| e.to_string())?;
    let _ = fs::remove_dir_all(&staging);
    Ok(Restored {
        databases: dbs,
        created_at,
    })
}

fn parse_key(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())
}
