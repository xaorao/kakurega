//! What a real crash of the real program leaves behind.

use std::process::{Command, Stdio};

#[test]
fn cp01_a_real_crash_writes_one_readable_report_without_its_message() {
    let dir = std::env::temp_dir().join(format!("kk-crash-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_kaku"))
        .args(["crash-probe", dir.to_str().unwrap()])
        .env_clear()
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(101),
        "the program still dies the usual way"
    );
    let shown = String::from_utf8_lossy(&out.stderr);
    assert!(
        shown.contains("probe: could not read"),
        "the usual message still reaches the terminal: {shown}"
    );
    let all = kk_service::crash::reports(&dir);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(
        all[0].place.split(':').next().unwrap(),
        "crates/kk-cli/src/main.rs"
    );
    assert_eq!(all[0].version, env!("CARGO_PKG_VERSION"));
    let file = std::fs::read_dir(dir.join("crashes"))
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .path();
    let raw = std::fs::read_to_string(file).unwrap();
    for leak in ["someone", "private", "notes.txt", "sk_live", "/Users"] {
        assert!(!raw.contains(leak), "{leak} reached the report: {raw}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
