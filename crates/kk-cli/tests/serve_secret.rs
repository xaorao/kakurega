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

/// Kills the service if a test ends early, so a failing run leaves nothing running.
struct Reaper(std::process::Child);

impl Drop for Reaper {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_until(what: &str, secs: u64, mut done: impl FnMut() -> bool) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < end {
        if done() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn ss02_a_service_started_by_a_shell_stays_while_the_pipe_is_open_and_leaves_when_the_shell_is_gone(
) {
    let dir = std::env::temp_dir().join(format!("kk-serve-pipe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Reaper(
        Command::new(env!("CARGO_BIN_EXE_kaku"))
            .args(["serve", "--shell-secret-stdin", "--scripted"])
            .arg(std::env::temp_dir().join("kk-nonexistent-script.json"))
            .arg("--data-dir")
            .arg(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut stdin = child.0.stdin.take().unwrap();
    writeln!(stdin, "{}", "s".repeat(43)).unwrap();
    stdin.flush().unwrap();
    let admin = dir.join("admin.json");
    wait_until("the service to start", 60, || {
        admin.exists() || child.0.try_wait().unwrap().is_some()
    });
    assert!(
        child.0.try_wait().unwrap().is_none(),
        "the service started and is running"
    );
    // while the shell holds the pipe, the service stays (it would otherwise leave on its own after the secret was read)
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert!(
        child.0.try_wait().unwrap().is_none(),
        "the service left while its shell was still there"
    );
    assert!(admin.exists());
    // the shell goes away: the pipe closes and the service leaves by itself, taking its admin file with it
    drop(stdin);
    wait_until("the service to leave", 30, || {
        child.0.try_wait().unwrap().is_some()
    });
    assert!(!admin.exists(), "the admin file is removed");
    let _ = std::fs::remove_dir_all(&dir);
}
