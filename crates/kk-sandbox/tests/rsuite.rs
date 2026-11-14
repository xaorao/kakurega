#![cfg(target_os = "linux")]

use kk_sandbox::{
    run, run_unchecked, selfcheck, Bind, Exit, Limits, SandboxError, SandboxSpec, Weakening,
};
use std::fs;
use std::net::{TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kk-rsuite-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).expect("scratch");
    d
}

fn py_spec(dir: &Path, code: &str, env: &[(&str, String)]) -> SandboxSpec {
    let script = dir.join("w.py");
    fs::write(&script, code).expect("script");
    let mut spec = SandboxSpec::new("/usr/bin/python3");
    spec.args = vec!["-I".into(), "/tmp/w.py".into()];
    spec.binds.push(Bind {
        host: script,
        inside: "/tmp/w.py".into(),
        writable: false,
    });
    for (k, v) in env {
        spec.env.push((k.to_string(), v.clone()));
    }
    spec.limits.wall = Duration::from_secs(20);
    spec
}

fn out_of(spec: &SandboxSpec) -> String {
    let o = run_unchecked(spec).expect("run");
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

const NET_PROBE: &str = r#"
import os, socket, subprocess
res = {}
port = int(os.environ["P"])
def tcp(host, fam=socket.AF_INET):
    s = None
    try:
        s = socket.socket(fam, socket.SOCK_STREAM); s.settimeout(1.0)
        s.connect((host, port)); return True
    except Exception: return False
    finally:
        if s is not None: s.close()
res["tcp4"] = tcp("127.0.0.1")
res["tcp6"] = tcp("::1", socket.AF_INET6)
res["tcp4mapped"] = tcp("::ffff:127.0.0.1", socket.AF_INET6)
try:
    u = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); u.settimeout(1.0); u.sendto(b"x", ("127.0.0.1", int(os.environ["U"]))); res["udp_sent"] = True
except Exception: res["udp_sent"] = False
try:
    socket.socket(socket.AF_INET, socket.SOCK_RAW, socket.IPPROTO_ICMP); res["raw"] = True
except Exception: res["raw"] = False
try:
    socket.getaddrinfo("example.com", 80); res["dns"] = True
except Exception: res["dns"] = False
for name, cmd in {"curl": ["curl", "-s", "-m", "2", f"http://127.0.0.1:{port}"], "bash_dev_tcp": ["bash", "-c", f"exec 3<>/dev/tcp/127.0.0.1/{port}"]}.items():
    try:
        res[name] = subprocess.run(cmd, capture_output=True, timeout=5).returncode == 0
    except Exception: res[name] = False
for var in ("http_proxy", "HTTP_PROXY", "all_proxy"):
    os.environ[var] = f"http://127.0.0.1:{port}"
try:
    import urllib.request
    urllib.request.urlopen("http://example.com", timeout=2); res["via_proxy_env"] = True
except Exception: res["via_proxy_env"] = False
print(res)
"#;

