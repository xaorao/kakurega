#!/usr/bin/env python3
"""A review of a change as a person types it in the terminal workspace, in a real terminal with the scripted test model:
/review FOLDER for WHO against BASE, then the findings.
  python3 tests/e2e/tui_change.py      screens in target/tui-change/
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
# the small terminal emulator the scripted terminal test uses, taken from its source so there is one of it
source = (repo / "tests/e2e/tui_pty.py").read_text()
exec(source[source.index("import unicodedata"):source.index("import codecs")])
work = repo / "target/tui-change"
subprocess.run(["pkill", "-f", "kaku serve --data-dir " + str(work)], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
proj = work / "root/payments-service"
(work / "root").mkdir(parents=True)
shutil.copytree(repo / "fixtures/synthetic-repo", proj)
os.remove(proj / "_env")


def git(*args):
    e = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@example.com", GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@example.com", GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_SYSTEM="/dev/null")
    p = subprocess.run(["/usr/bin/git", "-C", str(proj), *args], capture_output=True, text=True, env=e)
    assert p.returncode == 0, p.stderr
    return p.stdout.strip()


session = proj / "payments/auth/session.py"
broken = session.read_text()
fixed = broken.replace('    # Clears the browser cookie on the client side; the server keeps the entry.\n    cookie_jar = {"session": token}\n    cookie_jar.pop("session", None)\n', "    ACTIVE_TOKENS.pop(token, None)\n")
assert fixed != broken
session.write_text(fixed)
git("init", "-q", "-b", "main")
git("add", "-A")
git("commit", "-q", "-m", "payments service")
git("checkout", "-q", "-b", "faster-logout")
session.write_text(broken)
git("commit", "-q", "-am", "logout: skip the table")
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
env = dict(os.environ, TERM="xterm-256color", LANG="en_US.UTF-8")
env.pop("NO_COLOR", None)
proc = subprocess.Popen([str(repo / "target/debug/kaku"), "tui", "--data-dir", str(work / "data"), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/change-script.json")],
                        stdin=slave, stdout=slave, stderr=slave, env=env, close_fds=True)
os.close(slave)
screen = Screen(40, 120)
decoder = codecs.getincrementaldecoder("utf-8")("replace")
failed = 0


def pump(seconds):
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
            screen.feed(decoder.decode(chunk))


def wait_for(pattern, timeout=40):
    end = time.time() + timeout
    while time.time() < end:
        pump(0.3)
        if re.search(pattern, screen.text()):
            return True
        if proc.poll() is not None:
            break
    return False


def send(text):
    os.write(master, text.encode())


def check(name, ok):
    global failed
    print(("PASS  " if ok else "FAIL  ") + name, flush=True)
    if not ok:
        failed += 1
        print(screen.text()[-900:].replace("\n", " | "), flush=True)


try:
    check("the workspace opens", wait_for(r"ready", 90))
    send("/review payments-service for acme-audit against no-such-branch\r")
    check("a base that does not exist is refused in words", wait_for(r"no branch, tag or commit of that name"))
    send("/review payments-service for acme-audit against main\r")
    check("the review of the change starts and runs to ready", wait_for(r"job-1.*(ready|finished)", 240))
    (work / "1-review.txt").write_text(screen.text())
    send("/findings\r")
    check("the finding in the change is confirmed", wait_for(r"Token still valid after logout"))
    check("the finding in untouched code says it is outside the change", wait_for(r"did not\s+touch"))
    (work / "2-findings.txt").write_text(screen.text())
    send("/quit\r")
    pump(3)
finally:
    if proc.poll() is None:
        proc.kill()
print("all terminal change-review checks passed" if not failed else f"{failed} check(s) failed")
sys.exit(1 if failed else 0)
