//! A file holding a password written out in words is withheld from a review like any other secret file.
use kk_review::import::scan_tree;
use kk_types::label::Level;

#[test]
fn im_a_password_written_in_words_withholds_the_file_and_ordinary_talk_does_not() {
    let dir = std::env::temp_dir().join(format!("kk-spoken-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("handover.md"), "# Handover\nThe password for the staging router is Blue42Harbor, ask Sam before changing it.\n").unwrap();
    std::fs::write(
        dir.join("policy.md"),
        "# Policy\nThe password policy requires twelve characters. max_tokens: 4096\n",
    )
    .unwrap();
    let files = scan_tree(&dir, Level::Internal).files;
    let of = |name: &str| files.iter().find(|f| f.path == name).unwrap();
    assert_eq!(
        of("handover.md").label.level,
        Level::Restricted,
        "{:?}",
        of("handover.md").notes
    );
    assert!(of("handover.md")
        .notes
        .iter()
        .any(|n| n.contains("written out in words")));
    assert_eq!(
        of("policy.md").label.level,
        Level::Internal,
        "{:?}",
        of("policy.md").notes
    );
    let _ = std::fs::remove_dir_all(&dir);
}