#[test]
fn r01_to_r06_network_is_closed_while_the_control_run_proves_the_probe_works() {
    let dir = scratch("net");
    let tcp = TcpListener::bind("127.0.0.1:0").expect("listener");
    let udp = UdpSocket::bind("127.0.0.1:0").expect("udp");
    let (p, u) = (
        tcp.local_addr().unwrap().port(),
        udp.local_addr().unwrap().port(),
    );
    let script = dir.join("w.py");
    fs::write(&script, NET_PROBE).expect("script");
    let control = Command::new("python3")
        .arg("-I")
        .arg(&script)
        .env("P", p.to_string())
        .env("U", u.to_string())
        .output()
        .expect("control");
    let control_out = String::from_utf8_lossy(&control.stdout).to_string();
    assert!(
        control_out.contains("'tcp4': True"),
        "control must reach the listener: {control_out}"
    );
    assert!(
        control_out.contains("'udp_sent': True"),
        "control: {control_out}"
    );
    tcp.set_nonblocking(true).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let mut control_conns = 0;
    while tcp.accept().is_ok() {
        control_conns += 1;
    }
    assert!(
        control_conns >= 1,
        "the control run must have connected to the listener"
    );
    // the control's datagram arrives, which proves the listener works; drain it so only the sandbox's could be left
    let mut drain = [0u8; 8];
    udp.set_read_timeout(Some(Duration::from_millis(1000)))
        .unwrap();
    assert!(
        udp.recv_from(&mut drain).is_ok(),
        "the control run's datagram must reach the listener"
    );
    let spec = py_spec(
        &dir,
        NET_PROBE,
        &[("P", p.to_string()), ("U", u.to_string())],
    );
    let sandboxed = out_of(&spec);
    for key in [
        "tcp4",
        "tcp6",
        "tcp4mapped",
        "raw",
        "dns",
        "curl",
        "bash_dev_tcp",
        "via_proxy_env",
    ] {
        assert!(
            sandboxed.contains(&format!("'{key}': False")),
            "{key} not blocked: {sandboxed}"
        );
    }
    let mut buf = [0u8; 8];
    udp.set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    assert!(
        udp.recv_from(&mut buf).is_err(),
        "UDP datagram must not reach the host"
    );
    tcp.set_nonblocking(true).unwrap();
    let seen = tcp.accept();
    assert!(
        seen.is_err(),
        "host listener saw a connection after the sandbox run: {:?}; sandbox output: {sandboxed}",
        seen.as_ref().map(|(_, a)| *a)
    );
}

const FS_PROBE: &str = r#"
import os, json
res = {}
secret = os.environ["SECRET"]
def readable(p):
    try:
        open(p).read(); return True
    except Exception: return False
res["direct"] = readable(secret)
res["home"] = readable(os.environ["HOME_SECRET"])
res["etc_shadow"] = readable("/etc/shadow")
res["proc_root"] = readable("/proc/1/root" + secret)
try:
    os.symlink(secret, "/tmp/link"); res["symlink"] = readable("/tmp/link")
except Exception: res["symlink"] = False
try:
    os.link(secret, "/tmp/hard"); res["hardlink"] = True
except Exception: res["hardlink"] = False
res["dotdot"] = readable("/tmp/../" + secret.lstrip("/"))
res["home_listing"] = sorted(os.listdir("/home")) if os.path.isdir("/home") else []
res["fds"] = sorted(int(n) for n in os.listdir("/proc/self/fd"))
print(json.dumps(res))
"#;

#[test]
fn r09_r11_r12_filesystem_is_an_allowlist_and_only_stdio_is_inherited() {
    let dir = scratch("fs");
    let secret = dir.join("host-secret.txt");
    fs::write(&secret, "TOP SECRET").unwrap();
    let home_secret = PathBuf::from(std::env::var("HOME").unwrap()).join(".kk-rsuite-home-secret");
    fs::write(&home_secret, "HOME SECRET").unwrap();
    let held = TcpListener::bind("127.0.0.1:0").unwrap();
    let _fd_in_parent = std::net::TcpStream::connect(held.local_addr().unwrap()).unwrap();
    let spec = py_spec(
        &dir,
        FS_PROBE,
        &[
            ("SECRET", secret.to_string_lossy().into_owned()),
            ("HOME_SECRET", home_secret.to_string_lossy().into_owned()),
        ],
    );
    let o = out_of(&spec);
    let _ = fs::remove_file(&home_secret);
    for key in [
        "direct",
        "home",
        "etc_shadow",
        "proc_root",
        "symlink",
        "hardlink",
        "dotdot",
    ] {
        assert!(
            o.contains(&format!("\"{key}\": false")),
            "{key} escaped: {o}"
        );
    }
    assert!(
        o.contains("\"home_listing\": []"),
        "home must be empty: {o}"
    );
    assert!(!o.contains("TOP SECRET") && !o.contains("HOME SECRET"));
    let fds: Vec<i32> = o
        .split("\"fds\": [")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_default();
    assert!(
        fds.iter().all(|fd| *fd <= 3),
        "unexpected inherited descriptors {fds:?}: {o}"
    );
}

const PROC_PROBE: &str = r#"
import os, signal, subprocess, json
res = {}
res["pids"] = sorted(int(n) for n in os.listdir("/proc") if n.isdigit())
res["pid_self"] = os.getpid()
try:
    os.kill(1, 0); res["kill_pid1_visible"] = os.getppid() == 0
