#!/usr/bin/env python3
"""The terminal's @file as a person uses it, with a real local model (default qwen2.5:3b, needs Ollama): Tab finishes a file name,
the file is read on this computer and sent with the question, the answer comes from it, and a secret-named file is refused."""
import codecs, fcntl, os, pty, re, select, struct, subprocess, sys, termios, time
from pathlib import Path

_src = (Path(__file__).resolve().parent / "tui_pty.py").read_text().split("import codecs")[0]
exec(_src)  # Screen, check, paths (a fresh work folder)
model = sys.argv[1] if len(sys.argv) > 1 else "qwen2.5:3b"
docs = work / "root" / "docs"
docs.mkdir(parents=True, exist_ok=True)
(docs / "terms.txt").write_text("Payment terms: invoices are due net 45 days from the invoice date. Late fee is 2 percent per month.\n")
(docs / ".env").write_text("API_TOKEN=tok-9f8e7d6c5b4a\n")
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
    send("/ask do")
    pump(0.5)
    send("\t")
    check("Tab finishes the folder after /ask, as it does after /review", wait_for(r"/ask docs/", 15) is not None, screen.text()[-300:])
    send("\x7f" * 12)
    pump(0.5)
    send("/code do")
    pump(0.5)
    send("\t")
    check("and after /code", wait_for(r"/code docs/", 15) is not None, screen.text()[-300:])
    send("\x7f" * 12)
    pump(0.5)
    send("what are the payment terms in @ter")
    pump(0.5)
    send("\t")
    check("Tab finishes the file name", wait_for(r"@terms\.txt", 15) is not None, screen.text()[-400:])
    send("\r")
    check("the file is attached and says so", wait_for(r"Attached terms\.txt", 30) is not None, screen.text()[-500:])
    end = time.time() + 150
    while time.time() < end and not re.search(r"1 attached file\(s\) read", screen.text()):
        pump(0.5)
    tail = screen.text()
    check("the answer says one attached file was read", "1 attached file(s) read" in tail, tail[-500:])
    check("and the answer is from the file (net 45)", "45" in tail.split("what are the payment terms")[-1], tail[-500:])
    send("tell me about @.env now\r")
    check("a secret-named file is refused, and the screen says why", wait_for(r"Could not attach @\.env.*kept out of conversations", 30) is not None or wait_for(r"kept out of conversations", 10) is not None, screen.text()[-600:])
    check("and the message is back in the box, not sent", "tell me about @.env now" in screen.text().split("kept out of conversations")[-1])
    send("\x7f" * 40)
    pump(0.5)
    send("what is in @/etc/hosts please\r")
    check("an absolute file outside the allowed folders is refused, with the reason", wait_for(r"not in a folder this service may read", 30) is not None, screen.text()[-600:])
    check("and the token value never reached the screen", "tok-9f8e7d6c5b4a" not in screen.text())
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
