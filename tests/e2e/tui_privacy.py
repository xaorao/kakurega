#!/usr/bin/env python3
"""The terminal's two privacy settings as a person types them, in a real terminal with the scripted test model: names to hide
(/hide) and how far a project's material may go (/private).
  python3 tests/e2e/tui_privacy.py      screens in target/tui-privacy/
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
work = repo / "target/tui-privacy"
subprocess.run(["pkill", "-f", "kaku serve --data-dir " + str(work)], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root/acme").mkdir(parents=True)
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
env = dict(os.environ, TERM="xterm-256color", LANG="en_US.UTF-8")
env.pop("NO_COLOR", None)
proc = subprocess.Popen([str(repo / "target/debug/kaku"), "tui", "--data-dir", str(work / "data"), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json")],
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
    send("/hide add Asha Rao\r")
    check("a name is added to the list of names to hide", wait_for(r"Done\. 1 name is on the list"))
    send("/hide\r")
    check("the list shows it and says it is not in use while the model is on this computer", wait_for(r"Not in use now") and "Asha Rao" in screen.text())
    (work / "1-hide.txt").write_text(screen.text())
    # a made-up key, put together here so no key-shaped text sits in this file
    send("/hide add " + "sk_li" + "ve_abcdefghijklmnop0123" + "\r")
    check("a secret is refused as a name, in words", wait_for(r"looks like a secret, not a name"))
    send("/hide remove Asha Rao\r")
    check("and taken off again", wait_for(r"Done\. 0 names are on the list"))
    send("/private acme here\r")
    check("a project is kept on this computer", wait_for(r"Done\. acme goes to a model on this computer only"))
    send("/private\r")
    check("the projects are listed with how far each may go", wait_for(r"acme: only a model on this computer"))
    (work / "2-private.txt").write_text(screen.text())
    send("/private acme nowhere\r")
    check("a word that is not a level shows the usage", wait_for(r"usage: /private \[PROJECT here\|confidential\|any\]"))
    send("/quit\r")
    pump(3)
finally:
    if proc.poll() is None:
        proc.kill()
print("all terminal privacy checks passed" if not failed else f"{failed} check(s) failed")
sys.exit(1 if failed else 0)
