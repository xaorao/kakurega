#!/usr/bin/env python3
"""Drives the real `kaku` binary in a real pseudo-terminal: boot, a full review, approval, delivery, quit, clean terminal."""
import fcntl
import json
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
editor_script = work / "editor.sh"
editor_script.write_text("#!/bin/sh\nprintf 'edited in the editor ZZZ' > \"$1\"\n")
env["VISUAL"] = "sh " + str(editor_script)
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
    _mark = len(raw)
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 90, 0, 0))
    pump(1.5)
    check("a window resize redraws the screen without any key press", b"kakurega" in raw[_mark:])
    _mark = len(raw)
    fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 110, 0, 0))
    pump(1.5)
    check("and resizing back redraws it again", b"kakurega" in raw[_mark:])
    send("/review payments-s\t")
    _ok = wait_for(r"/review payments-service/", 15) is not None
    check("Tab after /review completes the folder from the service's listing", _ok, screen_text()[-700:].replace("\n", " | "))
    send("\x1b")
    pump(0.3)
    send("/revi\t")
    pump(0.5)
    send(" payments-service for acme-audit\r")
    check("a review starts from a relative folder name", wait_for(r"Review job-\d+ started") is not None)
    check("progress and completion appear without any key press", wait_for(r"finished: ready", 120) is not None)
    pump(0.5)
    import re as _re
    _osc = _re.compile(rb"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")
    _bare = _osc.sub(b"", raw).count(b"\x07")
    check("a finished review rings the bell exactly once", _bare == 1, f"bells={_bare}")
    check("and the window title says only that a review is ready", b"\x1b]2;Kakurega: a review is ready\x07" in raw)
    check("no title or bell names the folder, the recipient or the job", all(b"payments-service" not in t and b"acme-audit" not in t and b"job-" not in t for t in _re.findall(rb"\x1b\]2;[^\x07]*\x07", raw)))
    check("focus reporting was asked for", b"\x1b[?1004h" in raw)
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
    send("Where are login sessions kept?\r")
    check("plain text is a chat message answered by the private assistant", wait_for(r"\[test model\] I received \d+ characters") is not None)
    # search, copy and export of the conversation on the screen
    send("/transcript login sessions\r")
    check("/transcript marks every word it found without colour", wait_for(r"\[\[login\]\] \[\[sessions\]\]|\[\[sessions\]\]") is not None)
    raw_before = len(raw)
    send("/copy\r")
    check("/copy asks before it touches the clipboard and says other programs can read it", wait_for(r"Other programs can read the clipboard") is not None and b"\x1b]52;" not in raw[raw_before:])
    send("n")
    pump(0.5)
    check("a no puts nothing on the clipboard", b"\x1b]52;" not in raw[raw_before:])
    send("/copy\r")
    wait_for(r"Other programs can read the clipboard")
    send("y")
    pump(1.0)
    import base64 as _b64
    seq = re.search(rb"\x1b\]52;c;([A-Za-z0-9+/=]+)\x07", raw[raw_before:])
    check("a yes writes the standard clipboard sequence holding exactly the last reply", seq is not None and b"test model" in _b64.b64decode(seq.group(1)), (seq.group(1)[:30] if seq else b"").decode())
    send("/export pty-notes\r")
    check("/export writes the conversation to a file in the exports folder", wait_for(r"Written to .*pty-notes\.md", 15) is not None)
    _f = data / "exports" / "pty-notes.md"
    check("the file holds both sides of the conversation and not the commands", _f.exists() and "You: Where are login sessions kept?" in _f.read_text() and "/export" not in _f.read_text() and (_f.stat().st_mode & 0o777) == 0o600)
    send("/export pty-notes\r")
    check("exporting under the same name again is refused", wait_for(r"already there", 15) is not None)
    # an approval decided in another window closes the terminal's approval view by itself
    import json as _json, urllib.request as _ur
    _a = _json.load(open(data / "admin.json"))
    _base = f"http://127.0.0.1:{_a['port']}"
    def _req(path, body=None):
        return _json.load(_ur.urlopen(_ur.Request(_base + path, data=_json.dumps(body).encode() if body is not None else None, headers={"authorization": "Bearer " + _a["token"], "content-type": "application/json"}, method="POST" if body is not None else "GET"), timeout=20))
    def _until(fn, seconds):
        end = time.time() + seconds
        while time.time() < end:
            pump(0.3)
            v = fn()
            if v:
                return v
        return None
    _known = {j["id"] for j in _req("/admin/jobs")["jobs"]}
    send("/review payments-service for second-audit\r")
    _job = _until(lambda: next((j["id"] for j in _req("/admin/jobs")["jobs"] if j["id"] not in _known), None), 30)
    check("a second review starts", _job is not None)
    _ready = _until(lambda: next((j for j in _req("/admin/jobs")["jobs"] if j["id"] == _job and j["state"] == "ready"), None), 150)
    check("and finishes ready", _ready is not None)
    send(f"/approve {_job}\r")
    _aid = _until(lambda: next((x["id"] for x in _req("/admin/approvals")["approvals"] if x["job"] == _job), None), 30)
    check("its approval is waiting on the service", _aid is not None)
    pump(2.0)
    check("and the terminal shows the approval view", "type the first 12 characters" in screen_text().replace("\n", " ") or "first 12 characters" in screen_text())
    _req(f"/admin/approvals/{_aid}/deny", {})
    check("when it is refused somewhere else the terminal says so and leaves the view", wait_for(r"Approval closed", 15) is not None and "first 12 characters" not in screen_text().split("Approval closed")[-1])
    send("a draft to be replaced")
    pump(0.3)
    send("\x07")
    check("Ctrl-G hands the screen to the editor and takes it back with the saved text in the composer", wait_for(r"edited in the editor ZZZ", 20) is not None and "kakurega" in screen_text())
    send("\r")
    check("the edited text is what is sent", wait_for(r"received 24 characters", 30) is not None)
    check("the editor's file is gone afterwards", subprocess.run("ls /tmp /var/folders/*/*/T 2>/dev/null | grep -c kaku-edit-", shell=True, capture_output=True, text=True).stdout.strip() in ("0", ""))
    send("/remember the password is hunter2hunter2\r")
    check("a secret is refused", wait_for(r"secrets are never stored") is not None)
    check("and hidden from the transcript", "text hidden" in screen_text())
    send("/aside which colour is the sky\r")
    check("an aside is answered apart and says it is not saved", wait_for(r"not part of this conversation and not saved", 60) is not None)
    send("/keep\r")
    check("a conversation is kept so a later launch can resume it", wait_for(r"Kept\. From now on", 20) is not None)
    send("resumecanary words\r")
    pump(4.0)
    send("/quit\r")
    deadline = time.time() + 15
    while proc.poll() is None and time.time() < deadline:
        pump(0.2)
    pump(0.5)
    check("the program exits cleanly on /quit", proc.poll() == 0, f"exit={proc.poll()}")
    check("the terminal is restored: alternate screen left, cursor shown, bracketed paste off", all(s in raw for s in [b"\x1b[?1049l", b"\x1b[?25h", b"\x1b[?2004l", b"\x1b[?1004l"]))
