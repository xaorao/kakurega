//! The reader against real repositories made with the git program (used here only to build them).

use kk_git::{read_change, valid_base, GitError, Kind, MAX_FILES};
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn repo(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kk-git-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(
        dir.join("src/session.py"),
        "def logout(token):\n    return True\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/keep.py"), "X = 1\n").unwrap();
    std::fs::write(dir.join("old.txt"), "going away\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "first"]);
    dir
}

fn feature(dir: &Path) {
    git(dir, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(
        dir.join("src/session.py"),
        "def logout(token):\n    TOKENS.discard(token)\n    return True\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/new.py"), "Y = 2\n").unwrap();
    std::fs::remove_file(dir.join("old.txt")).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "change"]);
}

#[test]
fn gt01_a_branchs_change_is_read_as_added_modified_and_deleted_files_with_both_versions() {
    let dir = repo("basic");
    feature(&dir);
    // main moves on after the branch was cut: the change is measured from where the branch left, not from main's tip
    git(&dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("src/keep.py"), "X = 100\n").unwrap();
    git(&dir, &["commit", "-q", "-am", "main moves"]);
    git(&dir, &["checkout", "-q", "feature"]);
    // an edit that is not committed is not part of the change
    std::fs::write(dir.join("src/new.py"), "Y = 999\n").unwrap();

    let set = read_change(&dir, "main").unwrap();
    assert_eq!((set.base.len(), set.head.len()), (40, 40));
    assert_ne!(set.base, set.head);
    let got: Vec<(&str, &Kind)> = set
        .changes
        .iter()
        .map(|c| (c.path.as_str(), &c.kind))
        .collect();
    assert_eq!(
        got,
        vec![
            ("old.txt", &Kind::Deleted),
            ("src/new.py", &Kind::Added),
            ("src/session.py", &Kind::Modified)
        ]
    );
    let session = &set.changes[2];
    assert_eq!(
        session.old.as_deref(),
        Some(&b"def logout(token):\n    return True\n"[..])
    );
    assert!(String::from_utf8_lossy(session.new.as_ref().unwrap()).contains("TOKENS.discard"));
    assert_eq!(
        set.changes[1].new.as_deref(),
        Some(&b"Y = 2\n"[..]),
        "the committed text, not the edit on disk"
    );
    assert!(set.changes[0].new.is_none() && set.changes[1].old.is_none());
    assert!(set.left_out.is_empty());
}

#[test]
fn gt02_what_cannot_be_reviewed_is_refused_in_words() {
    let dir = repo("refusals");
    assert_eq!(read_change(&dir, "main"), Err(GitError::NothingChanged));
    assert_eq!(
        read_change(&dir, "no-such-branch"),
        Err(GitError::UnknownBase)
    );
    for bad in [
        "",
        "--upload-pack=x",
        "a..b",
        "@{-1}",
        ":/fix",
        "main; rm -rf",
        "a b",
    ] {
        assert!(!valid_base(bad), "{bad}");
        assert_eq!(read_change(&dir, bad), Err(GitError::BadBase), "{bad}");
    }
    for good in [
        "main",
        "origin/main",
        "v1.2.3",
        "feature/x-1",
        "HEAD~2",
        "abc123^",
    ] {
        assert!(valid_base(good), "{good}");
    }
    let plain = std::env::temp_dir().join(format!("kk-git-plain-{}", std::process::id()));
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(read_change(&plain, "main"), Err(GitError::NotARepository));
    // two histories with nothing in common
    git(&dir, &["checkout", "-q", "--orphan", "island"]);
    git(&dir, &["commit", "-q", "--allow-empty", "-m", "alone"]);
    assert_eq!(read_change(&dir, "main"), Err(GitError::NoCommonHistory));
    // a change over the limit is refused with its size, not cut
    let big = repo("big");
    git(&big, &["checkout", "-q", "-b", "many"]);
    for i in 0..=MAX_FILES {
        std::fs::write(big.join(format!("src/f{i}.py")), format!("V = {i}\n")).unwrap();
    }
    git(&big, &["add", "-A"]);
    git(&big, &["commit", "-q", "-m", "many"]);
    match read_change(&big, "main") {
        Err(GitError::TooLarge { files, .. }) => assert_eq!(files, MAX_FILES + 1),
        other => panic!("{other:?}"),
    }
    assert!(GitError::TooLarge {
        files: 301,
        bytes: 9
    }
    .to_string()
    .contains("whole or not at all"));
}

