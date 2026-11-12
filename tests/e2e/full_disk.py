#!/usr/bin/env python3
"""A full disk, made for real with a small disk image. On a full disk SQLite can still write now and then (space a failed
write left behind is reused), so every attempt is checked against the rule rather than assumed to fail:
  - a review starts only if its start is in the signed log, and a review that is refused leaves nothing behind;
  - while the log is missing entries, the readiness checks say so, and once there is room the first entry written says
    how many were missed, and the log still verifies;
  - a turn of a kept conversation that could not be saved makes `kaku chat` fail, and one that was saved does not;
  - with ANTHROPIC_API_KEY set (the outside test route), a question goes to the model elsewhere only if the log recorded
    it: every answered question has its outbound entry, and a refused one says the log could not record it.
Needs hdiutil (macOS)."""
import http.server
import json
import os
import shutil
import subprocess
import sys
import threading
import time
import urllib.request
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/full-disk-tmp"
mnt = work / "mnt"
data = mnt / "data"
kaku = str(repo / "target/debug/kaku")
results = []

if sys.platform != "darwin":
    print("SKIP  a full disk is made with hdiutil, which only macOS has")
    sys.exit(0)


def check(name, ok, detail=""):
    ok = bool(ok)
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


def k(*args, env=None, timeout=180):
    p = subprocess.run([kaku, *args, "--data-dir", str(data)], capture_output=True, text=True, timeout=timeout, env=env)
    return p.returncode, p.stdout, p.stderr


