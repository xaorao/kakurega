use serde_json::Value;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kk-config-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn kaku(home: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_kaku"));
    c.args(args)
        .env("HOME", home)
        .env_remove("KAKU_NOTIFY")
        .env_remove("NO_COLOR")
        .env_remove("KAKU_LINEAR");
    for (k, v) in env {
        c.env(k, v);
    }
    c.output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn data(home: &Path) -> String {
    home.join("data").to_str().unwrap().to_string()
}

#[test]
fn ee01_show_lists_every_setting_with_its_value_and_where_the_value_came_from() {
    let home = temp("show");
    let d = data(&home);
    std::fs::create_dir_all(home.join("data")).unwrap();
    std::fs::write(
        home.join("data/config.toml"),
        "quiet = true\nnotify = \"bell\"\ncolor = \"never\"\n",
    )
    .unwrap();
    let o = kaku(
        &home,
        &["config", "show", "--data-dir", &d, "--json", "--linear"],
        &[("KAKU_NOTIFY", "off")],
    );
    assert_eq!(o.status.code(), Some(0));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["status"], "applied");
    let row = |name: &str| {
        v["settings"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(
        (
            row("notify")["value"].clone(),
            row("notify")["source"].clone()
        ),
        ("off".into(), "the environment (KAKU_NOTIFY)".into())
    );
    assert_eq!(
        (
            row("color")["value"].clone(),
            row("color")["source"].clone()
        ),
        ("never".into(), "config.toml".into())
    );
    assert_eq!(
        (
            row("quiet")["value"].clone(),
            row("quiet")["source"].clone()
        ),
        (true.into(), "config.toml".into())
    );
    assert_eq!(
        (
            row("linear")["value"].clone(),
            row("linear")["source"].clone()
        ),
        (true.into(), "the option --linear".into())
    );
    assert_eq!(v["settings"].as_array().unwrap().len(), 5);
    assert_eq!(
        (
            row("theme")["value"].clone(),
            row("theme")["source"].clone()
        ),
        ("auto".into(), "the default".into())
    );
    // the plain text says the same in words
    let t = out(&kaku(&home, &["config", "--data-dir", &d], &[]));
    assert!(
        t.contains("applied, 3 settings set")
            && t.contains("from config.toml")
            && t.contains("from the default"),
        "{t}"
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn ee02_check_is_ok_for_no_file_and_a_correct_file_and_lists_every_problem_with_exit_1_otherwise() {
    let home = temp("check");
    let d = data(&home);
    std::fs::create_dir_all(home.join("data")).unwrap();
    let o = kaku(&home, &["config", "check", "--data-dir", &d], &[]);
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).contains("defaults are in use"));
    std::fs::write(home.join("data/config.toml"), "quiet = true\n").unwrap();
    let o = kaku(&home, &["config", "check", "--data-dir", &d], &[]);
    assert_eq!(
        (o.status.code(), out(&o).contains("is correct. Set: quiet")),
        (Some(0), true),
        "{}",
        out(&o)
    );
    std::fs::write(
        home.join("data/config.toml"),
        "quite = true\nnotify = \"loud\"\nlinear = \"yes\"\n[ui]\nx = 1\n",
    )
    .unwrap();
    let o = kaku(&home, &["config", "check", "--data-dir", &d], &[]);
    assert_eq!(o.status.code(), Some(1));
    let t = out(&o);
    assert!(t.contains("4 problems, so none of it is applied"), "{t}");
    for part in [
        "quite: unknown setting (did you mean `quiet`?)",
        "notify: must be one of",
        "linear: must be true or false",
        "ui: unknown setting",
    ] {
        assert!(t.contains(part), "{part}: {t}");
    }
    let o = kaku(&home, &["config", "check", "--data-dir", &d, "--json"], &[]);
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        (
            v["status"].as_str(),
            v["problems"].as_array().unwrap().len(),
            o.status.code()
        ),
        (Some("invalid"), 4, Some(1))
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn ee03_init_writes_an_owner_only_template_and_refuses_to_overwrite_and_path_and_bad_commands_behave(
) {
    let home = temp("init");
    let d = data(&home);
    let o = kaku(&home, &["config", "init", "--data-dir", &d], &[]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    let file = home.join("data/config.toml");
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        kaku(&home, &["config", "check", "--data-dir", &d], &[])
            .status
            .code(),
        Some(0),
        "the template is a correct file"
    );
    std::fs::write(&file, "quiet = true\n").unwrap();
    let again = kaku(&home, &["config", "init", "--data-dir", &d], &[]);
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stderr).contains("already exists and was not changed"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "quiet = true\n");
    assert_eq!(
        out(&kaku(&home, &["config", "path", "--data-dir", &d], &[])).trim(),
        file.to_str().unwrap()
    );
    // a help flag after a command prints the usage and runs nothing: `serve --help` must not start a service
    for cmd in [
        &["serve", "--help"][..],
        &["verify", "-h"],
        &["backup", "--help"],
    ] {
        let h = kaku(&home, cmd, &[]);
        assert_eq!(h.status.code(), Some(0), "{cmd:?}");
        assert!(out(&h).starts_with("kaku: "), "{cmd:?}");
    }
    // a command that does not exist, and options before the command
    let bad = kaku(&home, &["config", "frobnicate"], &[]);
    assert_eq!(bad.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("unknown command `frobnicate`"));
    assert_eq!(
        kaku(&home, &["config", "--data-dir", &d, "--json", "check"], &[])
            .status
            .code(),
        Some(0)
    );
    let _ = std::fs::remove_dir_all(&home);
}

/// Runs the real terminal workspace with a line on its input and gives what it printed and how it ended.
fn workspace(home: &Path, args: &[&str], env: &[(&str, &str)]) -> (String, String, Option<i32>) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_kaku"));
    c.arg("tui")
        .args(args)
        .env("HOME", home)
        .env_remove("KAKU_LINEAR")
        .env_remove("KAKU_NOTIFY")
        .env_remove("NO_COLOR");
    for (k, v) in env {
        c.env(k, v);
    }
    let mut child = c
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
    let o = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
        o.status.code(),
    )
}

