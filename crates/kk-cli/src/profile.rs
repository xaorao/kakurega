//! Profiles: separate, complete data folders on one computer, each with its own keys, vault, memory, skills, connectors
//! and settings. Nothing is shared between two profiles, because every one of those lives inside the data folder.
//!
//! The first profile is the folder itself (`~/.kakurega`, called `default`). A named profile is
//! `~/.kakurega/profiles/NAME`. Which one a command uses, first match wins: `--data-dir DIR`, `--profile NAME`,
//! the `KAKU_PROFILE` variable, the profile chosen with `kaku profile use`, then `default`.

use std::io::Write;
use std::path::{Path, PathBuf};

pub const DEFAULT: &str = "default";
const MARKER: &str = "profile.json";
const CURRENT: &str = "current-profile";
const MAX_NAME: usize = 32;
/// Names that would be mistaken for something else.
const RESERVED: &[&str] = &[
    "default", "profiles", "kakurega", "kaku", "root", "tmp", "test",
];

/// The folder every profile lives under.
pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into())).join(".kakurega")
}

/// Lowercase letters, digits and dashes, starting with a letter or digit, at most 32. The name becomes a folder name,
/// so anything else is refused.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

pub fn dir_of(home: &Path, name: &str) -> Option<PathBuf> {
    if name == DEFAULT {
        return Some(home.to_path_buf());
    }
    valid_name(name).then(|| home.join("profiles").join(name))
}

/// A named profile exists when its folder holds the marker written at creation. A folder without it (left by a
/// program that only made the directory) is not a profile and is never listed or used.
pub fn exists(home: &Path, name: &str) -> bool {
    name == DEFAULT
        || dir_of(home, name).is_some_and(|d| {
            std::fs::symlink_metadata(&d).is_ok_and(|m| m.is_dir()) && d.join(MARKER).is_file()
        })
}

/// The named profiles, sorted. `default` is not in the list.
pub fn names(home: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(home.join("profiles"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| valid_name(n) && exists(home, n))
        .collect();
    out.sort();
    out
}

/// The profile chosen with `use`. A choice that no longer exists counts as `default`.
pub fn current(home: &Path) -> String {
    std::fs::read_to_string(home.join(CURRENT))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|n| n != DEFAULT && exists(home, n))
        .unwrap_or_else(|| DEFAULT.to_string())
}

pub fn set_current(home: &Path, name: &str) -> Result<(), String> {
    if !exists(home, name) {
        return Err(format!(
            "there is no profile named `{name}`; `kaku profile list` shows the ones there are"
        ));
    }
    let file = home.join(CURRENT);
    if name == DEFAULT {
        return match std::fs::remove_file(&file) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("the choice could not be saved: {e}")),
        };
    }
    std::fs::create_dir_all(home).map_err(|e| format!("the choice could not be saved: {e}"))?;
    // written whole or not at all
    let tmp = home.join(format!("{CURRENT}.tmp"));
    std::fs::write(&tmp, format!("{name}\n"))
        .and_then(|()| std::fs::rename(&tmp, &file))
        .map_err(|e| format!("the choice could not be saved: {e}"))
}

pub fn create(home: &Path, name: &str) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err(format!(
            "`{name}` cannot be a profile's name: use lowercase letters, digits and dashes, at most {MAX_NAME}, starting and ending with a letter or digit (for example `work`)"
        ));
    }
    if RESERVED.contains(&name) {
        return Err(format!(
            "`{name}` is kept for the program itself; choose another name"
        ));
    }
    let dir = dir_of(home, name).expect("a valid name");
    if std::fs::symlink_metadata(&dir).is_ok() {
        return Err(if exists(home, name) {
            format!("there is already a profile named `{name}`")
        } else {
            format!(
                "{} is in the way and is not a profile; move it away first",
                dir.display()
            )
        });
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("the profile's folder could not be made: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        let _ = std::fs::set_permissions(
            home.join("profiles"),
            std::fs::Permissions::from_mode(0o700),
        );
    }
    std::fs::write(
        dir.join(MARKER),
        format!("{}\n", serde_json::json!({"name": name})),
    )
    .map_err(|e| format!("the profile's folder could not be written: {e}"))?;
    Ok(dir)
}