subprocess.run(["pkill", "-f", f"kaku serve --data-dir {data}"], capture_output=True)
subprocess.run(["hdiutil", "detach", str(mnt), "-force", "-quiet"], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
shutil.copytree(repo / "fixtures/synthetic-repo", work / "root/payments-service")
os.rename(work / "root/payments-service/_env", work / "root/payments-service/.env")
# HFS+: once the filler is written and synced, a new write fails for good (an APFS image keeps a reserve and lets some through)
subprocess.run(["hdiutil", "create", "-size", "64m", "-fs", "HFS+", "-volname", "kkfull", "-type", "UDIF", str(work / "disk.dmg"), "-quiet"], check=True)
subprocess.run(["hdiutil", "attach", str(work / "disk.dmg"), "-mountpoint", str(mnt), "-nobrowse", "-quiet"], check=True)

# a stand-in for Ollama on this computer, counting what reached it
received = []


class Model(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def send(self, code, body, kind="application/json"):
        b = body.encode()
        self.send_response(code)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        if self.path.startswith("/api/tags"):
            self.send(200, json.dumps({"models": [{"name": "fake:latest"}]}))
        else:
            self.send(404, "{}")

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length") or 0)) or b"{}")
        if not self.path.startswith("/api/chat"):
            self.send(404, "{}")
            return
        received.append(json.dumps(body))
        msg = {"role": "assistant", "content": "fake answer"}
        end = {"done": True, "prompt_eval_count": 10, "eval_count": 2}
        if body.get("stream"):
            self.send(200, json.dumps({"message": msg, "done": False}) + "\n" + json.dumps({"message": {"role": "assistant", "content": ""}, **end}) + "\n", "application/x-ndjson")
        else:
            self.send(200, json.dumps({"message": msg, **end}))


model = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Model)
threading.Thread(target=model.serve_forever, daemon=True).start()
port = model.server_port
filler = None


def fill():
    """Writes zeros until the disk refuses even one byte, and syncs, so the space is really gone."""
    global filler
    filler = os.open(mnt / "filler", os.O_WRONLY | os.O_CREAT | os.O_APPEND)
    for size in (1 << 20, 1 << 16, 4096, 512, 1):
        chunk = b"\0" * size
        while True:
            try:
                if os.write(filler, chunk) < size:
                    break
            except OSError:
                break
    try:
        os.fsync(filler)
    except OSError:
        pass
    st = os.statvfs(mnt)
    return st.f_bavail * st.f_frsize


def free():
    global filler
    if filler is not None:
        os.close(filler)
        filler = None
    os.remove(mnt / "filler")


def serve(*extra, env=None):
    p = subprocess.Popen([kaku, "serve", "--data-dir", str(data), "--root", str(work / "root"), "--ollama", f"http://127.0.0.1:{port}", "--model", "fake", "--ui-dir", str(repo / "apps/web/dist"), *extra], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)
    for _ in range(300):
        try:
            a = json.loads((data / "admin.json").read_text())
            urllib.request.urlopen(f"http://127.0.0.1:{a['port']}/api/health", timeout=1)
            return p, a
        except Exception:
            time.sleep(0.1)
    return p, json.loads((data / "admin.json").read_text())


def stop(p):
    p.terminate()
    try:
        p.wait(timeout=20)
    except subprocess.TimeoutExpired:
        p.kill()
    (data / "admin.json").unlink(missing_ok=True)


def adm(a, path):
    return json.load(urllib.request.urlopen(urllib.request.Request(f"http://127.0.0.1:{a['port']}{path}", headers={"Authorization": "Bearer " + a["token"]})))


def log_entries(a):
    req = urllib.request.Request(f"http://127.0.0.1:{a['port']}/admin/log", headers={"Authorization": "Bearer " + a["token"]})
    text = urllib.request.urlopen(req).read().decode()
    return text, [json.loads(e["entry"]) for e in json.loads(text)["entries"]]


def log_problem(a):
    return "Signed log" in adm(a, "/admin/overview")["setup_problems"]


srv = None
try:
    # ---- the model is on this computer
    srv, a = serve()
    rc, out, err = k("chat", "--keep", "--json", "Where are login sessions kept?")
    first = json.loads(out) if rc == 0 and out.strip() else {}
    sid = first.get("session")
    check("a kept chat on a disk with room is answered and saved", rc == 0 and first.get("reply") == "fake answer" and first.get("saved") is True and sid, f"{rc} {out} {err}")
    left = fill()
    check("the disk is full", left == 0, str(left))

    # reviews first, while nothing has written since the disk filled: each attempt is either refused with nothing left
    # behind, or started with its start in the log
    refused, started, problem_seen = 0, [], False
    for _ in range(8):
        before = {j["id"] for j in adm(a, "/admin/jobs").get("jobs", [])}
        rc, out, err = k("review", str(work / "root/payments-service"), "--for", "acme-audit", "--json")
        now = {j["id"] for j in adm(a, "/admin/jobs").get("jobs", [])}
        if "signed log could not be written" in out + err:
            refused += 1
            check("a refused review leaves no review behind", now == before, f"{before} -> {now}")
            # nothing has been written since: the readiness checks name the log
            problem_seen = problem_seen or log_problem(a)
            break
        started += sorted(now - before)
    check("on a full disk a review was refused because its start could not be recorded", refused >= 1, f"refused {refused}, started {started}")
    check("while entries are missing, the readiness checks name the signed log", problem_seen)

    # a kept turn: whatever the store managed, the command's exit code and words match what happened
    turns = []
    for i in range(6):
        rc, out, err = k("chat", "--session", sid or "", "--json", f"Follow-up question {i}")
        v = json.loads(out.strip().splitlines()[0]) if out.strip() else {}
        turns.append((rc, v))
    lost = [t for t in turns if t[1].get("saved") is False]
    check("on a full disk a kept turn is at least once not saved", lost, json.dumps(turns)[:400])
    check("a turn not saved is still answered, says why, and the command fails; a saved one succeeds", all((rc == 4 and v.get("reply") == "fake answer" and v.get("not_saved")) if v.get("saved") is False else (rc == 0 and v.get("saved") is True) for rc, v in turns), json.dumps(turns)[:400])
    rc, out, err = k("chat", "--session", sid or "", "And who can read them?")
    check("in words a lost turn says it was NOT saved and why", (rc == 4 and "NOT saved" in err and "disk" in err) or rc == 0, f"{rc} {out} {err}")

    # notes brought in from another assistant: a note the store could not write is counted apart from refused ones, and the
    # command fails when any was not saved
    notes = work / "notes"
    notes.mkdir(exist_ok=True)
    for i in range(3):
        (notes / f"note-{i}.md").write_text(f"The payments service retries a failed charge {i + 2} times.\n")
    rc, out, err = k("memory", "import", str(notes), "--source", "claude-code")
    check("importing notes on a full disk says how many could NOT be saved, and fails", rc == 1 and "could NOT be saved" in err and "refused 0" in out, f"{rc} {out} {err}")

    free()
    rc, out, err = k("chat", "--json", "Question once there is room again")
    text, log = log_entries(a)
    kinds = [e["kind"] for e in log]
    gaps = [e for e in log if e["kind"] == "log.unwritten"]
    check("once there is room the gap is written, saying how many entries were missed", rc == 0 and gaps and all(int(g["data"]["entries"]) >= 1 for g in gaps), f"{rc} {err} {kinds[-8:]}")
    check("every review that did start has its start in the log", all(any(e["kind"] == "job.started" and e["subject"] == j for e in log) for j in started), str(started))
    (work / "log.json").write_text(text)
    rc, out, err = k("log", "verify", str(work / "log.json"))
    check("the exported log still verifies", rc == 0 and "VALID" in out, f"{rc} {out} {err}")
    check("the readiness checks are clear again", not log_problem(a))
    stop(srv)
    srv = None

    # ---- a model elsewhere: the outside test route, only with a key for it
    key = os.environ.get("ANTHROPIC_API_KEY")
    if not key:
        print("SKIP  a model elsewhere: set ANTHROPIC_API_KEY (the outside test route) to check that nothing leaves unless the log recorded it")
    else:
        env = dict(os.environ)
        srv, a = serve("--route", "outside", "--outside-model", os.environ.get("KK_OUTSIDE_MODEL", "anthropic:claude-haiku-5-5"), env=env)
        check("the outside route counts as elsewhere", adm(a, "/admin/overview")["route"]["stays_here"] is False, json.dumps(adm(a, "/admin/overview")["route"])[:300])
        rc, out, err = k("chat", "--json", "Reply with the single word: ok", env=env)
        _, log = log_entries(a)
        check("with room, a question goes to the model elsewhere and its outbound entry is in the log", rc == 0 and any(e["kind"] == "outbound.model" for e in log), f"{rc} {out} {err}")
        before_out = sum(e["kind"] == "outbound.model" for e in log)
        fill()
        answered, refused = 0, 0
        for i in range(8):
            rc, out, err = k("chat", "--json", f"Reply with the single word: ok ({i})", env=env)
            if rc == 0:
                answered += 1
            elif "signed log could not be written" in out + err:
                refused += 1
                if refused >= 2:
                    break
        free()
        _, log = log_entries(a)
        after_out = sum(e["kind"] == "outbound.model" for e in log)
        check("on a full disk a question to the model elsewhere was refused because the log could not record it", refused >= 1, f"answered {answered} refused {refused}")
        check("every question that was answered there has its outbound entry", after_out - before_out >= answered, f"{after_out - before_out} entries for {answered} answers")
finally:
    if srv:
        stop(srv)
    model.shutdown()
    if filler is not None:
        os.close(filler)
    subprocess.run(["hdiutil", "detach", str(mnt), "-force", "-quiet"], capture_output=True)

print(f"{sum(results)}/{len(results)} full-disk checks passed")
sys.exit(0 if results and all(results) else 1)
