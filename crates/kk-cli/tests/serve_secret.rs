use std::io::Write;
use std::process::{Command, Stdio};

fn serve_with_stdin(input: &[u8]) -> (Option<i32>, String) {
    let dir = std::env::temp_dir().join(format!("kk-serve-secret-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_kaku"))
        .args(["serve", "--shell-secret-stdin", "--data-dir"])
        .arg(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let _ = stdin.write_all(input);
    drop(stdin);
    let out = child.wait_with_output().unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn ss01_serve_refuses_to_start_when_the_shell_secret_is_missing_or_weak() {
    for input in [
        &b""[..],
        b"\n",
        b"short\n",
        b"   \n",
        &[b'a'; 31],
        b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    ] {
        let (code, err) = serve_with_stdin(input);
        assert_eq!(code, Some(1), "{input:?}: {err}");
        assert!(err.contains("at least 32 characters"), "{input:?}: {err}");
    }
}
