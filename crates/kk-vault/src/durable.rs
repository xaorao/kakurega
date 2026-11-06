//! Writing small files that must never be half-written: keys, settings, a bound chat. The new bytes go to a new file beside
//! the old one, are forced to disk, and only then take the old file's name, so a crash leaves the old file or the new file
//! and never a mixture. A copy of the previous version is kept as `NAME.bak`, written the same way, and reading falls back to
//! it when the main file is missing or fails a check.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

static COUNTER: AtomicU64 = AtomicU64::new(0);
/// A leftover temporary file is removed once it is this old (a crash left it).
const STALE: Duration = Duration::from_secs(3600);

fn name_of(path: &Path) -> io::Result<String> {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))
}

fn parent_of(path: &Path) -> &Path {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    }
}

/// The backup's path: the name with `.bak` added.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".bak");
    PathBuf::from(s)
}

fn sync_dir(dir: &Path) {
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
}

/// Writes `bytes` as the whole of `path` with permissions `mode` (for example 0o600), atomically and durably. A symbolic
/// link at `path` is replaced, never written through.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    let name = name_of(path)?;
    let dir = parent_of(path);
    let unique = format!(
        "{name}.{}.{}.{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let tmp = dir.join(unique);
    let result = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&tmp)?;
        // the umask may have narrowed the mode at creation; the stated mode is what the file gets
        f.set_permissions(fs::Permissions::from_mode(mode))?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
        return result;
    }
    sync_dir(dir);
    clean_stale(dir, &name);
    Ok(())
}

/// Creates `path` with `bytes` only if it does not exist yet, atomically: the file appears whole or not at all, and of two
/// callers racing to create it exactly one wins. Returns whether this call created it.
pub fn create_atomic(path: &Path, bytes: &[u8], mode: u32) -> io::Result<bool> {
    let name = name_of(path)?;
    let dir = parent_of(path);
    let tmp = dir.join(format!(
        "{name}.{}.{}.new.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let made = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&tmp)?;
        f.set_permissions(fs::Permissions::from_mode(mode))?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        // a hard link fails when the name is taken, which a rename would not
        match fs::hard_link(&tmp, path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e),
        }
    })();
    let _ = fs::remove_file(&tmp);
    if matches!(made, Ok(true)) {
        sync_dir(dir);
    }
    made
}

/// Like `write_atomic`, and first keeps the file being replaced as `NAME.bak` (itself written atomically).
pub fn write_atomic_with_backup(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    if let Ok(previous) = fs::read(path) {
        // a failure to keep the copy does not stop the save: the new bytes are what matters, and the copy is a convenience
        let _ = write_atomic(&backup_path(path), &previous, mode);
    }
    write_atomic(path, bytes, mode)
}

/// Which file a read came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Main,
    Backup,
}

/// The contents of `path` if `valid` accepts them, else of its backup (and the main file is put back from it). `Ok(None)` when
/// neither file exists. An error when a file exists and nothing valid can be found: damage is reported, never papered over.
pub fn read_checked(
    path: &Path,
    valid: impl Fn(&[u8]) -> bool,
    mode: u32,
) -> io::Result<Option<(Vec<u8>, Source)>> {
    let bak = backup_path(path);
    let main = fs::read(path);
    if let Ok(b) = &main {
        if valid(b) {
            return Ok(Some((b.clone(), Source::Main)));
        }
    }
    if let Ok(b) = fs::read(&bak) {
        if valid(&b) {
            write_atomic(path, &b, mode)?;
            return Ok(Some((b, Source::Backup)));
        }
    }
    if main.is_err() && !bak.exists() {
        return match main {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
            Ok(_) => Ok(None),
        };
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{} is damaged and has no usable backup", path.display()),
    ))
}

fn clean_stale(dir: &Path, name: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let prefix = format!("{name}.");
    for e in entries.flatten() {
        let file = e.file_name();
        let Some(f) = file.to_str() else {
            continue;
        };
        if f.starts_with(&prefix) && f.ends_with(".tmp") {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|m| m.elapsed().ok())
                .is_some_and(|age| age > STALE);
            if old {
                let _ = fs::remove_file(e.path());
            }
        }
    }
}