except Exception: res["kill_pid1_visible"] = False
host = int(os.environ["HOSTPID"])
try:
    os.kill(host, signal.SIGKILL); res["killed_host_pid"] = True
except Exception: res["killed_host_pid"] = False
for name, cmd in {"unshare_user": ["unshare", "-U", "true"], "unshare_net": ["unshare", "-n", "true"], "sudo": ["sudo", "-n", "true"],
                  "mount": ["mount", "-t", "tmpfs", "none", "/mnt"], "chroot": ["chroot", "/", "true"]}.items():
    try:
        res[name] = subprocess.run(cmd, capture_output=True, timeout=5).returncode == 0
    except Exception: res[name] = False
res["caps"] = [l for l in open("/proc/self/status").read().splitlines() if l.startswith("CapEff")]
print(json.dumps(res))
"#;

#[test]
fn r13_r15_host_processes_are_invisible_and_privilege_escalation_is_closed() {
    let dir = scratch("proc");
    let mut victim = Command::new("sleep").arg("120").spawn().expect("victim");
    let spec = py_spec(&dir, PROC_PROBE, &[("HOSTPID", victim.id().to_string())]);
    let o = out_of(&spec);
    assert!(
        victim.try_wait().unwrap().is_none(),
        "victim was killed from inside the sandbox"
    );
    let _ = victim.kill();
    for key in [
        "killed_host_pid",
        "unshare_user",
        "unshare_net",
        "sudo",
        "mount",
        "chroot",
    ] {
        assert!(
            o.contains(&format!("\"{key}\": false")),
            "{key} succeeded: {o}"
        );
    }
    assert!(
        o.contains("CapEff:\\t0000000000000000"),
        "capabilities not dropped: {o}"
    );
    assert!(
        !o.contains(&format!("{}", victim.id())) || !o.contains("\"pids\": ["),
        "host pid visible"
    );
}

fn limited(dir: &Path, code: &str, f: impl FnOnce(&mut Limits)) -> SandboxSpec {
    let mut spec = py_spec(dir, code, &[]);
    f(&mut spec.limits);
    spec
}

#[test]
fn r18_resource_exhaustion_is_bounded() {
    let dir = scratch("limits");
    let forkbomb = "import os\nn = 0\nwhile True:\n    try:\n        if os.fork() == 0:\n            import time; time.sleep(30); os._exit(0)\n        n += 1\n    except OSError:\n        print('fork refused after', n); break\n";
    let s = limited(&dir, forkbomb, |l| l.processes = 30);
    let fb = run_unchecked(&s).unwrap();
    assert!(
        String::from_utf8_lossy(&fb.stdout).contains("fork refused"),
        "fork bomb not bounded: exit {:?}, stdout {:?}, stderr {:?}",
        fb.exit,
        String::from_utf8_lossy(&fb.stdout),
        String::from_utf8_lossy(&fb.stderr)
    );
    let memory = "x = []\ntry:\n    while True: x.append(bytearray(50_000_000))\nexcept MemoryError:\n    print('memory refused')\n";
    let s = limited(&dir, memory, |l| l.memory_bytes = 400 << 20);
    let o = run_unchecked(&s).unwrap();
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("memory refused")
            || matches!(o.exit, Exit::Signal(9) | Exit::Code(137)),
        "memory not bounded: exit {:?}, stdout {:?}, stderr {:?}",
        o.exit,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    let spin = "while True: pass\n";
    let s = limited(&dir, spin, |l| l.cpu_secs = 2);
    let o = run_unchecked(&s).unwrap();
    assert!(
        matches!(o.exit, Exit::Signal(_) | Exit::Code(_)) && o.exit != Exit::Code(0),
        "cpu spin not stopped: {:?}",
        o.exit
    );
    let sleeper = "import time; time.sleep(60)\n";
    let s = limited(&dir, sleeper, |l| l.wall = Duration::from_secs(2));
    let t = std::time::Instant::now();
    assert_eq!(run_unchecked(&s).unwrap().exit, Exit::TimedOut);
    assert!(t.elapsed() < Duration::from_secs(8));
    // the owner's stop ends a run at once, long before its wall time, and a stop that is never asked changes nothing
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let s = limited(&dir, sleeper, |l| {
        l.wall = Duration::from_secs(60);
        l.stop = Some(flag.clone());
    });
    let setter = {
        let flag = flag.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        })
    };
    let t = std::time::Instant::now();
    assert_eq!(run_unchecked(&s).unwrap().exit, Exit::Stopped);
    assert!(
        t.elapsed() >= Duration::from_millis(600) && t.elapsed() < Duration::from_secs(8),
        "{:?}",
        t.elapsed()
    );
    setter.join().unwrap();
    let never = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let s = limited(&dir, "print('fine')\n", |l| l.stop = Some(never));
    assert_eq!(run_unchecked(&s).unwrap().exit, Exit::Code(0));
    let disk = "f = open('/tmp/big', 'wb')\ntry:\n    while True: f.write(b'x' * 1_000_000)\nexcept Exception as e:\n    print('disk refused', type(e).__name__)\n";
    let s = limited(&dir, disk, |l| l.file_bytes = 5_000_000);
    assert!(
        out_of(&s).contains("disk refused") || run_unchecked(&s).unwrap().exit != Exit::Code(0)
    );
    let flood = "import sys\nwhile True: sys.stdout.write('A' * 100000)\n";
    let s = limited(&dir, flood, |l| {
        l.output_bytes = 200_000;
        l.wall = Duration::from_secs(3);
    });
    let o = run_unchecked(&s).unwrap();
    assert!(
        o.stdout.len() <= 200_000 && o.truncated,
        "output not capped: {}",
        o.stdout.len()
    );
}

