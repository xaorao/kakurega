#!/usr/bin/env python3
"""The terminal's /branch pick as a person uses it, with a real local model (default qwen2.5:3b, needs Ollama): a project that
answers only from its own notes gets a plain 'nothing here' and no model is asked; with a saved note the model answers from it."""
import codecs, fcntl, os, pty, re, select, struct, subprocess, sys, termios, time
from pathlib import Path

_src = (Path(__file__).resolve().parent / "tui_pty.py").read_text().split("import codecs")[0]
exec(_src)  # Screen, check, paths (a fresh work folder)
model = sys.argv[1] if len(sys.argv) > 1 else "qwen2.5:3b"
screen = Screen(40, 120)
decoder = codecs.getincrementaldecoder("utf8")(errors="replace")
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
env = dict(os.environ, TERM="xterm-256color", LANG="en_US.UTF-8")
env.pop("NO_COLOR", None)
proc = subprocess.Popen([str(kaku), "tui", "--data-dir", str(data), "--root", str(work / "root"), "--model", model, "--ui-dir", str(repo / "apps/web/dist")],
                        stdin=slave, stdout=slave, stderr=slave, env=env, close_fds=True)
os.close(slave)


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


try:
    check("the workspace is ready", wait_for(r"ready", 40) is not None)
    send("/keep\r")
    check("the conversation is kept", wait_for(r"[Kk]ept|saved", 20) is not None, screen.text()[-300:])
    send("name one colour in one word\r")
    end = time.time() + 120
    while time.time() < end and not re.search(r"tokens of context", screen.text()):
        pump(0.5)
    send("name one animal in one word\r")
    end = time.time() + 120
    while time.time() < end and len(re.findall(r"tokens of context", screen.text())) < 2:
        pump(0.5)
    send("/branch pick\r")
    check("the list shows both questions and both answers, numbered", wait_for(r"Branch the conversation after one of its messages", 15) is not None and "keeps 4 messages" in screen.text() and "you: name one colour" in screen.text(), screen.text()[-900:])
    send("\x1b[B\x1b[B")
    pump(0.5)
    send("\r")
    check("choosing the second message opens a copy that keeps two messages", wait_for(r"Opened the kept conversation", 30) is not None, screen.text()[-500:])
    pump(2)
    t = screen.text()
    check("the copy holds the first question and its answer and not the second question", "name one colour" in t and "animal" not in t, t[-700:])
    send("\x03")
    pump(1)
finally:
    try:
        proc.kill()
    except Exception:
        pass
bad = results.count(False)
print(f"\n{len(results) - bad}/{len(results)} checks passed" if bad == 0 else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
