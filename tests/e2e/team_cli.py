#!/usr/bin/env python3
"""`kaku team` as a script uses it: plan, show, start and wait, answer the expert's question, list, delete.
The model is the stand-in server of tests/e2e/team_script.py, reached through the real local-model route."""
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/team-cli-tmp"
subprocess.run(["pkill", "-f", "kaku serve --data-dir " + str(work)], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
data = work / "data"
kaku = repo / "target/debug/kaku"
port = 19000 + os.getpid() % 1000
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


model = subprocess.Popen([sys.executable, str(repo / "tests/e2e/team_script.py"), str(port)])
time.sleep(0.6)
common = ["--data-dir", str(data), "--root", str(work / "root"), "--model", "team-script", "--ollama", f"http://127.0.0.1:{port}"]
server = subprocess.Popen([str(kaku), "serve", *common, "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def k(*args):
    p = subprocess.run([str(kaku), "team", *args, *common], capture_output=True, text=True, timeout=120)
    return p.returncode, p.stdout, p.stderr


try:
    deadline = time.time() + 30
    while not (data / "admin.json").exists() and time.time() < deadline:
        time.sleep(0.1)
    rc, out, err = k()
    check("without words it prints the usage, exit 2", rc == 2 and "usage: kaku team plan GOAL" in err, f"{rc} {err}")
    rc, out, err = k("show", "team-nope")
    check("an id that is not a run id is a usage error, and nothing is asked of the service", rc == 2, f"{rc} {err}")
    rc, out, err = k("list")
    check("an empty list says so", rc == 0 and out.strip() == "No team runs yet.", f"{rc} {out} {err}")
    rc, out, err = k("plan", "refuse-this-plan please")
    check("an unusable plan is refused in words, exit 6", rc == 6 and "did not write a usable plan" in err and "Nothing was started" in err, f"{rc} {err}")
    rc, out, err = k("plan", "Say it.", "--experts", "plain-writer,the-boss")
    check("an unknown expert is refused before the lead is asked", rc == 6 and "no expert with that name" in err, f"{rc} {err}")
    rc, out, err = k("plan", "Decide where the payment key lives.")
    m = re.search(r"^(team-[0-9a-f]{12})  planned$", out, re.M)
    check("a plan prints its id, its tasks with experts and order, and says nothing has run", rc == 0 and m is not None and "1. [waiting] Security reviewer: List the ways" in out and "3. [waiting] Plain-language writer (after 1, 2):" in out and "Nothing has run. Start it with: kaku team start " in out, f"{rc} {out} {err}")
    rid = m.group(1)
    rc, out, err = k("show", rid, "--json")
    shown = json.loads(out)
    check("show --json is one object with the tasks", rc == 0 and shown["status"] == "planned" and [t["state"] for t in shown["tasks"]] == ["waiting"] * 3, out[:200])
    rc, out, err = k("start", rid, "--wait")
    check("start --wait follows the run until an expert asks the owner, exit 3", rc == 3 and "2. [needs_input] Software architect" in out and "asks you: Which cloud do you deploy to?" in out and "1. [done] Security reviewer" in out, f"{rc} {out} {err}")
    check("a result's lines are printed under its task, without control characters", "   1. It leaks through logs." in out and "\x1b" not in out)
    rc, out, err = k("answer", rid, "3", "x")
    check("answering a task that did not ask is refused", rc == 6 and "not waiting for an answer" in err, f"{rc} {err}")
    rc, out, err = k("answer", rid, "2", "Our own cloud in Mumbai.", "--wait")
    check("the answer carries the run on to its end, with the lead's result", rc == 0 and re.search(r"^team-[0-9a-f]{12}  done$", out, re.M) is not None and "Result:\n**Keep the payment key in the vault.**" in out and "3. [done] Plain-language writer" in out, f"{rc} {out} {err}")
    rc, out, err = k("start", rid)
    check("an ended run is not started again", rc == 6 and "has ended" in err, f"{rc} {err}")
    rc, out, err = k("list")
    check("the list shows the run finished, 3 of 3", rc == 0 and re.search(rf"^{rid}  done\s+3 of 3 done  Decide where", out, re.M) is not None, out)
    rc, out, err = k("plan", "Decide again.")
    rid2 = re.search(r"^(team-[0-9a-f]{12})  planned$", out, re.M).group(1)
    rc, out, err = k("stop", rid2)
    check("a planned run can be put away", rc == 0 and out.strip() == "Stopped.", f"{rc} {out}")
    rc, out, err = k("delete", rid2)
    rc2, out2, _ = k("show", rid2)
    check("delete removes it", rc == 0 and out.strip() == "Deleted." and rc2 == 6, f"{rc} {out} {rc2}")
finally:
    server.kill()
    model.kill()
bad = results.count(False)
print(f"\n{'all ' + str(len(results)) + ' team command checks passed' if not bad else str(bad) + ' FAILED'}")
sys.exit(1 if bad else 0)
