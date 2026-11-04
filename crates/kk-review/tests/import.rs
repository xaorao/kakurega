use kk_review::import::{keyed_commitment, manifest, scan_tree, MAX_FILE_BYTES};
use kk_types::label::{Integrity, Level};
use std::fs;
use std::path::{Path, PathBuf};

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for e in fs::read_dir(src).unwrap().flatten() {
        let name = e.file_name();
        let target = if name == "_env" {
            dst.join(".env")
        } else {
            dst.join(&name)
        };
        if e.path().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            fs::copy(e.path(), target).unwrap();
        }
    }
}

fn fixture(tag: &str) -> PathBuf {
    let dst = std::env::temp_dir().join(format!("kk-import-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dst);
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic-repo"),
        &dst,
    );
    dst
}

#[test]
fn i01_the_synthetic_repository_is_labelled_the_way_the_truth_file_says() {
    let root = fixture("labels");
    let imp = scan_tree(&root, Level::Internal);
    let by = |p: &str| {
        imp.files
            .iter()
            .find(|f| f.path == p)
            .unwrap_or_else(|| panic!("missing {p}"))
    };
    let env = by(".env");
    assert_eq!(env.label.level, Level::Restricted);
    assert!(env.label.compartments.contains("secrets"));
    assert_eq!(by("payments/auth/session.py").label.level, Level::Internal);
    assert_eq!(
        by("payments/config.py").label.level,
        Level::Internal,
        "reading a variable name is not a secret"
    );
    for f in &imp.files {
        assert_eq!(
            f.label.integrity,
            Integrity::Untrusted,
            "{}: imported text is never trusted",
            f.path
        );
    }
    for hostile in ["README.md", "docs/vendor-notes.md"] {
        assert!(
            by(hostile)
                .notes
                .iter()
                .any(|n| n.contains("prompt injection")),
            "{hostile}: {:?}",
            by(hostile).notes
        );
    }
    assert!(!by("payments/auth/session.py")
        .notes
        .iter()
        .any(|n| n.contains("injection")));
    assert!(
        imp.files
            .iter()
            .filter(|f| f.label.level == Level::Restricted)
            .count()
            == 1
    );
}

#[test]
fn i02_unsafe_inputs_are_skipped_with_a_reason() {
    let root = fixture("unsafe");
    std::os::unix::fs::symlink("/etc/hosts", root.join("link-to-hosts")).unwrap();
    fs::write(root.join("blob.bin"), [0u8, 1, 2, 3]).unwrap();
    fs::write(root.join("latin1.txt"), [0xff, 0xfe, 0x41]).unwrap();
    fs::write(
        root.join("huge.txt"),
        vec![b'a'; (MAX_FILE_BYTES + 1) as usize],
    )
    .unwrap();
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".git/config"), "[core]").unwrap();
    let imp = scan_tree(&root, Level::Internal);
    let reason = |p: &str| {
        imp.skipped
            .iter()
            .find(|s| s.path == p)
            .map(|s| s.reason.clone())
            .unwrap_or_default()
    };
    assert!(reason("link-to-hosts").contains("symbolic link"));
    assert!(reason("blob.bin").contains("binary"));
    assert!(reason("latin1.txt").contains("UTF-8"));
    assert!(reason("huge.txt").contains("limit"));
    assert!(reason(".git/").contains("directory"));
    assert!(!imp
        .files
        .iter()
        .any(|f| f.path.starts_with(".git") || f.path == "link-to-hosts"));
}

#[test]
fn i03_secrets_are_found_by_name_and_by_content_not_by_the_word_secret_alone() {
    let root = std::env::temp_dir().join(format!("kk-import-secrets-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("deploy.pem"), "anything").unwrap();
    fs::write(
        root.join("notes.txt"),
        "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n",
    )
    .unwrap();
    fs::write(
        root.join("settings.ini"),
        "db_password = correcthorsebatterystaple\n",
    )
    .unwrap();
    fs::write(
        root.join("url.txt"),
        "postgres://user:pa55word@db.internal/prod\n",
    )
    .unwrap();
    fs::write(
        root.join("docs.md"),
        "We never store a secret in the repository.\n",
    )
    .unwrap();
    fs::write(
        root.join("code.py"),
        "TOKEN = os.environ['TOKEN_VALUE_FROM_ENV']\n",
    )
    .unwrap();
    let imp = scan_tree(&root, Level::Internal);
    let level = |p: &str| imp.files.iter().find(|f| f.path == p).unwrap().label.level;
    for p in ["deploy.pem", "notes.txt", "settings.ini", "url.txt"] {
        assert_eq!(level(p), Level::Restricted, "{p}");
    }
    for p in ["docs.md", "code.py"] {
        assert_eq!(level(p), Level::Internal, "{p}");
    }
}

#[test]
fn i04_the_manifest_is_canonical_and_the_commitment_needs_the_key() {
    let root = fixture("manifest");
    let imp = scan_tree(&root, Level::Internal);
    let m = manifest(&imp.files);
    assert_eq!(m.file_count, imp.files.len());
    assert!(kk_evidence::canon::canonicalize(m.canonical.as_bytes()).unwrap() == m.canonical);
    assert_eq!(manifest(&imp.files).digest, m.digest, "deterministic");
    let k1 = [1u8; 32];
    let k2 = [2u8; 32];
    let c1 = keyed_commitment(&k1, &m);
    assert_eq!(c1.len(), 64);
    assert_ne!(
        c1,
        keyed_commitment(&k2, &m),
        "a different key gives a different commitment"
    );
    assert_ne!(c1, m.digest, "the commitment is not the plain digest");
    fs::write(root.join("payments/auth/session.py"), "# changed\n").unwrap();
    let changed = manifest(&scan_tree(&root, Level::Internal).files);
    assert_ne!(changed.digest, m.digest);
    assert_ne!(keyed_commitment(&k1, &changed), c1);
    let mut reordered = imp.files.clone();
    reordered.reverse();
    assert_eq!(
        manifest(&reordered).digest,
        m.digest,
        "file order does not matter"
    );
}
