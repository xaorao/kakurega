#!/usr/bin/env python3
"""The terminal's @file#lines as a person uses it, with a real local model (default qwen2.5:3b, needs Ollama): only the chosen lines of a file are read and sent, and the answer comes from them."""
import codecs, fcntl, os, pty, re, select, struct, subprocess, sys, termios, time
from pathlib import Path

_src = (Path(__file__).resolve().parent / "tui_pty.py").read_text().split("import codecs")[0]
exec(_src)  # Screen, check, paths (a fresh work folder)
model = sys.argv[1] if len(sys.argv) > 1 else "qwen2.5:3b"
docs = work / "root" / "docs"
docs.mkdir(parents=True, exist_ok=True)
docs = work / "root" / "docs"
docs.mkdir(parents=True, exist_ok=True)
(docs / "fruit.txt").write_text("1 The first fruit is apple.\n2 The second fruit is cherry.\n3 The third fruit is mango.\n4 The fourth fruit is plum.\n5 The fifth fruit is grape.\n6 The sixth fruit is lemon.\n7 Seven.\n8 Eight.\n")
screen = Screen(40, 120)
decoder = codecs.getincrementaldecoder("utf8")(errors="replace")
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
env = dict(os.environ, TERM="xterm-256color", LANG="en_US.UTF-8")
env.pop("NO_COLOR", None)
proc = subprocess.Popen([str(kaku), "tui", "--data-dir", str(data), "--root", str(work / "root"), "--model", model, "--ui-dir", str(repo / "apps/web/dist")],
                        stdin=slave, stdout=slave, stderr=slave, env=env, close_fds=True, cwd=str(docs))
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
    send("list every fruit named in @fruit.txt#3-4\r")
    check("only the chosen lines are attached, and the name says which", wait_for(r"Attached fruit\.lines3-4\.txt", 40) is not None, screen.text()[-500:])
    end = time.time() + 150
    while time.time() < end and not re.search(r"1 attached file\(s\) read", screen.text()):
        pump(0.5)
    tail = screen.text().split("list every fruit")[-1]
    check("the answer names the fruits of lines 3 and 4", "mango" in tail.lower() and "plum" in tail.lower(), tail[-500:])
    check("and none of the other lines reached it", not any(w in tail.lower() for w in ["apple", "cherry", "grape", "lemon"]), tail[-500:])
    send("what about @fruit.txt#40\r")
    check("a line past the end is refused with the file's real length", wait_for(r"the file has 8 lines", 30) is not None, screen.text()[-500:])
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
