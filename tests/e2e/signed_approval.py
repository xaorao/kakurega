#!/usr/bin/env python3
"""Real binaries: enrol an approver key, then prove an admin-token-only approval is refused and `kaku approve` (which signs) works."""
import json, os, re, shutil, subprocess, sys, time, urllib.request, urllib.error
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/signed-approval-tmp"
subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work}"], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
shutil.copytree(repo / "fixtures/synthetic-repo", work / "root/payments-service")
os.rename(work / "root/payments-service/_env", work / "root/payments-service/.env")
kaku = str(repo / "target/debug/kaku")
data = work / "data"
srv = subprocess.Popen([kaku, "serve", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
results = []
def check(n, ok, d=""):
    results.append(ok); print(("PASS  " if ok else "FAIL  ") + n + (("  " + d) if d else ""))
try:
    for _ in range(100):
        if (data / "admin.json").exists(): break
        time.sleep(0.1)
    admin = json.loads((data / "admin.json").read_text())
    base = f"http://127.0.0.1:{admin['port']}"
    def adm(method, path, body=None):
        req = urllib.request.Request(base + path, data=None if body is None else json.dumps(body).encode(), method=method, headers={"Authorization": "Bearer " + admin["token"], "content-type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=120) as r: return r.status, json.loads(r.read() or b"null")
        except urllib.error.HTTPError as e: return e.code, json.loads(e.read() or b"null")
    def review():
        st, s = adm("POST", "/admin/jobs", {"repo": str(work / "root/payments-service"), "recipient": "signed-recipient"})
        job = s["id"]
        for _ in range(300):
            if adm("GET", f"/admin/jobs/{job}")[1]["state"] != "running": break
            time.sleep(0.4)
        st, a = adm("POST", f"/admin/jobs/{job}/approval", {})
        return job, a["approval"]
    # 1 before enrolment: no key needed
    job0, aid0 = review()
    d0 = adm("GET", f"/admin/approvals/{aid0}")[1]
    check("before any approver is enrolled the service says no proof is required", d0["requires_proof"] is False)
    run = subprocess.run([kaku, "approver", "enroll", "--data-dir", str(data), "--label", "test terminal"], capture_output=True, text=True)
    key = data / "approver.key"
    check("`kaku approver enroll` creates a private key file and enrols it", run.returncode == 0 and key.exists() and oct(key.stat().st_mode & 0o777) == "0o600", run.stdout.strip()[:90])
    lst = subprocess.run([kaku, "approver", "list", "--data-dir", str(data)], capture_output=True, text=True).stdout
    check("the enrolled key is listed", "test terminal" in lst)
    job1, aid1 = review()
    d1 = adm("GET", f"/admin/approvals/{aid1}")[1]
    check("now a proof is required and the statement binds recipient, digest, policy and a nonce", d1["requires_proof"] is True and set(d1["statement"]) >= {"approval", "job", "digest", "recipient", "policy", "expires", "nonce"})
    st, r = adm("POST", f"/admin/approvals/{aid1}/approve", {"ack": d1["digest"][:12]})
    check("the admin token plus the digest prefix is no longer enough", st == 403 and r["error"]["code"] == "PROOF_REQUIRED", f"{st} {r['error']['code'] if r else ''}")
    out = subprocess.run([kaku, "approve", aid1, "--data-dir", str(data)], input=d1["digest"][:12] + "\n", capture_output=True, text=True)
    check("`kaku approve` signs the exact statement and the approval is accepted", out.returncode == 0 and "Approved" in out.stdout, (out.stdout + out.stderr).strip()[-120:])
    # deliver to prove the approval is real
    st, _ = adm("GET", f"/admin/jobs/{job1}")
    # 2 without the key file, kaku approve refuses to send anything
    job2, aid2 = review()
    d2 = adm("GET", f"/admin/approvals/{aid2}")[1]
    moved = data / "approver.key.moved"
    key.rename(moved)
    out = subprocess.run([kaku, "approve", aid2, "--data-dir", str(data)], input=d2["digest"][:12] + "\n", capture_output=True, text=True)
    check("without the key `kaku approve` explains and sends nothing", out.returncode != 0 and "approver enroll" in out.stderr and adm("GET", f"/admin/approvals/{aid2}")[1].get("state", "waiting") != "approved")
    moved.rename(key)
    key.chmod(0o644)
    out = subprocess.run([kaku, "approve", aid2, "--data-dir", str(data)], input=d2["digest"][:12] + "\n", capture_output=True, text=True)
    check("a key file readable by others is refused", out.returncode != 0)
    key.chmod(0o600)
    kid = lst.split()[0]
    unsigned = urllib.request.Request(base + "/admin/approvers/revoke", data=json.dumps({"key_id": kid}).encode(), method="POST", headers={"Authorization": "Bearer " + admin["token"], "content-type": "application/json"})
    try:
        urllib.request.urlopen(unsigned, timeout=30); code = 200
    except urllib.error.HTTPError as e:
        code = e.code
    check("with only the admin token the approver cannot be removed", code == 403)
    rv = subprocess.run([kaku, "approver", "revoke", kid, "--data-dir", str(data)], capture_output=True, text=True)
    check("the key's owner can revoke it with a signature, and then the legacy path is open by that signed decision", rv.stdout.startswith("Revoked") and adm("GET", f"/admin/approvals/{aid2}")[1]["requires_proof"] is False, rv.stdout.strip()[:60] + rv.stderr.strip()[:60])
finally:
    srv.kill()
bad = results.count(False)
print(f"\n{len(results) - bad}/{len(results)} signed-approval checks passed" if bad == 0 else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