/// Whether a service is answering for this data folder right now.
pub fn service_running(dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join("admin.json")) else {
        return false;
    };
    let Some(port) = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v["port"].as_u64())
        .and_then(|p| u16::try_from(p).ok())
    else {
        return false;
    };
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(400),
    )
    .is_ok()
}

/// Removes a named profile and everything in it. Its keys go with it, so what it held cannot be read again.
pub fn remove(home: &Path, name: &str) -> Result<(), String> {
    if name == DEFAULT {
        return Err(
            "the default profile is the program's own folder and is not removed this way".into(),
        );
    }
    if !exists(home, name) {
        return Err(format!("there is no profile named `{name}`"));
    }
    let dir = dir_of(home, name).expect("it exists");
    if service_running(&dir) {
        return Err(format!(
            "a service is running for `{name}`; stop it first, then remove the profile"
        ));
    }
    // the marker goes first: from here on the folder is no profile, even if removing the rest is cut short
    std::fs::remove_file(dir.join(MARKER))
        .map_err(|e| format!("the profile could not be removed: {e}"))?;
    if current_raw(home).as_deref() == Some(name) {
        let _ = std::fs::remove_file(home.join(CURRENT));
    }
    std::fs::remove_dir_all(&dir).map_err(|e| {
        format!(
            "the profile is no longer used, but its folder {} could not be removed whole: {e}",
            dir.display()
        )
    })
}

fn current_raw(home: &Path) -> Option<String> {
    std::fs::read_to_string(home.join(CURRENT))
        .ok()
        .map(|s| s.trim().to_string())
}

/// The data folder a command uses, and the profile's name when it is one. `Err` is a sentence for the person.
pub fn resolve(
    home: &Path,
    data_dir: Option<String>,
    flag: Option<String>,
    env: Option<String>,
) -> Result<(PathBuf, Option<String>), String> {
    if let Some(d) = data_dir {
        return Ok((PathBuf::from(d), None));
    }
    let asked = flag.or(env.filter(|e| !e.trim().is_empty()));
    let name = match asked {
        Some(n) => {
            let n = n.trim().to_string();
            if !exists(home, &n) {
                return Err(format!(
                    "there is no profile named `{n}`; make it with `kaku profile new {n}`"
                ));
            }
            n
        }
        None => current(home),
    };
    Ok((dir_of(home, &name).expect("it exists"), Some(name)))
}

