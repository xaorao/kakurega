//! Text recognition for pictures and scanned pages, through the operating system's own engine.
//!
//! The engine is a small helper program (`kaku-ocr`, macOS Vision). It is started by the attachment reader, never by the
//! service, with no environment, under a Seatbelt profile that denies network and file writes, with a time limit and an
//! output cap, so a hostile picture can only cost one attachment. Nothing is sent anywhere. Without the helper, or without
//! the sandbox tool, nothing is recognised and the file says so.

use kk_parse::{Medium, Recognised, Recogniser};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long one recognition may take.
pub const TIME_LIMIT: Duration = Duration::from_secs(45);
/// The most the helper may answer with.
const OUTPUT_LIMIT: u64 = 4 * 1024 * 1024;
const SANDBOX: &str = "/usr/bin/sandbox-exec";
pub const PROFILE: &str = "(version 1)(allow default)(deny network*)(deny file-write*)";

pub struct Helper {
    pub exe: PathBuf,
    pub limit: Duration,
    /// Run under the Seatbelt profile. Only the tests with a stand-in program turn this off.
    pub confined: bool,
}

impl Helper {
    pub fn new(exe: PathBuf) -> Self {
        Helper {
            exe,
            limit: TIME_LIMIT,
            confined: true,
        }
    }
}

/// Where the helper is: `KAKU_OCR`, then next to this program. `None` when there is none.
pub fn find() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("KAKU_OCR") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let beside = std::env::current_exe().ok()?.with_file_name("kaku-ocr");
    beside.is_file().then_some(beside)
}