#[test]
fn r21_the_launcher_proves_its_profile_and_refuses_when_it_is_weakened() {
    assert!(
        selfcheck::verify_profile().is_ok(),
        "healthy profile must pass its own probes"
    );
    let weak_net = selfcheck::probe(Some(Weakening::ShareNetwork)).unwrap();
    assert!(
        !weak_net.is_empty(),
        "shared network must be detected by the probe"
    );
    let weak_userns = selfcheck::probe(Some(Weakening::AllowNestedUserNamespaces)).unwrap();
    assert!(
        !weak_userns.is_empty(),
        "nested user namespaces must be detected by the probe"
    );
    let dir = scratch("selfcheck");
    let spec = py_spec(&dir, "print('hello')", &[]);
    let o = run(&spec).expect("healthy launcher runs the worker");
    assert!(String::from_utf8_lossy(&o.stdout).contains("hello"));
    let _: Option<SandboxError> = None;
}

#[test]
fn the_parent_environment_does_not_reach_the_worker() {
    let dir = scratch("env");
    std::env::set_var("KK_PARENT_SECRET", "must-not-leak");
    std::env::set_var("NEAR_AI_API_KEY", "must-not-leak-either");
    let spec = py_spec(
        &dir,
        "import os; print(sorted(os.environ))",
        &[("ALLOWED", "1".into())],
    );
    let o = out_of(&spec);
    assert!(
        !o.contains("KK_PARENT_SECRET") && !o.contains("NEAR_AI_API_KEY"),
        "parent environment leaked: {o}"
    );
    assert!(o.contains("ALLOWED"), "declared variables must arrive: {o}");
}

const SOCKET_PROBE: &str = r#"
import ctypes, json, os, socket
libc = ctypes.CDLL(None, use_errno=True)
res = {}
def make(name, family, kind):
    try:
        s = socket.socket(family, kind)
        res[name] = "created"
        return s
    except OSError as e:
        res[name] = errno_name(e.errno)
def errno_name(n):
    return {1: "EPERM", 13: "EACCES", 97: "EAFNOSUPPORT"}.get(n, str(n))
v = make("vsock", 40, socket.SOCK_STREAM)
if v is not None:
    v.settimeout(3)
    try:
        v.connect((2, 22)); res["vsock_connect"] = "CONNECTED"
    except OSError as e:
        res["vsock_connect"] = "reached-host:" + errno_name(e.errno)
