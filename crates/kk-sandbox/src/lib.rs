use std::ffi::OsString;
use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Limits {
    pub cpu_secs: u64,
    pub memory_bytes: u64,
    pub processes: u64,
    pub file_bytes: u64,
    pub open_files: u64,
    pub wall: Duration,
    pub output_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            cpu_secs: 30,
            memory_bytes: 1 << 30,
            processes: 64,
            file_bytes: 64 << 20,
            open_files: 256,
            wall: Duration::from_secs(60),
            output_bytes: 1 << 20,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Bind {
    pub host: PathBuf,
    pub inside: PathBuf,
    pub writable: bool,
}

#[derive(Debug, Clone)]
pub struct SandboxSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub env: Vec<(String, String)>,
    pub binds: Vec<Bind>,
    pub limits: Limits,
    pub chdir: Option<PathBuf>,
    pub weaken_for_self_test: Option<Weakening>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weakening {
    ShareNetwork,
    AllowNestedUserNamespaces,
}

impl SandboxSpec {
    pub fn new(program: &str) -> Self {
        SandboxSpec {
            program: PathBuf::from(program),
            args: Vec::new(),
            env: Vec::new(),
            binds: Vec::new(),
            limits: Limits::default(),
            chdir: None,
            weaken_for_self_test: None,
        }
    }
}

#[derive(Debug)]
pub enum SandboxError {
    Unsupported(&'static str),
    Spawn(String),
    SelfCheckFailed(Vec<String>),
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SandboxError::Unsupported(m) => write!(f, "sandbox unsupported: {m}"),
            SandboxError::Spawn(m) => write!(f, "could not start the sandbox: {m}"),
            SandboxError::SelfCheckFailed(v) => {
                write!(f, "sandbox self-check failed: {}", v.join("; "))
            }
        }
    }
}

impl std::error::Error for SandboxError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    Signal(i32),
    TimedOut,
}

#[derive(Debug)]
pub struct Outcome {
    pub exit: Exit,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
}

pub fn bwrap_args(spec: &SandboxSpec) -> Vec<OsString> {
    let mut a: Vec<OsString> = Vec::new();
    a.push(OsString::from("--unshare-all"));
    a.push(OsString::from("--unshare-user"));
    if spec.weaken_for_self_test == Some(Weakening::ShareNetwork) {
        a.push(OsString::from("--share-net"));
    }
    if spec.weaken_for_self_test != Some(Weakening::AllowNestedUserNamespaces) {
        a.push(OsString::from("--disable-userns"));
    }
    a.push(OsString::from("--die-with-parent"));
    a.push(OsString::from("--new-session"));
    a.push(OsString::from("--clearenv"));
    a.push(OsString::from("--cap-drop"));
    a.push(OsString::from("ALL"));
    a.push(OsString::from("--proc"));
    a.push(OsString::from("/proc"));
    a.push(OsString::from("--dev"));
    a.push(OsString::from("/dev"));
    a.push(OsString::from("--tmpfs"));
    a.push(OsString::from("/tmp"));
    a.push(OsString::from("--ro-bind"));
    a.push(OsString::from("/usr"));
    a.push(OsString::from("/usr"));
    a.push(OsString::from("--symlink"));
    a.push(OsString::from("usr/bin"));
    a.push(OsString::from("/bin"));
    a.push(OsString::from("--symlink"));
    a.push(OsString::from("usr/sbin"));
    a.push(OsString::from("/sbin"));
    a.push(OsString::from("--symlink"));
    a.push(OsString::from("usr/lib"));
    a.push(OsString::from("/lib"));
    if std::path::Path::new("/lib64").exists() {
        a.push(OsString::from("--symlink"));
        a.push(OsString::from("usr/lib64"));
        a.push(OsString::from("/lib64"));
    }
    a.push(OsString::from("--setenv"));
    a.push(OsString::from("PATH"));
    a.push(OsString::from("/usr/bin:/bin"));
    a.push(OsString::from("--setenv"));
    a.push(OsString::from("HOME"));
    a.push(OsString::from("/tmp"));
    for (k, v) in &spec.env {
        a.push(OsString::from("--setenv"));
        a.push(OsString::from(k));
        a.push(OsString::from(v));
    }
    for b in &spec.binds {
        a.push(OsString::from(if b.writable {
            "--bind"
        } else {
            "--ro-bind"
        }));
        a.push(b.host.clone().into_os_string());
        a.push(b.inside.clone().into_os_string());
    }
    a.push(OsString::from("--chdir"));
    a.push(
        spec.chdir
            .clone()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .into_os_string(),
    );
    a.push(OsString::from("--"));
    a.push(spec.program.clone().into_os_string());
    a.extend(spec.args.iter().cloned());
    a
}

