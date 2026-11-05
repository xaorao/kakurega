use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One file of a fix: the bytes it holds now, the bytes it should hold after, and the permissions to keep.
pub struct Change {
    pub path: PathBuf,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
    pub perms: std::fs::Permissions,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Every file now holds its new bytes.
    Done,
    /// A write failed and every file written before it was read again and is as it was. Nothing changed.
    RolledBack,
    /// A write failed and these files are not known to be as they were. They were left alone.
    Stuck(Vec<PathBuf>),
}

/// Why one replacement did not complete.
#[derive(Debug, PartialEq, Eq)]
pub enum Failed {
    /// The file is exactly as it was.
    NotWritten,
    /// The new file was moved into place but could not be confirmed, so the state of the file is not known.
    Unverified,
}

type Replace<'a> =
    &'a mut dyn FnMut(&Path, &[u8], &std::fs::Permissions, &[u8]) -> Result<(), Failed>;

fn temp_beside(path: &Path) -> std::io::Result<(std::fs::File, PathBuf)> {
    for _ in 0..8 {
        let mut r = [0u8; 6];
        getrandom::fill(&mut r).map_err(|e| std::io::Error::other(e.to_string()))?;
        let tag: String = r.iter().map(|b| format!("{b:02x}")).collect();
        let tmp = path.with_file_name(format!(".kakurega-fix-{}-{tag}.tmp", std::process::id()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        // owner-only until the real permissions are set, so a private file's new text is never briefly readable by others
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&tmp) {
            Ok(f) => return Ok((f, tmp)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("could not make a temporary file"))
}

fn stage_and_swap(
    path: &Path,
    tmp: &Path,
    mut file: std::fs::File,
    bytes: &[u8],
    perms: &std::fs::Permissions,
    expect: &[u8],
) -> std::io::Result<()> {
    file.write_all(bytes)?;
    file.set_permissions(perms.clone())?;
    file.sync_all()?;
    drop(file);
    // the file must still be what the change was planned against, checked as late as possible
    if std::fs::read(path)? != expect {
        return Err(std::io::Error::other(
            "the file changed while it was being written",
        ));
    }
    std::fs::rename(tmp, path)
}

/// Replaces `path` with `bytes` through a new temporary file next to it, but only while it still holds `expect`. The data
/// is flushed to disk, then the folder, and the result is read back and compared.
pub fn replace_file(
    path: &Path,
    bytes: &[u8],
    perms: &std::fs::Permissions,
    expect: &[u8],
) -> Result<(), Failed> {
    let (file, tmp) = temp_beside(path).map_err(|_| Failed::NotWritten)?;
    if stage_and_swap(path, &tmp, file, bytes, perms, expect).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return Err(Failed::NotWritten);
    }
    if let Some(dir) = path.parent().and_then(|dir| std::fs::File::open(dir).ok()) {
        let _ = dir.sync_all();
    }
    match std::fs::read(path) {
        Ok(now) if now == bytes => Ok(()),
        _ => Err(Failed::Unverified),
    }
}

/// Writes every change or none. If one fails, the files written before it are put back and each is read again to prove
/// it. A file that someone edited in the meantime is never overwritten; it is reported as stuck.
pub fn write_all_or_none(changes: &[Change]) -> Outcome {
    write_with(changes, &mut replace_file)
}

fn write_with(changes: &[Change], replace: Replace) -> Outcome {
    for (i, c) in changes.iter().enumerate() {
        let failure = match replace(&c.path, &c.after, &c.perms, &c.before) {
            Ok(()) => continue,
            Err(f) => f,
        };
        let mut stuck = Vec::new();
        if failure == Failed::Unverified {
            match std::fs::read(&c.path) {
                Ok(now) if now == c.before => {}
                Ok(now) if now == c.after => {
                    if replace(&c.path, &c.before, &c.perms, &c.after).is_err() {
                        stuck.push(c.path.clone());
                    }
                }
                _ => stuck.push(c.path.clone()),
            }
        }
        for earlier in changes[..i].iter().rev() {
            match std::fs::read(&earlier.path) {
                Ok(now) if now == earlier.before => {}
                Ok(now) if now == earlier.after => {
                    if replace(
                        &earlier.path,
                        &earlier.before,
                        &earlier.perms,
                        &earlier.after,
                    )
                    .is_err()
                    {
                        stuck.push(earlier.path.clone());
                    }
                }
                _ => stuck.push(earlier.path.clone()),
            }
        }
        return if stuck.is_empty() {
            Outcome::RolledBack
        } else {
            Outcome::Stuck(stuck)
        };
    }
    Outcome::Done
}

/// A file of a fix as the store remembers it: the original bytes, and the checksum of what the fix wrote.
pub struct Tracked {
    pub path: PathBuf,
    pub original: Vec<u8>,
    pub fixed_sha: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    Original,
    Fixed,
    /// Anything else: edited by the owner, missing, not a plain file.
    Other,
}

pub fn classify(t: &Tracked) -> Where {
    match std::fs::symlink_metadata(&t.path) {
        Ok(m) if m.is_file() => {}
        _ => return Where::Other,
    }
    match std::fs::read(&t.path) {
        Ok(now) if kk_evidence::capsule::sha256_hex(&now) == t.fixed_sha => Where::Fixed,
        Ok(now) if now == t.original => Where::Original,
        _ => Where::Other,
    }
}

/// Puts every file that holds exactly what the fix wrote back to its original text. Other files are not touched.
/// Returns the files that should have been put back and could not be.
pub fn restore_fixed(files: &[Tracked]) -> Vec<PathBuf> {
    let mut failed = Vec::new();
    for t in files {
        if classify(t) != Where::Fixed {
            continue;
        }
        let restored = match (std::fs::read(&t.path), std::fs::metadata(&t.path)) {
            (Ok(now), Ok(meta)) => {
                replace_file(&t.path, &t.original, &meta.permissions(), &now).is_ok()
            }
            _ => false,
        };
        if !restored {
            failed.push(t.path.clone());
        }
    }
    failed
}

/// Removes temporary files that an earlier run left beside a project's files when it stopped in the middle of a write.
/// Only files made by this code (`.kakurega-fix-*.tmp`) that are at least `older_than` old are removed. Returns how many.
pub fn sweep_stale(dir: &Path, older_than: std::time::Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !(name.starts_with(".kakurega-fix-") && name.ends_with(".tmp")) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let old_enough = meta
            .modified()
            .ok()
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age >= older_than);
        if meta.is_file() && old_enough && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// File names for a message to the owner: relative to the project folder, at most five.
pub fn describe_files(repo: &Path, files: &[PathBuf]) -> String {
    let mut names: Vec<String> = files
        .iter()
        .map(|p| {
            p.strip_prefix(repo)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let more = names.len().saturating_sub(5);
    names.truncate(5);
    let mut text = names.join(", ");
    if more > 0 {
        text += &format!(" and {more} more");
    }
    text
}

/// Which reviews have an apply or undo running. Two at once on the same review would race on the same files.
#[derive(Default)]
pub struct JobLocks(Mutex<HashSet<String>>);

pub struct JobLockGuard<'a> {
    locks: &'a JobLocks,
    id: String,
}

impl JobLocks {
    pub fn try_lock(&self, id: &str) -> Option<JobLockGuard<'_>> {
        let mut held = self.0.lock().unwrap_or_else(|e| e.into_inner());
        held.insert(id.to_string()).then(|| JobLockGuard {
            locks: self,
            id: id.to_string(),
        })
    }
}

impl Drop for JobLockGuard<'_> {
    fn drop(&mut self) {
        self.locks
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kk-ff-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn change(path: &Path, before: &str, after: &str, mode: u32) -> Change {
        std::fs::write(path, before).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        Change {
            path: path.to_path_buf(),
            before: before.into(),
            after: after.into(),
            perms: std::fs::metadata(path).unwrap().permissions(),
        }
    }

    fn leftovers(d: &Path) -> usize {
        std::fs::read_dir(d)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".kakurega-fix-")
            })
            .count()
    }

    /// A folder that refuses new files and holds one file, or None when this user ignores folder permissions.
    fn locked(tag: &str, name: &str, content: &str) -> Option<(PathBuf, PathBuf)> {
        let d = dir(tag);
        let f = d.join(name);
        std::fs::write(&f, content).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o555)).unwrap();
        if std::fs::write(d.join(".probe"), b"x").is_ok() {
            let _ = std::fs::remove_file(d.join(".probe"));
            unlock(&d);
            return None;
        }
        Some((d, f))
    }

    fn unlock(d: &Path) {
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn tracked(path: &Path, original: &str, fixed: &str) -> Tracked {
        Tracked {
            path: path.to_path_buf(),
            original: original.into(),
            fixed_sha: kk_evidence::capsule::sha256_hex(fixed.as_bytes()),
        }
    }

    #[test]
    fn ff01_every_file_is_written_keeps_its_permissions_and_leaves_no_temporary_files() {
        let d = dir("ff01");
        let (a, b) = (d.join("a.txt"), d.join("b.txt"));
        let changes = vec![
            change(&a, "one before", "one after", 0o644),
            change(&b, "two before", "two after", 0o640),
        ];
        assert_eq!(write_all_or_none(&changes), Outcome::Done);
        assert_eq!(std::fs::read(&a).unwrap(), b"one after");
        assert_eq!(std::fs::read(&b).unwrap(), b"two after");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!((mode(&a), mode(&b)), (0o644, 0o640));
        assert_eq!(leftovers(&d), 0);
    }

    #[test]
    fn ff02_a_failed_second_write_puts_the_first_file_back_and_says_so() {
        let Some((locked_dir, b)) = locked("ff02-locked", "b.txt", "two before") else {
            return;
        };
        let d = dir("ff02");
        let a = d.join("a.txt");
        let changes = vec![
            change(&a, "one before", "one after", 0o644),
            Change {
                path: b.clone(),
                before: b"two before".to_vec(),
                after: b"two after".to_vec(),
                perms: std::fs::metadata(&b).unwrap().permissions(),
            },
        ];
        let out = write_all_or_none(&changes);
        let stray = leftovers(&locked_dir);
        unlock(&locked_dir);
        assert_eq!(out, Outcome::RolledBack);
        assert_eq!(std::fs::read(&a).unwrap(), b"one before");
        assert_eq!(std::fs::read(&b).unwrap(), b"two before");
        assert_eq!((leftovers(&d), stray), (0, 0));
    }

    #[test]
    fn ff03_a_file_edited_after_the_plan_is_never_overwritten() {
        let d = dir("ff03");
        let (a, b) = (d.join("a.txt"), d.join("b.txt"));
        let changes = vec![
            change(&a, "one before", "one after", 0o644),
            change(&b, "two before", "two after", 0o644),
        ];
        std::fs::write(&b, "two, edited by the owner").unwrap();
        assert_eq!(write_all_or_none(&changes), Outcome::RolledBack);
        assert_eq!(std::fs::read(&a).unwrap(), b"one before");
        assert_eq!(std::fs::read(&b).unwrap(), b"two, edited by the owner");
        assert_eq!(leftovers(&d), 0);
    }

    #[test]
    fn ff04_a_file_that_cannot_be_put_back_is_reported_instead_of_hidden() {
        let d = dir("ff04");
        let (a, b) = (d.join("a.txt"), d.join("b.txt"));
        let changes = vec![
            change(&a, "one before", "one after", 0o644),
            change(&b, "two before", "two after", 0o644),
        ];
        let mut calls = 0;
        let out = write_with(&changes, &mut |p, bytes, perms, expect| {
            calls += 1;
            if calls == 1 {
                replace_file(p, bytes, perms, expect)
            } else {
                Err(Failed::NotWritten)
            }
        });
        assert_eq!(out, Outcome::Stuck(vec![a.clone()]));
        assert_eq!(std::fs::read(&a).unwrap(), b"one after");
        assert_eq!(std::fs::read(&b).unwrap(), b"two before");
    }

    #[test]
    fn ff05_a_file_edited_between_the_write_and_the_rollback_is_left_alone_and_reported() {
        let d = dir("ff05");
        let (a, b) = (d.join("a.txt"), d.join("b.txt"));
        let changes = vec![
            change(&a, "one before", "one after", 0o644),
            change(&b, "two before", "two after", 0o644),
        ];
        let mut calls = 0;
        let edited = a.clone();
        let out = write_with(&changes, &mut |p, bytes, perms, expect| {
            calls += 1;
            if calls == 1 {
                return replace_file(p, bytes, perms, expect);
            }
            std::fs::write(&edited, "one, edited meanwhile").unwrap();
            Err(Failed::NotWritten)
        });
        assert_eq!(out, Outcome::Stuck(vec![a.clone()]));
        assert_eq!(std::fs::read(&a).unwrap(), b"one, edited meanwhile");
    }

    #[test]
    fn ff06_a_file_that_was_moved_into_place_but_not_confirmed_is_put_back_when_it_holds_the_new_text(
    ) {
        let d = dir("ff06");
        let (a, b) = (d.join("a.txt"), d.join("b.txt"));
        let changes = vec![
            change(&a, "one before", "one after", 0o644),
            change(&b, "two before", "two after", 0o644),
        ];
        let mut calls = 0;
        let out = write_with(&changes, &mut |p, bytes, perms, expect| {
            calls += 1;
            match calls {
                2 => {
                    replace_file(p, bytes, perms, expect).unwrap();
                    Err(Failed::Unverified)
                }
                _ => replace_file(p, bytes, perms, expect),
            }
        });
        assert_eq!(out, Outcome::RolledBack);
        assert_eq!(std::fs::read(&a).unwrap(), b"one before");
        assert_eq!(std::fs::read(&b).unwrap(), b"two before");
    }

    #[test]
    fn ff07_a_file_in_an_unknown_state_after_an_unconfirmed_write_is_named() {
        let d = dir("ff07");
        let (a, b) = (d.join("a.txt"), d.join("b.txt"));
        let changes = vec![
            change(&a, "one before", "one after", 0o644),
            change(&b, "two before", "two after", 0o644),
        ];
        let mut calls = 0;
        let garbled = b.clone();
        let out = write_with(&changes, &mut |p, bytes, perms, expect| {
            calls += 1;
            match calls {
                2 => {
                    std::fs::write(&garbled, "???").unwrap();
                    Err(Failed::Unverified)
                }
                _ => replace_file(p, bytes, perms, expect),
            }
        });
        assert_eq!(out, Outcome::Stuck(vec![b.clone()]));
        assert_eq!(
            std::fs::read(&a).unwrap(),
            b"one before",
            "the other file still went back"
        );
        assert_eq!(std::fs::read(&b).unwrap(), b"???");
    }

    #[test]
    fn ff08_only_files_that_hold_exactly_the_fix_are_put_back() {
        let d = dir("ff08");
        let (fx, orig, other) = (d.join("fixed.txt"), d.join("orig.txt"), d.join("other.txt"));
        std::fs::write(&fx, "fixed text").unwrap();
        std::fs::write(&orig, "original text").unwrap();
        std::fs::write(&other, "the owner's own text").unwrap();
        let files = vec![
            tracked(&fx, "fixed original", "fixed text"),
            tracked(&orig, "original text", "orig fixed"),
            tracked(&other, "other original", "other fixed"),
        ];
        let kinds: Vec<Where> = files.iter().map(classify).collect();
        assert_eq!(kinds, [Where::Fixed, Where::Original, Where::Other]);
        assert!(restore_fixed(&files).is_empty());
        assert_eq!(std::fs::read(&fx).unwrap(), b"fixed original");
        assert_eq!(std::fs::read(&orig).unwrap(), b"original text");
        assert_eq!(std::fs::read(&other).unwrap(), b"the owner's own text");
        assert_eq!(leftovers(&d), 0);
        assert_eq!(
            classify(&tracked(&d.join("gone.txt"), "x", "y")),
            Where::Other
        );
    }

    #[test]
    fn ff09_a_symlink_is_never_written_through() {
        let d = dir("ff09");
        let (real, link) = (d.join("real.txt"), d.join("link.txt"));
        std::fs::write(&real, "fixed text").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let files = vec![tracked(&link, "original", "fixed text")];
        assert_eq!(classify(&files[0]), Where::Other);
        assert!(restore_fixed(&files).is_empty());
        assert_eq!(std::fs::read(&real).unwrap(), b"fixed text");
    }

    #[test]
    fn ff10_a_file_that_cannot_be_restored_is_named() {
        let Some((locked_dir, f)) = locked("ff10-locked", "f.txt", "fixed text") else {
            return;
        };
        let failed = restore_fixed(&[tracked(&f, "original", "fixed text")]);
        unlock(&locked_dir);
        assert_eq!(failed, vec![f.clone()]);
        assert_eq!(std::fs::read(&f).unwrap(), b"fixed text");
    }

    #[test]
    fn ff11_one_review_cannot_run_two_changes_at_once() {
        let locks = JobLocks::default();
        let first = locks.try_lock("job-1").expect("free");
        assert!(locks.try_lock("job-1").is_none(), "held");
        assert!(
            locks.try_lock("job-2").is_some(),
            "another review is independent"
        );
        drop(first);
        assert!(
            locks.try_lock("job-1").is_some(),
            "released when the guard is dropped"
        );
    }

    #[test]
    fn ff12_file_names_in_a_message_are_relative_and_at_most_five() {
        let repo = Path::new("/p");
        let one = vec![PathBuf::from("/p/a/b.py")];
        assert_eq!(describe_files(repo, &one), "a/b.py");
        let many: Vec<PathBuf> = (0..8)
            .map(|i| PathBuf::from(format!("/p/f{i}.py")))
            .collect();
        assert_eq!(
            describe_files(repo, &many),
            "f0.py, f1.py, f2.py, f3.py, f4.py and 3 more"
        );
        assert_eq!(
            describe_files(repo, &[PathBuf::from("/elsewhere/x")]),
            "/elsewhere/x"
        );
    }

    #[test]
    fn ff13_only_old_leftovers_made_by_this_code_are_swept() {
        let d = dir("ff13");
        let old = d.join(".kakurega-fix-1-aabbcc.tmp");
        let fresh = d.join(".kakurega-fix-2-ddeeff.tmp");
        let notes = d.join(".kakurega-fix-notes.txt");
        let normal = d.join("session.py");
        for f in [&old, &fresh, &notes, &normal] {
            std::fs::write(f, "x").unwrap();
        }
        let hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(hour_ago)
            .unwrap();
        std::fs::File::options()
            .write(true)
            .open(&notes)
            .unwrap()
            .set_modified(hour_ago)
            .unwrap();
        let link = d.join(".kakurega-fix-3-link.tmp");
        std::os::unix::fs::symlink(&normal, &link).unwrap();
        assert_eq!(sweep_stale(&d, std::time::Duration::from_secs(60)), 1);
        assert!(!old.exists());
        assert!(
            fresh.exists(),
            "a recent temporary file may belong to a write in progress"
        );
        assert!(
            notes.exists() && normal.exists(),
            "files not made by this code stay"
        );
        assert!(
            std::fs::symlink_metadata(&link).is_ok(),
            "a link is never followed or removed"
        );
        assert_eq!(
            sweep_stale(&d, std::time::Duration::ZERO),
            1,
            "with no age limit the recent one goes too"
        );
        assert_eq!(
            sweep_stale(&d.join("missing"), std::time::Duration::ZERO),
            0
        );
    }

    #[test]
    fn ff14_the_temporary_file_is_private_until_the_real_permissions_are_set() {
        let d = dir("ff14");
        let f = d.join("secret.env");
        std::fs::write(&f, "old").unwrap();
        let (file, tmp) = temp_beside(&f).unwrap();
        let mode = file.metadata().unwrap().permissions().mode() & 0o777;
        drop(file);
        std::fs::remove_file(tmp).unwrap();
        assert_eq!(mode, 0o600);
    }
}
