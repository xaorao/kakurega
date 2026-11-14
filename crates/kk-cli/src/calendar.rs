//! The owner's calendar and open reminders, read on this computer to attach to a question.
//!
//! The reader is a small helper program (`kaku-calendar`, macOS EventKit). It is started with no network (a Seatbelt profile),
//! a time limit and an output cap. `status` never asks macOS for access; `ask` does, and is used only after a person pressed a
//! button or typed `@calendar`. What it reads becomes an attached file like any other, so the same checks apply before
//! anything reaches a model. Nothing is sent anywhere by reading it.

use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a read may take, and how long a person has to answer macOS's question.
const READ_LIMIT: Duration = Duration::from_secs(30);
const ASK_LIMIT: Duration = Duration::from_secs(130);
const OUTPUT_LIMIT: u64 = 2 * 1024 * 1024;
const SANDBOX: &str = "/usr/bin/sandbox-exec";
/// The calendar is the owner's own data, so the reader may keep the caches EventKit writes; it may not reach the network.
pub const PROFILE: &str = "(version 1)(allow default)(deny network*)";
/// The most days one attachment covers.
pub const MAX_DAYS: u32 = 31;

pub struct Helper {
    pub exe: PathBuf,
    /// Run under the Seatbelt profile. Only the tests with a stand-in program turn this off.
    pub confined: bool,
}

/// Where the reader is: `KAKU_CALENDAR`, then next to this program. `None` when there is none.
pub fn find() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("KAKU_CALENDAR") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let beside = std::env::current_exe()
        .ok()?
        .with_file_name("kaku-calendar");
    (cfg!(target_os = "macos") && beside.is_file()).then_some(beside)
}

impl Helper {
    fn run(&self, args: &[&str], limit: Duration) -> Result<Value, String> {
        let mut cmd = if self.confined {
            if !std::path::Path::new(SANDBOX).is_file() {
                return Err("the sandbox needed to read the calendar safely is missing".into());
            }
            let mut c = Command::new(SANDBOX);
            c.arg("-p").arg(PROFILE).arg(&self.exe);
            c
        } else {
            Command::new(&self.exe)
        };
        // the helper inherits nothing this program was started with
        kk_sandbox::no_stray_descriptors();
        cmd.args(args).env_clear();
        // EventKit finds its own store through the home folder; nothing else is passed on
        for key in ["HOME", "TMPDIR"] {
            if let Ok(v) = std::env::var(key) {
                cmd.env(key, v);
            }
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|_| "the calendar reader could not be started".to_string())?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or("the calendar reader could not be started")?;
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
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => {
                    let _ = child.kill();
                    break None;
                }
            }
        };
        let Some(status) = status else {
            drop(rx);
            return Err("the calendar reader took too long and was stopped".into());
        };
        let out = rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
        if out.len() as u64 > OUTPUT_LIMIT {
            return Err("the calendar reader's answer was too large".into());
        }
        if !status.success() {
            return Err("the calendar reader stopped unexpectedly".into());
        }
        let v: Value = serde_json::from_slice(&out)
            .map_err(|_| "the calendar reader's answer could not be understood".to_string())?;
        if let Some(code) = v.get("error").and_then(Value::as_str) {
            return Err(match code {
                "calendar_not_allowed" => {
                    "macOS does not allow this program to read the calendar".to_string()
                }
                "reminders_not_allowed" => {
                    "macOS does not allow this program to read reminders".to_string()
                }
                "timed_out" => "the reminders did not arrive in time".to_string(),
                _ => "the calendar could not be read".to_string(),
            });
        }
        Ok(v)
    }

    /// `{"calendar": state, "reminders": state}`. Never asks macOS anything, so it never shows a prompt.
    pub fn status(&self) -> Result<Value, String> {
        let v = self.run(&["status"], READ_LIMIT)?;
        Ok(states(&v))
    }

    /// Asks macOS for access (it shows its own prompt) and gives the state after the person answered.
    pub fn ask(&self) -> Result<Value, String> {
        let v = self.run(&["ask"], ASK_LIMIT)?;
        Ok(states(&v))
    }

    /// The next `days` days of the calendar, and the open reminders when `reminders`, as text to attach.
    pub fn read(&self, days: u32, reminders: bool) -> Result<String, String> {
        if !(1..=MAX_DAYS).contains(&days) {
            return Err(format!("a calendar is attached for 1 to {MAX_DAYS} days"));
        }
        let events = self.run(&["events", "--days", &days.to_string()], READ_LIMIT)?;
        let open = if reminders {
            Some(self.run(&["reminders"], READ_LIMIT)?)
        } else {
            None
        };
        Ok(as_text(&events, open.as_ref(), days))
    }
}

