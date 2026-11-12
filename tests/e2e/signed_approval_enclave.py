#!/usr/bin/env python3
"""Real binaries: enrol an approver key, then prove an admin-token-only approval is refused and `kaku approve` (which signs) works."""
import json, os, re, shutil, subprocess, sys, time, urllib.request, urllib.error
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from term_input import run_tty  # noqa: E402

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/signed-approval-enclave-tmp"
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
    env = dict(os.environ, KAKU_ENCLAVE_BIN=str(repo / "target/debug/kaku-enclave-test"))
    run = subprocess.run([kaku, "approver", "enroll", "--enclave", "--data-dir", str(data), "--label", "secure enclave"], capture_output=True, text=True, env=env)
    blob = data / "approver.enclave"
    check("`kaku approver enroll --enclave` creates a Secure Enclave key (no private key file exists)", run.returncode == 0 and blob.exists() and not (data / "approver.key").exists(), (run.stdout + run.stderr).strip()[:100])
    lst = subprocess.run([kaku, "approver", "list", "--data-dir", str(data)], capture_output=True, text=True).stdout
    check("the service lists it as a p256 key", lst.startswith("p256:"), lst.strip()[:40])
    def review():
        st, s = adm("POST", "/admin/jobs", {"repo": str(work / "root/payments-service"), "recipient": "enclave-recipient"})
        job = s["id"]
        for _ in range(300):
            if adm("GET", f"/admin/jobs/{job}")[1]["state"] != "running": break
            time.sleep(0.4)
        st, a = adm("POST", f"/admin/jobs/{job}/approval", {})
        return job, a["approval"]
    job1, aid1 = review()
    d1 = adm("GET", f"/admin/approvals/{aid1}")[1]
    st, r = adm("POST", f"/admin/approvals/{aid1}/approve", {"ack": d1["digest"][:12]})
    check("the admin token plus the digest is refused", st == 403 and r["error"]["code"] == "PROOF_REQUIRED")
    out = run_tty([kaku, "approve", aid1, "--data-dir", str(data)], d1["digest"][:12] + "\n", env=env)
    check("`kaku approve` signs inside the Secure Enclave and the approval is accepted", out.returncode == 0 and "Approved" in out.stdout, (out.stdout + out.stderr).strip()[-120:])
    job2, aid2 = review()
    d2 = adm("GET", f"/admin/approvals/{aid2}")[1]
    bad = dict(env, KAKU_ENCLAVE_BIN="/nonexistent/kaku-enclave")
    out = run_tty([kaku, "approve", aid2, "--data-dir", str(data)], d2["digest"][:12] + "\n", env=bad)
    check("with the helper missing nothing is approved and the reason is shown", out.returncode != 0 and adm("GET", f"/admin/approvals/{aid2}")[1].get("state", "waiting") != "approved", (out.stderr).strip()[:100])
    blob_copy = data / "other.enclave"
    open(blob_copy, "wb").write(b"junk")
    os.replace(blob, data / "approver.enclave.bak"); os.replace(blob_copy, blob)
    out = run_tty([kaku, "approve", aid2, "--data-dir", str(data)], d2["digest"][:12] + "\n", env=env)
    check("a damaged key blob signs nothing", out.returncode != 0 and "Approved" not in out.stdout, out.stderr.strip()[:100])
    os.replace(data / "approver.enclave.bak", blob)
finally:
    srv.kill()
bad = results.count(False)
print(f"\n{len(results) - bad}/{len(results)} secure-enclave approval checks passed" if bad == 0 else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
