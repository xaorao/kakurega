use kk_tui::project::detect;
use std::path::PathBuf;

fn tree(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kk-proj-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn pj01_the_project_is_the_folder_of_the_nearest_repository_above_the_start() {
    let root = tree("a");
    let repo = root.join("papermark");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("apps/web/src")).unwrap();
    assert_eq!(detect(&repo).as_deref(), Some("papermark"));
    assert_eq!(
        detect(&repo.join("apps/web/src")).as_deref(),
        Some("papermark")
    );
    // a repository inside a repository: the nearest one
    let inner = repo.join("vendor/lib");
    std::fs::create_dir_all(inner.join(".git")).unwrap();
    assert_eq!(detect(&inner).as_deref(), Some("lib"));
    // a Mercurial checkout counts, and a `.git` file (a worktree or submodule) counts too
    let hg = root.join("hgproj");
    std::fs::create_dir_all(hg.join(".hg")).unwrap();
    assert_eq!(detect(&hg).as_deref(), Some("hgproj"));
    let wt = root.join("worktree");
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join(".git"), "gitdir: ../elsewhere\n").unwrap();
    assert_eq!(detect(&wt).as_deref(), Some("worktree"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pj02_outside_any_repository_there_is_no_project_so_a_home_folder_claims_none() {
    // a folder tree with no repository marker anywhere above it up to the temp root
    let root = tree("b");
    let plain = root.join("notes/today");
    std::fs::create_dir_all(&plain).unwrap();
    // the temp folder itself may sit inside a repository on some machines; only assert when it does not
    let above_has_marker = root
        .ancestors()
        .any(|d| d.join(".git").exists() || d.join(".hg").exists());
    if !above_has_marker {
        assert_eq!(detect(&plain), None);
    }
    assert_eq!(detect(std::path::Path::new("/")), None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn pj03_a_folder_name_the_service_would_refuse_is_never_offered_as_the_project() {
    let root = tree("c");
    let long = root.join("x".repeat(81));
    std::fs::create_dir_all(long.join(".git")).unwrap();
    assert_eq!(detect(&long), None, "81 characters is too long");
    let spaced = root.join("a project ");
    std::fs::create_dir_all(spaced.join(".git")).unwrap();
    assert_eq!(
        detect(&spaced),
        None,
        "a trailing space is refused by the service"
    );
    let _ = std::fs::remove_dir_all(&root);
}