make("inet", socket.AF_INET, socket.SOCK_STREAM)
make("inet6", socket.AF_INET6, socket.SOCK_DGRAM)
make("netlink", 16, socket.SOCK_DGRAM)
make("packet", 17, socket.SOCK_RAW)
make("unix", socket.AF_UNIX, socket.SOCK_STREAM)
try:
    a, b = socket.socketpair(); a.send(b"x"); res["socketpair"] = b.recv(1) == b"x"
except OSError as e:
    res["socketpair"] = errno_name(e.errno)
r = libc.syscall(425, 1, ctypes.create_string_buffer(120))
res["io_uring_setup"] = "created" if r >= 0 else errno_name(ctypes.get_errno())
libc.ptrace(0xFFFF, 0, 0, 0)
res["ptrace"] = errno_name(ctypes.get_errno())
print(json.dumps(res, sort_keys=True))
"#;

fn socket_report(weaken: Option<Weakening>) -> serde_json::Value {
    let dir = scratch("sockets");
    let mut spec = py_spec(&dir, SOCKET_PROBE, &[]);
    spec.weaken_for_self_test = weaken;
    let text = out_of(&spec);
    serde_json::from_str(text.lines().last().unwrap_or_default())
        .unwrap_or_else(|e| panic!("probe output was not JSON ({e}): {text}"))
}

#[test]
fn r30_vm_sockets_and_every_non_unix_family_are_refused_while_unix_sockets_still_work() {
    let on = socket_report(None);
    for k in ["vsock", "inet", "inet6", "netlink", "packet"] {
        assert_eq!(on[k], "EPERM", "{k}: {on}");
    }
    assert!(
        on.get("vsock_connect").is_none(),
        "no vsock socket, so no connection: {on}"
    );
    assert_eq!(on["unix"], "created", "{on}");
    assert_eq!(on["socketpair"], true, "{on}");
    assert_eq!(on["io_uring_setup"], "EPERM", "{on}");
    assert_eq!(on["ptrace"], "EPERM", "{on}");
}

#[test]
fn r31_without_the_filter_the_same_code_reaches_the_host_over_a_vm_socket() {
    // the control that makes r30 mean something: the hole was real, in this very sandbox profile
    let off = socket_report(Some(Weakening::NoSyscallFilter));
    assert_eq!(off["vsock"], "created", "{off}");
    let connect = off["vsock_connect"].as_str().unwrap_or_default();
    assert!(
        connect.starts_with("reached-host:") || connect == "CONNECTED",
        "{off}"
    );
    assert_ne!(off["ptrace"], "EPERM", "{off}");
}

#[test]
fn r32_the_profile_check_notices_a_missing_filter_and_still_passes_when_it_is_installed() {
    assert!(selfcheck::verify_profile().is_ok());
    let weak = selfcheck::probe(Some(Weakening::NoSyscallFilter)).unwrap();
    assert!(
        weak.iter()
            .any(|f| f.starts_with("probe vsock_socket_creatable was not confirmed blocked")),
        "{weak:?}"
    );
    assert!(
        weak.iter()
            .any(|f| f.starts_with("probe ptrace_refused_by_filter was not confirmed")),
        "{weak:?}"
    );
}

#[test]
fn r33_ordinary_python_work_is_unaffected_by_the_filter() {
    let dir = scratch("ordinary-python");
    let code = r#"
import concurrent.futures, json, multiprocessing, os, subprocess, sys, threading, zlib, hashlib
with concurrent.futures.ThreadPoolExecutor(4) as ex:
    total = sum(ex.map(lambda n: n * n, range(100)))
out = subprocess.run(["/usr/bin/env", "echo", "child"], capture_output=True, text=True).stdout.strip()
ctx = multiprocessing.get_context("fork")
with ctx.Pool(2) as pool:
    squares = pool.map(abs, [-1, -2, -3])
open("/tmp/x", "w").write("data")
print(json.dumps({"total": total, "child": out, "squares": squares, "hash": hashlib.sha256(b"a").hexdigest()[:8], "z": len(zlib.compress(b"a" * 1000))}))
"#;
    let spec = py_spec(&dir, code, &[]);
    let text = out_of(&spec);
    assert!(
        text.contains("\"total\": 328350")
            && text.contains("\"child\": \"child\"")
            && text.contains("[1, 2, 3]"),
        "{text}"
    );
}