pub fn prlimit_args(l: &Limits) -> Vec<OsString> {
    vec![
        OsString::from(format!("--cpu={}", l.cpu_secs)),
        OsString::from(format!("--fsize={}", l.file_bytes)),
        OsString::from(format!("--nofile={}", l.open_files)),
        OsString::from("--core=0"),
    ]
}

pub fn scope_args(l: &Limits, unit: &str) -> Vec<OsString> {
    vec![
        OsString::from("--user"),
        OsString::from("--scope"),
        OsString::from("--quiet"),
        OsString::from(format!("--unit={unit}")),
        OsString::from("-p"),
        OsString::from(format!("TasksMax={}", l.processes)),
        OsString::from("-p"),
        OsString::from(format!("MemoryMax={}", l.memory_bytes)),
        OsString::from("-p"),
        OsString::from("MemorySwapMax=0"),
    ]
}

static UNIT_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn read_capped<R: Read>(mut r: R, cap: usize) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    let mut truncated = false;
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if out.len() < cap {
                    let take = n.min(cap - out.len());
                    out.extend_from_slice(&buf[..take]);
                    if take < n {
                        truncated = true;
                    }
                } else {
                    truncated = true;
                }
            }
        }
    }
    (out, truncated)
}

pub fn run_unchecked(spec: &SandboxSpec) -> Result<Outcome, SandboxError> {
    if !cfg!(target_os = "linux") {
        return Err(SandboxError::Unsupported(
            "the bubblewrap sandbox runs on Linux only",
        ));
    }
    let unit = format!(
        "kk-worker-{}-{}",
        std::process::id(),
        UNIT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let mut cmd = Command::new("systemd-run");
    cmd.args(scope_args(&spec.limits, &unit));
    cmd.arg("prlimit").args(prlimit_args(&spec.limits));
    cmd.arg("--").arg("bwrap").args(bwrap_args(spec));
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        cmd.env_clear().env("XDG_RUNTIME_DIR", dir);
    } else {
        return Err(SandboxError::Unsupported(
            "a systemd user session (XDG_RUNTIME_DIR) is required for cgroup limits",
        ));
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| SandboxError::Spawn(e.to_string()))?;
    let cap = spec.limits.output_bytes;
    let out = child.stdout.take();
    let err = child.stderr.take();
    let t_out = std::thread::spawn(move || out.map(|o| read_capped(o, cap)).unwrap_or_default());
    let t_err = std::thread::spawn(move || err.map(|o| read_capped(o, cap)).unwrap_or_default());
    let start = Instant::now();
    let exit = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                use std::os::unix::process::ExitStatusExt;
                break match (status.code(), status.signal()) {
                    (Some(c), _) => Exit::Code(c),
                    (None, Some(s)) => Exit::Signal(s),
                    _ => Exit::Code(-1),
                };
            }
            Ok(None) => {
                if start.elapsed() >= spec.limits.wall {
                    let _ = Command::new("systemctl")
                        .args(["--user", "stop", &format!("{unit}.scope")])
                        .env_clear()
                        .env(
                            "XDG_RUNTIME_DIR",
                            std::env::var("XDG_RUNTIME_DIR").unwrap_or_default(),
                        )
                        .output();
                    let _ = child.kill();
                    let _ = child.wait();
                    break Exit::TimedOut;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Err(SandboxError::Spawn(e.to_string())),
        }
    };
    let (stdout, t1) = t_out.join().unwrap_or_default();
    let (stderr, t2) = t_err.join().unwrap_or_default();
    Ok(Outcome {
        exit,
        stdout,
        stderr,
        truncated: t1 || t2,
    })
}

pub mod selfcheck;

pub fn run(spec: &SandboxSpec) -> Result<Outcome, SandboxError> {
    selfcheck::verify_profile()?;
    run_unchecked(spec)
}

pub const MAX_LINE: usize = 8 << 20;

pub struct ServeOutcome {
    pub exit: Exit,
    pub stderr: Vec<u8>,
    pub lines_served: u64,
    pub oversize_line: bool,
}

