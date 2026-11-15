#!/usr/bin/env python3
"""The recovery key typed at a terminal: not shown as it is typed, and Ctrl-C at the prompt cancels and gives the terminal
back with what is typed shown again (it once left the terminal showing nothing). The real binary in a pseudo-terminal."""
import os, pty, re, select, shutil, subprocess, sys, termios, time
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
kaku = str(repo / "target/debug/kaku")
work = repo / "target/restore-key-tmp"
shutil.rmtree(work, ignore_errors=True)
work.mkdir(parents=True)
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


data = work / "data"
srv = subprocess.Popen([kaku, "serve", "--data-dir", str(data), "--root", str(work), "--scripted", str(repo / "tests/e2e/script.json")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    for _ in range(200):
        if (data / "admin.json").exists():
            break
        time.sleep(0.1)
    made = subprocess.run([kaku, "backup", "--out", str(work / "b.kkb"), "--data-dir", str(data)], capture_output=True, text=True, timeout=120)
    key = (re.search(r"KKRK-[A-Za-z0-9-]+", made.stdout) or [None])[0]
    check("a backup is made and its recovery key printed once", made.returncode == 0 and key is not None, made.stdout[-300:] + made.stderr[-200:])
finally:
    srv.terminate()
    srv.wait(timeout=10)


def at_terminal(target, typed):
    """Runs `kaku restore` with a pseudo-terminal as its input, types `typed` once the prompt shows, and returns the exit
    code, everything the terminal showed, and whether echo is on once it has left."""
    m, s = pty.openpty()
    p = subprocess.Popen([kaku, "restore", str(work / "b.kkb"), "--data-dir", str(target)], stdin=s, stdout=s, stderr=s, start_new_session=True)
    shown = b""
    end = time.time() + 30
    while time.time() < end and b"Recovery key" not in shown:
        r, _, _ = select.select([m], [], [], 0.2)
        if r:
            shown += os.read(m, 4096)
    os.write(m, typed)
    end = time.time() + 60
    while time.time() < end and p.poll() is None:
        r, _, _ = select.select([m], [], [], 0.2)
        if r:
            try:
                shown += os.read(m, 4096)
            except OSError:
                break
    code = p.wait(timeout=30)
    echo_on = bool(termios.tcgetattr(s)[3] & termios.ECHO)
    os.close(m)
    os.close(s)
    return code, shown.decode(errors="replace"), echo_on


if key:
    code, shown, echo_on = at_terminal(work / "restored", key.encode() + b"\r")
    check("the typed key restores the backup", code == 0 and "Restored" in shown, shown[-300:])
    check("and the key is never shown on the terminal as it is typed", key not in shown and "not shown as you type" in shown, shown[-300:])
    check("and the terminal shows what is typed again afterwards", echo_on)
    code, shown, echo_on = at_terminal(work / "cancelled", b"KKRK-ab\x03")
    check("Ctrl-C at the prompt cancels in words and restores nothing", code == 1 and "Cancelled. Nothing was restored." in shown and not (work / "cancelled" / "admin.json").exists(), f"{code} {shown[-200:]}")
    check("and gives the terminal back with what is typed shown again", echo_on)
    code, shown, echo_on = at_terminal(work / "wrong", b"KKRK-not-the-key\r")
    check("a wrong key fails in words, and echo is back", code != 0 and echo_on and "KKRK-not-the-key" not in shown, f"{code} {shown[-200:]}")

bad = results.count(False)
print(f"all {len(results)} restore-key checks passed" if bad == 0 else f"{bad} FAILED")
sys.exit(1 if bad else 0)
