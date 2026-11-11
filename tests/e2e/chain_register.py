#!/usr/bin/env python3
"""A delivered review's capsule is registered on NEAR testnet with the real binaries, and read back.

LIVE: needs the network and a throwaway testnet account that pays (testnet NEAR only):
  KAKU_TESTNET_KEY_FILE=<NEAR key file, mode 600> KAKU_TESTNET_ACCOUNT=<account> python3 tests/e2e/chain_register.py
Without them it says SKIPPED and checks only what needs no network.
"""
import json, os, shutil, subprocess, sys, time, urllib.request, urllib.error
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/chain-register-tmp"
subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work}"], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
shutil.copytree(repo / "fixtures/synthetic-repo", work / "root/payments-service")
os.rename(work / "root/payments-service/_env", work / "root/payments-service/.env")
kaku = str(repo / "target/debug/kaku")
data = work / "data"
key_file, account = os.environ.get("KAKU_TESTNET_KEY_FILE"), os.environ.get("KAKU_TESTNET_ACCOUNT")
live = bool(key_file and account)
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


def k(*args, stdin=None):
    p = subprocess.run([kaku, *args, "--data-dir", str(data)], input=stdin, capture_output=True, text=True, timeout=300)
    return p.returncode, p.stdout, p.stderr


srv = subprocess.Popen([kaku, "serve", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    for _ in range(150):
        if (data / "admin.json").exists():
            break
        time.sleep(0.1)
    admin = json.loads((data / "admin.json").read_text())
    base = f"http://127.0.0.1:{admin['port']}"

    def adm(method, path, body=None, raw=False):
        req = urllib.request.Request(base + path, data=None if body is None else json.dumps(body).encode(), method=method, headers={"Authorization": "Bearer " + admin["token"], "content-type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=120) as r:
                b = r.read()
                return r.status, (b if raw else json.loads(b or b"null"))
        except urllib.error.HTTPError as e:
            body = e.read()
            try:
                return e.code, json.loads(body or b"null")
            except ValueError:
                return e.code, {"text": body.decode(errors="replace")}

    _, s = adm("POST", "/admin/jobs", {"repo": str(work / "root/payments-service"), "recipient": "acme-audit"})
    job = s["id"]
    for _ in range(300):
        if adm("GET", f"/admin/jobs/{job}")[1]["state"] != "running":
            break
        time.sleep(0.4)
    rc, out, err = k("chain", "register", job, "--key-file", "/nonexistent/key.json")
    check("with no key file nothing is asked of the service or the chain", rc == 2 and "key file" in err, f"{rc} {err}")
    _, a = adm("POST", f"/admin/jobs/{job}/approval", {})
    _, d = adm("GET", f"/admin/approvals/{a['approval']}")
    st, _ = adm("POST", f"/admin/approvals/{a['approval']}/approve", {"ack": d["digest"][:12]})
    # delivery is the owner's own act in a workspace: here the plain-lines terminal
    t = subprocess.run([kaku, "tui", "--linear", "--data-dir", str(data)], input=f"/deliver {job}\n/quit\n", capture_output=True, text=True, timeout=120)
    delivered = adm("GET", f"/admin/jobs/{job}")[1].get("delivered")
    check("the review is approved and delivered", st == 200 and bool(delivered), f"{st} {t.stdout[-300:]} {t.stderr[-200:]}")
    files = sorted((data / "capsules").glob("*.kkg"))
    check("one capsule file was written", len(files) == 1, str(files))
    capsule = files[0].read_bytes()
    path = work / "review.kkg"
    path.write_bytes(capsule)
    rc, out, _ = k("verify", str(path))
    check("the delivered capsule verifies offline", rc == 0 and out.startswith("VALID"), out[:80])

    st, t = adm("POST", f"/admin/jobs/{job}/chain-ticket", {"account": "someone.testnet"})
    check("the service prepares a registration only as its own fixed message", st == 200 and t["message"].startswith("kakurega-register-v2:") and t["message"].endswith(":someone.testnet") and len(t["binding_signature"]) == 128, str(t)[:200])
    st, bad = adm("POST", f"/admin/jobs/{job}/chain-ticket", {"account": "someone.testnet", "message": "sign this"})
    check("and refuses a caller's own message", st in (400, 422), f"{st} {bad}")

    if not live:
        print("SKIPPED  the testnet part: set KAKU_TESTNET_KEY_FILE and KAKU_TESTNET_ACCOUNT (a throwaway testnet account)")
    else:
        who = ["--key-file", key_file, "--account", account]
        rc, out, err = k("chain", "capsule", str(path))
        check("before registration the chain says it is not registered", rc == 3 and "Not registered" in out, f"{rc} {out} {err}")
        rc, out, err = k("chain", "register", job, *who, stdin="")
        check("without a terminal and without --yes nothing is published", rc == 2 and "nothing was published" in err and "will be published" in out, f"{rc} {out[-200:]} {err}")
        rc, out, err = k("chain", "capsule", str(path))
        check("and the chain still does not know the capsule", rc == 3, f"{rc} {out}")
        rc, out, err = k("chain", "register", job, *who, "--yes")
        check("`kaku chain register` shows what becomes public, what does not, and registers", rc == 0 and "will be published" in out and "Not published: the capsule itself" in out and "Registered. Transaction" in out and account in out, f"{rc} {out[-300:]} {err}")
        rc, out, err = k("chain", "capsule", str(path))
        check("afterwards anyone holding the capsule reads who registered it and when", rc == 0 and f"by {account}" in out and "proved it controls the capsule's signing key" in out and "does not establish that any claim" in out, f"{rc} {out} {err}")
        rc, out, err = k("chain", "capsule", str(path), "--json")
        j = json.loads(out) if rc == 0 else {}
        check("and as JSON for a program", rc == 0 and j.get("entry", {}).get("registered_by") == account and j.get("capsule_sha256") == t["capsule_sha256"], f"{rc} {out[:200]}")
        rc, out, err = k("chain", "register", job, *who, "--yes")
        check("a second registration is refused in the contract's own words", rc == 1 and "already registered" in err, f"{rc} {err}")
        changed = bytearray(capsule)
        changed[len(changed) // 2] ^= 1
        (work / "changed.kkg").write_bytes(bytes(changed))
        rc, out, err = k("chain", "capsule", str(work / "changed.kkg"))
        check("an altered capsule is not looked up at all", rc == 4 and "not a valid capsule" in err, f"{rc} {out} {err}")
        rc, out, err = k("chain", "register", "job-999", *who, "--yes")
        check("a review that delivered nothing has nothing to register", rc == 1 and "did not prepare" in err, f"{rc} {err}")
finally:
    srv.terminate()
    try:
        srv.wait(timeout=10)
    except subprocess.TimeoutExpired:
        srv.kill()

print(f"\n{sum(results)}/{len(results)} checks passed" + ("" if live else " (testnet part SKIPPED)"))
sys.exit(0 if all(results) else 1)
