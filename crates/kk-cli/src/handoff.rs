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
pub fn tool() -> Option<String> {
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
        let mut child = Command::new(&program)
            .args(ARGS)
            .current_dir(&dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Claude Code could not be started: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| e.to_string())?;
        }
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
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        answer_in(&String::from_utf8_lossy(&out.stdout))
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
    }
}
