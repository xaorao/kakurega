use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kk-doctor-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn mode(p: &Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

/// Runs the real binary with a home folder of its own, so nothing of the person running the tests is read or changed.
fn doctor(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kaku"))
        .arg("doctor")
        .args(args)
        .env("HOME", home)
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(o)))
}

fn status<'a>(v: &'a Value, id: &str) -> &'a str {
    v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap_or_else(|| panic!("no {id}"))["status"]
        .as_str()
        .unwrap()
}

const QUICK: &str = "setup,web,disk";

#[test]
fn dd01_a_fresh_machine_is_a_warning_and_exit_0_and_fix_makes_the_folder_owner_only() {
    let home = temp("fresh");
    let data = home.join("data");
    let o = doctor(
        &home,
        &[
            "--data-dir",
            data.to_str().unwrap(),
            "--skip",
            QUICK,
            "--json",
        ],
    );
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    let v = json(&o);
    assert_eq!(status(&v, "data_dir"), "warn");
    assert_eq!(v["summary"]["fail"], 0);
    assert!(!data.exists(), "reading made nothing");
    let o = doctor(
        &home,
        &[
            "--data-dir",
            data.to_str().unwrap(),
            "--skip",
            QUICK,
            "--fix",
            "--json",
        ],
    );
    assert_eq!(o.status.code(), Some(0));
    assert!(data.is_dir() && mode(&data) == 0o700, "{:o}", mode(&data));
    assert_eq!(json(&o)["summary"]["fixed"], 1);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn dd02_a_stale_admin_file_and_loose_permissions_are_found_and_fix_changes_only_what_it_says() {
    let home = temp("stale");
    let data = home.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
    let key = data.join("vault.key");
    std::fs::write(&key, "k").unwrap();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
    // a port nothing listens on
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let admin = data.join("admin.json");
    std::fs::write(
        &admin,
        format!("{{\"port\": {port}, \"token\": \"SECRET-TOKEN-VALUE\"}}"),
    )
    .unwrap();
    std::fs::set_permissions(&admin, std::fs::Permissions::from_mode(0o600)).unwrap();
    let notes = data.join("notes.txt");
    std::fs::write(&notes, "my notes").unwrap();
    let before = std::fs::read(&notes).unwrap();
    let args = ["--data-dir", data.to_str().unwrap(), "--skip", QUICK];
    let o = doctor(&home, &[&args[..], &["--json"]].concat());
    let v = json(&o);
    assert_eq!(status(&v, "key_perms"), "fail");
    assert_eq!(status(&v, "data_perms"), "warn");
    assert_eq!(status(&v, "service"), "warn");
    assert_eq!(o.status.code(), Some(1), "a readable key is a failure");
    let all = text(&o);
    assert!(
        !all.contains("SECRET-TOKEN-VALUE"),
        "the token is never printed"
    );
    assert!(
        admin.exists() && mode(&key) == 0o644 && mode(&data) == 0o755,
        "a plain run changes nothing"
    );
    // fix
    let o = doctor(&home, &[&args[..], &["--fix", "--json"]].concat());
    let v = json(&o);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert_eq!((mode(&data), mode(&key)), (0o700, 0o600));
    assert!(!admin.exists(), "the stale file is gone");
    assert_eq!(
        std::fs::read(&notes).unwrap(),
        before,
        "nothing else was touched"
    );
    assert!(data.join("vault.key").exists());
    assert_eq!(v["summary"]["fixed"], 3, "{}", text(&o));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn dd03_fix_leaves_a_folder_that_is_not_ours_alone() {
    let home = temp("notours");
    let data = home.join("photos");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(data.join("holiday.jpg"), "x").unwrap();
    let o = doctor(
        &home,
        &[
            "--data-dir",
            data.to_str().unwrap(),
            "--skip",
            QUICK,
            "--fix",
            "--json",
        ],
    );
    assert_eq!(mode(&data), 0o755, "an unrelated folder is not changed");
    let v = json(&o);
    assert_eq!(status(&v, "data_perms"), "warn");
    assert_eq!(v["summary"]["fixed"], 0);
    assert!(text(&o).contains("none of Kakurega's own files"));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn dd04_a_folder_that_cannot_be_written_fails_with_exit_1() {
    let home = temp("readonly");
    let data = home.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("jobs.db"), "x").unwrap();
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o500)).unwrap();
    let o = doctor(
        &home,
        &[
            "--data-dir",
            data.to_str().unwrap(),
            "--skip",
            QUICK,
            "--json",
        ],
    );
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700)).unwrap();
    // a process allowed to write anyway cannot show this
    if status(&json(&o), "data_write") == "fail" {
        assert_eq!(o.status.code(), Some(1));
    }
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn dd05_an_unknown_skip_id_is_refused_with_exit_2_and_names_the_known_ones() {
    let home = temp("skip");
    let o = doctor(&home, &["--skip", "nonsense"]);
    assert_eq!(o.status.code(), Some(2));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("`nonsense`") && err.contains("data_perms") && err.contains("setup"),
        "{err}"
    );
    assert!(text(&o).is_empty(), "nothing is checked after a bad option");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn dd06_the_plain_report_reads_as_words_with_no_colour_and_the_json_is_the_same_rows() {
    let home = temp("plain");
    let data = home.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("jobs.db"), "x").unwrap();
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o755)).unwrap();
    let args = ["--data-dir", data.to_str().unwrap(), "--skip", QUICK];
    let o = doctor(&home, &args);
    let t = text(&o);
    assert!(t.starts_with("Kakurega doctor\n"), "{t}");
    assert!(
        t.contains("[warn]") && t.contains("[ok]") && t.contains("[skip]"),
        "{t}"
    );
    assert!(t.contains("Result: 0 failures, 1 warning,"), "{t}");
    assert!(!t.contains('\u{1b}'), "no escape sequences");
    let v = json(&doctor(&home, &[&args[..], &["--json"]].concat()));
    assert_eq!(v["doctor"], 1);
    for id in ["home", "data_dir", "data_perms", "data_write", "service"] {
        assert!(
            t.contains(
                v["checks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|c| c["id"] == id)
                    .unwrap()["label"]
                    .as_str()
                    .unwrap()
            ),
            "{id}"
        );
    }
    assert_eq!(v["fix_does_only"].as_array().unwrap().len(), 5);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn dd07_a_live_looking_but_foreign_server_never_receives_the_token() {
    use std::io::{Read, Write};
    let home = temp("foreign");
    let data = home.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700)).unwrap();
    // something else listens on the port named in admin.json and records everything it is sent
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let mut all = String::new();
        listener.set_nonblocking(false).unwrap();
        for _ in 0..3 {
            let Ok((mut s, _)) = listener.accept() else {
                break;
            };
            s.set_read_timeout(Some(std::time::Duration::from_millis(800)))
                .unwrap();
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            all.push_str(&String::from_utf8_lossy(&buf[..n]));
            let body = "{\"hello\":\"world\"}";
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            if all.contains("GET /api/health") {
                break;
            }
        }
        all
    });
    let admin = data.join("admin.json");
    std::fs::write(
        &admin,
        format!("{{\"port\": {port}, \"token\": \"FOREIGN-SERVER-MUST-NOT-SEE-THIS\"}}"),
    )
    .unwrap();
    std::fs::set_permissions(&admin, std::fs::Permissions::from_mode(0o600)).unwrap();
    let o = doctor(
        &home,
        &[
            "--data-dir",
            data.to_str().unwrap(),
            "--skip",
            QUICK,
            "--json",
        ],
    );
    let seen = server.join().unwrap();
    assert!(seen.starts_with("GET /api/health"), "{seen}");
    assert!(
        !seen.contains("FOREIGN-SERVER-MUST-NOT-SEE-THIS")
            && !seen.to_lowercase().contains("authorization"),
        "{seen}"
    );
    let v = json(&o);
    assert_eq!(status(&v, "service"), "warn");
    assert!(text(&o).contains("not Kakurega"), "{}", text(&o));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn dd08_the_real_serve_makes_a_new_data_folder_owner_only() {
    let home = temp("serve-new");
    let data = home.join("data");
    let script = home.join("script.json");
    std::fs::write(&script, "[]").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_kaku"))
        .args([
            "serve",
            "--data-dir",
            data.to_str().unwrap(),
            "--scripted",
            script.to_str().unwrap(),
            "--transport",
            "local",
        ])
        .env("HOME", &home)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let admin = data.join("admin.json");
    for _ in 0..80 {
        if admin.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let seen = (mode(&data), mode(&data.join("vault.key")), admin.exists());
    let _ = child.kill();
    let _ = child.wait();
    assert!(seen.2, "the service started");
    assert_eq!(
        seen.0, 0o700,
        "the folder serve made is owner only: {:o}",
        seen.0
    );
    assert_eq!(seen.1 & 0o077, 0);
    // and the doctor finds nothing to say about its permissions
    let o = doctor(
        &home,
        &[
            "--data-dir",
            data.to_str().unwrap(),
            "--skip",
            QUICK,
            "--json",
        ],
    );
    assert_eq!(status(&json(&o), "data_perms"), "ok");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn dd09_the_terminal_workspace_that_starts_the_service_itself_also_makes_the_folder_owner_only() {
    use std::io::Write;
    let home = temp("tui-new");
    let data = home.join("data");
    let mut child = Command::new(env!("CARGO_BIN_EXE_kaku"))
        .args(["tui", "--linear", "--data-dir", data.to_str().unwrap()])
        .env("HOME", &home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"/quit\n").unwrap();
    let started = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() {
        if started.elapsed() > std::time::Duration::from_secs(90) {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = child.wait();
    assert!(data.is_dir(), "the workspace made the folder");
    assert_eq!(mode(&data), 0o700, "owner only: {:o}", mode(&data));
    let _ = std::fs::remove_dir_all(&home);
}
