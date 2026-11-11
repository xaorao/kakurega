#!/usr/bin/env python3
"""`/team` in the real terminal workspace (plain-line mode, through pipes): plan, start, the expert's question, the answer,
the result. The model is the stand-in server of tests/e2e/team_script.py, reached through the real local-model route."""
import os
import re
import select
import shutil
import subprocess
import sys
import time
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/tui-team-tmp"
subprocess.run(["pkill", "-f", "kaku serve --data-dir " + str(work)], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
data = work / "data"
kaku = repo / "target/debug/kaku"
port = 19500 + os.getpid() % 400
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


model = subprocess.Popen([sys.executable, str(repo / "tests/e2e/team_script.py"), str(port)])
time.sleep(0.6)
proc = subprocess.Popen(
    [str(kaku), "tui", "--linear", "--data-dir", str(data), "--root", str(work / "root"), "--model", "team-script", "--ollama", f"http://127.0.0.1:{port}", "--ui-dir", str(repo / "apps/web/dist")],
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
    check("the workspace starts", wait_for(r"plain-line mode", 60) is not None)
    send("/team")
    check("/team alone says there are no runs and how to make one", wait_for(r"No team runs yet\. /team plan GOAL", 30) is not None)
    send("/team plan refuse-this-plan please")
    check("an unusable plan is refused in a line", wait_for(r"\[Error\] the lead did not write a usable plan", 60) is not None)
    send("/team plan Decide where our payment key should live.")
    m = wait_for(r"Team run (team-[0-9a-f]{12})", 60)
    check("a plan is read out with its tasks, and nothing has run", m is not None and wait_for(r"3\. \[waiting\] Plain-language writer \(after 1, 2\)", 10) is not None and wait_for(r"Nothing has run\. /team start runs this plan", 10) is not None)
    send("/team start")
    check("progress is said task by task", wait_for(r"Task 1 \(Security reviewer\) is done\.", 60) is not None)
    check("the expert's question arrives with how to answer it", wait_for(r"Task 2 asks: Which cloud do you deploy to\?\s+Answer with: /team answer 2 YOUR ANSWER", 60) is not None)
    check("and the owner is told a question is waiting", wait_for(r"\[Notice\] An expert has a question for you", 15) is not None)
    send("/team answer 2 Our own cloud in Mumbai.")
    check("after the answer the run ends with each task's result and the lead's", wait_for(r"Team: task 3 · Plain-language writer[\s\S]*Team: result[\s\S]*Keep the payment key in the vault[\s\S]*advice, not a check", 90) is not None)
    send("/team")
    check("the list shows it finished", wait_for(rf"{m.group(1) if m else 'x'}  done\s+3 of 3 done", 30) is not None)
    send("/quit")
    deadline = time.time() + 15
    while proc.poll() is None and time.time() < deadline:
        pump(0.2)
    check("it exits cleanly", proc.poll() == 0, f"exit={proc.poll()}")
    check("not one control or escape byte was written", not re.search(rb"[\x00-\x08\x0b-\x1f\x7f]", out), repr(re.findall(rb"[\x00-\x08\x0b-\x1f\x7f]", out)[:5]))
finally:
    if proc.poll() is None:
        proc.kill()
    model.kill()
(work / "output.txt").write_bytes(out)
bad = results.count(False)
print(f"\n{'all ' + str(len(results)) + ' terminal team checks passed' if not bad else str(bad) + ' FAILED'}")
if bad:
    print(out.decode("utf8", "replace")[-2500:])
sys.exit(1 if bad else 0)
