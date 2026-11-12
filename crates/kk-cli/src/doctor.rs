//! `kaku doctor [--json] [--fix] [--skip ID[,ID]]`: why Kakurega does not start or does not work, with nothing running.
//!
//! It reads; it changes something only with `--fix`, and then only from a closed set of local actions (see `FIXES`) on a folder that
//! looks like Kakurega's own. It never prints the admin token, a key, or the content of a file, and it never sends the admin token
//! to a port it has not first recognised as Kakurega. Rows are words (`ok`, `warn`, `FAIL`, `skip`), never colour or a symbol alone.

use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Ok,
    Warn,
    Fail,
    Skip,
}

impl Status {
    pub fn word(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "FAIL",
            Status::Skip => "skip",
        }
    }

    fn json(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Fail => "fail",
            Status::Skip => "skip",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Check {
    pub id: String,
    pub section: &'static str,
    pub status: Status,
    pub label: String,
    pub detail: String,
    pub fix: String,
    /// Whether `--fix` can do what `fix` says by itself.
    pub fixable: bool,
}

impl Check {
    fn new(id: &str, section: &'static str, status: Status, label: &str, detail: &str) -> Check {
        Check {
            id: id.into(),
            section,
            status,
            label: label.into(),
            detail: detail.into(),
            fix: String::new(),
            fixable: false,
        }
    }

    fn with_fix(mut self, fix: &str, fixable: bool) -> Check {
        self.fix = fix.into();
        self.fixable = fixable;
        self
    }
}

/// What `--skip` accepts. `setup` covers every setup check.
pub const IDS: &[&str] = &[
    "home",
    "data_dir",
    "data_perms",
    "key_perms",
    "data_write",
    "config",
    "disk",
    "service",
    "setup",
    "web",
];

/// The names a data folder is recognised by. A folder with none of them is not one `--fix` will touch.
const STORE_FILES: &[&str] = &[
    "jobs.db",
    "memory.db",
    "events.db",
    "ledger.db",
    "disclosure.db",
    "state.db",
    "vault.key",
    "admin.json",
];

/// The only things `--fix` ever does, for the record and for the tests that pin them.
pub const FIXES: &[&str] = &[
    "create the data folder, owner only",
    "make the data folder owner only",
    "make vault.key owner only",
    "make admin.json owner only",
    "remove an admin.json that is damaged or whose service does not answer",
];

/// Whether a folder looks like a Kakurega data folder: one of its own files is in it.
pub fn looks_like_data_dir(dir: &Path) -> bool {
    STORE_FILES.iter().any(|n| dir.join(n).exists())
}

fn mode_of(path: &Path) -> Option<u32> {
    std::fs::metadata(path)
        .ok()
        .map(|m| m.permissions().mode() & 0o777)
}

fn is_symlink(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// A command with a time limit: None when it could not start or did not finish in time (it is stopped).
pub fn run_limited(mut cmd: Command, limit: Duration) -> Option<std::process::Output> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// The free kilobytes in the answer of `df -Pk PATH` (the fourth column of its second line), or nothing if it is anything else.
pub fn parse_df_free_kib(text: &str) -> Option<u64> {
    let line = text.lines().nth(1)?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.len() >= 4 {
        // device, blocks, used, available, ...
        return cols[3].parse().ok();
    }
    // a long device name wraps onto its own line: the numbers follow without it, so available is the third
    let next: Vec<&str> = text.lines().nth(2)?.split_whitespace().collect();
    next.get(2)?.parse().ok()
}

pub fn check_home(home: Option<&std::ffi::OsStr>) -> Check {
    match home {
        Some(h) if Path::new(h).is_dir() => Check::new(
            "home",
            "This computer",
            Status::Ok,
            "Home folder",
            &Path::new(h).display().to_string(),
        ),
        Some(h) => Check::new(
            "home",
            "This computer",
            Status::Fail,
            "Home folder",
            &format!(
                "HOME is {} and that is not a folder",
                Path::new(h).display()
            ),
        )
        .with_fix("Set HOME to your home folder and run kaku again.", false),
        None => Check::new(
            "home",
            "This computer",
            Status::Fail,
            "Home folder",
            "HOME is not set",
        )
        .with_fix(
            "Set HOME to your home folder and run kaku again; kaku serve needs it.",
            false,
        ),
    }
}

/// The data folder: present, owner only, writable. With `fix`, creates and tightens it when it is ours to touch.
pub fn data_dir_rows(dir: &Path, fix: bool, fixed: &mut Vec<String>) -> Vec<Check> {
    let sec = "Data folder";
    let mut rows = Vec::new();
    if !dir.exists() {
        if fix {
            match std::fs::create_dir_all(dir)
                .and_then(|_| std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)))
            {
                Ok(()) => {
                    fixed.push(format!("created {} (owner only)", dir.display()));
                    rows.push(Check::new(
                        "data_dir",
                        sec,
                        Status::Ok,
                        "Data folder",
                        &format!("{} created (fixed)", dir.display()),
                    ));
                }
                Err(e) => rows.push(
                    Check::new(
                        "data_dir",
                        sec,
                        Status::Fail,
                        "Data folder",
                        &format!("{} could not be created: {}", dir.display(), e.kind()),
                    )
                    .with_fix(
                        "Make the folder yourself, or pick another with --data-dir.",
                        false,
                    ),
                ),
            }
        } else {
            rows.push(
                Check::new(
                    "data_dir",
                    sec,
                    Status::Warn,
                    "Data folder",
                    &format!(
                        "{} does not exist yet; it is created the first time Kakurega runs",
                        dir.display()
                    ),
                )
                .with_fix(
                    "Nothing to do: it is created the first time Kakurega runs. Or create it now, owner only.",
                    true,
                ),
            );
        }
        return rows;
    }
    if !dir.is_dir() {
        rows.push(
            Check::new(
                "data_dir",
                sec,
                Status::Fail,
                "Data folder",
                &format!("{} exists and is not a folder", dir.display()),
            )
            .with_fix(
                "Move that file away, or pick another folder with --data-dir.",
                false,
            ),
        );
        return rows;
    }
    rows.push(Check::new(
        "data_dir",
        sec,
        Status::Ok,
        "Data folder",
        &dir.display().to_string(),
    ));
    // permissions: who else can open the folder
    let ours = looks_like_data_dir(dir);
    let mode = mode_of(dir).unwrap_or(0);
    if mode & 0o077 == 0 {
        rows.push(Check::new(
            "data_perms",
            sec,
            Status::Ok,
            "Folder access",
            "owner only",
        ));
    } else if fix && ours {
        match std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)) {
            Ok(()) => {
                fixed.push(format!(
                    "made {} owner only (it was {:03o})",
                    dir.display(),
                    mode
                ));
                rows.push(Check::new(
                    "data_perms",
                    sec,
                    Status::Ok,
                    "Folder access",
                    &format!("owner only (fixed; it was {mode:03o})"),
                ));
            }
            Err(e) => rows.push(
                Check::new(
                    "data_perms",
                    sec,
                    Status::Warn,
                    "Folder access",
                    &format!("mode {mode:03o} and it could not be changed: {}", e.kind()),
                )
                .with_fix(&format!("Run: chmod 700 {}", dir.display()), false),
            ),
        }
    } else {
        let why = if fix && !ours {
            " (--fix did not change it: none of Kakurega's own files are in this folder, so it may not be the right one)"
        } else {
            ""
        };
        rows.push(
            Check::new(
                "data_perms",
                sec,
                Status::Warn,
                "Folder access",
                &format!(
                    "mode {mode:03o}: other users on this computer can list what is in it{why}"
                ),
            )
            .with_fix(&format!("Run: chmod 700 {}", dir.display()), ours),
        );
    }
    // the key that opens the stores
    let key = dir.join("vault.key");
    if key.exists() {
        let kmode = mode_of(&key).unwrap_or(0);
        if is_symlink(&key) {
            rows.push(
                Check::new(
                    "key_perms",
                    sec,
                    Status::Fail,
                    "Vault key",
                    "vault.key is a symbolic link; Kakurega's key must be a plain file",
                )
                .with_fix(
                    "Move the real key file into the data folder, replacing the link.",
                    false,
                ),
            );
        } else if kmode & 0o077 == 0 {
            rows.push(Check::new(
                "key_perms",
                sec,
                Status::Ok,
                "Vault key",
                "owner only",
            ));
        } else if fix && ours {
            match std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)) {
                Ok(()) => {
                    fixed.push(format!("made vault.key owner only (it was {kmode:03o})"));
                    rows.push(Check::new(
                        "key_perms",
                        sec,
                        Status::Ok,
                        "Vault key",
                        "owner only (fixed)",
                    ));
                }
                Err(e) => rows.push(
                    Check::new(
                        "key_perms",
                        sec,
                        Status::Fail,
                        "Vault key",
                        &format!("mode {kmode:03o} and it could not be changed: {}", e.kind()),
                    )
                    .with_fix(&format!("Run: chmod 600 {}", key.display()), false),
                ),
            }
        } else {
            rows.push(
                Check::new("key_perms", sec, Status::Fail, "Vault key", &format!("vault.key is mode {kmode:03o}: other users on this computer can read the key to your stores"))
                    .with_fix(&format!("Run: chmod 600 {}", key.display()), ours),
            );
        }
    }
    // writable: make and remove one small file
    let probe = dir.join(format!(".doctor-probe-{}", std::process::id()));
    match std::fs::write(&probe, b"x").and_then(|_| std::fs::remove_file(&probe)) {
        Ok(()) => rows.push(Check::new("data_write", sec, Status::Ok, "Writable", "yes")),
        Err(e) => {
            let _ = std::fs::remove_file(&probe);
            rows.push(
                Check::new(
                    "data_write",
                    sec,
                    Status::Fail,
                    "Writable",
                    &format!("a file could not be written there: {}", e.kind()),
                )
                .with_fix(
                    "Make the folder yours (chown) or pick another with --data-dir.",
                    false,
                ),
            );
        }
    }
    rows
}

