#!/usr/bin/env python3
"""The full-screen terminal workspace ends when its terminal goes away, with or without a hang-up signal, and takes the
service it started with it. Found in real use: a terminal closed under a detached workspace left it spinning on a whole
processor for ever, with its service still running."""
import fcntl
import os
import pty
import select
import shutil
import struct
import subprocess
import sys
import termios
import time
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/tui-gone-tmp"
kaku = repo / "target/debug/kaku"
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


def service_running(data):
    return bool(subprocess.run(["pgrep", "-f", "serve --data-dir " + str(data)], capture_output=True, text=True).stdout.strip())


def run(tag, controlling):
    data = work / tag
    subprocess.run(["pkill", "-f", "serve --data-dir " + str(data)], capture_output=True)
    shutil.rmtree(data, ignore_errors=True)
    (work / "root").mkdir(parents=True, exist_ok=True)
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))

    def pre():
        if controlling:
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

    p = subprocess.Popen(
        [str(kaku), "tui", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")],
        stdin=slave, stdout=slave, stderr=slave, close_fds=True, preexec_fn=pre, env=dict(os.environ, TERM="xterm-256color"),
    )
    os.close(slave)
    seen = b""
    end = time.time() + 60
    while time.time() < end and not service_running(data):
        if select.select([master], [], [], 0.2)[0]:
            seen += os.read(master, 65536)
    time.sleep(2)
    while select.select([master], [], [], 0.1)[0]:
        seen += os.read(master, 65536)
    started = service_running(data) and p.poll() is None
    check(f"{tag}: the workspace is up with the service it started", started, seen[-200:].decode(errors="replace"))
    # the terminal's window closes
    os.close(master)
    gone_by = None
    for i in range(8):
        time.sleep(1)
        if p.poll() is not None:
            gone_by = i + 1
            break
    cpu = "" if gone_by else subprocess.run(["ps", "-o", "pcpu=", "-p", str(p.pid)], capture_output=True, text=True).stdout.strip()
    check(f"{tag}: the workspace ends within a few seconds of its terminal going away", gone_by is not None, f"still running after 8 s at {cpu}% of a processor")
    time.sleep(1.5)
    check(f"{tag}: and the service it started is stopped too", not service_running(data))
    if p.poll() is None:
        p.kill()
        p.wait()
    subprocess.run(["pkill", "-f", "serve --data-dir " + str(data)], capture_output=True)


try:
    run("without-signal", False)
    run("with-hangup", True)
finally:
    subprocess.run(["pkill", "-f", "serve --data-dir " + str(work)], capture_output=True)

bad = results.count(False)
print(f"\n{len(results) - bad}/{len(results)} checks passed" if bad == 0 else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
