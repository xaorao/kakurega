#![cfg(unix)]
//! A descriptor leaked by whatever started this program does not reach a program it starts after
//! `no_stray_descriptors`. Runs on macOS and Linux: the same call guards the sandbox and the helper launches.
use std::process::Command;

fn child_sees_seven() -> bool {
    // a child lists the descriptors it was handed
    let o = Command::new("/bin/sh")
        .arg("-c")
        .arg("ls /dev/fd")
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout)
        .split_whitespace()
        .any(|n| n == "7")
}

/// Started by the test below under a shell that holds descriptor 7 open with no close-on-exec.
#[test]
#[ignore = "run by st01 with a stray descriptor held open"]
fn st01_inner() {
    if std::env::var("KK_ST01_INNER").is_err() {
        return;
    }
    // the control first: without the call a child inherits it
    println!("ST01 before {}", child_sees_seven());
    kk_sandbox::no_stray_descriptors();
    println!("ST01 after {}", child_sees_seven());
}

#[test]
fn st01_a_leaked_descriptor_reaches_a_child_until_no_stray_descriptors_is_called() {
    let exe = std::env::current_exe().unwrap();
    let o = Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"exec 7</dev/null; exec "$@""#)
        .arg("st01")
        .arg(&exe)
        .args([
            "--exact",
            "st01_inner",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("KK_ST01_INNER", "1")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(
        out.contains("ST01 before true"),
        "the control failed: the stray descriptor was not inherited to begin with: {out}"
    );
    assert!(
        out.contains("ST01 after false"),
        "after the call a child still inherits it: {out}"
    );
}