impl Recogniser for Helper {
    fn recognise(
        &self,
        medium: Medium,
        bytes: &[u8],
        pages: &[u32],
    ) -> Result<Vec<Recognised>, String> {
        let kind = match medium {
            Medium::Picture => "picture",
            Medium::Pdf => "pdf",
        };
        let mut cmd = if self.confined {
            if !std::path::Path::new(SANDBOX).is_file() {
                return Err("the sandbox needed to run recognition safely is missing".into());
            }
            let mut c = Command::new(SANDBOX);
            c.arg("-p").arg(PROFILE).arg(&self.exe);
            c
        } else {
            Command::new(&self.exe)
        };
        // the helper reads a picture from outside: it inherits nothing this program was started with
        kk_sandbox::no_stray_descriptors();
        cmd.arg("--kind").arg(kind);
        if !pages.is_empty() {
            let list: Vec<String> = pages.iter().take(200).map(|n| n.to_string()).collect();
            cmd.arg("--pages").arg(list.join(","));
        }
        cmd.env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|_| "the recogniser could not be started".to_string())?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or("the recogniser could not be started")?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or("the recogniser could not be started")?;
        let input = bytes.to_vec();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let _ = (&mut stdout).take(OUTPUT_LIMIT + 1).read_to_end(&mut out);
            let _ = tx.send(out);
        });
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) if started.elapsed() >= self.limit => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(_) => {
                    let _ = child.kill();
                    break None;
                }
            }
        };
        let Some(status) = status else {
            // a program the helper started may still hold the pipe open: the answer is not waited for
            drop(rx);
            return Err("recognition took too long and was stopped".into());
        };
        let _ = writer.join();
        let out = rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
        if out.len() as u64 > OUTPUT_LIMIT {
            return Err("the recogniser's answer was too large".into());
        }
        if !status.success() {
            return Err("the recogniser stopped unexpectedly".into());
        }
        let v: serde_json::Value = serde_json::from_slice(&out)
            .map_err(|_| "the recogniser's answer could not be understood".to_string())?;
        if let Some(code) = v.get("error").and_then(|e| e.as_str()) {
            return Err(match code {
                "too_large" => "the picture is too large to recognise".to_string(),
                _ => "the picture could not be read".to_string(),
            });
        }
        let list = v
            .get("pages")
            .and_then(|p| p.as_array())
            .ok_or("the recogniser's answer could not be understood")?;
        let mut done = Vec::new();
        for p in list.iter().take(200) {
            let page = p
                .get("page")
                .and_then(|n| n.as_u64())
                .ok_or("the recogniser's answer could not be understood")?;
            let text = p
                .get("text")
                .and_then(|t| t.as_str())
                .ok_or("the recogniser's answer could not be understood")?;
            let confidence = p.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.0);
            done.push(Recognised {
                page: page as u32,
                text: text.to_string(),
                confidence,
            });
        }
        Ok(done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kk-ocr-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn stand_in(name: &str, body: &str, limit: Duration) -> Helper {
        Helper {
            exe: script(name, body),
            limit,
            confined: false,
        }
    }

    const GOOD: &str = r#"{"pages":[{"page":1,"text":"hello","confidence":0.8}],"skipped":[]}"#;

    #[test]
    fn oc01_a_good_answer_is_read_back_with_the_pages_asked_for() {
        let h = stand_in(
            "good",
            &format!("cat >/dev/null; echo '{GOOD}'"),
            Duration::from_secs(10),
        );
        let r = h.recognise(Medium::Picture, b"x", &[]).unwrap();
        assert_eq!((r.len(), r[0].page, r[0].text.as_str()), (1, 1, "hello"));
        let echo_args = stand_in(
            "args",
            "cat >/dev/null; echo \"{\\\"pages\\\":[{\\\"page\\\":1,\\\"text\\\":\\\"$*\\\"}]}\"",
            Duration::from_secs(10),
        );
        let r = echo_args.recognise(Medium::Pdf, b"x", &[3, 5]).unwrap();
        assert_eq!(r[0].text, "--kind pdf --pages 3,5");
    }

    #[test]
    fn oc06_a_helper_is_confined_unless_a_test_says_otherwise() {
        let h = Helper::new(PathBuf::from("/x/kaku-ocr"));
        assert!(h.confined);
        assert_eq!(h.limit, TIME_LIMIT);
    }

    #[test]
    fn oc02_a_helper_that_never_finishes_is_stopped_at_the_limit() {
        let h = stand_in("hang", "sleep 30", Duration::from_millis(300));
        let started = Instant::now();
        let e = h.recognise(Medium::Picture, b"x", &[]).unwrap_err();
        assert!(e.contains("too long"), "{e}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn oc02b_a_helper_that_leaves_a_child_holding_the_pipe_does_not_hold_up_the_reader() {
        let h = stand_in(
            "orphan",
            "sleep 30 & cat >/dev/null; sleep 30",
            Duration::from_millis(300),
        );
        let started = Instant::now();
        let e = h.recognise(Medium::Picture, b"x", &[]).unwrap_err();
        assert!(e.contains("too long"), "{e}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn oc03_a_crash_junk_a_flood_or_a_missing_helper_is_one_failed_picture() {
        for (i, body) in [
            "exit 3",
            "kill -9 $$",
            "echo not json",
            "echo '{}'",
            "echo '{\"pages\":[{\"page\":\"one\"}]}'",
            "true",
        ]
        .iter()
        .enumerate()
        {
            let h = stand_in(&format!("bad{i}"), body, Duration::from_secs(10));
            assert!(h.recognise(Medium::Picture, b"x", &[]).is_err(), "{body}");
        }
        let flood = stand_in(
            "flood",
            "cat >/dev/null; head -c 9000000 /dev/zero | tr '\\0' 'a'",
            Duration::from_secs(20),
        );
        assert!(flood
            .recognise(Medium::Picture, b"x", &[])
            .unwrap_err()
            .contains("too large"));
        let missing = Helper {
            exe: PathBuf::from("/nonexistent/kaku-ocr"),
            limit: Duration::from_secs(1),
            confined: false,
        };
        assert!(missing
            .recognise(Medium::Picture, b"x", &[])
            .unwrap_err()
            .contains("could not be started"));
    }

    #[test]
    fn oc04_the_helper_gets_no_environment_and_its_error_codes_become_plain_reasons() {
        std::env::set_var("KK_SECRET_PROBE", "leak");
        let h = stand_in("env", "cat >/dev/null; if [ -n \"$KK_SECRET_PROBE\" ]; then echo '{\"error\":\"leaked\"}'; else echo '{\"error\":\"too_large\"}'; fi", Duration::from_secs(10));
        assert_eq!(
            h.recognise(Medium::Picture, b"x", &[]).unwrap_err(),
            "the picture is too large to recognise"
        );
        let h = stand_in(
            "unreadable",
            "cat >/dev/null; echo '{\"error\":\"unreadable\"}'",
            Duration::from_secs(10),
        );
        assert_eq!(
            h.recognise(Medium::Picture, b"x", &[]).unwrap_err(),
            "the picture could not be read"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn oc05_the_profile_denies_writing_files_and_the_network() {
        let dir = std::env::temp_dir().join(format!("kk-ocr-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("written.txt");
        let write = Command::new(SANDBOX)
            .args([
                "-p",
                PROFILE,
                "/bin/sh",
                "-c",
                &format!("echo x > {}", target.display()),
            ])
            .env_clear()
            .status()
            .unwrap();
        assert!(
            !write.success() && !target.exists(),
            "a confined helper must not write files"
        );
        // a listener on this computer: the same connection succeeds outside the profile and fails inside it
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port().to_string();
        let free = Command::new("/usr/bin/nc")
            .args(["-z", "-w", "2", "127.0.0.1", &port])
            .env_clear()
            .status()
            .unwrap();
        assert!(
            free.success(),
            "the control connection must work without the profile"
        );
        let net = Command::new(SANDBOX)
            .args([
                "-p",
                PROFILE,
                "/usr/bin/nc",
                "-z",
                "-w",
                "2",
                "127.0.0.1",
                &port,
            ])
            .env_clear()
            .status()
            .unwrap();
        assert!(
            !net.success(),
            "a confined helper must not reach the network"
        );
        let ok = Command::new(SANDBOX)
            .args(["-p", PROFILE, "/bin/echo", "fine"])
            .env_clear()
            .status()
            .unwrap();
        assert!(
            ok.success(),
            "the profile must still let a harmless program run"
        );
    }
}
