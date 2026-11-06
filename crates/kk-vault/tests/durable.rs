//! Small files that must survive a crash: the old file or the new file, never a mixture; the right permissions from the
//! first moment; a backup that is itself whole.

use kk_vault::durable::{
    backup_path, read_checked, write_atomic, write_atomic_with_backup, Source,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kk-durable-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn mode(p: &std::path::Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

fn listing(d: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(d)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn du01_a_write_gives_exactly_the_bytes_and_the_stated_mode_and_leaves_nothing_behind() {
    let d = dir("write");
    let p = d.join("settings.json");
    write_atomic(&p, b"{\"a\":1}", 0o600).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"{\"a\":1}");
    assert_eq!(mode(&p), 0o600);
    assert_eq!(listing(&d), ["settings.json"], "no temporary file is left");
    // replacing keeps the new mode even when the old file was open to everyone
    fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
    write_atomic(&p, b"second", 0o600).unwrap();
    assert_eq!(
        (fs::read(&p).unwrap(), mode(&p)),
        (b"second".to_vec(), 0o600)
    );
    // the stated mode is what the file gets whatever the process umask would have taken away at creation
    write_atomic(&p, b"shared", 0o664).unwrap();
    assert_eq!(mode(&p), 0o664);
    write_atomic(&p, b"second", 0o600).unwrap();
    // an empty file and a large one
    write_atomic(&p, b"", 0o600).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"");
    let big = vec![7u8; 3 << 20];
    write_atomic(&p, &big, 0o600).unwrap();
    assert_eq!(fs::read(&p).unwrap(), big);
}

#[test]
fn du02_a_failed_write_leaves_the_old_file_untouched_and_no_temporary_file() {
    let d = dir("fail");
    let p = d.join("keep.json");
    write_atomic(&p, b"old", 0o600).unwrap();
    fs::set_permissions(&d, fs::Permissions::from_mode(0o500)).unwrap();
    let r = write_atomic(&p, b"new", 0o600);
    fs::set_permissions(&d, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        r.is_err(),
        "a directory that cannot be written is an error, not a silent skip"
    );
    assert_eq!(fs::read(&p).unwrap(), b"old");
    assert_eq!(listing(&d), ["keep.json"]);
    // a failure after the temporary file exists (the name is taken by a directory) removes it
    let blocked = d.join("blocked.json");
    fs::create_dir_all(blocked.join("inside")).unwrap();
    assert!(write_atomic(&blocked, b"x", 0o600).is_err());
    assert_eq!(
        listing(&d),
        ["blocked.json", "keep.json"],
        "no temporary file is left"
    );
    // a path with no directory that exists
    assert!(write_atomic(&d.join("nope/x.json"), b"x", 0o600).is_err());
    assert!(
        write_atomic(std::path::Path::new("/"), b"x", 0o600).is_err(),
        "no file name"
    );
}

#[test]
fn du03_a_link_at_the_path_is_replaced_and_never_written_through() {
    let d = dir("link");
    let target = d.join("elsewhere.txt");
    fs::write(&target, b"someone else's file").unwrap();
    let p = d.join("settings.json");
    std::os::unix::fs::symlink(&target, &p).unwrap();
    write_atomic(&p, b"mine", 0o600).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"someone else's file");
    assert!(!fs::symlink_metadata(&p).unwrap().file_type().is_symlink());
    assert_eq!(fs::read(&p).unwrap(), b"mine");
}

#[test]
fn du04_the_previous_version_is_kept_whole_as_a_backup() {
    let d = dir("bak");
    let p = d.join("state.json");
    write_atomic_with_backup(&p, b"one", 0o600).unwrap();
    assert!(
        !backup_path(&p).exists(),
        "nothing to back up the first time"
    );
    write_atomic_with_backup(&p, b"two", 0o600).unwrap();
    assert_eq!(
        (fs::read(&p).unwrap(), fs::read(backup_path(&p)).unwrap()),
        (b"two".to_vec(), b"one".to_vec())
    );
    write_atomic_with_backup(&p, b"three", 0o600).unwrap();
    assert_eq!(
        (fs::read(&p).unwrap(), fs::read(backup_path(&p)).unwrap()),
        (b"three".to_vec(), b"two".to_vec())
    );
    assert_eq!((mode(&p), mode(&backup_path(&p))), (0o600, 0o600));
    assert_eq!(listing(&d), ["state.json", "state.json.bak"]);
}

