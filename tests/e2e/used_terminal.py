#!/usr/bin/env python3
"""The terminal workspace as a person uses it, with a real local model and the real sandbox (run by hand: it takes
minutes and the model's words differ run to run). `kaku` is started in a real pseudo-terminal; a question is asked, the
model and route are named, a review is run on the sample project, read, approved and delivered, and the terminal is left
clean. The screen is saved as text at each step for reading.

    python3 tests/e2e/used_terminal.py [MODEL]        default qwen2.5-coder:7b; writes target/used-terminal/*.txt
"""
import codecs
import fcntl
import os
import pty
import re
import select
import shutil
import struct
import subprocess
import sys
import termios
import time
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
model = sys.argv[1] if len(sys.argv) > 1 else "qwen2.5-coder:7b"
work = repo / "target/used-terminal"
subprocess.run(["pkill", "-f", "kaku serve --data-dir " + str(work)], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
shutil.copytree(repo / "fixtures/synthetic-repo", work / "root/payments-service")
os.rename(work / "root/payments-service/_env", work / "root/payments-service/.env")
data = work / "data"
kaku = repo / "target/debug/kaku"

# the small terminal emulator the scripted terminal test uses, taken from its source so there is one of it
source = (repo / "tests/e2e/tui_pty.py").read_text()
exec(source[source.index("import unicodedata"):source.index("import codecs")])

results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (("  " + detail) if detail and not ok else ""), flush=True)


screen = Screen(40, 120)
decoder = codecs.getincrementaldecoder("utf8")(errors="replace")
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
env = dict(os.environ, TERM="xterm-256color", LANG="en_US.UTF-8")
env.pop("NO_COLOR", None)
proc = subprocess.Popen([str(kaku), "tui", "--data-dir", str(data), "--root", str(work / "root"), "--model", model, "--ui-dir", str(repo / "apps/web/dist")],
                        stdin=slave, stdout=slave, stderr=slave, env=env, close_fds=True)
os.close(slave)
raw = b""
t0 = time.time()


def pump(seconds):
    global raw
    end = time.time() + seconds
    while time.time() < end:
        r, _, _ = select.select([master], [], [], 0.1)
        if r:
            try:
                chunk = os.read(master, 65536)
            except OSError:
                return
            if not chunk:
                return
            raw += chunk
            screen.feed(decoder.decode(chunk))


def wait_for(pattern, timeout=60):
    end = time.time() + timeout
    while time.time() < end:
        pump(0.3)
        m = re.search(pattern, screen.text())
        if m:
            return m
        if proc.poll() is not None:
            break
    return None


def send(text):
    os.write(master, text.encode())


def save(name):
    (work / f"{name}.txt").write_text(screen.text())


try:
    check("the workspace opens and says the service is ready", wait_for(r"kakurega", 60) is not None and wait_for(r"ready", 90) is not None)
    save("1-open")
    send("/model\r")
    check("/model names the real model in use", wait_for(re.escape(model), 30) is not None, screen.text()[-400:])
    send("In one short sentence: what is a session token?\r")
    m = wait_for(r"answer: about \d+ tokens in [\d.]+ (ms|s)", 240)
    check("a question is answered by the model on this computer, with how long it took", m is not None and re.search(r"token", screen.text(), re.I) is not None, screen.text()[-500:].replace("\n", " | "))
    print(f"chat answered at {round(time.time() - t0)} s: {m.group(0) if m else ''}", flush=True)
    save("2-answer")
    send("/where\r")
    where = wait_for(r"Where your data goes", 30) is not None and wait_for(r"Left this computer \(from the signed log\): nothing", 30) is not None
    text = screen.text()
    check("/where says the model is on this computer, lists each door, and that nothing has left", where and "on this computer. Your questions do not leave it." in text and "Web search: off" in text, text[-700:].replace("\n", " | "))
    save("3-where")
    send("/review payments-service for Acme audit\r")
    started = time.time()
    check("a review starts from a folder name", wait_for(r"Review job-\d+ started", 60) is not None, screen.text()[-400:].replace("\n", " | "))
    done = wait_for(r"finished: ready", 900)
    print(f"review finished after {round(time.time() - started)} s", flush=True)
    save("4-review")
    check("the review of the sample project finishes ready, with no key pressed", done is not None, screen.text()[-600:].replace("\n", " | "))
    send("/findings\r")
    pump(3)
    save("5-findings")
    check("/findings lists what the sandbox reproduced", re.search(r"payments/auth/\w+\.py", screen.text()) is not None, screen.text()[-600:].replace("\n", " | "))
    send("/approve\r")
    m = wait_for(r"first 12 characters of the digest \(([0-9a-f]{12})\)", 60)
    save("6-approval")
    check("the approval view names the digest's first characters to type", m is not None, screen.text()[-400:].replace("\n", " | "))
    if m:
        send(m.group(1) + "\r")
        check("typing them approves", wait_for(r"Approved\.", 60) is not None)
        send("/deliver\r")
        check("delivery produces a capsule file", wait_for(r"Delivered", 60) is not None and ".kkg" in screen.text(), screen.text()[-400:].replace("\n", " | "))
        save("7-delivered")
        send("/where\r")
        check("/where then counts one release as having left, and nothing else", wait_for(r"releases 1\.", 30) is not None and re.search(r"questions to a model elsewhere 0", screen.text()) is not None, screen.text()[-500:].replace("\n", " | "))
    send("/quit\r")
    pump(3)
    if proc.poll() is None:
        send("\x03")
        pump(2)
    check("the workspace ends and the terminal is given back clean", proc.poll() is not None and b"\x1b[?1049l" in raw, f"exit={proc.poll()}")
finally:
    if proc.poll() is None:
        proc.kill()
    subprocess.run(["pkill", "-f", "kaku serve --data-dir " + str(data)], capture_output=True)

print(f"\n{sum(results)}/{len(results)} checks passed in {round(time.time() - t0)} s")
sys.exit(0 if all(results) else 1)