#[test]
fn gt03_nothing_a_repository_configures_is_run_and_links_are_named_not_followed() {
    let dir = repo("hostile");
    feature(&dir);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/hosts", dir.join("link")).unwrap();
        git(&dir, &["add", "link"]);
        git(&dir, &["commit", "-q", "-m", "a link"]);
    }
    // every way a repository's own configuration can make git run a program
    let marker = dir.join("RAN");
    let script = dir.join("evil.sh");
    std::fs::write(
        &script,
        format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let s = script.display().to_string();
    let config = format!(
        "\n[core]\n\tfsmonitor = {s}\n\tpager = {s}\n\thooksPath = {d}\n\tsshCommand = {s}\n\teditor = {s}\n[diff]\n\texternal = {s}\n[diff \"x\"]\n\ttextconv = {s}\n[filter \"x\"]\n\tclean = {s}\n\tsmudge = {s}\n[alias]\n\tdiff = !{s}\n[uploadpack]\n\tpackObjectsHook = {s}\n",
        d = dir.display()
    );
    let path = dir.join(".git/config");
    let before = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, before + &config).unwrap();
    std::fs::write(dir.join(".gitattributes"), "* diff=x filter=x\n").unwrap();

    let set = read_change(&dir, "main").unwrap();
    assert!(
        !marker.exists(),
        "a program named in the repository's configuration ran"
    );
    assert!(set.changes.iter().any(|c| c.path == "src/session.py"));
    #[cfg(unix)]
    {
        assert_eq!(
            set.left_out,
            vec![(
                "link".to_string(),
                "a link or a nested repository, not a file"
            )]
        );
        assert!(!set.changes.iter().any(|c| c.path == "link"));
    }
}

#[test]
fn gt04_the_lines_a_change_wrote_are_found_in_the_new_version() {
    use kk_git::{changed_lines, Change};
    let c = |old: Option<&str>, new: Option<&str>| Change {
        path: "f.py".into(),
        kind: Kind::Modified,
        old: old.map(|s| s.as_bytes().to_vec()),
        new: new.map(|s| s.as_bytes().to_vec()),
    };
    // one line replaced in the middle, two added at the end
    let old = "a\nb\nc\nd\ne\n";
    let new = "a\nB\nc\nd\ne\nf\ng\n";
    assert_eq!(
        changed_lines(&c(Some(old), Some(new))),
        vec![(2, 2), (6, 7)]
    );
    // a line removed writes nothing
    assert_eq!(changed_lines(&c(Some(old), Some("a\nc\nd\ne\n"))), vec![]);
    // an added file is all its lines; a deleted one has none; bytes that are not text have none
    assert_eq!(changed_lines(&c(None, Some("x\ny\n"))), vec![(1, 2)]);
    assert_eq!(changed_lines(&c(Some(old), None)), vec![]);
    let mut binary = c(Some(old), None);
    binary.new = Some(vec![0xff, 0xfe, 0x00]);
    assert_eq!(changed_lines(&binary), vec![]);
    // neighbouring edits are one range
    assert_eq!(
        changed_lines(&c(Some("a\nb\nc\n"), Some("A\nB\nc\n"))),
        vec![(1, 2)]
    );
}

#[test]
fn gt05_the_committed_version_is_written_out_not_what_lies_on_disk_and_unsafe_names_never() {
    use kk_git::{export_head, safe_path};
    let dir = repo("export");
    feature(&dir);
    // on disk only: an edit, a new file, a secrets file that was never committed
    std::fs::write(dir.join("src/new.py"), "Y = 999\n").unwrap();
    std::fs::write(dir.join(".env"), "KEY=1\n").unwrap();
    let dest = dir
        .join("..")
        .join(format!("kk-git-export-out-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dest);
    let left = export_head(&dir, &dest, 1000).unwrap();
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(
        std::fs::read_to_string(dest.join("src/new.py")).unwrap(),
        "Y = 2\n"
    );
    assert!(dest.join("src/session.py").exists() && dest.join("src/keep.py").exists());
    assert!(
        !dest.join(".env").exists()
            && !dest.join("old.txt").exists()
            && !dest.join(".git").exists()
    );
    // an existing folder is never written into, and too many files is refused
    assert!(export_head(&dir, &dest, 1000).is_err());
    let other = dir
        .join("..")
        .join(format!("kk-git-export-two-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&other);
    assert!(export_head(&dir, &other, 2).is_err());
    for bad in [
        "",
        "/etc/passwd",
        "../up",
        "a/../b",
        "a//b",
        "./a",
        ".git/config",
        "a/.GIT/hooks/x",
        "a\\b",
        "a\nb",
    ] {
        assert!(!safe_path(bad), "{bad:?}");
    }
    for good in [
        "a.py",
        "src/auth/session.py",
        ".github/workflows/ci.yml",
        ".gitignore",
        "a b/c d.txt",
    ] {
        assert!(safe_path(good), "{good:?}");
    }
}

#[test]
fn gt06_committed_files_edited_or_removed_since_are_counted_and_new_files_are_not() {
    use kk_git::edited_since_commit;
    let dir = repo("edited");
    feature(&dir);
    assert_eq!(edited_since_commit(&dir, 1000, 100), 0);
    std::fs::write(dir.join("src/new.py"), "Y = 999\n").unwrap();
    std::fs::write(dir.join("never_committed.py"), "Z = 1\n").unwrap();
    assert_eq!(edited_since_commit(&dir, 1000, 100), 1);
    std::fs::remove_file(dir.join("src/keep.py")).unwrap();
    assert_eq!(edited_since_commit(&dir, 1000, 100), 2);
    assert_eq!(edited_since_commit(&dir, 1000, 1), 1);
    assert_eq!(edited_since_commit(&dir, 1, 100), 0);
}
