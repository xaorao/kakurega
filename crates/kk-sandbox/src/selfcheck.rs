use crate::{run_unchecked, Bind, SandboxError, SandboxSpec, Weakening};
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;

const PROBE: &str = r#"
import json, os, socket, subprocess, sys
r = {}
port = int(os.environ["KK_PROBE_PORT"])
def tcp(host, family=socket.AF_INET):
    s = None
    try:
        s = socket.socket(family, socket.SOCK_STREAM); s.settimeout(1.0)
        s.connect((host, port)); return True
    except Exception:
        return False
    finally:
        if s is not None:
            s.close()
r["tcp_loopback_reachable"] = tcp("127.0.0.1")
try:
    r["tcp6_loopback_reachable"] = tcp("::1", socket.AF_INET6)
except Exception:
    r["tcp6_loopback_reachable"] = False
try:
    socket.getaddrinfo("example.com", 80); r["dns_resolves"] = True
except Exception:
    r["dns_resolves"] = False
try:
    r["secret_readable"] = open(os.environ["KK_PROBE_SECRET"]).read() == "probe-secret"
except Exception:
    r["secret_readable"] = False
p = subprocess.run(["/usr/bin/unshare", "-U", "true"], capture_output=True)
r["nested_userns_allowed"] = (p.returncode == 0)
r["host_pids_visible"] = sum(1 for n in os.listdir("/proc") if n.isdigit()) > 8
import ctypes
libc = ctypes.CDLL(None, use_errno=True)
def creatable(family, kind):
    try:
        socket.socket(family, kind).close(); return True
    except OSError:
        return False
r["vsock_socket_creatable"] = creatable(40, socket.SOCK_STREAM)
r["inet_socket_creatable"] = creatable(socket.AF_INET, socket.SOCK_STREAM)
r["netlink_socket_creatable"] = creatable(16, socket.SOCK_DGRAM)
r["io_uring_usable"] = libc.syscall(425, 1, ctypes.create_string_buffer(120)) >= 0
libc.ptrace(0xFFFF, 0, 0, 0)
r["ptrace_refused_by_filter"] = (ctypes.get_errno() == 1)
try:
    a, b = socket.socketpair(); a.close(); b.close(); r["unix_socketpair_works"] = True
except OSError:
    r["unix_socketpair_works"] = False
print(json.dumps(r))
"#;

pub fn verify_profile() -> Result<(), SandboxError> {
    let failures = probe(None)?;
    if failures.is_empty() {
        Ok(())
    } else {
        Err(SandboxError::SelfCheckFailed(failures))
    }
}

pub fn probe(weaken: Option<Weakening>) -> Result<Vec<String>, SandboxError> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| SandboxError::Spawn(e.to_string()))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| SandboxError::Spawn(e.to_string()))?;
    let port = listener
        .local_addr()
        .map_err(|e| SandboxError::Spawn(e.to_string()))?
        .port();
    let dir: PathBuf =
        std::env::temp_dir().join(format!("kk-selfcheck-{}-{port}", std::process::id()));
    fs::create_dir_all(&dir).map_err(|e| SandboxError::Spawn(e.to_string()))?;
    let secret = dir.join("secret.txt");
    fs::write(&secret, "probe-secret").map_err(|e| SandboxError::Spawn(e.to_string()))?;
    let script = dir.join("probe.py");
    fs::write(&script, PROBE).map_err(|e| SandboxError::Spawn(e.to_string()))?;
    let mut spec = SandboxSpec::new("/usr/bin/python3");
    spec.args = vec!["-I".into(), "/tmp/probe.py".into()];
    spec.env = vec![
        ("KK_PROBE_PORT".into(), port.to_string()),
        (
            "KK_PROBE_SECRET".into(),
            secret.to_string_lossy().into_owned(),
        ),
    ];
    spec.binds.push(Bind {
        host: script,
        inside: "/tmp/probe.py".into(),
        writable: false,
    });
    spec.weaken_for_self_test = weaken;
    spec.limits.wall = std::time::Duration::from_secs(15);
    let outcome = run_unchecked(&spec);
    let _ = fs::remove_dir_all(&dir);
    let outcome = outcome?;
    let text = String::from_utf8_lossy(&outcome.stdout);
    let mut failures = Vec::new();
    for key in [
        "tcp_loopback_reachable",
        "tcp6_loopback_reachable",
        "dns_resolves",
        "secret_readable",
        "nested_userns_allowed",
        "host_pids_visible",
        "vsock_socket_creatable",
        "inet_socket_creatable",
        "netlink_socket_creatable",
        "io_uring_usable",
    ] {
        let needle = format!("\"{key}\": false");
        if !text.contains(&needle) {
            failures.push(format!(
                "probe {key} was not confirmed blocked (output: {})",
                text.trim()
            ));
        }
    }
    // the controls: what the filter must still allow, and the refusal that proves the filter itself is installed
    for key in ["unix_socketpair_works", "ptrace_refused_by_filter"] {
        let needle = format!("\"{key}\": true");
        if !text.contains(&needle) {
            failures.push(format!(
                "probe {key} was not confirmed (output: {})",
                text.trim()
            ));
        }
    }
    Ok(failures)
}
