#!/usr/bin/env python3
"""Drives the real `kaku` binary in a real pseudo-terminal: boot, a full review, approval, delivery, quit, clean terminal."""
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
work = repo / "target/tui-pty-tmp"
subprocess.run(["pkill", "-f", "kaku serve --data-dir " + str(Path(__file__).resolve().parent.parent.parent / "target/tui-pty-tmp")], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
shutil.copytree(repo / "fixtures/synthetic-repo", work / "root/payments-service")
os.rename(work / "root/payments-service/_env", work / "root/payments-service/.env")
data = work / "data"
kaku = repo / "target/debug/kaku"

results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (("  " + detail) if detail else ""))


import unicodedata


class Screen:
    """A small terminal emulator: enough of VT100 to read what a ratatui program put on the screen."""

    def __init__(self, rows, cols):
        self.rows, self.cols = rows, cols
        self.grid = [[" "] * cols for _ in range(rows)]
        self.r = self.c = 0
        self.buf = ""

    def feed(self, text):
        self.buf += text
        i = 0
        b = self.buf
        while i < len(b):
            ch = b[i]
            if ch == "\x1b":
                if i + 1 >= len(b):
                    break
                nxt = b[i + 1]
                if nxt == "[":
                    m = re.match(r"\x1b\[([0-9;?]*)([ -/]*)([@-~])", b[i:])
                    if not m:
                        break
                    self.csi(m.group(1), m.group(3))
                    i += m.end()
                    continue
                if nxt == "]":
                    m = re.match(r"\x1b\][^\x07\x1b]*(\x07|\x1b\\)", b[i:])
                    if not m:
                        break
                    i += m.end()
                    continue
                i += 2 if nxt in "()=>78" and False else 2
                if nxt in "()":
                    i += 1
                continue
            i += 1
            if ch == "\r":
                self.c = 0
            elif ch == "\n":
                self.r = min(self.r + 1, self.rows - 1)
            elif ch == "\b":
                self.c = max(0, self.c - 1)
            elif ch >= " ":
                w = 2 if unicodedata.east_asian_width(ch) in "WF" else 1
                if self.c + w > self.cols:
                    self.c = 0
                    self.r = min(self.r + 1, self.rows - 1)
                self.grid[self.r][self.c] = ch
                if w == 2 and self.c + 1 < self.cols:
                    self.grid[self.r][self.c + 1] = ""
                self.c += w
        self.buf = b[i:]

    def csi(self, params, final):
        nums = [int(x) if x.isdigit() else 0 for x in params.replace("?", "").split(";")] if params.replace("?", "") else []
        n = nums[0] if nums and nums[0] else 1
        if params.startswith("?"):
            return
        if final in "Hf":
            self.r = min(max((nums[0] if nums and nums[0] else 1) - 1, 0), self.rows - 1)
            self.c = min(max((nums[1] if len(nums) > 1 and nums[1] else 1) - 1, 0), self.cols - 1)
        elif final == "A":
            self.r = max(0, self.r - n)
        elif final == "B":
            self.r = min(self.rows - 1, self.r + n)
        elif final == "C":
            self.c = min(self.cols - 1, self.c + n)
        elif final == "D":
            self.c = max(0, self.c - n)
        elif final == "G":
            self.c = min(max(n - 1, 0), self.cols - 1)
        elif final == "J":
            mode = nums[0] if nums else 0
            if mode == 2 or mode == 3:
                self.grid = [[" "] * self.cols for _ in range(self.rows)]
            elif mode == 0:
                for c in range(self.c, self.cols):
                    self.grid[self.r][c] = " "
                for r in range(self.r + 1, self.rows):
                    self.grid[r] = [" "] * self.cols
            elif mode == 1:
                for c in range(0, self.c + 1):
                    self.grid[self.r][c] = " "
                for r in range(0, self.r):
                    self.grid[r] = [" "] * self.cols
        elif final == "K":
            mode = nums[0] if nums else 0
            rng = range(self.c, self.cols) if mode == 0 else range(0, self.c + 1) if mode == 1 else range(self.cols)
            for c in rng:
                self.grid[self.r][c] = " "

    def text(self):
        return "\n".join("".join(row).rstrip() for row in self.grid)


import codecs

screen = Screen(32, 110)
decoder = codecs.getincrementaldecoder("utf8")(errors="replace")