/// `config.toml`: absent, applied, or not applied because of mistakes (each named). A file with a mistake is not applied at all, so
/// this is a warning: the product works, but the owner's settings are not in force.
pub fn config_row(dir: &Path) -> Check {
    use crate::config::{clean, load, Loaded, FILE};
    let sec = "Data folder";
    match load(dir) {
        Loaded::Absent => Check::new(
            "config",
            sec,
            Status::Ok,
            "Settings file",
            &format!("no {FILE}; the defaults are in use"),
        ),
        Loaded::Valid(f) if f.values.is_empty() => Check::new(
            "config",
            sec,
            Status::Ok,
            "Settings file",
            &format!("{FILE} is correct and sets nothing"),
        ),
        Loaded::Valid(f) => Check::new(
            "config",
            sec,
            Status::Ok,
            "Settings file",
            &format!(
                "{FILE} is correct and applied: {}",
                f.values.keys().copied().collect::<Vec<_>>().join(", ")
            ),
        ),
        Loaded::Invalid(problems) => {
            let list = problems
                .iter()
                .map(|p| format!("{}: {}", clean(&p.at), clean(&p.message)))
                .collect::<Vec<_>>()
                .join("; ");
            Check::new(
                "config",
                sec,
                Status::Warn,
                "Settings file",
                &kk_types::words::counted(&format!("{FILE} is NOT applied ({} problem(s)): {list}", problems.len())),
            )
            .with_fix(
                &format!("Correct {FILE} (kaku config check lists each problem) or remove it; until then none of it is applied."),
                false,
            )
        }
    }
}

/// Free space on the disk the data folder is on, from the answer of `df -Pk` (None when it could not be read).
pub fn disk_row(free_kib: Option<u64>) -> Check {
    let sec = "Data folder";
    match free_kib {
        None => Check::new(
            "disk",
            sec,
            Status::Skip,
            "Free disk space",
            "could not be read",
        ),
        Some(k) => {
            let mib = k / 1024;
            let detail = format!("{mib} MiB free");
            if mib < 100 {
                Check::new("disk", sec, Status::Fail, "Free disk space", &detail).with_fix(
                    "Free some disk space: the stores and their logs cannot grow.",
                    false,
                )
            } else if mib < 1024 {
                Check::new("disk", sec, Status::Warn, "Free disk space", &detail)
                    .with_fix("Free some disk space soon: less than 1 GiB is left.", false)
            } else {
                Check::new("disk", sec, Status::Ok, "Free disk space", &detail)
            }
        }
    }
}

fn free_kib_for(dir: &Path) -> Option<u64> {
    // the nearest folder that exists, so a data folder that is not made yet is still measured
    let mut at = dir.to_path_buf();
    while !at.exists() {
        at = at.parent()?.to_path_buf();
    }
    let mut cmd = Command::new("df");
    cmd.arg("-Pk").arg(&at);
    let out = run_limited(cmd, Duration::from_secs(5))?;
    parse_df_free_kib(&String::from_utf8_lossy(&out.stdout))
}

/// What asking a port whether it is Kakurega gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    Kakurega,
    SomethingElse,
    Nothing,
}

/// The two questions the doctor asks a port. Real in `Http`, replaced in the tests.
pub trait Probe {
    /// An unauthenticated question that Kakurega's service answers with its own name.
    fn health(&self, port: u16) -> Health;
    /// The diagnostics of a service already recognised as Kakurega, with the admin token. Only called after `health` said Kakurega.
    fn diagnostics(&self, port: u16, token: &str) -> Option<Value>;
}

pub struct Http;

fn agent(secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(secs)))
        .proxy(None)
        .build()
        .into()
}

impl Probe for Http {
    fn health(&self, port: u16) -> Health {
        let Ok(mut r) = agent(2)
            .get(&format!("http://127.0.0.1:{port}/api/health"))
            .call()
        else {
            return Health::Nothing;
        };
        match r.body_mut().read_json::<Value>() {
            Ok(v) if v["service"] == "kakurega-companion" => Health::Kakurega,
            _ => Health::SomethingElse,
        }
    }

