#!/usr/bin/env python3
"""Drives `kaku tui --linear` through pipes: a full review, a refused and an accepted approval, a chat, quit.

The output must be plain appended lines: not one escape byte, ever, and every overlay of the full-screen view must be a prompt
that says what to type. Pipes stand in for a screen reader's text stream."""
import os
import re
import select
import shutil
import subprocess
import sys
import time
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/tui-linear-tmp"
subprocess.run(["pkill", "-f", "kaku serve --data-dir " + str(work)], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
shutil.copytree(repo / "fixtures/synthetic-repo", work / "root/payments-service")
os.rename(work / "root/payments-service/_env", work / "root/payments-service/.env")
data = work / "data"
kaku = repo / "target/debug/kaku"
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


proc = subprocess.Popen(
    [str(kaku), "tui", "--linear", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=dict(os.environ, TERM="dumb"),
)
out = b""


def pump(seconds):
    global out
    end = time.time() + seconds
    while time.time() < end:
        r, _, _ = select.select([proc.stdout], [], [], 0.1)
        if r:
            chunk = os.read(proc.stdout.fileno(), 65536)
            if not chunk:
                return
            out += chunk


def wait_for(pattern, timeout=60):
    end = time.time() + timeout
    while time.time() < end:
        m = re.search(pattern, out.decode("utf8", "replace"))
        if m:
            return m
        pump(0.2)
    return None


def send(line):
    proc.stdin.write((line + "\n").encode())
    proc.stdin.flush()


try:
    check("it greets in plain words", wait_for(r"plain-line mode", 60) is not None)
    check("the status is announced as a line", wait_for(r"\[Status\] ", 30) is not None)
    send("/help")
    check("/help lists the commands", wait_for(r"/review FOLDER for RECIPIENT") is not None)
    send("/review payments-service for acme-audit")
    check("a review starts", wait_for(r"Review job-\d+ started") is not None)
    check("it finishes without any key press", wait_for(r"finished: ready", 150) is not None)
    check("and the owner is told in a line", wait_for(r"\[Notice\] A review is ready", 10) is not None)
    send("/approve")
    m = wait_for(r"\[Approval needed\] release (job-\d+)", 30)
    check("the approval is a sequential prompt", m is not None)
    want = wait_for(r"first 12 characters of the digest, ([0-9a-f]{12}), and press Enter")
    check("it names the prefix to type and how to refuse", want is not None and "type refuse" in out.decode())
    send("000000000000")
    check("a wrong prefix is refused with a line saying so", wait_for(r"\[Error\] That is not the first 12 characters", 15) is not None)
    check("and the whole release is not read out again", out.decode().count("[Approval needed] release job-1") == 1)
    send(want.group(1))
    check("the right prefix approves", wait_for(r"Approved\. The files are fixed", 30) is not None)
    send("/deliver")
    check("delivery reports in a line", wait_for(r"(?i)capsule", 60) is not None)
    send("/review payments-service for second-audit")
    check("a second review finishes", wait_for(r"job-\d+ finished: ready[\s\S]*job-\d+ finished: ready", 150) is not None)
    send("/approve")
    check("the second approval opens", wait_for(r"release job-3 \(appr-\d+\)", 30) is not None)
    send("refuse")
    check("refusing is a typed word", wait_for(r"Refused\. Nothing was approved", 15) is not None)
    send("hello there, which colour is the sky")
    check("a chat message is answered once it is finished", wait_for(r"\[Note\] The assistant is answering", 30) is not None and wait_for(r"\[Note\] Assistant|\[Note\]\n  ", 60) is not None)
    send("/roundtable")
    check("/roundtable alone lists the experts and how to ask", wait_for(r"security-reviewer\s+Security reviewer \(Engineering\)[\s\S]*Ask 2 to 5 of them", 30) is not None)
    send("/roundtable security-reviewer,data-analyst : should the key live in a vault")
    check("each expert's answer is read out whole, under its name, in order", wait_for(r"Roundtable: Security reviewer[\s\S]*scripted reply[\s\S]*Roundtable: Data analyst[\s\S]*scripted reply", 90) is not None)
    check("then the resolution, with the plain warning that it is advice", wait_for(r"Roundtable: resolution[\s\S]*advice, not a check", 60) is not None)
    check("and the half-written answers were never read out", "(being asked)" not in out.decode())
    send("/roundtable security-reviewer,the-boss : q")
    check("an unknown expert is refused in a line", wait_for(r"\[Error\] there is no expert with that name", 30) is not None)
    send("/sessions")
    check("a list is numbered and says how to choose", wait_for(r"Type a number and press Enter to open it", 30) is not None)
    send("cancel")
    pump(1.0)
    send("/forget note-nope")
    check("a yes/no question says how to answer", wait_for(r"Type y and press Enter to say yes", 15) is not None)
    send("n")
    pump(0.5)
    # what a command puts in the message line is read out, and an empty line sends it as it is
    send("/prompt save greet : Say hello to {{name}}")
    check("a saved prompt is kept", wait_for(r"Saved\. /prompt NAME puts it into the message line", 15) is not None)
    send("/prompt greet name=Asha")
    check("the filled prompt is read out from the message line", wait_for(r"In the message line now: Say hello to Asha", 15) is not None)
    send("")
    check("an empty line sends it as it is", wait_for(r"\[You\] Say hello to Asha", 30) is not None)
    send("/undo")
    check("a command name completed from a partial one is read out, not lost", wait_for(r"In the message line now: /undofix", 15) is not None)
    send("/status")
    pump(2.0)
    after = out.decode().split("In the message line now: /undofix")[-1]
    check("any other line replaces it", "[You] /undofix" not in out.decode() and "/undofix N" not in after)
    send("/quit")
    deadline = time.time() + 15
    while proc.poll() is None and time.time() < deadline:
        pump(0.2)
    pump(0.5)
    check("it says goodbye and exits cleanly", proc.poll() == 0 and b"Goodbye." in out, f"exit={proc.poll()}")
    check("not one control or escape byte was written", not re.search(rb"[\x00-\x08\x0b-\x1f\x7f]", out), repr(re.findall(rb"[\x00-\x08\x0b-\x1f\x7f]", out)[:5]))
finally:
    if proc.poll() is None:
        proc.kill()

left = subprocess.run(["pgrep", "-fl", f"kaku serve --data-dir {data}"], capture_output=True, text=True)
check("no service process is left behind", left.returncode != 0, left.stdout.strip()[:200])
(work / "output.txt").write_bytes(out)
# found in real use: piped lines all arrive at once, and a question followed by the end of the input must still get its
# answer printed before the program leaves
piped = subprocess.run([str(kaku), "tui", "--linear", "--data-dir", str(work / "data-eof"), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json")], input="/where\n", capture_output=True, text=True, timeout=120)
check("a question piped in, then the end of the input: its answer is printed before Goodbye", "Releases: nothing leaves without your approval" in piped.stdout and piped.stdout.rstrip().endswith("Goodbye."), piped.stdout[-400:])
piped = subprocess.run([str(kaku), "tui", "--linear", "--data-dir", str(work / "data-eof"), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json")], input="/where\n/quit\n", capture_output=True, text=True, timeout=120)
check("and the same with /quit right after it", "Releases: nothing leaves without your approval" in piped.stdout, piped.stdout[-400:])
subprocess.run(["pkill", "-f", f"serve --data-dir {work / 'data-eof'}"], capture_output=True)
bad = results.count(False)
print(f"\n{'all ' + str(len(results)) + ' plain-line checks passed' if not bad else str(bad) + ' FAILED'}")
if bad:
    print(out.decode("utf8", "replace")[-3000:])
sys.exit(1 if bad else 0)