fn size_of(dir: &Path, skip: Option<&Path>) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if skip == Some(p.as_path()) {
                continue;
            }
            match e.metadata() {
                Ok(m) if m.is_dir() => stack.push(p),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

pub fn run(args: &[String]) -> i32 {
    run_in(
        &home(),
        args,
        std::io::IsTerminal::is_terminal(&std::io::stdin()),
    )
}

pub fn run_in(home: &Path, args: &[String], interactive: bool) -> i32 {
    let json = args.iter().any(|a| a == "--json");
    let confirm = crate::flag(args, "--confirm");
    // the name given to --confirm is that flag's value, not a word of the command
    let mut words: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--confirm" {
            it.next();
        } else if !a.starts_with("--") {
            words.push(a);
        }
    }
    let fail = |why: String| {
        eprintln!("{why}");
        2
    };
    match words.as_slice() {
        [] | ["list"] => {
            // the star is on the profile commands use right now: the variable (or --profile) when set, else the saved choice
            let cur = std::env::var("KAKU_PROFILE")
                .ok()
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty() && exists(home, n))
                .unwrap_or_else(|| current(home));
            let mut rows = vec![DEFAULT.to_string()];
            rows.extend(names(home));
            let list: Vec<serde_json::Value> = rows
                .iter()
                .map(|n| {
                    let dir = dir_of(home, n).expect("listed");
                    let profiles = home.join("profiles");
                    serde_json::json!({
                        "name": n,
                        "current": *n == cur,
                        "folder": dir.to_string_lossy(),
                        "bytes": size_of(&dir, (n == DEFAULT).then_some(profiles.as_path())),
                        "running": service_running(&dir),
                    })
                })
                .collect();
            if json {
                println!("{}", serde_json::json!({"profiles": list}));
            } else {
                for p in &list {
                    println!(
                        "{} {:<20} {:>10} KB  {}{}",
                        if p["current"] == true { "*" } else { " " },
                        p["name"].as_str().unwrap_or(""),
                        p["bytes"].as_u64().unwrap_or(0) / 1024,
                        p["folder"].as_str().unwrap_or(""),
                        if p["running"] == true { "  (service running)" } else { "" },
                    );
                }
                println!("\n* is the profile commands use. Each profile has its own keys, memory, skills, connectors and settings; nothing is shared.");
            }
            0
        }
        ["new", name] => match create(home, name) {
            Ok(dir) => {
                println!("Made the profile `{name}` in {}.\nUse it once with `--profile {name}`, or always with `kaku profile use {name}`.", dir.display());
                0
            }
            Err(e) => fail(e),
        },
        ["use", name] => match set_current(home, name) {
            Ok(()) => {
                println!("Commands now use the profile `{name}`. A service that is already running keeps the profile it was started with.");
                0
            }
            Err(e) => fail(e),
        },
        ["current"] => {
            println!("{}", current(home));
            0
        }
        ["remove", name] => {
            let name = name.to_string();
            let confirmed = match confirm.clone() {
                Some(c) => c == name,
                None if interactive => {
                    print!("This removes the profile `{name}` with its keys, memory and everything it holds. It cannot be undone.\nType the profile's name to remove it: ");
                    let _ = std::io::stdout().flush();
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line).is_ok() && line.trim() == name
                }
                None => false,
            };
            if !confirmed {
                return fail(format!("nothing was removed: confirm with `--confirm {name}`"));
            }
            match remove(home, &name) {
                Ok(()) => {
                    println!("Removed the profile `{name}`.");
                    0
                }
                Err(e) => fail(e),
            }
        }
        _ => fail("usage: kaku profile [list] | new NAME | use NAME | current | remove NAME [--confirm NAME]   (add --json to list)".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kk-profile-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn pf01_names_that_could_leave_the_folder_or_be_mistaken_are_refused() {
        let h = tmp("names");
        for bad in [
            "",
            "../x",
            "a/b",
            "Work",
            "a b",
            "-a",
            "a-",
            ".",
            "..",
            "a.b",
            &"a".repeat(33),
            "ａ",
        ] {
            assert!(!valid_name(bad), "{bad}");
            assert!(create(&h, bad).is_err(), "{bad}");
            assert!(dir_of(&h, bad).is_none(), "{bad}");
        }
        for reserved in RESERVED {
            assert!(create(&h, reserved).is_err(), "{reserved}");
        }
        assert!(valid_name("work") && valid_name("client-7") && valid_name("9"));
        assert!(
            std::fs::read_dir(&h).unwrap().next().is_none(),
            "nothing was made"
        );
    }

    #[test]
    fn pf02_a_profile_is_its_own_folder_and_a_bare_folder_is_not_a_profile() {
        let h = tmp("make");
        let d = create(&h, "work").unwrap();
        assert_eq!(d, h.join("profiles/work"));
        assert!(exists(&h, "work") && exists(&h, DEFAULT));
        assert!(create(&h, "work").unwrap_err().contains("already"));
        std::fs::create_dir_all(h.join("profiles/ghost")).unwrap();
        assert!(!exists(&h, "ghost"));
        assert_eq!(names(&h), ["work"]);
        assert!(create(&h, "ghost").unwrap_err().contains("in the way"));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&d, h.join("profiles/linked")).unwrap();
            assert!(
                !exists(&h, "linked"),
                "a link to a profile is not a profile"
            );
        }
    }

    #[test]
    fn pf03_which_profile_a_command_uses_follows_the_stated_order() {
        let h = tmp("order");
        create(&h, "work").unwrap();
        create(&h, "home-lab").unwrap();
        let r = |d: Option<&str>, f: Option<&str>, e: Option<&str>| {
            resolve(
                &h,
                d.map(String::from),
                f.map(String::from),
                e.map(String::from),
            )
        };
        assert_eq!(
            r(None, None, None).unwrap(),
            (h.clone(), Some("default".into()))
        );
        set_current(&h, "work").unwrap();
        assert_eq!(r(None, None, None).unwrap().0, h.join("profiles/work"));
        assert_eq!(
            r(None, None, Some("home-lab")).unwrap().0,
            h.join("profiles/home-lab")
        );
        assert_eq!(r(None, Some("default"), Some("home-lab")).unwrap().0, h);
        assert_eq!(
            r(Some("/x/y"), Some("work"), Some("home-lab")).unwrap(),
            (PathBuf::from("/x/y"), None)
        );
        assert_eq!(
            r(None, None, Some("  ")).unwrap().0,
            h.join("profiles/work"),
            "an empty variable is no choice"
        );
        // a name that does not exist is an error when asked for, never a silent fall back to another profile's data
        assert!(r(None, Some("nope"), None)
            .unwrap_err()
            .contains("no profile named `nope`"));
        assert!(r(None, None, Some("../../etc")).is_err());
        assert!(set_current(&h, "nope").is_err());
        // a saved choice whose profile is gone counts as default
        std::fs::write(h.join(CURRENT), "gone\n").unwrap();
        assert_eq!(current(&h), "default");
        std::fs::write(h.join(CURRENT), "../../etc\n").unwrap();
        assert_eq!(current(&h), "default");
        set_current(&h, "work").unwrap();
        set_current(&h, "default").unwrap();
        assert!(!h.join(CURRENT).exists());
    }

    #[test]
    fn pf04_removing_needs_the_name_refuses_default_and_a_running_service_and_forgets_the_choice() {
        let h = tmp("remove");
        let d = create(&h, "work").unwrap();
        std::fs::write(d.join("vault.db"), b"x").unwrap();
        set_current(&h, "work").unwrap();
        assert_eq!(
            run_in(&h, &s(&["remove", "work"]), false),
            2,
            "no confirmation, nothing removed"
        );
        assert_eq!(
            run_in(&h, &s(&["remove", "work", "--confirm", "wrk"]), false),
            2
        );
        assert!(exists(&h, "work"));
        assert_eq!(
            run_in(
                &h,
                &s(&["remove", "default", "--confirm", "default"]),
                false
            ),
            2
        );
        assert!(h.exists());
        // a service answering for the profile: refused
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std::fs::write(
            d.join("admin.json"),
            format!(
                "{{\"port\": {}, \"token\": \"t\"}}",
                listener.local_addr().unwrap().port()
            ),
        )
        .unwrap();
        assert!(remove(&h, "work").unwrap_err().contains("running"));
        drop(listener);
        assert_eq!(
            run_in(&h, &s(&["remove", "work", "--confirm", "work"]), false),
            0
        );
        assert!(!d.exists() && !exists(&h, "work"));
        assert_eq!(current(&h), "default");
        assert!(!h.join(CURRENT).exists());
        assert_eq!(
            run_in(&h, &s(&["remove", "work", "--confirm", "work"]), false),
            2,
            "already gone"
        );
    }

    #[test]
    fn pf05_the_commands_say_what_happened_and_refuse_what_they_do_not_know() {
        let h = tmp("cmd");
        assert_eq!(run_in(&h, &s(&["new", "work"]), false), 0);
        assert_eq!(run_in(&h, &s(&["new", "Work"]), false), 2);
        assert_eq!(run_in(&h, &s(&["use", "work"]), false), 0);
        assert_eq!(run_in(&h, &s(&["use", "nope"]), false), 2);
        assert_eq!(run_in(&h, &s(&["list", "--json"]), false), 0);
        assert_eq!(run_in(&h, &s(&["frobnicate"]), false), 2);
        assert_eq!(current(&h), "work");
    }
}