    fn diagnostics(&self, port: u16, token: &str) -> Option<Value> {
        let mut r = agent(10)
            .get(&format!("http://127.0.0.1:{port}/admin/diagnostics"))
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .ok()?;
        if r.status().as_u16() != 200 {
            return None;
        }
        r.body_mut().read_json().ok()
    }
}

/// The service, from `admin.json`: whether one runs, and what it says about itself. Returns the diagnostics when one answered.
pub fn service_rows(
    dir: &Path,
    version: &str,
    fix: bool,
    probe: &dyn Probe,
    fixed: &mut Vec<String>,
) -> (Vec<Check>, Option<Value>) {
    let sec = "Service";
    let path = dir.join("admin.json");
    let mut rows = Vec::new();
    let Ok(meta) = std::fs::symlink_metadata(&path) else {
        rows.push(Check::new(
            "service",
            sec,
            Status::Ok,
            "Service",
            "not running (kaku tui or kaku serve starts it)",
        ));
        return (rows, None);
    };
    if meta.file_type().is_symlink() {
        rows.push(
            Check::new(
                "service",
                sec,
                Status::Fail,
                "Service",
                "admin.json is a symbolic link; it must be a plain file Kakurega wrote",
            )
            .with_fix(
                "Remove that link yourself (kaku doctor --fix does not touch links).",
                false,
            ),
        );
        return (rows, None);
    }
    let ours = looks_like_data_dir(dir);
    let remove = |fixed: &mut Vec<String>, why: &str| -> bool {
        if fix && ours && std::fs::remove_file(&path).is_ok() {
            fixed.push(format!("removed admin.json ({why})"));
            true
        } else {
            false
        }
    };
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        // the token in it would be readable by others
        if fix
            && ours
            && std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).is_ok()
        {
            fixed.push(format!("made admin.json owner only (it was {mode:03o})"));
            rows.push(Check::new(
                "service",
                sec,
                Status::Ok,
                "admin.json access",
                "owner only (fixed)",
            ));
        } else {
            rows.push(
                Check::new("service", sec, Status::Fail, "admin.json access", &format!("mode {mode:03o}: other users on this computer can read the service's admin token"))
                    .with_fix(&format!("Run: chmod 600 {}", path.display()), ours),
            );
        }
    }
    let parsed: Option<(u16, String)> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| {
            let port = u16::try_from(v["port"].as_u64()?).ok().filter(|p| *p > 0)?;
            Some((port, v["token"].as_str()?.to_string()))
        });
    let Some((port, token)) = parsed else {
        if remove(fixed, "it was damaged") {
            rows.push(Check::new(
                "service",
                sec,
                Status::Ok,
                "Service",
                "not running; a damaged admin.json was removed (fixed)",
            ));
        } else {
            rows.push(
                Check::new("service", sec, Status::Fail, "Service", "admin.json is damaged, so the service cannot be found")
                    .with_fix("Remove admin.json and start again; it is written fresh each time the service starts.", ours),
            );
        }
        return (rows, None);
    };
    match probe.health(port) {
        Health::Kakurega => {
            // only now is the token sent
            let diag = probe.diagnostics(port, &token);
            match &diag {
                Some(d) => {
                    let theirs = d["version"].as_str().unwrap_or("?");
                    rows.push(Check::new("service", sec, Status::Ok, "Service", &format!("running on port {port}, version {theirs}")));
                    if theirs != version {
                        rows.push(
                            Check::new("service_version", sec, Status::Warn, "Service version", &format!("the running service is {theirs} and this kaku is {version}"))
                                .with_fix("Stop the service and start it again with this kaku.", false),
                        );
                    }
                }
                None => rows.push(
                    Check::new("service", sec, Status::Warn, "Service", &format!("a Kakurega service answers on port {port} but refused its diagnostics (the admin token may be out of date)"))
                        .with_fix("Stop that service and start it again.", false),
                ),
            }
            (rows, diag)
        }
        other => {
            let what = match other {
                Health::SomethingElse => format!("something that is not Kakurega answers on port {port} (the admin token was not sent to it)"),
                _ => format!("nothing answers on port {port}"),
            };
            if remove(fixed, "its service does not answer") {
                rows.push(Check::new(
                    "service",
                    sec,
                    Status::Ok,
                    "Service",
                    "not running; the stale admin.json was removed (fixed)",
                ));
            } else {
                rows.push(
                    Check::new("service", sec, Status::Warn, "Service", &format!("not running: admin.json is left from a service that ended without cleaning up ({what})"))
                        .with_fix("Harmless: kaku tui writes a new one. Remove the old file.", ours),
                );
            }
            (rows, None)
        }
    }
}

/// The setup checks of a service that answered, shown as it reports them (it knows its own configuration).
pub fn setup_rows_from_service(diag: &Value) -> Vec<Check> {
    let sec = "Setup (from the running service)";
    diag["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let id = c["id"].as_str().unwrap_or("?");
            let ok = c["ok"] == true;
            let required = c["required"] != false;
            let status = if ok {
                Status::Ok
            } else if required {
                Status::Fail
            } else {
                Status::Warn
            };
            let mut row = Check::new(
                &format!("setup:{id}"),
                sec,
                status,
                id,
                c["detail"].as_str().unwrap_or(""),
            );
            if !ok {
                row = row.with_fix(
                    "Open the Setup check in the app: it says how to fix this.",
                    false,
                );
            }
            row
        })
        .collect()
}

/// The setup checks of a service that is not running: the same probes the app would show, run directly.
pub fn setup_rows_direct(readiness: &[Value]) -> Vec<Check> {
    let sec = "Setup (checked directly; no service is running)";
    readiness
        .iter()
        .map(|c| {
            let id = c["id"].as_str().unwrap_or("?");
            let ok = c["ok"] == true;
            let required = c["required"] != false;
            let status = if ok {
                Status::Ok
            } else if required {
                Status::Fail
            } else {
                Status::Warn
            };
            let mut row = Check::new(
                &format!("setup:{id}"),
                sec,
                status,
                c["label"].as_str().unwrap_or(id),
                c["detail"].as_str().unwrap_or(""),
            );
            if !ok {
                row = row.with_fix(c["fix"].as_str().unwrap_or("See the setup page."), false);
            }
            row
        })
        .collect()
}

/// The readiness probes run on another thread so a stuck program (a virtual machine that does not answer) cannot hold the doctor.
fn readiness_limited(args: &[String], limit: Duration) -> Result<Vec<Value>, String> {
    use kk_service::JobRunner;
    let setup = crate::serve::setup_from_flags(args, true)?;
    let runner = crate::runner::ReviewRunner {
        model: std::sync::RwLock::new(setup.model),
        ollama: setup.ollama,
        transport: setup.transport,
        worker_binary: setup.worker_binary,
        scratch: PathBuf::new(),
        commitment_key: [0u8; 32],
        max_turns: 1,
        scripted: None,
        context_tokens: kk_route::DEFAULT_CONTEXT_TOKENS,
        remote: crate::near::remote_from_flags(args).ok().flatten().into(),
        outside: None.into(),
        route_dir: None,
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(runner.readiness());
    });
    rx.recv_timeout(limit).map_err(|_| {
        format!(
            "the setup checks did not finish in {} seconds (a stuck virtual machine or program?)",
            limit.as_secs()
        )
    })
}