/// Only the two known keys, each one of the known states, so nothing else from the helper's answer travels on.
fn states(v: &Value) -> Value {
    let one = |k: &str| {
        let s = v[k].as_str().unwrap_or("unknown");
        match s {
            "allowed" | "not_asked" | "denied" | "restricted" | "write_only" => s,
            _ => "unknown",
        }
    };
    serde_json::json!({"calendar": one("calendar"), "reminders": one("reminders")})
}

/// `2026-11-14T09:00:00+05:30` as `2026-11-14 09:00`; anything else as it came, cut short.
fn when(iso: &str, date_only: bool) -> String {
    let ok = iso.len() >= 16 && iso.as_bytes().get(10) == Some(&b'T');
    match (ok, date_only) {
        (true, true) => iso[..10].to_string(),
        (true, false) => format!("{} {}", &iso[..10], &iso[11..16]),
        _ => iso.chars().take(25).collect(),
    }
}

fn text_of(v: &Value, key: &str) -> String {
    v[key]
        .as_str()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(600)
        .collect()
}

/// The calendar and reminders as a short Markdown file: one line per event or reminder, its notes indented under it.
pub fn as_text(events: &Value, reminders: Option<&Value>, days: u32) -> String {
    let mut out = format!(
        "# Calendar: the next {days} day{}, read on this computer\n\n",
        if days == 1 { "" } else { "s" }
    );
    let list = events["events"].as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        out.push_str("Nothing is on the calendar for these days.\n");
    }
    for e in &list {
        let all_day = e["all_day"].as_bool().unwrap_or(false);
        let (start, end) = (text_of(e, "start"), text_of(e, "end"));
        let span = if all_day {
            format!("{} all day", when(&start, true))
        } else if when(&start, true) == when(&end, true) {
            format!(
                "{} to {}",
                when(&start, false),
                when(&end, false).get(11..).unwrap_or("")
            )
        } else {
            format!("{} to {}", when(&start, false), when(&end, false))
        };
        let mut line = format!("- {span}: {}", text_of(e, "title"));
        let cal = text_of(e, "calendar");
        if !cal.is_empty() {
            line.push_str(&format!(" ({cal})"));
        }
        let at = text_of(e, "location");
        if !at.is_empty() {
            line.push_str(&format!(", at {at}"));
        }
        out.push_str(&line);
        out.push('\n');
        let notes = text_of(e, "notes");
        if !notes.trim().is_empty() {
            out.push_str(&format!("  Notes: {}\n", notes.replace('\n', " ").trim()));
        }
    }
    if events["truncated"].as_bool() == Some(true) {
        out.push_str("\n(Only the first 200 events are included.)\n");
    }
    if let Some(r) = reminders {
        out.push_str("\n# Open reminders\n\n");
        let list = r["reminders"].as_array().cloned().unwrap_or_default();
        if list.is_empty() {
            out.push_str("No open reminders.\n");
        }
        for x in &list {
            let mut line = format!("- {}", text_of(x, "title"));
            let mut about = Vec::new();
            let due = text_of(x, "due");
            if !due.is_empty() {
                about.push(format!("due {}", when(&due, false)));
            }
            let in_list = text_of(x, "list");
            if !in_list.is_empty() {
                about.push(format!("list {in_list}"));
            }
            if !about.is_empty() {
                line.push_str(&format!(" ({})", about.join(", ")));
            }
            out.push_str(&line);
            out.push('\n');
            let notes = text_of(x, "notes");
            if !notes.trim().is_empty() {
                out.push_str(&format!("  Notes: {}\n", notes.replace('\n', " ").trim()));
            }
        }
        if r["truncated"].as_bool() == Some(true) {
            out.push_str("\n(Only the first 200 reminders are included.)\n");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ca01_events_and_reminders_read_as_one_line_each_with_their_notes() {
        let events = json!({"events": [
            {"title": "Standup", "start": "2026-11-14T09:00:00+05:30", "end": "2026-11-14T09:15:00+05:30", "all_day": false,
             "location": "Room 4", "calendar": "Work", "notes": "bring the\nnumbers"},
            {"title": "Holiday", "start": "2026-11-15T00:00:00+05:30", "end": "2026-11-16T00:00:00+05:30", "all_day": true,
             "location": "", "calendar": "Home", "notes": ""},
            {"title": "Offsite\u{1b}[2J", "start": "2026-11-16T18:00:00+05:30", "end": "2026-11-17T10:00:00+05:30", "all_day": false,
             "location": "", "calendar": "", "notes": ""}
        ], "truncated": false});
        let reminders = json!({"reminders": [
            {"title": "Pay the invoice", "due": "2026-11-16T17:00:00+05:30", "list": "Personal", "notes": ""},
            {"title": "Call the bank", "due": "", "list": "", "notes": "ask about fees"}
        ], "truncated": false});
        let text = as_text(&events, Some(&reminders), 7);
        assert!(text.starts_with("# Calendar: the next 7 days, read on this computer"));
        assert!(text.contains("- 2026-11-14 09:00 to 09:15: Standup (Work), at Room 4\n  Notes: bring the numbers"), "{text}");
        assert!(
            text.contains("- 2026-11-15 all day: Holiday (Home)"),
            "{text}"
        );
        assert!(
            text.contains("- 2026-11-16 18:00 to 2026-11-17 10:00: Offsite[2J"),
            "a control character is dropped: {text}"
        );
        assert!(text.contains("# Open reminders\n\n- Pay the invoice (due 2026-11-16 17:00, list Personal)\n- Call the bank\n  Notes: ask about fees"), "{text}");
        assert!(as_text(&json!({"events": []}), None, 1).contains("the next 1 day,"));
        assert!(as_text(&json!({"events": []}), None, 1).contains("Nothing is on the calendar"));
    }

    #[test]
    fn ca02_only_known_permission_states_travel_on() {
        assert_eq!(
            states(&json!({"calendar": "allowed", "reminders": "not_asked", "extra": 1})),
            json!({"calendar": "allowed", "reminders": "not_asked"})
        );
        assert_eq!(
            states(&json!({"calendar": "<b>", "reminders": null})),
            json!({"calendar": "unknown", "reminders": "unknown"})
        );
    }

    #[cfg(unix)]
    #[test]
    fn ca03_a_stand_in_reader_is_run_with_limits_and_its_refusals_are_said_in_words() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("kk-calendar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("kaku-calendar");
        std::fs::write(
            &exe,
            "#!/bin/sh\ncase \"$1\" in\n status) echo '{\"calendar\":\"allowed\",\"reminders\":\"denied\"}';;\n events) echo '{\"events\":[{\"title\":\"Board call\",\"start\":\"2026-11-14T10:00:00Z\",\"end\":\"2026-11-14T11:00:00Z\",\"all_day\":false}]}';;\n reminders) echo '{\"error\":\"reminders_not_allowed\"}';;\n *) sleep 5;;\nesac\n",
        )
        .unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let h = Helper {
            exe: exe.clone(),
            confined: false,
        };
        assert_eq!(
            h.status().unwrap(),
            json!({"calendar": "allowed", "reminders": "denied"})
        );
        assert!(h
            .read(7, false)
            .unwrap()
            .contains("- 2026-11-14 10:00 to 11:00: Board call"));
        assert_eq!(
            h.read(7, true).unwrap_err(),
            "macOS does not allow this program to read reminders"
        );
        assert!(h.read(0, false).is_err() && h.read(32, false).is_err());
        let slow = h.run(&["other"], Duration::from_millis(300)).unwrap_err();
        assert!(slow.contains("took too long"), "{slow}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