fn scope_command(spec: &SandboxSpec) -> Result<(Command, String), SandboxError> {
    if !cfg!(target_os = "linux") {
        return Err(SandboxError::Unsupported(
            "the bubblewrap sandbox runs on Linux only",
        ));
    }
    let unit = format!(
        "kk-worker-{}-{}",
        std::process::id(),
        UNIT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") else {
        return Err(SandboxError::Unsupported(
            "a systemd user session (XDG_RUNTIME_DIR) is required for cgroup limits",
        ));
    };
    let mut cmd = Command::new("systemd-run");
    cmd.args(scope_args(&spec.limits, &unit));
    cmd.arg("prlimit").args(prlimit_args(&spec.limits));
    cmd.arg("--").arg("bwrap").args(bwrap_args(spec));
    cmd.env_clear().env("XDG_RUNTIME_DIR", dir);
    Ok((cmd, unit))
}

pub fn serve_lines(
    spec: &SandboxSpec,
    handler: impl FnMut(&str) -> String,
) -> Result<ServeOutcome, SandboxError> {
    serve_lines_with(spec, &Transport::Local, handler)
}

pub fn serve_lines_with(
    spec: &SandboxSpec,
    transport: &Transport,
    mut handler: impl FnMut(&str) -> String,
) -> Result<ServeOutcome, SandboxError> {
    let (mut cmd, unit) = match transport {
        Transport::Local => {
            selfcheck::verify_profile()?;
            let (c, u) = scope_command(spec)?;
            (c, Some(u))
        }
        Transport::Lima { vm, launcher } => (lima_command(vm, launcher, spec), None),
    };
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| SandboxError::Spawn(e.to_string()))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| SandboxError::Spawn("no stdin".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| SandboxError::Spawn("no stdout".into()))?;
    let stderr = child.stderr.take();
    let cap = spec.limits.output_bytes;
    let t_err = std::thread::spawn(move || stderr.map(|o| read_capped(o, cap)).unwrap_or_default());
    let (tx, rx) = std::sync::mpsc::channel::<Result<String, ()>>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut buf = Vec::new();
            let n = match reader
                .by_ref()
                .take(MAX_LINE as u64 + 1)
                .read_until(b'\n', &mut buf)
            {
                Ok(n) => n,
                Err(_) => break,
            };
            if n == 0 {
                break;
            }
            if buf.len() > MAX_LINE {
                let _ = tx.send(Err(()));
                break;
            }
            if buf.last() == Some(&b'\n') {
                buf.pop();
            }
            match String::from_utf8(buf) {
                Ok(line) => {
                    if tx.send(Ok(line)).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = tx.send(Err(()));
                    break;
                }
            }
        }
    });
    let stop_scope = |unit: &Option<String>| {
        if let Some(unit) = unit {
            let _ = Command::new("systemctl")
                .args(["--user", "stop", &format!("{unit}.scope")])
                .env_clear()
                .env(
                    "XDG_RUNTIME_DIR",
                    std::env::var("XDG_RUNTIME_DIR").unwrap_or_default(),
                )
                .output();
        }
    };
    let start = Instant::now();
    let mut served = 0u64;
    let mut oversize = false;
    let exit = loop {
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Ok(line)) => {
                served += 1;
                let reply = handler(&line);
                if writeln!(stdin, "{reply}").is_err() || stdin.flush().is_err() {
                    // the worker is gone; the exit status is collected below
                }
            }
            Ok(Err(())) => {
                oversize = true;
                stop_scope(&unit);
                let _ = child.kill();
                let _ = child.wait();
                break Exit::Signal(9);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
        }
        if let Ok(Some(status)) = child.try_wait() {
            use std::os::unix::process::ExitStatusExt;
            while let Ok(Ok(line)) = rx.try_recv() {
                served += 1;
                let _ = handler(&line);
            }
            break match (status.code(), status.signal()) {
                (Some(c), _) => Exit::Code(c),
                (None, Some(sig)) => Exit::Signal(sig),
                _ => Exit::Code(-1),
            };
        }
        if start.elapsed() >= spec.limits.wall {
            stop_scope(&unit);
            let _ = child.kill();
            let _ = child.wait();
            break Exit::TimedOut;
        }
    };
    let (stderr_bytes, _) = t_err.join().unwrap_or_default();
    Ok(ServeOutcome {
        exit,
        stderr: stderr_bytes,
        lines_served: served,
        oversize_line: oversize,
    })
}

