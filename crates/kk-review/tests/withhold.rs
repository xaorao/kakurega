use kk_review::import::scan_tree;
use kk_review::withhold::Withhold;
use kk_types::label::Level;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn w(text: &str) -> Withhold {
    Withhold::parse(text)
}

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kk-withhold-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[test]
fn ki01_patterns_mean_what_a_gitignore_reader_expects_for_the_supported_forms() {
    let cases: &[(&str, &str, bool)] = &[
        ("secret.txt", "secret.txt", true),
        ("secret.txt", "a/b/secret.txt", true),
        ("secret.txt", "secret.txt.bak", false),
        ("/secret.txt", "secret.txt", true),
        ("/secret.txt", "a/secret.txt", false),
        ("private/", "private/a.py", true),
        ("private/", "x/private/deep/a.py", true),
        ("private/", "private.py", false),
        ("private/", "private", false),
        ("*.sql", "dump.sql", true),
        ("*.sql", "db/dump.sql", true),
        ("*.sql", "dump.sqlx", false),
        ("docs/*.md", "docs/a.md", true),
        ("docs/*.md", "docs/sub/a.md", false),
        ("docs/*.md", "x/docs/a.md", false),
        ("**/fixtures", "fixtures/x.json", true),
        ("**/fixtures", "a/b/fixtures/x.json", true),
        ("a/**/b.txt", "a/b.txt", true),
        ("a/**/b.txt", "a/x/y/b.txt", true),
        ("a/**/b.txt", "b.txt", false),
        ("file-?.md", "file-1.md", true),
        ("file-?.md", "file-10.md", false),
        ("Secret.TXT", "secret.txt", true),
        ("secret.txt", "SECRET.txt", true),
        ("notes", "notes/today.md", true),
        ("notes", "mynotes/today.md", false),
        ("a?b", "a/b", false),
        ("*", "anything.txt", true),
        ("a*", "a", true),
        ("file-*", "file-", true),
        ("pre*suf", "presuf", true),
        ("pre*suf", "pre-x-suf", true),
        ("pre*suf", "pre-x-sux", false),
        ("*x*", "x", true),
    ];
    for (pattern, path, want) in cases {
        assert_eq!(
            w(pattern).matches(path),
            *want,
            "pattern {pattern:?} against {path:?}"
        );
    }
}

#[test]
fn ki02_comments_blanks_spacing_and_negation_are_handled_without_ever_un_withholding() {
    let list = "# my private things\n\n   \nsecret.txt   \n#not-a-pattern.txt\n*.txt\n!keep.txt\n";
    let h = w(list);
    assert!(h.matches("secret.txt"));
    assert!(
        h.matches("keep.txt"),
        "a negation line is ignored: nothing can be un-withheld"
    );
    assert!(!h.matches("not-a-pattern.txt.md"));
    assert!(!h.matches("code.py"));
    assert_eq!(
        h.ignored_lines(),
        1,
        "the negation line is counted as not supported"
    );
    // an empty or comment-only list withholds nothing
    assert!(!w("").matches("a.py") && !w("# nothing\n").matches("a.py"));
    // windows line endings
    assert!(w("secret.txt\r\nother.txt\r\n").matches("other.txt"));
}

#[test]
fn ki03_a_hostile_list_cannot_make_matching_slow_or_unbounded() {
    // many stars against a long name: a naive matcher takes exponential time
    let h = w(&format!("{}\n", "a*".repeat(30) + "b"));
    let long = "a".repeat(2000);
    let start = Instant::now();
    assert!(!h.matches(&long));
    assert!(
        start.elapsed() < Duration::from_millis(500),
        "took {:?}",
        start.elapsed()
    );
    // many patterns and very long lines are capped and counted
    let big: String = (0..2000).map(|i| format!("p{i}.txt\n")).collect();
    let h = w(&big);
    assert!(
        h.ignored_lines() >= 1500,
        "only the first 500 patterns count: {}",
        h.ignored_lines()
    );
    assert!(h.matches("p0.txt") && !h.matches("p1999.txt"));
    let long_line = format!("{}\n", "x".repeat(5000));
    assert!(w(&long_line).ignored_lines() == 1);
    // a path with a hundred directories is still answered, quickly
    let deep = (0..100)
        .map(|i| format!("d{i}"))
        .collect::<Vec<_>>()
        .join("/")
        + "/f.txt";
    let t = Instant::now();
    let _ = w("**/f.txt").matches(&deep);
    assert!(t.elapsed() < Duration::from_millis(500));
}

