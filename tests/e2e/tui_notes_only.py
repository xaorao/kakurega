#!/usr/bin/env python3
"""The terminal's /notesonly as a person uses it, with a real local model (default qwen2.5:3b, needs Ollama): a project that
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
    send("/project kestrel\r")
    check("a project is set for the conversation", wait_for(r'Project set to "kestrel"', 15) is not None)
    send("/notesonly on\r")
    check("/notesonly on names the project and says what it does", wait_for(r"On for kestrel: it answers only from its own notes", 20) is not None)
    send("/notesonly\r")
    check("/notesonly lists the project", wait_for(r"Projects that answer only from their own notes", 15) is not None)
    send("what is the rounding rule\r")
    check("with nothing saved the reply is the plain 'nothing here'", wait_for(r"Nothing in this project's notes", 60) is not None, screen.text()[-600:])
    check("and the footer says no model was asked, with no model figures", wait_for(r"no model was asked", 15) is not None and "tokens of context" not in screen.text().split("Nothing in this project")[-1])
    send("/remember in kestrel: The rounding rule is banker's rounding to two decimals.\r")
    pump(2)
    send("what is the rounding rule\r")
    def after_question():
        t = screen.text()
        return t.split("› what is the rounding rule")[-1]
    end = time.time() + 150
    while time.time() < end and not re.search(r"1 note\(s\) used", after_question()):
        pump(0.5)
    tail = after_question()
    check("with a saved note the model answers from it, shown as an answer with 1 note used", bool(re.search(r"banker", tail)) and "1 note(s) used" in tail and "no model was asked" not in tail, tail[-500:])
    send("/notesonly off\r")
    check("and it turns off", wait_for(r"Off for kestrel", 20) is not None)
    send("/quit\r")
    pump(2)
finally:
    try:
        proc.kill()
    except Exception:
        pass
bad = results.count(False)
print(f"\n{len(results) - bad}/{len(results)} checks passed" if bad == 0 else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
