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
