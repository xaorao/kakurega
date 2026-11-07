#!/usr/bin/env python3
"""The scripting commands (review, jobs, findings, chat) against a real service: output shapes, exit codes, and that none of them can approve."""
import json
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/script-cli-tmp"
subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work}"], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
shutil.copytree(repo / "fixtures/synthetic-repo", work / "root/payments-service")
os.rename(work / "root/payments-service/_env", work / "root/payments-service/.env")
data = work / "data"
kaku = str(repo / "target/debug/kaku")
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


def k(*args, stdin=None, timeout=300):
    p = subprocess.run([kaku, *args, "--data-dir", str(data)], input=stdin, capture_output=True, text=True, timeout=timeout)
    return p.returncode, p.stdout, p.stderr


srv = subprocess.Popen([kaku, "serve", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    for _ in range(150):
        if (data / "admin.json").exists():
            break
        time.sleep(0.1)
    admin = json.loads((data / "admin.json").read_text())
    base = f"http://127.0.0.1:{admin['port']}"
    adm = lambda path: json.load(urllib.request.urlopen(urllib.request.Request(base + path, headers={"Authorization": "Bearer " + admin["token"]})))

    rc, out, err = k("jobs", "--json")
    check("jobs --json on a fresh service is an empty list", rc == 0 and json.loads(out) == {"jobs": []}, f"{rc} {out} {err}")
    rc, out, _ = k("jobs")
    check("jobs without --json says so in words", rc == 0 and out.strip() == "no reviews yet", out)

    rc, out, err = k("review", str(work / "root/payments-service"), "--for", "acme-audit", "--json")
    events = [json.loads(l) for l in out.strip().splitlines()]
    check("review --json prints one JSON object per line", rc == 0 and len(events) >= 3 and all(isinstance(e, dict) for e in events), f"{rc} {out[:200]} {err}")
    check("it starts with `started`, reports stages, and ends at awaiting_approval", events[0]["event"] == "started" and any(e["event"] == "stage" for e in events) and events[-1]["event"] == "awaiting_approval" and events[-1]["next"] == "kaku approve", str(events[-1:]))
    job = events[0]["job"]
    check("no approval was requested or given by the command", adm("/admin/approvals")["approvals"] == [] and not adm(f"/admin/jobs/{job}").get("delivered"), str(adm("/admin/approvals")))
    check("a script gets the instruction, not a bypass", "cannot approve" in events[-1]["note"])

    rc, out, _ = k("jobs", "--json")
    listed = json.loads(out)["jobs"]
    check("jobs lists the review with its state", rc == 0 and listed and listed[0]["id"] == job and listed[0]["state"] == "ready", out)
    rc, out, _ = k("jobs")
    check("jobs in words is one line per review", rc == 0 and re.fullmatch(rf"{job}  ready  for acme-audit\n", out) is not None, repr(out))

    rc, out, _ = k("findings", job, "--json")
    f = json.loads(out)
    check("findings --json lists what the kernel confirmed", rc == 0 and f["job"] == job and f["findings"] and f["findings"][0]["status"] == "reproduced", out[:200])
    rc, out, _ = k("findings", job)
    check("findings in words names status, place and title", rc == 0 and re.search(rf"{job}  reproduced  payments/auth/session\.py:\d+  ", out) is not None, out)

    rc, out, err = k("chat", "hello there", "--json")
    c = json.loads(out)
    check("chat --json prints the reply and the route class", rc == 0 and "scripted reply" in c["reply"] and c["route"]["class"] == "device_only", out[:200])
    rc, out, _ = k("chat", "-", stdin="a message piped in\n")
    check("chat reads the message from standard input", rc == 0 and "received 18 characters" in out, out)

    # wrong use and refusals
    rc, out, err = k("review", "/no/such/folder", "--for", "x")
    check("a folder that does not exist is a usage error (2)", rc == 2 and "does not exist" in err, f"{rc} {err}")
    rc, _, err = k("review", str(work / "root/payments-service"))
    check("a review with no recipient is a usage error (2) that shows the usage", rc == 2 and "usage: kaku review" in err, err)
    rc, _, err = k("review", str(work), "--for", "x")
    check("a folder outside the allowed roots is refused by the service (6)", rc == 6, f"{rc} {err}")
    rc, _, err = k("findings", "not-a-job")
    check("a bad review name is a usage error (2)", rc == 2, err)
    rc, out, err = k("findings", "job-999999", "--json")
    e = json.loads(out)
    check("an unknown review is refused (6) with a JSON error", rc == 6 and e["event"] == "error" and e["code"] == 6, f"{rc} {out}")
    rc, _, err = k("chat", "")
    check("an empty message is a usage error (2)", rc == 2)
    # no control characters ever reach a script's output
    allout = "".join(k("jobs")[1] + k("findings", job)[1] for _ in range(1))
    check("no control character in any output", not re.search(r"[\x00-\x08\x0b-\x1f\x7f]", allout))
    # the other ways a review can end, against a stand-in service that answers as the real one does
    import threading
    from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
    mode = ["nothing"]

    class H(BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def send(self, code, obj):
            body = json.dumps(obj).encode()
            self.send_response(code)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            n = int(self.headers.get("content-length", 0))
            self.rfile.read(n)
            if self.path == "/admin/pairing":
                self.send(200, {"code": "c"})
            elif self.path == "/api/pair":
                self.send(200, {"csrf": "x"})
            elif self.path == "/api/jobs":
                self.send(202, {"id": "job-1"})
            elif self.path == "/api/chat":
                self.send(200, {"reply": "fine\u001b[2Jgone\u0007", "route": {"class": "device_only", "simulated": True}, "notes_used": 0})
            else:
                self.send(404, {})

        def do_GET(self):
            if self.path == "/api/jobs/job-1":
                state = {"nothing": "nothing_to_release", "failed": "failed", "running": "running"}[mode[0]]
                self.send(200, {"state": state, "note": "the agent stopped\u001b[2J early", "events": [{"stage": "import", "detail": "reading"}]})
            else:
                self.send(404, {})

    fake = ThreadingHTTPServer(("127.0.0.1", 0), H)
    threading.Thread(target=fake.serve_forever, daemon=True).start()
    fdata = work / "fake-data"
    fdata.mkdir()
    (fdata / "admin.json").write_text(json.dumps({"port": fake.server_port, "token": "t"}))
    kf = lambda *a, timeout=120: subprocess.run([kaku, *a, "--data-dir", str(fdata)], capture_output=True, text=True, timeout=timeout)
    mode[0] = "nothing"
    p = kf("review", str(work / "root/payments-service"), "--for", "x", "--json")
    last = json.loads(p.stdout.strip().splitlines()[-1])
    check("a review with nothing that can be released ends with exit 3 and says so", p.returncode == 3 and last["state"] == "nothing_to_release", f"{p.returncode} {p.stdout}")
    mode[0] = "failed"
    p = kf("review", str(work / "root/payments-service"), "--for", "x")
    check("a review that failed ends with exit 4 and its reason, cleaned of control characters", p.returncode == 4 and "the agent stopped" in p.stdout and "\x1b" not in p.stdout, f"{p.returncode} {p.stdout!r}")
    mode[0] = "running"
    p = kf("review", str(work / "root/payments-service"), "--for", "x", "--timeout", "10", timeout=60)
    check("a review that does not finish in the time given ends with exit 5 and says it is still running", p.returncode == 5 and "still running" in p.stderr, f"{p.returncode} {p.stderr}")
    p = kf("chat", "hello")
    check("a reply with control characters is cleaned before it reaches the terminal", p.returncode == 0 and not re.search(r"[\x00-\x08\x0b-\x1f\x7f]", p.stdout) and "fine" in p.stdout and "gone" in p.stdout, repr(p.stdout))
    fake.shutdown()
    # with no service at all the command starts one for its own use and stops it again; with no model behind it the answer is exit 5
    d2 = work / "own-service"
    p = subprocess.run([kaku, "chat", "hi", "--data-dir", str(d2), "--ollama", "http://127.0.0.1:9", "--root", str(work / "root")], capture_output=True, text=True, timeout=120)
    check("with no service running the command starts its own, and a model that cannot be reached is exit 5", p.returncode == 5, f"{p.returncode} {p.stderr[:200]}")
    left = subprocess.run(["pgrep", "-f", f"kaku serve --data-dir {d2}"], capture_output=True, text=True)
    check("and it stops that service when it is done", left.returncode != 0, left.stdout)
finally:
    srv.kill()
    subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work}"], capture_output=True)

bad = results.count(False)
print(f"\nall {len(results)} scripting checks passed" if not bad else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