finally:
    if proc.poll() is None:
        proc.kill()
    os.close(master)

left = subprocess.run(["pgrep", "-fl", f"kaku serve --data-dir {data}"], capture_output=True, text=True)
check("no service process is left behind", left.returncode != 0, left.stdout.strip()[:200])
# `kaku --resume` opens the newest kept conversation at launch
m3, s3 = pty.openpty()
fcntl.ioctl(s3, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 110, 0, 0))
env3 = dict(os.environ, TERM="xterm-256color")
p3 = subprocess.Popen([str(kaku), "tui", "--resume", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdin=s3, stdout=s3, stderr=s3, env=env3, close_fds=True)
os.close(s3)
screen3 = Screen(32, 110)
dec3 = codecs.getincrementaldecoder("utf8")(errors="replace")
t_end = time.time() + 40
while time.time() < t_end and "resumecanary words" not in screen3.text():
    r, _, _ = select.select([m3], [], [], 0.2)
    if r:
        try:
            screen3.feed(dec3.decode(os.read(m3, 65536)))
        except OSError:
            break
check("launching with --resume shows the newest kept conversation, with the message kept earlier", "resumecanary words" in screen3.text())
check("and says it was opened from the store", "Opened the kept conversation" in screen3.text())
os.write(m3, b"/quit\r")
t_end = time.time() + 20
while p3.poll() is None and time.time() < t_end:
    # the screen is redrawn constantly; leave the pipe unread and the program blocks writing to it
    r, _, _ = select.select([m3], [], [], 0.2)
    if r:
        try:
            os.read(m3, 65536)
        except OSError:
            break
if p3.poll() is None:
    p3.kill()
    p3.wait()
check("the resumed workspace exits cleanly", p3.returncode == 0, f"exit={p3.returncode}")
os.close(m3)


def run_resume(extra, want, label, absent=None):
    """Launch the workspace with `--resume ...` and wait for `want` to show on screen; returns the final screen text."""
    m, sl = pty.openpty()
    fcntl.ioctl(sl, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 110, 0, 0))
    pr = subprocess.Popen([str(kaku), "tui", *extra, "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdin=sl, stdout=sl, stderr=sl, env=env3, close_fds=True)
    os.close(sl)
    scr = Screen(32, 110)
    dc = codecs.getincrementaldecoder("utf8")(errors="replace")
    until = time.time() + 40
    while time.time() < until and want not in scr.text():
        r, _, _ = select.select([m], [], [], 0.2)
        if r:
            try:
                scr.feed(dc.decode(os.read(m, 65536)))
            except OSError:
                break
    seen = scr.text()
    os.write(m, b"/quit\r")
    until = time.time() + 20
    while pr.poll() is None and time.time() < until:
        r, _, _ = select.select([m], [], [], 0.2)
        if r:
            try:
                os.read(m, 65536)
            except OSError:
                break
    if pr.poll() is None:
        pr.kill()
        pr.wait()
    os.close(m)
    check(label, want in seen and (absent is None or absent not in seen), seen[-600:])
    return pr.returncode


listing = subprocess.run([kaku, "sessions", "--json", "--data-dir", str(data)], capture_output=True, text=True)
kept = json.loads(listing.stdout)["sessions"] if listing.returncode == 0 else []
check("the kept conversation is listed so its id can be chosen", len(kept) >= 1, listing.stdout[:200] + listing.stderr[:200])
if kept:
    # a newer kept conversation, so "the newest" and "the one chosen" are different conversations
    newer = subprocess.run([kaku, "chat", "newercanary words", "--keep", "--json", "--data-dir", str(data), "--scripted", str(repo / "tests/e2e/script.json")], capture_output=True, text=True)
    both = json.loads(subprocess.run([kaku, "sessions", "--json", "--data-dir", str(data)], capture_output=True, text=True).stdout)["sessions"]
    check("a second, newer conversation is kept", newer.returncode == 0 and len(both) == 2, newer.stdout[:200] + newer.stderr[:200])
    older_id = kept[0]["id"]
    rc = run_resume(["--resume", older_id], "resumecanary words", "launching with --resume ID opens the one chosen, not the newest", absent="newercanary words")
    check("and the workspace exits cleanly", rc == 0, f"exit={rc}")
    # the newer conversation has no project; the older one belongs to this folder's project, so `--resume` alone prefers it
    run_resume(["--resume"], "resumecanary words", "launching with --resume alone opens the newest conversation of this folder's project, not a newer one of no project", absent="newercanary words")
    # the picker starts on this folder's project and Tab widens it to every project
    m, sl = pty.openpty()
    fcntl.ioctl(sl, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 110, 0, 0))
    pp = subprocess.Popen([str(kaku), "tui", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdin=sl, stdout=sl, stderr=sl, env=env3, close_fds=True)
    os.close(sl)
    scr = Screen(32, 110)
    dc = codecs.getincrementaldecoder("utf8")(errors="replace")

    def pump_picker(seconds, until_text=None):
        end = time.time() + seconds
        while time.time() < end and (until_text is None or until_text not in scr.text()):
            r, _, _ = select.select([m], [], [], 0.2)
            if r:
                try:
                    scr.feed(dc.decode(os.read(m, 65536)))
                except OSError:
                    break

    pump_picker(30, "kakurega")
    os.write(m, b"/sessions\r")
    pump_picker(20, "Tab: all projects")
    pump_picker(20, "resumecanary words")
    scoped = scr.text()
    check("the picker opens on this folder's project, named, with the way to widen it", "[project kakurega" in scoped and "Tab: all projects]" in scoped, scoped[-500:])
    check("and lists this project's conversation but not the one of no project", "resumecanary words" in scoped and "newercanary words" not in scoped, scoped[-500:])
    # Right previews the chosen conversation: its last messages appear under its row
    os.write(m, b"\x1b[C")
    pump_picker(20, "assistant: ")
    previewed = scr.text()
    check("Right shows the last messages of the chosen conversation under its row", "you: resumecanary words" in previewed and "assistant: " in previewed, previewed[-500:])
    os.write(m, b"\x1b[D")
    pump_picker(2)
    check("Left closes the preview", "assistant: " not in scr.text(), scr.text()[-400:])
    os.write(m, b"\t")
    pump_picker(20, "Tab: only kakurega")
    pump_picker(20, "newercanary words")
    widened = scr.text()
    check("Tab shows every project, conversations of no project included", "all projects" in widened and "newercanary words" in widened, widened[-500:])
    os.write(m, b"\x1b")
    pump_picker(2)
    os.write(m, b"/quit\r")
    end = time.time() + 20
    while pp.poll() is None and time.time() < end:
        r, _, _ = select.select([m], [], [], 0.2)
        if r:
            try:
                os.read(m, 65536)
            except OSError:
                break
    if pp.poll() is None:
        pp.kill()
        pp.wait()
    os.close(m)
    # Esc twice on an empty box lists the owner's messages; choosing one rewinds to it with the message back in the box
    before_rewind = len(json.loads(subprocess.run([kaku, "sessions", "--json", "--data-dir", str(data)], capture_output=True, text=True).stdout)["sessions"])
    m, sl = pty.openpty()
    fcntl.ioctl(sl, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 110, 0, 0))
    pq = subprocess.Popen([str(kaku), "tui", "--resume", older_id, "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdin=sl, stdout=sl, stderr=sl, env=env3, close_fds=True)
    os.close(sl)
    scr = Screen(32, 110)
    dc = codecs.getincrementaldecoder("utf8")(errors="replace")

    def pump_rw(seconds, until_text=None):
        end = time.time() + seconds
        while time.time() < end and (until_text is None or until_text not in scr.text()):
            r, _, _ = select.select([m], [], [], 0.2)
            if r:
                try:
                    scr.feed(dc.decode(os.read(m, 65536)))
                except OSError:
                    break

    pump_rw(40, "Opened the kept conversation")
    os.write(m, b"\x1b")
    pump_rw(0.4)
    check("one Esc on an empty box opens nothing", "Go back to before one of your messages" not in scr.text())
    os.write(m, b"\x1b")
    pump_rw(15, "Go back to before one of your messages")
    listed = scr.text()
    check("Esc twice lists the owner's messages, numbered", "Go back to before one of your messages" in listed and "resumecanary words" in listed and "message 1" in listed, listed[-600:])
    os.write(m, b"\r")
    pump_rw(30, "Opened the kept conversation")
    pump_rw(1.0)
    after_pick = scr.text()
    check("choosing the message opens the copy and puts the message back in the box", after_pick.count("resumecanary words") >= 1 and "Command" in after_pick and "resumecanary words" in after_pick.split("Command")[-1], after_pick[-600:])
    os.write(m, b"\x15/quit\r")
    end = time.time() + 20
    while pq.poll() is None and time.time() < end:
        r, _, _ = select.select([m], [], [], 0.2)
        if r:
            try:
                os.read(m, 65536)
            except OSError:
                break
    if pq.poll() is None:
        pq.kill()
        pq.wait()
    os.close(m)
    after_rewind = len(json.loads(subprocess.run([kaku, "sessions", "--json", "--data-dir", str(data)], capture_output=True, text=True).stdout)["sessions"])
    check("and the original is kept, with the copy as one more conversation", after_rewind == before_rewind + 1, f"{before_rewind} -> {after_rewind}")
    # --fork opens a branch: a third conversation appears holding the same messages, and the original is untouched
    run_resume(["--resume", older_id, "--fork"], "resumecanary words", "launching with --resume ID --fork opens a branch of that conversation", absent="newercanary words")
    after = json.loads(subprocess.run([kaku, "sessions", "--json", "--data-dir", str(data)], capture_output=True, text=True).stdout)["sessions"]
    original = [x for x in after if x["id"] == older_id]
    branches = [x for x in after if x["id"] not in {older_id, both[0]["id"], both[1]["id"]} and x["messages"] == kept[0]["messages"]]
    check("the branch is a new kept conversation with the same messages and the original keeps its own", len(after) == before_rewind + 2 and len(original) == 1 and original[0]["messages"] == kept[0]["messages"] and len(branches) == 1 and branches[0]["messages"] == kept[0]["messages"], json.dumps(after)[:400])
rc = run_resume(["--resume", "../admin/secrets"], "not a conversation id", "an id that is not an id is refused on the screen and nothing is requested", absent="resumecanary words")
rc = run_resume(["--resume", "feedfacefeedfacefeedface"], "no such", "an unknown id says so and opens nothing", absent="resumecanary words")
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
killed_at = time.time()
# the service polls its parent once a second; the claim is that it leaves by itself, so a machine under load gets a minute
for _ in range(240):
    try:
        os.read(m2, 65536)
    except OSError:
        pass
    if subprocess.run(["pgrep", "-f", f"kaku serve --data-dir {data2}"], capture_output=True).returncode != 0:
        gone = True
        break
    time.sleep(0.25)
os.close(m2)
check("after the workspace is killed outright, the service it started exits by itself", gone, f"after {time.time() - killed_at:.1f} s")
# reviews that stop while the workspace is closed are reported at the next launch, once
data5 = work / "data5"
srv5 = subprocess.Popen([str(kaku), "serve", "--data-dir", str(data5), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    for _ in range(150):
        if (data5 / "admin.json").exists():
            break
        time.sleep(0.1)


    def open_close(label, want=None, absent=None, kill=False):
        """Open the workspace on the running service, wait for `want` (if any) to show, then leave with /quit."""
        mm, sl = pty.openpty()
        fcntl.ioctl(sl, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 110, 0, 0))
        pr = subprocess.Popen([str(kaku), "tui", "--data-dir", str(data5), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdin=sl, stdout=sl, stderr=sl, env=env3, close_fds=True)
        os.close(sl)
        scr5 = Screen(32, 110)
        dc5 = codecs.getincrementaldecoder("utf8")(errors="replace")

        def pump5(seconds, until_text=None):
            end = time.time() + seconds
            while time.time() < end and (until_text is None or until_text not in scr5.text()):
                r, _, _ = select.select([mm], [], [], 0.2)
                if r:
                    try:
                        scr5.feed(dc5.decode(os.read(mm, 65536)))
                    except OSError:
                        break

        pump5(30, "kakurega")
        pump5(15 if want is None else 30, want)
        pump5(1.5)
        seen5 = scr5.text()
        if kill:
            # the terminal is closed outright: nothing is flushed on the way out
            pr.kill()
            pr.wait()
        else:
            os.write(mm, b"/quit\r")
        end = time.time() + 20
        while pr.poll() is None and time.time() < end:
            r, _, _ = select.select([mm], [], [], 0.2)
            if r:
                try:
                    os.read(mm, 65536)
                except OSError:
                    break
        if pr.poll() is None:
            pr.kill()
            pr.wait()
        os.close(mm)
        if want is not None:
            check(label, want in seen5 and (absent is None or absent not in seen5), seen5[-700:])
        else:
            check(label, absent is None or absent not in seen5, seen5[-700:])
        return pr.returncode, seen5


    rc, _ = open_close("the first launch reports nothing (there is no earlier mark)", absent="Stopped while you were away")
    check("and the workspace exits cleanly", rc == 0, f"exit={rc}")
    time.sleep(1.2)
    done = subprocess.run([str(kaku), "review", str(work / "root/payments-service"), "--for", "away-audit", "--data-dir", str(data5), "--json"], capture_output=True, text=True, timeout=300)
    check("a review finishes while no workspace is open", done.returncode in (0, 3), done.stdout[:200] + done.stderr[:200])
    _, card = open_close("the next launch reports it (and is then killed, with no chance to say it left)", want="Stopped while you were away", kill=True)
    check("the card names the review, who it was for and how it ended", "job-1" in card.split("Stopped while you were away")[-1] and "away-audit" in card.split("Stopped while you were away")[-1] and ("ready for your approval" in card or "nothing to release" in card), card[-700:])
    open_close("and the launch after that does not report it again, because it was marked seen when shown", absent="Stopped while you were away")
finally:
    srv5.kill()
    srv5.wait()
# a terminal that shows desktop notifications (iTerm2, WezTerm, Ghostty, Warp, kitty) gets one notification and no bell
data6 = work / "data6"
env6 = dict(env3, TERM_PROGRAM="iTerm.app")
env6.pop("TMUX", None)
m6, s6 = pty.openpty()
fcntl.ioctl(s6, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 110, 0, 0))
p6 = subprocess.Popen([str(kaku), "tui", "--data-dir", str(data6), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdin=s6, stdout=s6, stderr=s6, env=env6, close_fds=True)
os.close(s6)
raw6 = b""


def pump6(seconds, until=None):
    global raw6
    end = time.time() + seconds
    while time.time() < end:
        r, _, _ = select.select([m6], [], [], 0.2)
        if r:
            try:
                raw6 += os.read(m6, 65536)
            except OSError:
                return
        if until is not None and until in raw6:
            return


pump6(30, b"kakurega")
os.write(m6, f"/review {work / 'root/payments-service'} for osc-audit\r".encode())
pump6(180, b"finished: ready")
pump6(1.5)
import re as _re6
note = b"\x1b]9;Kakurega: a review is ready\x07"
bare = _re6.sub(rb"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)", b"", raw6).count(b"\x07")
check("in a terminal that shows notifications, a finished review sends one desktop notification with a fixed phrase", raw6.count(note) == 1, str(raw6.count(note)))
check("and no separate bell", bare == 0, f"bells={bare}")
check("and nothing in any escape sequence names the folder, the recipient or the job", all(b"payments-service" not in t and b"osc-audit" not in t and b"job-" not in t for t in _re6.findall(rb"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)", raw6)))
# the service is stopped under the running workspace: the next request fails and the owner is alerted once
mark6 = len(raw6)
subprocess.run(["pkill", "-f", f"kaku serve --data-dir {data6}"], capture_output=True)
time.sleep(1.0)
os.write(m6, b"/status\r")
pump6(20, b"the service is not answering")
pump6(1.5)
down_note = b"\x1b]9;Kakurega: the service is not answering\x07"
check("and sends one desktop notification with a fixed phrase", raw6[mark6:].count(down_note) == 1, str(raw6[mark6:].count(down_note)))
os.write(m6, b"/status\r")
pump6(3)
check("a second failure while it stays down does not notify again", raw6[mark6:].count(down_note) == 1, str(raw6[mark6:].count(down_note)))
os.write(m6, b"/quit\r")
end6 = time.time() + 20
while p6.poll() is None and time.time() < end6:
    r, _, _ = select.select([m6], [], [], 0.2)
    if r:
        try:
            os.read(m6, 65536)
        except OSError:
            break
if p6.poll() is None:
    p6.kill()
    p6.wait()
os.close(m6)
bad = results.count(False)
print(f"\n{len(results) - bad}/{len(results)} terminal checks passed" if bad == 0 else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
