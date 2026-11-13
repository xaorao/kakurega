#!/usr/bin/env python3
"""A review of a change (a branch against its base) through the real commands: the committed version is reviewed, only
findings that touch the change are offered, and the release states the two commits. The same model script reviewing the
whole folder is the control."""
import json
import os
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/change-review-tmp"
subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work}"], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
proj = work / "root/payments-service"
(work / "root").mkdir(parents=True)
shutil.copytree(repo / "fixtures/synthetic-repo", proj)
os.remove(proj / "_env")
data = work / "data"
kaku = str(repo / "target/debug/kaku")
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


def git(*args):
    env = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@example.com", GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@example.com", GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_SYSTEM="/dev/null")
    p = subprocess.run(["/usr/bin/git", "-C", str(proj), *args], capture_output=True, text=True, env=env)
    assert p.returncode == 0, p.stderr
    return p.stdout.strip()


def k(*args, timeout=600):
    p = subprocess.run([kaku, *args, "--data-dir", str(data)], capture_output=True, text=True, timeout=timeout)
    return p.returncode, p.stdout, p.stderr


# main: logout really ends the session. The branch: the change that breaks it.
session = proj / "payments/auth/session.py"
broken = session.read_text()
fixed = broken.replace('    # Clears the browser cookie on the client side; the server keeps the entry.\n    cookie_jar = {"session": token}\n    cookie_jar.pop("session", None)\n', "    ACTIVE_TOKENS.pop(token, None)\n")
assert fixed != broken
session.write_text(fixed)
git("init", "-q", "-b", "main")
git("add", "-A")
git("commit", "-q", "-m", "payments service")
base_commit = git("rev-parse", "HEAD")
git("checkout", "-q", "-b", "faster-logout")
session.write_text(broken)
git("commit", "-q", "-am", "logout: skip the table")
head_commit = git("rev-parse", "HEAD")
# on disk only, never committed: must not be part of a review of the change
(proj / "scratch_notes.py").write_text("TODO = 'not committed'\n")

srv = subprocess.Popen([kaku, "serve", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/change-script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    for _ in range(150):
        if (data / "admin.json").exists():
            break
        time.sleep(0.1)
    admin = json.loads((data / "admin.json").read_text())
    base = f"http://127.0.0.1:{admin['port']}"

    import http.cookiejar

    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

    def post(path, body, headers):
        req = urllib.request.Request(base + path, data=json.dumps(body).encode(), headers={"Content-Type": "application/json", **headers}, method="POST")
        return json.load(opener.open(req))

    code = post("/admin/pairing", {}, {"Authorization": "Bearer " + admin["token"]})["code"]
    post("/api/pair", {"code": code}, {"Origin": base, "Sec-Fetch-Site": "same-origin", "x-kk-csrf": ""})

    def own(path):
        return json.load(opener.open(base + path))

    # control: the whole folder, same model script: both findings are confirmed
    rc, out, err = k("review", str(proj), "--for", "acme-audit", "--json")
    ev = [json.loads(l) for l in out.strip().splitlines()]
    whole = ev[0]["job"]
    rc, out, _ = k("findings", whole, "--json")
    f = json.loads(out)["findings"]
    check("control: reviewing the whole folder confirms both findings", len(f) == 2 and all(x.get("status") == "reproduced" for x in f), out[:600])

    # the change
    rc, out, err = k("review", str(proj), "--for", "acme-audit", "--change", "main", "--json")
    ev = [json.loads(l) for l in out.strip().splitlines()] if out.strip() else []
    check("review --change runs to awaiting approval", rc == 0 and ev and ev[-1]["event"] == "awaiting_approval", f"{rc} {out[-400:]} {err[-300:]}")
    job = ev[0]["job"] if ev else ""
    view = own(f"/api/jobs/{job}")
    ch = view.get("change") or {}
    check("the review records the two commits it read from the repository", ch.get("base") == base_commit and ch.get("head") == head_commit and ch.get("base_name") == "main", json.dumps(view)[:400])
    check("and the one changed file with the lines the change wrote", [(x["path"], x["kind"], x["lines"]) for x in ch.get("files", [])] == [("payments/auth/session.py", "modified", [[16, 18]])], json.dumps(ch.get("files"))[:300])
    rc, out, _ = k("findings", job, "--json")
    f = {x["file"]: x for x in json.loads(out)["findings"]}
    s, t = f.get("payments/auth/session.py", {}), f.get("payments/auth/tokens.py", {})
    check("the finding in the changed file is confirmed", s.get("status") == "reproduced", json.dumps(s)[:300])
    check("the finding in untouched code is not offered, and says why", t.get("status", "").startswith("unavailable") and "did not touch" in t.get("status", ""), json.dumps(t)[:400])
    rel = own(f"/api/jobs/{job}/release")
    names = [x["name"] for x in rel.get("files", [])]
    note = next((x.get("text", "") for x in rel.get("files", []) if x["name"] == "change.txt"), "")
    check("the release the owner is asked to approve states the two commits", f"reviewed commit: {head_commit}\n" in note and f"base commit: {base_commit}\n" in note, note)
    check("and holds one finding, not two", "finding-1.txt" in names and "finding-2.txt" not in names, str(names))
    # the statement says how its digest is made: do exactly that, with git's own answers
    import hashlib

    made = hashlib.sha256(f"base {base_commit}\nhead {head_commit}\nmodified payments/auth/session.py\n".encode()).hexdigest()
    check("the digest recomputed from the statement's own instructions matches", f"change digest (SHA-256): {made}\n" in note, made + " " + note)
    check("no file name of the project is in the statement", "session.py" not in note)
    check("the uncommitted file was not part of the review", "scratch_notes" not in json.dumps(own(f"/api/jobs/{job}/files")))
    pre = own("/api/change?repo=" + urllib.request.quote(str(proj)) + "&base=main")
    check("the preview shows the same change before any review starts", pre.get("digest") == ch.get("digest") and pre.get("head") == head_commit, json.dumps(pre)[:300])

    # refusals, in words, before anything starts
    for arg, words in [("no-such-branch", "no branch, tag or commit"), ("--upload-pack=x", ""), ("faster-logout", "nothing")]:
        rc, out, err = k("review", str(proj), "--for", "acme-audit", "--change", arg, "--json")
        check(f"--change {arg} is refused without starting a review", rc != 0 and words in (out + err), f"{rc} {out[-300:]} {err[-300:]}")
    plain = work / "root/plain"
    plain.mkdir()
    (plain / "a.py").write_text("x = 1\n")
    rc, out, err = k("review", str(plain), "--for", "acme-audit", "--change", "main", "--json")
    check("a folder that is not a repository is refused in words", rc != 0 and "repository" in out + err, out + err)
    n = len(json.loads(k("jobs", "--json")[1])["jobs"])
    check("none of the refusals left a review behind", n == 2, str(n))
finally:
    srv.terminate()
    srv.wait(timeout=20)
print(f"{sum(results)} of {len(results)} checks passed")
sys.exit(0 if all(results) else 1)