master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 110, 0, 0))
env = dict(os.environ, TERM="xterm-256color", LANG="en_US.UTF-8")
env.pop("NO_COLOR", None)
proc = subprocess.Popen(
    [str(kaku), "tui", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")],
    stdin=slave, stdout=slave, stderr=slave, env=env, close_fds=True,
)
os.close(slave)
raw = b""


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


def screen_text():
    return screen.text()


def wait_for(pattern, timeout=60):
    end = time.time() + timeout
    while time.time() < end:
        pump(0.3)
        m = re.search(pattern, screen_text())
        if m:
            return m
        if proc.poll() is not None:
            break
    return None


def send(text):
    os.write(master, text.encode())


try:
    check("the workspace draws its header and composer", wait_for(r"kakurega") is not None and wait_for(r"Command") is not None)
    check("the alternate screen and bracketed paste are entered", b"\x1b[?1049h" in raw and b"\x1b[?2004h" in raw)
    check("the status line says the service is ready", wait_for(r"ready", 30) is not None)
    send("/help\r")
    check("/help lists the commands", wait_for(r"/review FOLDER for RECIPIENT") is not None)
    send("/revi\t")
    pump(0.5)
    send(" payments-service for acme-audit\r")
    check("a review starts from a relative folder name", wait_for(r"Review job-\d+ started") is not None)
    check("progress and completion appear without any key press", wait_for(r"finished: ready", 120) is not None)
    send("/approve\r")
    m = wait_for(r"first 12 characters of the digest \(([0-9a-f]{12})\)")
    check("the approval view opens and names the digest prefix to type", m is not None)
    (repo / "target/e2e-shots").mkdir(parents=True, exist_ok=True)
    (repo / "target/e2e-shots/tui-approval.txt").write_text(screen_text())
    shown = screen_text()
    for _ in range(12):
        send("\x1b[6~")
        pump(0.15)
        shown += "\n" + screen_text()
    check("it shows the exact files, the disclosure and what is not established (after scrolling)", all(s in shown for s in ["finding-1.txt", "Cumulative disclosure", "does not establish"]), "" if all(s in shown for s in ["finding-1.txt", "Cumulative disclosure", "does not establish"]) else "missing: " + ", ".join(s for s in ["finding-1.txt", "Cumulative disclosure", "does not establish"] if s not in shown))
    for _ in range(12):
        send("\x1b[5~")
        pump(0.05)
    if m:
        send("000000000000\r")
        check("a wrong prefix is refused on screen", wait_for(r"Nothing was approved") is not None)
        send("\x7f" * 12 + m.group(1) + "\r")
        check("the right prefix approves", wait_for(r"Approved\.") is not None)
    send("/deliver\r")
    check("delivery produces a capsule", wait_for(r"Delivered") is not None and ".kkg" in screen_text())
    (repo / "target/e2e-shots/tui-delivered.txt").write_text(screen_text())
    send("/remember the password is hunter2hunter2\r")
    check("a secret is refused", wait_for(r"secrets are never stored") is not None)
    check("and hidden from the transcript", "text hidden" in screen_text())
    send("/quit\r")
    deadline = time.time() + 15
    while proc.poll() is None and time.time() < deadline:
        pump(0.2)
    pump(0.5)
    check("the program exits cleanly on /quit", proc.poll() == 0, f"exit={proc.poll()}")
    check("the terminal is restored: alternate screen left, cursor shown, bracketed paste off", all(s in raw for s in [b"\x1b[?1049l", b"\x1b[?25h", b"\x1b[?2004l"]))
finally:
    if proc.poll() is None:
        proc.kill()
    os.close(master)

left = subprocess.run(["pgrep", "-fl", f"kaku serve --data-dir {data}"], capture_output=True, text=True)
check("no service process is left behind", left.returncode != 0, left.stdout.strip()[:200])
# a hard kill of the workspace (terminal closed, SIGKILL) must not leave the service it started running
data2 = work / "data2"
m2, s2 = pty.openpty()
fcntl.ioctl(s2, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 110, 0, 0))
p2 = subprocess.Popen([str(kaku), "tui", "--data-dir", str(data2), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdin=s2, stdout=s2, stderr=s2, env=env, close_fds=True)
os.close(s2)
t_end = time.time() + 30
alive = False
while time.time() < t_end:
    r, _, _ = select.select([m2], [], [], 0.2)
    if r:
        try:
            os.read(m2, 65536)
        except OSError:
            break
    if subprocess.run(["pgrep", "-f", f"kaku serve --data-dir {data2}"], capture_output=True).returncode == 0:
        alive = True
        break
check("a second workspace started its own service", alive)
p2.kill()
p2.wait()
gone = False
for _ in range(40):
    try:
        os.read(m2, 65536)
    except OSError:
        pass
    if subprocess.run(["pgrep", "-f", f"kaku serve --data-dir {data2}"], capture_output=True).returncode != 0:
        gone = True
        break
    time.sleep(0.25)
os.close(m2)
check("after the workspace is killed outright, the service it started exits by itself", gone)
bad = results.count(False)
print(f"\n{len(results) - bad}/{len(results)} terminal checks passed" if bad == 0 else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