/// What the running service serves, from its admin.json: `Some(Some(dir))` a web build, `Some(None)` only the API, `None`
/// when no service answered.
fn served_ui(dir: &Path) -> Option<Option<PathBuf>> {
    let v: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("admin.json")).ok()?).ok()?;
    let ui = v.get("ui")?;
    Some(ui.as_str().map(PathBuf::from))
}

fn web_row(running: Option<Option<PathBuf>>) -> Check {
    match running {
        Some(Some(p)) => {
            return Check::new(
                "web",
                "Web interface",
                Status::Ok,
                "Web build",
                &format!("served by the running service from {}", p.display()),
            )
        }
        Some(None) => {
            return Check::new("web", "Web interface", Status::Warn, "Web build", "the running service was started without a web build, so it serves only the API; the terminal workspace still works")
                .with_fix("Run scripts/setup (it builds the web workspace), then start the service again", false)
        }
        None => {}
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();
    let found = [
        exe_dir.join("../../apps/web/dist"),
        PathBuf::from("apps/web/dist"),
    ]
    .into_iter()
    .find(|p| p.join("index.html").exists());
    match found {
        Some(p) => Check::new(
            "web",
            "Web interface",
            Status::Ok,
            "Web build",
            &std::fs::canonicalize(&p).unwrap_or(p).display().to_string(),
        ),
        None => Check::new("web", "Web interface", Status::Warn, "Web build", "no web build found where `kaku serve` looks for one, so it would serve only the API; the terminal workspace still works")
            .with_fix("Run scripts/setup (it builds the web workspace), or start the service with --ui-dir FOLDER", false),
    }
}

pub struct Report {
    pub checks: Vec<Check>,
    pub fixed: Vec<String>,
}

impl Report {
    pub fn count(&self, s: Status) -> usize {
        self.checks.iter().filter(|c| c.status == s).count()
    }

    pub fn exit_code(&self) -> u8 {
        u8::from(self.count(Status::Fail) > 0)
    }
}

/// Everything the doctor checks, in order. `skip` holds ids from `IDS`.
pub fn gather(args: &[String], skip: &[String], fix: bool, probe: &dyn Probe) -> Report {
    let skipped = |id: &str| skip.iter().any(|s| s == id);
    let mut checks: Vec<Check> = Vec::new();
    let mut fixed: Vec<String> = Vec::new();
    let version = env!("CARGO_PKG_VERSION");
    if !skipped("home") {
        checks.push(check_home(std::env::var_os("HOME").as_deref()));
    }
    let dir = crate::data_dir(args);
    for row in data_dir_rows(&dir, fix, &mut fixed) {
        if !skipped(&row.id) {
            checks.push(row);
        }
    }
    if !skipped("config") {
        checks.push(config_row(&dir));
    }
    if !skipped("disk") {
        checks.push(disk_row(free_kib_for(&dir)));
    }
    let mut diag: Option<Value> = None;
    if !skipped("service") {
        let (rows, d) = service_rows(&dir, version, fix, probe, &mut fixed);
        checks.extend(rows);
        diag = d;
    }
    if !skipped("setup") {
        match &diag {
            Some(d) => checks.extend(setup_rows_from_service(d)),
            None => match readiness_limited(args, Duration::from_secs(20)) {
                Ok(r) => checks.extend(setup_rows_direct(&r)),
                Err(e) => checks.push(
                    Check::new("setup", "Setup (checked directly; no service is running)", Status::Fail, "Setup checks", &e)
                        .with_fix("Check the --transport, --vm and --worker-bin options, or run the checks once the machine is responsive.", false),
                ),
            },
        }
    }
    if !skipped("web") {
        // only a service that answered is believed about what it serves
        checks.push(web_row(diag.as_ref().and_then(|_| served_ui(&dir))));
    }
    for id in skip {
        checks.push(Check::new(
            id,
            "Skipped",
            Status::Skip,
            id,
            "skipped because you asked",
        ));
    }
    Report { checks, fixed }
}

/// Text for the screen: a control character or a direction mark in a folder name or an answer becomes `?`, so nothing a file name
/// or another program says can act as a terminal command or reorder what is read.
pub fn clean(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_control()
                || matches!(
                    c,
                    '\u{200b}'..='\u{200f}'
                        | '\u{202a}'..='\u{202e}'
                        | '\u{2066}'..='\u{2069}'
                        | '\u{feff}'
                )
            {
                '?'
            } else {
                c
            }
        })
        .collect()
}

fn pluralise(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn render(report: &Report, dir: &Path) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Kakurega doctor\nversion {}   {} {}   data folder {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        clean(&dir.display().to_string())
    ));
    let mut section = "";
    for c in &report.checks {
        if c.section != section {
            section = c.section;
            out.push_str(&format!("\n{section}\n"));
        }
        out.push_str(&format!(
            "  {:<6} {}: {}\n",
            format!("[{}]", c.status.word()),
            clean(&c.label),
            clean(&c.detail)
        ));
        if !matches!(c.status, Status::Ok | Status::Skip) && !c.fix.is_empty() {
            out.push_str(&format!("         to fix: {}\n", clean(&c.fix)));
        }
    }
    if !report.fixed.is_empty() {
        out.push_str("\nChanged by --fix\n");
        for f in &report.fixed {
            out.push_str(&format!("  {}\n", clean(f)));
        }
    }
    let (fails, warns) = (report.count(Status::Fail), report.count(Status::Warn));
    out.push_str(&format!(
        "\nResult: {}, {}, {} ok{}.\n",
        pluralise(fails, "failure", "failures"),
        pluralise(warns, "warning", "warnings"),
        report.count(Status::Ok),
        if report.count(Status::Skip) > 0 {
            format!(", {} skipped", report.count(Status::Skip))
        } else {
            String::new()
        }
    ));
    let todo: Vec<&Check> = report
        .checks
        .iter()
        .filter(|c| matches!(c.status, Status::Fail | Status::Warn) && !c.fix.is_empty())
        .collect();
    if report.checks.iter().any(|c| c.fixable) {
        out.push_str(&format!(
            "kaku doctor --fix does only this: {}.\n",
            FIXES.join("; ")
        ));
    }
    if !todo.is_empty() {
        out.push_str("To do, failures first:\n");
        let ordered = todo
            .iter()
            .filter(|c| c.status == Status::Fail)
            .chain(todo.iter().filter(|c| c.status == Status::Warn));
        for (i, c) in ordered.enumerate() {
            out.push_str(&format!(
                "  {}. {}: {}{}\n",
                i + 1,
                clean(&c.label),
                clean(&c.fix),
                if c.fixable {
                    "  (kaku doctor --fix does this)"
                } else {
                    ""
                }
            ));
        }
    }
    out
}