#[test]
fn ee04_the_real_workspace_starts_in_the_plain_line_mode_from_the_file_and_the_environment_overrides_it(
) {
    let home = temp("linear");
    let d = data(&home);
    std::fs::create_dir_all(home.join("data")).unwrap();
    std::fs::write(home.join("data/config.toml"), "linear = true\n").unwrap();
    // no --linear option: the file asks for it, and with piped input only the plain-line mode can run
    let (stdout, stderr, code) = workspace(&home, &["--data-dir", &d], &[]);
    assert_eq!(code, Some(0), "{stdout} | {stderr}");
    assert!(stdout.contains("plain-line mode"), "{stdout}");
    // the environment says no, and beats the file: the full-screen workspace needs a terminal, so it refuses
    let (stdout, stderr, code) = workspace(&home, &["--data-dir", &d], &[("KAKU_LINEAR", "0")]);
    assert_ne!(code, Some(0), "{stdout} | {stderr}");
    assert!(stderr.contains("needs a terminal"), "{stderr}");
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn ee05_a_file_with_mistakes_is_not_applied_and_the_workspace_says_so_at_the_top_and_still_starts()
{
    let home = temp("notice");
    let d = data(&home);
    std::fs::create_dir_all(home.join("data")).unwrap();
    // `linear = true` is correct but is not applied because the file has a mistake: all or nothing
    std::fs::write(
        home.join("data/config.toml"),
        "linear = true\nquite = true\n",
    )
    .unwrap();
    let (stdout, stderr, code) = workspace(&home, &["--data-dir", &d, "--linear"], &[]);
    assert_eq!(code, Some(0), "it still starts: {stdout} | {stderr}");
    assert!(stdout.contains("[Warning] config.toml was not applied, so the defaults are in use: quite: unknown setting (did you mean `quiet`?)"), "{stdout}");
    assert!(stdout.contains("kaku config check"), "{stdout}");
    // without the option, the correct-but-unapplied `linear = true` is not honoured: the full-screen workspace is chosen and refuses the pipe
    let (_, stderr, code) = workspace(&home, &["--data-dir", &d], &[]);
    assert_ne!(code, Some(0));
    assert!(stderr.contains("needs a terminal"), "{stderr}");
    let _ = std::fs::remove_dir_all(&home);
}
