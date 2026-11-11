//! Handing a piece of text to the Claude Code installed on this computer, when the owner has read that exact text and
//! said yes. It is run for one answer with every tool switched off, no tool servers, nothing saved, in a new empty
//! folder: what it can work on is the text and nothing else. It is not private: Claude Code sends the text to Anthropic
//! under the owner's own account, together with its own instructions and the owner's own Claude Code settings.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROGRAM: &str = "claude";
const TIME_LIMIT: Duration = Duration::from_secs(300);
const ANSWER_MAX_BYTES: u64 = 1024 * 1024;
/// What of this service's environment the program is given: where programs and the owner's home are, who they are, and
/// how text is written. Nothing else.
const PASSED_ON: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "TMPDIR", "TERM",
];
/// The options, checked against `claude --help` of 2.1: one answer and exit; no built-in tools; only the tool servers
/// given here, which is none; no session kept on disk; one JSON object as the result.
const ARGS: &[&str] = &[
    "-p",
    "--tools",
    "",
    "--strict-mcp-config",
    "--no-session-persistence",
    "--output-format",
    "json",
];

/// The program on the owner's PATH, if it is there and is a file that can be run.
fn find() -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|d| d.is_absolute())
        .map(|d| d.join(PROGRAM))
        .find(|p| {
            std::fs::metadata(p)
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        })
}

/// "Claude Code" and its version as it reports it, or nothing when it is not installed.
/// The installed coding agent, as `tool_now` finds it, remembered for a minute: several screens ask, and asking starts
/// the agent's own program each time.
pub fn tool() -> Option<String> {
    static SEEN: std::sync::Mutex<Option<(std::time::Instant, Option<String>)>> =
        std::sync::Mutex::new(None);
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, found)) = seen.as_ref() {
        if at.elapsed() < std::time::Duration::from_secs(60) {
            return found.clone();
        }
    }
    let found = tool_now();
    *seen = Some((std::time::Instant::now(), found.clone()));
    found
}

fn tool_now() -> Option<String> {
    let program = find()?;
    let out = Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let version: String = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .take(20)
        .collect();
    Some(if version.is_empty() {
        "Claude Code".to_string()
    } else {
        format!("Claude Code {version}")
    })
}

/// The answer in the program's JSON result, or why there is none.
pub fn answer_in(stdout: &str) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|_| "Claude Code answered in a form this program cannot read".to_string())?;
    if v["is_error"] == true {
        return Err("Claude Code reported an error and gave no answer".into());
    }
    match v["result"].as_str().map(str::trim) {
        Some(r) if !r.is_empty() => Ok(r.to_string()),
        _ => Err("Claude Code gave no answer".into()),
    }
}

/// Runs it once on exactly this text and returns its answer.
pub fn run(text: &str) -> Result<String, String> {
    let program = find().ok_or("Claude Code is not installed on this computer")?;
    let mut raw = [0u8; 8];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut raw))
        .map_err(|e| e.to_string())?;
    let dir = std::env::temp_dir().join(format!(
        "kk-handoff-{}",
        raw.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    std::fs::create_dir(&dir)
        .map_err(|e| format!("cannot make an empty folder for the run: {e}"))?;
    let result = (|| {
        // The program gets the few settings it needs to find the owner's sign-in and nothing else of this service's
        // environment: no account key of another service, no token, no proxy setting.
        let mut cmd = Command::new(&program);
        cmd.args(ARGS)
            .current_dir(&dir)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for name in PASSED_ON {
            if let Some(v) = std::env::var_os(name) {
                cmd.env(name, v);
            }
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Claude Code could not be started: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| e.to_string())?;
        }
        // The answer is read while the program runs: a long answer would otherwise fill the pipe and the program would
        // wait for ever to write the rest. More than the cap is not an answer this takes.
        let mut stdout = child.stdout.take().ok_or("Claude Code gave no answer")?;
        let reader = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(
                &mut std::io::Read::take(&mut stdout, ANSWER_MAX_BYTES + 1),
                &mut buf,
            );
            // whatever comes after the cap is read and dropped, so the program can still end
            let _ = std::io::copy(&mut stdout, &mut std::io::sink());
            buf
        });
        let started = Instant::now();
        loop {
            match child.try_wait().map_err(|e| e.to_string())? {
                Some(_) => break,
                None if started.elapsed() > TIME_LIMIT => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(
                        "Claude Code did not answer within five minutes and was stopped"
                            .to_string(),
                    );
                }
                None => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        let out = reader
            .join()
            .map_err(|_| "Claude Code's answer could not be read".to_string())?;
        if out.len() as u64 > ANSWER_MAX_BYTES {
            return Err("Claude Code's answer was too long to take".into());
        }
        answer_in(&String::from_utf8_lossy(&out))
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ho01_only_a_clean_result_is_an_answer() {
        assert_eq!(
            answer_in("{\"is_error\": false, \"result\": \"  Use a mutex.  \"}\n"),
            Ok("Use a mutex.".into())
        );
        for bad in [
            "",
            "not json",
            "{\"is_error\": true, \"result\": \"x\"}",
            "{\"result\": \"  \"}",
            "{\"result\": 5}",
            "[]",
        ] {
            assert!(answer_in(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn ho02_it_is_run_for_one_answer_with_no_tools_no_tool_servers_and_nothing_saved() {
        assert_eq!(ARGS[0], "-p");
        let at = ARGS.iter().position(|a| *a == "--tools").unwrap();
        assert_eq!(ARGS[at + 1], "", "the list of built-in tools is empty");
        assert!(ARGS.contains(&"--strict-mcp-config") && !ARGS.contains(&"--mcp-config"));
        assert!(ARGS.contains(&"--no-session-persistence"));
        for never in [
            "--dangerously-skip-permissions",
            "--add-dir",
            "--continue",
            "--resume",
        ] {
            assert!(!ARGS.contains(&never));
        }
        // no key or token of this service's environment is on the list that is passed on
        for name in PASSED_ON {
            let n = name.to_lowercase();
            assert!(
                !n.contains("key")
                    && !n.contains("token")
                    && !n.contains("secret")
                    && !n.contains("proxy"),
                "{name}"
            );
        }
    }
}