#[derive(serde::Serialize, serde::Deserialize)]
struct WireBind {
    host: String,
    inside: String,
    writable: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct WireSpec {
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    binds: Vec<WireBind>,
    cpu_secs: u64,
    memory_bytes: u64,
    processes: u64,
    file_bytes: u64,
    open_files: u64,
    wall_ms: u64,
    output_bytes: usize,
    #[serde(default)]
    chdir: Option<String>,
}

impl SandboxSpec {
    pub fn to_wire(&self) -> String {
        let w = WireSpec {
            program: self.program.to_string_lossy().into_owned(),
            args: self
                .args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
            env: self.env.clone(),
            binds: self
                .binds
                .iter()
                .map(|b| WireBind {
                    host: b.host.to_string_lossy().into_owned(),
                    inside: b.inside.to_string_lossy().into_owned(),
                    writable: b.writable,
                })
                .collect(),
            cpu_secs: self.limits.cpu_secs,
            memory_bytes: self.limits.memory_bytes,
            processes: self.limits.processes,
            file_bytes: self.limits.file_bytes,
            open_files: self.limits.open_files,
            wall_ms: self.limits.wall.as_millis() as u64,
            output_bytes: self.limits.output_bytes,
            chdir: self
                .chdir
                .as_ref()
                .map(|c| c.to_string_lossy().into_owned()),
        };
        serde_json::to_string(&w).unwrap_or_default()
    }

    pub fn from_wire(text: &str) -> Result<SandboxSpec, SandboxError> {
        let w: WireSpec =
            serde_json::from_str(text).map_err(|e| SandboxError::Spawn(e.to_string()))?;
        Ok(SandboxSpec {
            program: PathBuf::from(w.program),
            args: w.args.into_iter().map(OsString::from).collect(),
            env: w.env,
            binds: w
                .binds
                .into_iter()
                .map(|b| Bind {
                    host: PathBuf::from(b.host),
                    inside: PathBuf::from(b.inside),
                    writable: b.writable,
                })
                .collect(),
            limits: Limits {
                cpu_secs: w.cpu_secs,
                memory_bytes: w.memory_bytes,
                processes: w.processes,
                file_bytes: w.file_bytes,
                open_files: w.open_files,
                wall: Duration::from_millis(w.wall_ms),
                output_bytes: w.output_bytes,
            },
            chdir: w.chdir.map(PathBuf::from),
            weaken_for_self_test: None,
        })
    }
}

#[derive(Debug, Clone)]
pub enum Transport {
    Local,
    Lima { vm: String, launcher: PathBuf },
}

/// Linux side of a Lima transport: verify the profile, then replace this process with the sandboxed worker so stdin and stdout are the broker channel.
pub fn exec_from_wire(wire: &str) -> SandboxError {
    use std::os::unix::process::CommandExt;
    if let Err(e) = selfcheck::verify_profile() {
        return e;
    }
    let spec = match SandboxSpec::from_wire(wire) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match scope_command(&spec) {
        Ok((mut cmd, _unit)) => {
            let err = cmd.exec();
            SandboxError::Spawn(err.to_string())
        }
        Err(e) => e,
    }
}

pub fn lima_command(vm: &str, launcher: &Path, spec: &SandboxSpec) -> Command {
    let mut cmd = Command::new("limactl");
    cmd.args(["shell", "--tty=false", vm, "--"])
        .arg(launcher)
        .arg(spec.to_wire());
    cmd
}

pub fn run_with(spec: &SandboxSpec, transport: &Transport) -> Result<Outcome, SandboxError> {
    match transport {
        Transport::Local => {
            selfcheck::verify_profile()?;
            run_unchecked(spec)
        }
        Transport::Lima { vm, launcher } => {
            let mut cmd = lima_command(vm, launcher, spec);
            cmd.stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = cmd
                .spawn()
                .map_err(|e| SandboxError::Spawn(e.to_string()))?;
            let cap = spec.limits.output_bytes;
            let out = child.stdout.take();
            let err = child.stderr.take();
            let t_out =
                std::thread::spawn(move || out.map(|o| read_capped(o, cap)).unwrap_or_default());
            let t_err =
                std::thread::spawn(move || err.map(|o| read_capped(o, cap)).unwrap_or_default());
            let start = Instant::now();
            let exit = loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        use std::os::unix::process::ExitStatusExt;
                        break match (status.code(), status.signal()) {
                            (Some(c), _) => Exit::Code(c),
                            (None, Some(sig)) => Exit::Signal(sig),
                            _ => Exit::Code(-1),
                        };
                    }
                    Ok(None) => {
                        if start.elapsed() >= spec.limits.wall {
                            let _ = child.kill();
                            let _ = child.wait();
                            break Exit::TimedOut;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => return Err(SandboxError::Spawn(e.to_string())),
                }
            };
            let (stdout, t1) = t_out.join().unwrap_or_default();
            let (stderr, t2) = t_err.join().unwrap_or_default();
            Ok(Outcome {
                exit,
                stdout,
                stderr,
                truncated: t1 || t2,
            })
        }
    }
}
