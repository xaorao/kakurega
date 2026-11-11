#!/usr/bin/env python3
"""Stopping a real review: the real binary, the scripted model, the real sandbox. A review is started and stopped while its
worker runs; it must end quickly as stopped, offer nothing, leave no worker behind, and the next review must still work."""
import json
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/stop-review-tmp"
subprocess.run(["pkill", "-f", "kaku serve --data-dir " + str(work)], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
shutil.copytree(repo / "fixtures/synthetic-repo", work / "root/payments-service")
(work / "root/payments-service/_env").rename(work / "root/payments-service/.env")
data = work / "data"
kaku = str(repo / "target/debug/kaku")
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


def k(*args, timeout=300):
    p = subprocess.run([kaku, *args, "--data-dir", str(data)], capture_output=True, text=True, timeout=timeout)
    return p.returncode, p.stdout, p.stderr


srv = subprocess.Popen([kaku, "serve", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    for _ in range(150):
        if (data / "admin.json").exists():
            break
        time.sleep(0.1)
    rc, out, err = k("stop", "job-1")
    check("stopping when nothing runs is refused in words", rc == 6 and "nothing with that id is running" in err, f"{rc} {err}")
    rc, out, err = k("stop", "review-1")
    check("an id that is not a review, coding task or team run is a usage error", rc == 2 and "usage: kaku stop ID" in err, f"{rc} {err}")

    # the review runs in its own thread; it is stopped as soon as the list of work shows it
    box = {}
    t = threading.Thread(target=lambda: box.update(r=k("review", str(work / "root/payments-service"), "--for", "acme-audit", "--json")))
    started = time.time()
    t.start()
    job = None
    while time.time() - started < 30 and job is None:
        rc, out, _ = k("work", "--json")
        items = json.loads(out)["items"] if out.strip().startswith("{") else []
        job = next((i["id"] for i in items if i["kind"] == "review"), None)
        time.sleep(0.05)
    check("the running review is in the list of work, and can be stopped", job is not None)
    rc, out, err = k("stop", job or "job-1")
    asked = time.time()
    check("kaku stop says it is stopping now", rc == 0 and out.strip() == "Stopping now.", f"{rc} {out} {err}")
    t.join(timeout=60)
    took = time.time() - asked
    rc, out, _ = k("jobs", "--json")
    listed = json.loads(out)["jobs"]
    check("the review ended as failed within seconds of the stop", bool(listed) and listed[0]["state"] == "failed" and took < 20, f"{listed} after {took:.1f}s")
    r = box.get("r", (None, "", ""))
    events = [json.loads(l) for l in r[1].strip().splitlines() if l.strip().startswith("{")]
    check("the command that was following it reports the stop, not a release", r[0] not in (0, None) and "stopped by you" in (r[1] + r[2]) and not any(e.get("event") == "awaiting_approval" for e in events), f"{r[0]} {r[1][-300:]} {r[2][-200:]}")
    rc, out, err = k("release", job or "job-1")
    check("nothing it found is offered for release", "stopped by you" in (out + err) or rc != 0, f"{rc} {out[:200]} {err[:200]}")
    rc, out, _ = k("work", "--json")
    check("nothing is left in the list of work", rc == 0 and json.loads(out)["count"] == 0, out)
    left = subprocess.run(["limactl", "shell", "kak", "--", "bash", "-lc", "systemctl --user list-units 'kk-worker-*' --no-legend --plain 2>/dev/null | grep -c running || true"], capture_output=True, text=True)
    check("no sandboxed worker is left running", left.stdout.strip().splitlines()[-1:] == ["0"] or left.returncode != 0 and "kak" not in left.stderr, left.stdout + left.stderr)

    # the next review is untouched by the earlier stop
    rc, out, err = k("review", str(work / "root/payments-service"), "--for", "second-audit", "--json")
    events = [json.loads(l) for l in out.strip().splitlines()]
    check("a review started afterwards runs to its end and is ready", rc == 0 and any(e.get("event") == "awaiting_approval" for e in events), f"{rc} {out[-300:]} {err}")
finally:
    srv.kill()
bad = results.count(False)
print(f"\n{'all ' + str(len(results)) + ' stop checks passed' if not bad else str(bad) + ' FAILED'}")
sys.exit(1 if bad else 0)