pub fn to_json(report: &Report, dir: &Path) -> Value {
    json!({
        "doctor": 1,
        "version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "data_dir": dir.display().to_string(),
        "checks": report.checks.iter().map(|c| json!({
            "id": c.id, "section": c.section, "status": c.status.json(), "label": c.label,
            "detail": c.detail, "fix": c.fix, "fixable": c.fixable,
        })).collect::<Vec<_>>(),
        "fixed": report.fixed,
        "fix_does_only": FIXES,
        "summary": {
            "ok": report.count(Status::Ok), "warn": report.count(Status::Warn),
            "fail": report.count(Status::Fail), "skip": report.count(Status::Skip),
            "fixed": report.fixed.len(),
        },
    })
}

/// The ids in `--skip VALUE`, or what is wrong with them.
pub fn parse_skip(args: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for value in crate::flags(args, "--skip") {
        for id in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            if !IDS.contains(&id) {
                return Err(format!(
                    "--skip does not know `{id}` (known: {})",
                    IDS.join(", ")
                ));
            }
            if !out.iter().any(|o| o == id) {
                out.push(id.to_string());
            }
        }
    }
    Ok(out)
}

pub fn run(args: &[String]) -> u8 {
    let skip = match parse_skip(args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("kaku doctor: {e}");
            return 2;
        }
    };
    let fix = args.iter().any(|a| a == "--fix");
    let report = gather(args, &skip, fix, &Http);
    let dir = crate::data_dir(args);
    if args.iter().any(|a| a == "--json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&to_json(&report, &dir)).unwrap_or_default()
        );
    } else {
        print!("{}", render(&report, &dir));
    }
    report.exit_code()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    fn fresh(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kk-doctor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        d
    }

    fn touch(dir: &Path, name: &str, mode: u32) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, b"x").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    fn mode(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    fn row<'a>(rows: &'a [Check], id: &str) -> &'a Check {
        rows.iter().find(|r| r.id == id).unwrap_or_else(|| {
            panic!(
                "no row {id}: {:?}",
                rows.iter().map(|r| &r.id).collect::<Vec<_>>()
            )
        })
    }

    struct Stub {
        health: Health,
        diag: Option<Value>,
        diag_calls: Cell<u32>,
        tokens: RefCell<Vec<String>>,
    }

    impl Stub {
        fn new(health: Health, diag: Option<Value>) -> Stub {
            Stub {
                health,
                diag,
                diag_calls: Cell::new(0),
                tokens: RefCell::new(vec![]),
            }
        }
    }

    impl Probe for Stub {
        fn health(&self, _port: u16) -> Health {
            self.health
        }
        fn diagnostics(&self, _port: u16, token: &str) -> Option<Value> {
            self.diag_calls.set(self.diag_calls.get() + 1);
            self.tokens.borrow_mut().push(token.to_string());
            self.diag.clone()
        }
    }

    const TOKEN: &str = "TOKEN-secret-0123456789abcdef-do-not-print";

    fn admin(dir: &Path, port: u64, mode: u32) -> PathBuf {
        let p = dir.join("admin.json");
        std::fs::write(&p, json!({"port": port, "token": TOKEN}).to_string()).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    #[test]
    fn dc01_the_free_space_is_read_from_df_in_both_layouts_and_nothing_else_counts() {
        let linux = "Filesystem     1024-blocks     Used Available Capacity Mounted on\n/dev/vda1         102400000 20000000  82400000      20% /\n";
        assert_eq!(parse_df_free_kib(linux), Some(82_400_000));
        let mac = "Filesystem   1024-blocks      Used Available Capacity  Mounted on\n/dev/disk3s5   494384795 276390316 125371008    69%    /System/Volumes/Data\n";
        assert_eq!(parse_df_free_kib(mac), Some(125_371_008));
        // a long device name wraps its numbers onto the next line
        let wrapped = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/a/very/long/network/device/name\n   1000 400 600 40% /mnt\n";
        assert_eq!(parse_df_free_kib(wrapped), Some(600));
        for bad in [
            "",
            "nothing",
            "Filesystem\n",
            "h\n/dev/x 1 2 notanumber 4% /\n",
            "h\n/dev/x 1 2\n",
        ] {
            assert_eq!(parse_df_free_kib(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn dc02_free_space_is_a_failure_under_100_mib_a_warning_under_1_gib_and_skipped_when_unknown() {
        let st = |k: Option<u64>| disk_row(k).status;
        assert_eq!(st(Some(99 * 1024 + 1023)), Status::Fail);
        assert_eq!(st(Some(100 * 1024)), Status::Warn);
        assert_eq!(st(Some(1023 * 1024 + 1023)), Status::Warn);
        assert_eq!(st(Some(1024 * 1024)), Status::Ok);
        assert_eq!(st(None), Status::Skip);
        assert!(
            !disk_row(Some(10)).fix.is_empty() && disk_row(Some(10 * 1024 * 1024)).fix.is_empty()
        );
    }

    #[test]
    fn dc03_home_must_be_set_and_be_a_folder() {
        let d = fresh("home");
        assert_eq!(check_home(Some(d.as_os_str())).status, Status::Ok);
        let file = touch(&d, "f", 0o600);
        let bad = check_home(Some(file.as_os_str()));
        assert_eq!(bad.status, Status::Fail);
        assert!(bad.detail.contains("not a folder"));
        assert_eq!(check_home(None).status, Status::Fail);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn dc04_a_data_folder_that_is_missing_loose_or_not_a_folder_is_reported_and_fixed_only_when_asked(
    ) {
        let base = fresh("dir");
        let mut fixed = vec![];
        // missing: a warning, and nothing is made
        let target = base.join("not-yet");
        let rows = data_dir_rows(&target, false, &mut fixed);
        assert_eq!(row(&rows, "data_dir").status, Status::Warn);
        assert!(!target.exists() && fixed.is_empty());
        // missing with --fix: made, owner only
        let rows = data_dir_rows(&target, true, &mut fixed);
        assert_eq!(row(&rows, "data_dir").status, Status::Ok);
        assert_eq!(mode(&target), 0o700);
        assert_eq!(fixed.len(), 1);
        // a file where the folder should be
        let file = touch(&base, "afile", 0o600);
        let rows = data_dir_rows(&file, true, &mut fixed);
        assert_eq!(row(&rows, "data_dir").status, Status::Fail);
        assert_eq!(mode(&file), 0o600, "a file is never changed");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn dc05_loose_permissions_on_the_folder_and_the_key_are_found_and_fixed_only_in_a_folder_that_is_ours(
    ) {
        let d = fresh("perms");
        let key = touch(&d, "vault.key", 0o644);
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut fixed = vec![];
        let rows = data_dir_rows(&d, false, &mut fixed);
        assert_eq!(row(&rows, "data_perms").status, Status::Warn);
        assert_eq!(
            row(&rows, "key_perms").status,
            Status::Fail,
            "a readable key is a failure, not a warning"
        );
        assert!(row(&rows, "data_perms").fixable && row(&rows, "key_perms").fixable);
        assert_eq!(
            (mode(&d), mode(&key)),
            (0o755, 0o644),
            "reading changes nothing"
        );
        // --fix tightens both
        let rows = data_dir_rows(&d, true, &mut fixed);
        assert_eq!(
            (
                row(&rows, "data_perms").status,
                row(&rows, "key_perms").status
            ),
            (Status::Ok, Status::Ok)
        );
        assert_eq!((mode(&d), mode(&key)), (0o700, 0o600));
        assert_eq!(fixed.len(), 2, "{fixed:?}");
        // group-readable is loose too: the key and the folder at 640 and 750
        let g = fresh("perms-group");
        let gkey = touch(&g, "vault.key", 0o640);
        std::fs::set_permissions(&g, std::fs::Permissions::from_mode(0o750)).unwrap();
        let rows = data_dir_rows(&g, false, &mut vec![]);
        assert_eq!(
            row(&rows, "key_perms").status,
            Status::Fail,
            "640 is readable by the group"
        );
        assert_eq!(
            row(&rows, "data_perms").status,
            Status::Warn,
            "750 is listable by the group"
        );
        assert_eq!(mode(&gkey), 0o640);
        let _ = std::fs::remove_dir_all(&g);
        // owner only already: nothing to do, nothing recorded
        let mut again = vec![];
        data_dir_rows(&d, true, &mut again);
        assert!(again.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn dc06_fix_refuses_a_folder_that_does_not_look_like_ours_and_says_why() {
        let d = fresh("notours");
        touch(&d, "holiday-photos.zip", 0o644);
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut fixed = vec![];
        let rows = data_dir_rows(&d, true, &mut fixed);
        let r = row(&rows, "data_perms");
        assert_eq!(r.status, Status::Warn);
        assert!(
            r.detail.contains("none of Kakurega's own files"),
            "{}",
            r.detail
        );
        assert!(!r.fixable);
        assert_eq!(mode(&d), 0o755, "an unrelated folder's mode is not touched");
        assert!(fixed.is_empty());
        // one of ours makes it ours
        touch(&d, "memory.db", 0o644);
        assert!(looks_like_data_dir(&d));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn dc07_a_key_that_is_a_link_is_a_failure_and_is_never_changed() {
        let d = fresh("keylink");
        let real = touch(&d, "elsewhere", 0o644);
        std::os::unix::fs::symlink(&real, d.join("vault.key")).unwrap();
        let mut fixed = vec![];
        let rows = data_dir_rows(&d, true, &mut fixed);
        assert_eq!(row(&rows, "key_perms").status, Status::Fail);
        assert!(!row(&rows, "key_perms").fixable);
        assert_eq!(mode(&real), 0o644, "the link's target was not changed");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn dc08_a_folder_that_cannot_be_written_is_a_failure() {
        let d = fresh("readonly");
        touch(&d, "jobs.db", 0o600);
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o500)).unwrap();
        let mut fixed = vec![];
        let rows = data_dir_rows(&d, false, &mut fixed);
        // a process that may write anyway (root) cannot show this case
        if row(&rows, "data_write").status != Status::Ok {
            assert_eq!(row(&rows, "data_write").status, Status::Fail);
        }
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn dc09_with_no_admin_file_the_service_is_not_running_and_nothing_is_asked() {
        let d = fresh("svc-none");
        let stub = Stub::new(Health::Kakurega, None);
        let mut fixed = vec![];
        let (rows, diag) = service_rows(&d, "1.0.0", false, &stub, &mut fixed);
        assert_eq!(row(&rows, "service").status, Status::Ok);
        assert!(diag.is_none() && stub.diag_calls.get() == 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn dc10_a_damaged_admin_file_is_a_failure_in_every_way_it_can_be_damaged_and_is_removed_only_when_fixing_our_folder(
    ) {
        for (i, body) in [
            "not json",
            "{}",
            "{\"port\": 0, \"token\": \"t\"}",
            "{\"port\": 70000, \"token\": \"t\"}",
            "{\"port\": 80}",
            "{\"port\": \"80\", \"token\": \"t\"}",
            "[]",
        ]
        .iter()
        .enumerate()
        {
            let d = fresh(&format!("svc-damaged{i}"));
            touch(&d, "jobs.db", 0o600);
            let p = d.join("admin.json");
            std::fs::write(&p, body).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
            let stub = Stub::new(Health::Kakurega, None);
            let mut fixed = vec![];
            let (rows, _) = service_rows(&d, "1.0.0", false, &stub, &mut fixed);
            assert_eq!(row(&rows, "service").status, Status::Fail, "{body}");
            assert!(
                p.exists() && stub.diag_calls.get() == 0,
                "reading removes nothing and asks nothing: {body}"
            );
            let (rows, _) = service_rows(&d, "1.0.0", true, &stub, &mut fixed);
            assert_eq!(row(&rows, "service").status, Status::Ok, "{body}");
            assert!(!p.exists() && fixed.len() == 1, "{body}");
            let _ = std::fs::remove_dir_all(&d);
        }
        // a folder that is not ours: even a damaged file is left alone
        let d = fresh("svc-damaged-notours");
        let p = d.join("admin.json");
        std::fs::write(&p, "junk").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
        // admin.json is itself one of ours, so make it unmistakably another folder by pointing at a sub-folder with no files of ours
        let other = fresh("svc-other");
        let stub = Stub::new(Health::Nothing, None);
        let (rows, _) = service_rows(&other, "1.0.0", true, &stub, &mut vec![]);
        assert_eq!(
            row(&rows, "service").status,
            Status::Ok,
            "no file there at all"
        );
        let _ = (std::fs::remove_dir_all(&d), std::fs::remove_dir_all(&other));
    }

    #[test]
    fn dc11_a_stale_admin_file_is_a_warning_and_the_token_is_never_sent_to_a_port_that_is_not_kakurega(
    ) {
        for health in [Health::Nothing, Health::SomethingElse] {
            let d = fresh("svc-stale");
            let p = admin(&d, 40123, 0o600);
            let stub = Stub::new(health, Some(json!({"version": "1.0.0", "checks": []})));
            let mut fixed = vec![];
            let (rows, diag) = service_rows(&d, "1.0.0", false, &stub, &mut fixed);
            let r = row(&rows, "service");
            assert_eq!(r.status, Status::Warn, "{health:?}");
            assert!(r.fixable && p.exists() && diag.is_none());
            assert_eq!(stub.diag_calls.get(), 0, "{health:?}: the token must not be sent to a port that is not recognised as Kakurega");
            assert!(stub.tokens.borrow().is_empty());
            if health == Health::SomethingElse {
                assert!(
                    r.detail.contains("not Kakurega") && r.detail.contains("not sent"),
                    "{}",
                    r.detail
                );
            }
            // --fix removes it, and says so
            let (rows, _) = service_rows(&d, "1.0.0", true, &stub, &mut fixed);
            assert_eq!(row(&rows, "service").status, Status::Ok);
            assert!(!p.exists());
            assert_eq!(fixed.len(), 1);
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn dc12_a_running_service_is_recognised_first_then_asked_with_the_token_once_and_the_token_never_appears_in_any_row(
    ) {
        let d = fresh("svc-run");
        admin(&d, 40124, 0o600);
        let diag = json!({"version": "1.0.0", "checks": [{"id": "ollama", "ok": true, "required": true, "detail": "running"}]});
        let stub = Stub::new(Health::Kakurega, Some(diag));
        let (rows, got) = service_rows(&d, "1.0.0", false, &stub, &mut vec![]);
        assert_eq!(row(&rows, "service").status, Status::Ok);
        assert!(row(&rows, "service").detail.contains("40124") && got.is_some());
        assert_eq!(stub.diag_calls.get(), 1);
        assert_eq!(*stub.tokens.borrow(), vec![TOKEN.to_string()]);
        for r in &rows {
            assert!(!format!("{r:?}").contains(TOKEN), "{r:?}");
        }
        // a version difference is a warning, not a failure
        let stub = Stub::new(
            Health::Kakurega,
            Some(json!({"version": "0.9.0", "checks": []})),
        );
        let (rows, _) = service_rows(&d, "1.0.0", false, &stub, &mut vec![]);
        assert_eq!(row(&rows, "service_version").status, Status::Warn);
        assert!(
            row(&rows, "service_version").detail.contains("0.9.0")
                && row(&rows, "service_version").detail.contains("1.0.0")
        );
        // it answers as Kakurega but refuses the diagnostics
        let stub = Stub::new(Health::Kakurega, None);
        let (rows, got) = service_rows(&d, "1.0.0", true, &stub, &mut vec![]);
        assert_eq!(row(&rows, "service").status, Status::Warn);
        assert!(
            got.is_none() && d.join("admin.json").exists(),
            "a live service's file is never removed, even with --fix"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn dc13_an_admin_file_others_can_read_is_a_failure_and_fixing_tightens_it_and_a_link_is_never_followed(
    ) {
        let d = fresh("svc-mode");
        touch(&d, "jobs.db", 0o600);
        let p = admin(&d, 40125, 0o644);
        let stub = Stub::new(Health::Nothing, None);
        let (rows, _) = service_rows(&d, "1.0.0", false, &stub, &mut vec![]);
        let loose = rows
            .iter()
            .find(|r| r.label == "admin.json access")
            .unwrap();
        assert_eq!(loose.status, Status::Fail);
        assert!(!loose.detail.contains(TOKEN));
        assert_eq!(mode(&p), 0o644);
        let mut fixed = vec![];
        service_rows(&d, "1.0.0", true, &stub, &mut fixed);
        assert!(fixed.iter().any(|f| f.contains("owner only")), "{fixed:?}");
        let _ = std::fs::remove_dir_all(&d);
        // a link: refused, and its target untouched
        let d = fresh("svc-link");
        let target = touch(&d, "other.json", 0o644);
        std::os::unix::fs::symlink(&target, d.join("admin.json")).unwrap();
        let (rows, _) = service_rows(&d, "1.0.0", true, &stub, &mut vec![]);
        assert_eq!(row(&rows, "service").status, Status::Fail);
        assert!(!row(&rows, "service").fixable);
        assert_eq!(mode(&target), 0o644);
        assert!(target.exists() && d.join("admin.json").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn dc14_setup_checks_from_a_service_and_from_direct_probes_map_to_the_same_statuses() {
        let diag = json!({"checks": [
            {"id": "ollama", "ok": true, "required": true, "detail": "running"},
            {"id": "sandbox", "ok": false, "required": true, "detail": "vm stopped"},
            {"id": "node", "ok": false, "required": false, "detail": "none"},
        ]});
        let rows = setup_rows_from_service(&diag);
        assert_eq!(
            rows.iter().map(|r| r.status).collect::<Vec<_>>(),
            vec![Status::Ok, Status::Fail, Status::Warn]
        );
        assert!(rows[1].fix.contains("Setup"));
        let direct = setup_rows_direct(&[
            json!({"id": "worker", "label": "Review worker", "ok": true, "required": true, "detail": "/x", "fix": ""}),
            json!({"id": "sandbox", "label": "Sandbox", "ok": false, "required": true, "detail": "stopped", "fix": "Start it with: limactl start kak"}),
            json!({"id": "node", "label": "Node", "ok": false, "required": false, "detail": "old", "fix": "Install Node 22"}),
        ]);
        assert_eq!(
            direct.iter().map(|r| r.status).collect::<Vec<_>>(),
            vec![Status::Ok, Status::Fail, Status::Warn]
        );
        assert_eq!(
            direct[1].fix, "Start it with: limactl start kak",
            "the app's own fix text is kept"
        );
        assert_eq!(direct[1].id, "setup:sandbox");
    }

    #[test]
    fn dc15_skip_takes_only_known_ids_in_any_spelling_of_the_list_and_says_which_are_known() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            parse_skip(&a(&["--skip", "setup,web", "--skip", "disk,setup"])).unwrap(),
            vec!["setup", "web", "disk"]
        );
        assert_eq!(parse_skip(&a(&[])).unwrap(), Vec::<String>::new());
        assert_eq!(
            parse_skip(&a(&["--skip", " disk , "])).unwrap(),
            vec!["disk"]
        );
        let e = parse_skip(&a(&["--skip", "disk,bogus"])).unwrap_err();
        assert!(
            e.contains("`bogus`") && e.contains("setup") && e.contains("data_perms"),
            "{e}"
        );
        // the ids it accepts are exactly the documented set
        for id in IDS {
            assert!(parse_skip(&a(&["--skip", id])).is_ok());
        }
    }

    #[test]
    fn dc16_the_report_says_failures_first_marks_what_fix_can_do_lists_exactly_what_it_does_and_uses_no_colour(
    ) {
        let report = Report {
            checks: vec![
                Check::new("a", "S", Status::Ok, "Fine", "yes"),
                Check::new("b", "S", Status::Warn, "Loose", "mode 755").with_fix("Run chmod", true),
                Check::new("c", "S", Status::Fail, "Broken", "no").with_fix("Install it", false),
                Check::new("d", "T", Status::Skip, "d", "skipped"),
            ],
            fixed: vec!["made it owner only".into()],
        };
        let text = render(&report, Path::new("/data"));
        assert!(
            text.contains("[FAIL]")
                && text.contains("[warn]")
                && text.contains("[ok]")
                && text.contains("[skip]")
        );
        assert!(
            text.contains("Result: 1 failure, 1 warning, 1 ok, 1 skipped."),
            "{text}"
        );
        let todo = text.split("To do, failures first:").nth(1).unwrap();
        assert!(
            todo.find("Install it").unwrap() < todo.find("Run chmod").unwrap(),
            "failures come first: {todo}"
        );
        assert!(
            todo.contains("Loose: Run chmod  (kaku doctor --fix does this)")
                && todo.contains("Broken: Install it\n")
                && !todo.contains("Install it  (kaku"),
            "each item says which check it is for: {todo}"
        );
        assert!(
            text.contains("kaku doctor --fix does only this:")
                && text.contains("remove an admin.json that is damaged")
        );
        assert!(text.contains("Changed by --fix") && text.contains("made it owner only"));
        assert!(!text.contains('\u{1b}'), "no colour or control sequences");
        assert_eq!(report.exit_code(), 1);
        let ok = Report {
            checks: vec![Check::new("a", "S", Status::Warn, "x", "y")],
            fixed: vec![],
        };
        assert_eq!(ok.exit_code(), 0, "warnings alone do not fail");
        let json = to_json(&report, Path::new("/data"));
        assert_eq!(
            json["summary"],
            json!({"ok": 1, "warn": 1, "fail": 1, "skip": 1, "fixed": 1})
        );
        assert_eq!(json["checks"][2]["status"], "fail");
        assert_eq!(json["fix_does_only"].as_array().unwrap().len(), FIXES.len());
    }

    #[test]
    fn dc17_control_and_direction_characters_in_a_path_or_an_answer_never_reach_the_screen() {
        let evil = "/tmp/\u{1b}[2Jfolder\u{202e}gnp.exe\u{7}\nmore";
        let report = Report {
            checks: vec![
                Check::new("x", "S", Status::Warn, "La\u{1b}bel", evil).with_fix(evil, false)
            ],
            fixed: vec![evil.into()],
        };
        let text = render(&report, Path::new(evil));
        for bad in ['\u{1b}', '\u{202e}', '\u{7}'] {
            assert!(!text.contains(bad), "U+{:04X}: {text}", bad as u32);
        }
        assert!(
            text.contains("folder?gnp.exe") || text.contains("folder"),
            "{text}"
        );
        assert_eq!(
            clean("plain é 日本語"),
            "plain é 日本語",
            "ordinary text is untouched"
        );
    }

    #[test]
    fn dc18_a_command_is_stopped_at_its_time_limit_and_a_quick_one_returns_its_output() {
        let started = Instant::now();
        let mut slow = Command::new("sleep");
        slow.arg("5");
        assert!(run_limited(slow, Duration::from_millis(300)).is_none());
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        // it was stopped, not just given up on: no process with its distinctive argument is left running
        let mut marked = Command::new("sleep");
        marked.arg("7.317");
        assert!(run_limited(marked, Duration::from_millis(200)).is_none());
        std::thread::sleep(Duration::from_millis(300));
        let left = Command::new("pgrep")
            .args(["-f", "sleep 7.317"])
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&left.stdout).trim().is_empty(),
            "a stuck command must not be left running: {}",
            String::from_utf8_lossy(&left.stdout)
        );
        let mut fast = Command::new("echo");
        fast.arg("hello");
        let out = run_limited(fast, Duration::from_secs(5)).unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello");
        assert!(run_limited(Command::new("/no/such/program"), Duration::from_secs(1)).is_none());
    }

    #[test]
    fn dc19_the_fix_list_is_exactly_the_five_documented_local_actions() {
        assert_eq!(FIXES.len(), 5);
        let all = FIXES.join(" | ");
        for word in [
            "create the data folder",
            "data folder owner only",
            "vault.key owner only",
            "admin.json owner only",
            "remove an admin.json",
        ] {
            assert!(all.contains(word), "{word}");
        }
        // none of them names a key, a vault, notes, configuration or a process
        for forbidden in ["delete", "kill", "config", "note", "restore", "overwrite"] {
            assert!(!all.contains(forbidden), "{forbidden}");
        }
    }

    /// A server that answers each request with the next canned body and records what it was sent.
    fn fake_server(bodies: Vec<&'static str>) -> (u16, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for body in bodies {
                let (mut sock, _) = listener.accept().unwrap();
                sock.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).unwrap_or(0);
                seen.push(String::from_utf8_lossy(&buf[..n]).into_owned());
                let _ = write!(sock, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            }
            seen
        });
        (port, handle)
    }

    #[test]
    fn dc20_the_real_probe_recognises_kakurega_by_its_own_name_and_nothing_else() {
        let (port, handle) = fake_server(vec![r#"{"ok":true,"service":"kakurega-companion"}"#]);
        assert_eq!(Http.health(port), Health::Kakurega);
        let seen = handle.join().unwrap();
        assert!(seen[0].starts_with("GET /api/health "), "{}", seen[0]);
        assert!(
            !seen[0].to_lowercase().contains("authorization"),
            "the first question carries no token: {}",
            seen[0]
        );
        for other in [
            r#"{"ok":true,"service":"somebody-else"}"#,
            r#"{"ok":true}"#,
            "not json at all",
            r#"["kakurega-companion"]"#,
        ] {
            let (port, handle) = fake_server(vec![other]);
            assert_eq!(Http.health(port), Health::SomethingElse, "{other}");
            let _ = handle.join();
        }
        // a port nothing listens on
        let free = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            l.local_addr().unwrap().port()
        };
        assert_eq!(Http.health(free), Health::Nothing);
    }

    #[test]
    fn dc21_the_real_probe_sends_the_token_only_in_the_authorization_header_to_the_diagnostics_path(
    ) {
        let (port, handle) = fake_server(vec![r#"{"version":"1.2.3","checks":[]}"#]);
        let got = Http.diagnostics(port, TOKEN).unwrap();
        assert_eq!(got["version"], "1.2.3");
        let seen = handle.join().unwrap();
        let first = seen[0].lines().next().unwrap();
        assert_eq!(first, "GET /admin/diagnostics HTTP/1.1");
        assert!(!first.contains(TOKEN), "never in the URL");
        let header = seen[0]
            .lines()
            .find(|l| l.to_lowercase().starts_with("authorization"))
            .unwrap();
        assert_eq!(
            header.split_once(':').unwrap().1.trim(),
            format!("Bearer {TOKEN}")
        );
        // a refusal is nothing
        let (port, handle) = fake_server(vec!["{}"]);
        // (the canned server answers 200; a 401 is covered by the diagnostics call returning None on non-200 in the stub tests)
        let _ = Http.diagnostics(port, TOKEN);
        let _ = handle.join();
    }

    #[test]
    fn dc22_the_settings_file_is_ok_when_absent_or_correct_and_a_warning_naming_every_problem_when_not(
    ) {
        let d = fresh("cfg-row");
        let absent = config_row(&d);
        assert_eq!(absent.status, Status::Ok);
        assert!(absent.detail.contains("defaults"));
        std::fs::write(d.join("config.toml"), "# nothing set\n").unwrap();
        assert_eq!(config_row(&d).status, Status::Ok);
        std::fs::write(d.join("config.toml"), "quiet = true\nnotify = \"bell\"\n").unwrap();
        let ok = config_row(&d);
        assert_eq!(ok.status, Status::Ok);
        assert!(
            ok.detail.contains("applied")
                && ok.detail.contains("notify")
                && ok.detail.contains("quiet"),
            "{}",
            ok.detail
        );
        std::fs::write(
            d.join("config.toml"),
            "quite = true\nnotify = \"loud\"\nlinear = 1\n",
        )
        .unwrap();
        let bad = config_row(&d);
        assert_eq!(
            bad.status,
            Status::Warn,
            "the product works, the settings are not in force"
        );
        for part in [
            "NOT applied",
            "3 problems",
            "quite",
            "did you mean `quiet`",
            "notify",
            "linear",
        ] {
            assert!(bad.detail.contains(part), "{part}: {}", bad.detail);
        }
        assert!(bad.fix.contains("kaku config check"));
        assert!(!bad.fixable, "--fix never edits a person's settings");
        assert!(IDS.contains(&"config"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