#[test]
fn du05_reading_uses_the_main_file_then_the_backup_and_restores_the_main_file() {
    let d = dir("read");
    let p = d.join("state.json");
    let ok = |b: &[u8]| b.starts_with(b"{") && b.ends_with(b"}");
    assert!(
        read_checked(&p, ok, 0o600).unwrap().is_none(),
        "neither file exists"
    );
    write_atomic_with_backup(&p, b"{1}", 0o600).unwrap();
    write_atomic_with_backup(&p, b"{2}", 0o600).unwrap();
    assert_eq!(
        read_checked(&p, ok, 0o600).unwrap(),
        Some((b"{2}".to_vec(), Source::Main))
    );
    // a crash that left the main file cut short
    fs::write(&p, b"{2").unwrap();
    assert_eq!(
        read_checked(&p, ok, 0o600).unwrap(),
        Some((b"{1}".to_vec(), Source::Backup))
    );
    assert_eq!(fs::read(&p).unwrap(), b"{1}", "the main file is put back");
    assert_eq!(mode(&p), 0o600);
    // the main file gone altogether
    fs::remove_file(&p).unwrap();
    assert_eq!(
        read_checked(&p, ok, 0o600).unwrap().unwrap().1,
        Source::Backup
    );
    // both damaged: an error that names the file, never invented content
    fs::write(&p, b"junk").unwrap();
    fs::write(backup_path(&p), b"junk").unwrap();
    let e = read_checked(&p, ok, 0o600).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::InvalidData);
    assert!(e.to_string().contains("state.json"), "{e}");
    // only a damaged main file and no backup
    fs::remove_file(backup_path(&p)).unwrap();
    assert!(read_checked(&p, ok, 0o600).is_err());
}

#[test]
fn du06_a_temporary_file_a_crash_left_does_not_block_writing_and_old_ones_are_removed() {
    let d = dir("stale");
    let p = d.join("state.json");
    // what a crash mid-write leaves: a temporary file with a plausible name
    let stale = d.join("state.json.1.1.1.tmp");
    fs::write(&stale, b"half").unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
    fs::File::options()
        .write(true)
        .open(&stale)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let fresh = d.join("state.json.2.2.2.tmp");
    fs::write(&fresh, b"in use by another writer").unwrap();
    let other = d.join("other.json.3.3.3.tmp");
    fs::write(&other, b"someone else's").unwrap();
    fs::File::options()
        .write(true)
        .open(&other)
        .unwrap()
        .set_modified(old)
        .unwrap();
    write_atomic(&p, b"ok", 0o600).unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"ok");
    assert!(!stale.exists(), "an old leftover of this file is removed");
    assert!(
        fresh.exists(),
        "a recent one may belong to a writer still running"
    );
    assert!(other.exists(), "another file's leftovers are not touched");
}

#[test]
fn du07_many_writers_at_once_leave_one_whole_value() {
    let d = dir("race");
    let p = d.join("state.json");
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let p = p.clone();
            std::thread::spawn(move || {
                for j in 0..40 {
                    let v = format!(
                        "{{\"writer\":{i},\"n\":{j},\"pad\":\"{}\"}}",
                        "x".repeat(2000)
                    );
                    write_atomic_with_backup(&p, v.as_bytes(), 0o600).unwrap();
                }
            })
        })
        .collect();
    let reader = {
        let p = p.clone();
        std::thread::spawn(move || {
            for _ in 0..400 {
                if let Ok(b) = fs::read(&p) {
                    assert!(
                        b.starts_with(b"{") && b.ends_with(b"}"),
                        "a reader saw a partial file"
                    );
                }
            }
        })
    };
    for h in handles {
        h.join().unwrap();
    }
    reader.join().unwrap();
    let last = fs::read(&p).unwrap();
    assert!(last.starts_with(b"{\"writer\"") && last.ends_with(b"}"));
    let leftovers: Vec<String> = listing(&d)
        .into_iter()
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}