#[test]
fn ki04_the_importer_withholds_what_the_owner_listed_and_nothing_else_and_never_sends_the_list() {
    let root = scratch("scan");
    put(
        &root,
        ".kakuignore",
        "# never leave this computer\nprivate/\n*.sql\ncontracts/acme-*.md\n",
    );
    put(&root, "private/plan.md", "the acquisition plan");
    put(&root, "db/dump.sql", "insert into t values (1);");
    put(&root, "contracts/acme-2025.md", "terms");
    put(&root, "contracts/other.md", "terms");
    put(&root, "src/app.py", "print('hi')\n");
    let out = scan_tree(&root, Level::Internal);
    let find = |p: &str| {
        out.files.iter().find(|f| f.path == p).unwrap_or_else(|| {
            panic!(
                "{p} missing from {:?}",
                out.files.iter().map(|f| &f.path).collect::<Vec<_>>()
            )
        })
    };
    for p in ["private/plan.md", "db/dump.sql", "contracts/acme-2025.md"] {
        let f = find(p);
        assert_eq!(f.label.level, Level::Restricted, "{p}");
        assert!(
            f.label.compartments.contains("owner-withheld"),
            "{p}: {:?}",
            f.label
        );
        assert!(
            f.notes.iter().any(|n| n.contains(".kakuignore")),
            "{p}: {:?}",
            f.notes
        );
    }
    for p in ["contracts/other.md", "src/app.py"] {
        assert_ne!(find(p).label.level, Level::Restricted, "{p} was not listed");
    }
    assert!(
        !out.files.iter().any(|f| f.path == ".kakuignore"),
        "the list itself is never part of what the model sees"
    );
    assert!(
        out.skipped
            .iter()
            .any(|s| s.path == ".kakuignore" && s.reason.contains("withhold list")),
        "{:?}",
        out.skipped
    );
}

#[test]
fn ki05_a_list_that_cannot_be_read_withholds_everything_and_says_so() {
    for (tag, make) in [
        (
            "binary",
            Box::new(|root: &PathBuf| {
                std::fs::write(root.join(".kakuignore"), [0xff, 0xfe, 0x00, 0x01]).unwrap()
            }) as Box<dyn Fn(&PathBuf)>,
        ),
        (
            "huge",
            Box::new(|root: &PathBuf| {
                std::fs::write(root.join(".kakuignore"), "x\n".repeat(60_000)).unwrap()
            }),
        ),
        (
            "dir",
            Box::new(|root: &PathBuf| std::fs::create_dir_all(root.join(".kakuignore")).unwrap()),
        ),
    ] {
        let root = scratch(tag);
        put(&root, "src/app.py", "print('hi')\n");
        make(&root);
        let out = scan_tree(&root, Level::Internal);
        let f = out
            .files
            .iter()
            .find(|f| f.path == "src/app.py")
            .expect("the file is still listed");
        assert_eq!(
            f.label.level,
            Level::Restricted,
            "{tag}: an unreadable list withholds everything"
        );
        assert!(
            f.notes.iter().any(|n| n.contains("could not be read")),
            "{tag}: {:?}",
            f.notes
        );
    }
}

#[test]
fn ki06_a_list_that_is_a_symbolic_link_is_not_followed() {
    let root = scratch("link");
    let outside = scratch("link-target");
    put(&outside, "list", "*.py\n");
    put(&root, "src/app.py", "print('hi')\n");
    std::os::unix::fs::symlink(outside.join("list"), root.join(".kakuignore")).unwrap();
    let out = scan_tree(&root, Level::Internal);
    let f = out.files.iter().find(|f| f.path == "src/app.py").unwrap();
    assert_eq!(
        f.label.level,
        Level::Restricted,
        "a link as the list counts as unreadable, never followed"
    );
    assert!(
        out.skipped
            .iter()
            .any(|s| s.path == ".kakuignore" && s.reason.contains("symbolic link")),
        "{:?}",
        out.skipped
    );
}

#[test]
fn ki07_without_a_list_nothing_changes() {
    let root = scratch("none");
    put(&root, "src/app.py", "print('hi')\n");
    put(&root, ".env", "A=1\n");
    let out = scan_tree(&root, Level::Internal);
    assert_eq!(
        out.files
            .iter()
            .find(|f| f.path == "src/app.py")
            .unwrap()
            .label
            .level,
        Level::Internal
    );
    assert_eq!(
        out.files
            .iter()
            .find(|f| f.path == ".env")
            .unwrap()
            .label
            .level,
        Level::Restricted,
        "the built-in secret rules still apply"
    );
    assert!(!out
        .files
        .iter()
        .any(|f| f.label.compartments.contains("owner-withheld")));
}