#[test]
fn r34_node_runs_normally_under_the_filter() {
    if !std::path::Path::new("/usr/bin/node").exists() {
        eprintln!("skipped: no /usr/bin/node in this machine");
        return;
    }
    let dir = scratch("ordinary-node");
    let script = dir.join("w.mjs");
    fs::write(
        &script,
        r#"
import { spawnSync } from "node:child_process";
import { Worker, isMainThread, parentPort } from "node:worker_threads";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
if (isMainThread) {
  const child = spawnSync("/usr/bin/env", ["echo", "child"], { encoding: "utf8" }).stdout.trim();
  writeFileSync("/tmp/x", "data");
  const w = new Worker(new URL(import.meta.url));
  w.on("message", async (m) => {
    await new Promise((r) => setTimeout(r, 10));
    console.log(JSON.stringify({ child, file: readFileSync("/tmp/x", "utf8"), worker: m, hash: createHash("sha256").update("a").digest("hex").slice(0, 8) }));
  });
} else {
  parentPort.postMessage(6 * 7);
}
"#,
    )
    .unwrap();
    let mut spec = SandboxSpec::new("/usr/bin/node");
    spec.args = vec!["/tmp/w.mjs".into()];
    spec.binds.push(Bind {
        host: script,
        inside: "/tmp/w.mjs".into(),
        writable: false,
    });
    spec.limits.wall = Duration::from_secs(20);
    let text = out_of(&spec);
    assert!(
        text.contains("\"child\":\"child\"")
            && text.contains("\"worker\":42")
            && text.contains("\"file\":\"data\""),
        "{text}"
    );
}

#[test]
fn r35_the_filter_file_never_outlives_the_launch() {
    let dir = scratch("filterfile");
    let spec = py_spec(&dir, "print('hi')", &[]);
    let o = run_unchecked(&spec).expect("run");
    assert!(String::from_utf8_lossy(&o.stdout).contains("hi"));
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap();
    let mine = format!("kk-seccomp-{}-", std::process::id());
    let left: Vec<String> = fs::read_dir(runtime)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(&mine))
        .collect();
    assert!(left.is_empty(), "left behind: {left:?}");
}

/// Started by `r13` under a shell that holds a descriptor open without close-on-exec, as a leaky parent would.
#[test]
#[ignore = "run by r13 with a stray descriptor held open"]
fn r13_inner_lists_the_descriptors_the_sandboxed_program_sees() {
    if std::env::var("KK_R13_INNER").is_err() {
        return;
    }
    let dir = scratch("fd-inner");
    let spec = py_spec(
        &dir,
        "import os, json\nprint(json.dumps({'fds': sorted(int(n) for n in os.listdir('/proc/self/fd'))}))\n",
        &[],
    );
    // the control: this process really does hold the stray descriptor the shell opened
    println!("R13HELD {}", Path::new("/proc/self/fd/7").exists());
    println!("R13 {}", out_of(&spec));
}

#[test]
fn r13_a_descriptor_leaked_by_whatever_started_this_program_does_not_reach_the_sandbox() {
    // this test binary again, started by a shell that holds /etc/hostname open as descriptor 7 with no close-on-exec
    let exe = std::env::current_exe().unwrap();
    let o = Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"exec 7</etc/hostname; exec "$@""#)
        .arg("r13")
        .arg(&exe)
        .args([
            "--exact",
            "r13_inner_lists_the_descriptors_the_sandboxed_program_sees",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("KK_R13_INNER", "1")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    let line = out.lines().find(|l| l.contains("R13 ")).unwrap_or_else(|| {
        panic!(
            "the inner run printed nothing: {out} {}",
            String::from_utf8_lossy(&o.stderr)
        )
    });
    let fds: Vec<i32> = line
        .split("\"fds\": [")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_default();
    assert!(
        out.contains("R13HELD true"),
        "the inner run did not hold descriptor 7, so the test would show nothing: {out}"
    );
    assert!(!fds.is_empty(), "no descriptor list: {line}");
    assert!(
        fds.iter().all(|fd| *fd <= 3),
        "a descriptor the parent leaked reached the sandboxed program: {fds:?}"
    );
}
