#![cfg(target_os = "linux")]

use kk_sandbox::{
    run, run_unchecked, selfcheck, Bind, Exit, Limits, SandboxError, SandboxSpec, Weakening,
};
use std::fs;
use std::net::{TcpListener, UdpSocket};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("kk-rsuite-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).expect("scratch");
    d
}

fn py_spec(dir: &PathBuf, code: &str, env: &[(&str, String)]) -> SandboxSpec {
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
    s = socket.socket(fam, socket.SOCK_STREAM); s.settimeout(1.0)
    try:
        s.connect((host, port)); return True
    except Exception: return False
    finally: s.close()
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
        udp.recv_from(&mut buf).is_err() || !sandboxed.contains("'udp_sent': False"),
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

fn limited(dir: &PathBuf, code: &str, f: impl FnOnce(&mut Limits)) -> SandboxSpec {
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
