//! Reading an attached file in a separate process.
//!
//! A file from a stranger is where parser bugs get used, so the service never parses one itself. It starts this program
//! again as `kaku parse-attachment`, hands the bytes over standard input, and reads one line of JSON back. The reader
//! gets no environment and no network code path, is stopped after a time limit, and can only answer through that line, so
//! a hang, a crash or a runaway output costs one attachment and nothing else.

use kk_parse::{Limits, ParseError, Parsed};
use std::io::{Read, Write};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

/// How long one file may take.
pub const TIME_LIMIT: Duration = Duration::from_secs(30);
/// The most JSON the reader may answer with: the text limit, with room for escaping.
const OUTPUT_LIMIT: u64 = 12 * 1024 * 1024;

/// `kaku parse-attachment --name NAME`: bytes on standard input, one JSON line on standard output.
pub fn child(args: &[String]) -> ExitCode {
    let limits = Limits::default();
    let name = crate::flag(args, "--name").unwrap_or_default();
    // the service names the helper; this reader has no environment of its own to look it up in
    let helper = crate::flag(args, "--ocr")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_file())
        .map(crate::ocr::Helper::new);
    let mut bytes = Vec::new();
    let read = std::io::stdin()
        .lock()
        .take(limits.max_input as u64 + 1)
        .read_to_end(&mut bytes);
    let answer = match read {
        Err(_) => ParseError::Damaged("the file could not be read".into()).to_json(),
        Ok(_) => match kk_parse::parse_with(
            &name,
            &bytes,
            &limits,
            helper.as_ref().map(|h| h as &dyn kk_parse::Recogniser),
        ) {
            Ok(p) => p.to_json(),
            Err(e) => e.to_json(),
        },
    };
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{answer}");
    let _ = out.flush();
    ExitCode::SUCCESS
}

/// Reads `bytes` in a child process of this program.
pub fn run_isolated(name: &str, bytes: &[u8]) -> Result<Parsed, ParseError> {
    let name = kk_parse::clean_name(name)?;
    let exe = std::env::current_exe()
        .map_err(|_| ParseError::Damaged("the reader could not be started".into()))?;
    // the reader works on a file from outside: it inherits nothing this program was started with
    kk_sandbox::no_stray_descriptors();
    let mut cmd = Command::new(exe);
    cmd.arg("parse-attachment").arg("--name").arg(&name);
    let mut limit = TIME_LIMIT;
    if let Some(helper) = crate::ocr::find() {
        cmd.arg("--ocr").arg(helper);
        // recognition has its own limit inside; the reader waits for it
        limit = TIME_LIMIT + crate::ocr::TIME_LIMIT;
    }
    run_child(cmd, bytes, limit)
}

/// Runs `cmd` as the reader: no environment, bytes in, one JSON line out, killed at the limit. The command is a parameter so
/// the ways a reader can go wrong are tested with stand-ins.
pub(crate) fn run_child(
    mut cmd: Command,
    bytes: &[u8],
    limit: Duration,
) -> Result<Parsed, ParseError> {
    let broken = |what: &str| ParseError::Damaged(what.to_string());
    cmd.env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|_| broken("the reader could not be started"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| broken("the reader could not be started"))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| broken("the reader could not be started"))?;
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
            Ok(None) if started.elapsed() >= limit => {
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
        // a program the reader started may still hold the pipe open: the answer is not waited for
        drop(rx);
        return Err(broken("reading took too long and was stopped"));
    };
    let _ = writer.join();
    let out = rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    // a reader cut off for writing too much dies of the closed pipe, so the size is judged before the exit status
    if out.len() as u64 > OUTPUT_LIMIT {
        return Err(broken("the reader's answer was too large"));
    }
    if !status.success() {
        return Err(broken("the reader stopped unexpectedly"));
    }
    let v: serde_json::Value = serde_json::from_slice(&out)
        .map_err(|_| broken("the reader's answer could not be understood"))?;
    if let Some(e) = ParseError::from_json(&v) {
        return Err(e);
    }
    Parsed::from_json(&v).ok_or_else(|| broken("the reader's answer could not be understood"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Command {
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(script);
        c
    }

    const GOOD: &str = r#"{"name":"a.txt","kind":"text","passages":[{"anchor":"line 1","text":"hi"}],"notes":[],"truncated":false}"#;

    #[test]
    fn pc01_a_good_answer_is_read_back() {
        let p = run_child(
            sh(&format!("cat >/dev/null; echo '{GOOD}'")),
            b"hello",
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!((p.name.as_str(), p.passages.len()), ("a.txt", 1));
    }

    #[test]
    fn pc02_a_reader_that_never_finishes_is_stopped_at_the_limit() {
        let started = Instant::now();
        let e = run_child(sh("sleep 30"), b"hello", Duration::from_millis(300)).unwrap_err();
        assert!(
            matches!(&e, ParseError::Damaged(w) if w.contains("too long")),
            "{e}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn pc02b_a_reader_that_leaves_a_child_holding_the_pipe_does_not_hold_up_the_service() {
        let started = Instant::now();
        let e = run_child(
            sh("sleep 30 & sleep 30"),
            b"hello",
            Duration::from_millis(300),
        )
        .unwrap_err();
        assert!(
            matches!(&e, ParseError::Damaged(w) if w.contains("too long")),
            "{e}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn pc03_a_crash_junk_or_a_missing_reader_is_one_failed_attachment() {
        for script in [
            "exit 3",
            "kill -9 $$",
            "echo not json",
            "echo '{}'",
            "echo '{\"error\":{\"code\":\"nope\"}}'",
            "true",
        ] {
            let e = run_child(sh(script), b"x", Duration::from_secs(10)).unwrap_err();
            assert!(matches!(e, ParseError::Damaged(_)), "{script}: {e}");
        }
        for crash in ["exit 3", "kill -9 $$"] {
            let e = run_child(sh(crash), b"x", Duration::from_secs(10)).unwrap_err();
            assert!(
                matches!(&e, ParseError::Damaged(w) if w.contains("stopped unexpectedly")),
                "{crash}: {e}"
            );
        }
        let e = run_child(
            Command::new("/nonexistent/reader"),
            b"x",
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(matches!(e, ParseError::Damaged(w) if w.contains("could not be started")));
    }

    #[test]
    fn pc04_a_reader_that_floods_its_output_is_cut_off() {
        let e = run_child(
            sh("cat >/dev/null; head -c 20000000 /dev/zero | tr '\\0' 'a'"),
            b"x",
            Duration::from_secs(20),
        )
        .unwrap_err();
        assert!(
            matches!(&e, ParseError::Damaged(w) if w.contains("too large")),
            "{e}"
        );
    }

    #[test]
    fn pc05_the_reader_gets_no_environment_and_a_parse_error_comes_through_as_itself() {
        std::env::set_var("KK_SECRET_PROBE", "leak");
        let e = run_child(sh("cat >/dev/null; if [ -n \"$KK_SECRET_PROBE\" ]; then echo '{\"error\":{\"code\":\"bomb\",\"detail\":\"leaked\"}}'; else echo '{\"error\":{\"code\":\"protected\",\"detail\":\"\"}}'; fi"), b"x", Duration::from_secs(10)).unwrap_err();
        assert_eq!(
            e,
            ParseError::Protected,
            "the environment did not reach the reader"
        );
    }

    #[test]
    fn pc06_a_bad_name_never_starts_a_reader() {
        assert_eq!(
            run_isolated("../x.txt", b"hello").unwrap_err(),
            ParseError::BadName
        );
    }
}
