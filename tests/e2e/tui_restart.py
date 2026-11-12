#!/usr/bin/env python3
"""The terminal workspace (plain-line mode, through pipes) keeps working when the service is stopped and started again:
while it is down the message box is not left waiting, and the next message after the restart is answered."""
import os
import re
import select
import shutil
import subprocess
import sys
import time
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/tui-restart-tmp"
subprocess.run(["pkill", "-f", "kaku serve --data-dir " + str(work)], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
data = work / "data"
kaku = repo / "target/debug/kaku"
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


def serve():
    p = subprocess.Popen([str(kaku), "serve", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    end = time.time() + 30
    while time.time() < end:
        try:
            if f'"pid":{p.pid}' in (data / "admin.json").read_text().replace(" ", ""):
                return p
        except OSError:
            pass
        time.sleep(0.1)
    raise SystemExit("the service did not start")


service = serve()
first_port = (data / "port").read_text().strip()
proc = subprocess.Popen([str(kaku), "tui", "--linear", "--data-dir", str(data)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=dict(os.environ, TERM="dumb"))
out = b""


def pump(seconds):
    global out
    end = time.time() + seconds
    while time.time() < end:
        r, _, _ = select.select([proc.stdout], [], [], 0.1)
        if r:
            chunk = os.read(proc.stdout.fileno(), 65536)
            if not chunk:
                return
            out += chunk


def wait_for(pattern, timeout=60, start=0):
    end = time.time() + timeout
    while time.time() < end:
        m = re.search(pattern, out.decode("utf8", "replace")[start:])
        if m:
            return m
        pump(0.2)
    return None


def send(line):
    proc.stdin.write((line + "\n").encode())
    proc.stdin.flush()


try:
    check("the workspace opens on the running service", wait_for(r"\[Status\] ", 60) is not None)
    send("What is the plan for today?")
    check("a first message is answered", wait_for(r"\[Assistant\]|Assistant", 60) is not None, out.decode("utf8", "replace")[-400:])
    service.kill()
    service.wait()
    mark = len(out.decode("utf8", "replace"))
    send("Are you still there?")
    check("with the service stopped the owner is told it could not be reached", wait_for(r"Could not reach the service", 30, mark) is not None, out.decode("utf8", "replace")[mark:][-400:])
    service = serve()
    check("the service comes back on the same address", (data / "port").read_text().strip() == first_port, f"{first_port} -> {(data / 'port').read_text().strip()}")
    mark = len(out.decode("utf8", "replace"))
    send("And now, after the restart?")
    answered = wait_for(r"\[Assistant\]|Assistant", 60, mark) is not None
    tail = out.decode("utf8", "replace")[mark:]
    check("after the restart the next message is answered, with nothing left waiting", answered and "Queued" not in tail and "Could not reach" not in tail, tail[-500:])
finally:
    try:
        send("/quit")
        proc.wait(timeout=10)
    except Exception:
        proc.kill()
    service.kill()

print(f"{sum(results)}/{len(results)} restart checks passed")
sys.exit(0 if all(results) else 1)
