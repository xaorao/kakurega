#!/usr/bin/env python3
"""One finding of a delivered review is shared as a file with a proof, with the real binary, and checked offline the way a
person who was not the recipient would check it."""
import json, os, re, shutil, subprocess, sys, time, urllib.request, urllib.error
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/share-finding-tmp"
subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work}"], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
shutil.copytree(repo / "fixtures/synthetic-repo", work / "root/payments-service")
os.rename(work / "root/payments-service/_env", work / "root/payments-service/.env")
kaku = str(repo / "target/debug/kaku")
data = work / "data"
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
    rc, out, err = k("share", job, "1")
    check("a review that is not delivered has nothing to share", rc == 6 and "approved and delivered" in err, f"{rc} {err}")
    _, a = adm("POST", f"/admin/jobs/{job}/approval", {})
    _, d = adm("GET", f"/admin/approvals/{a['approval']}")
    st, _ = adm("POST", f"/admin/approvals/{a['approval']}/approve", {"ack": d["digest"][:12]})
    t = subprocess.run([kaku, "tui", "--linear", "--data-dir", str(data)], input=f"/deliver {job}\n/quit\n", capture_output=True, text=True, timeout=120)
    check("the review is approved and delivered", st == 200 and bool(adm("GET", f"/admin/jobs/{job}")[1].get("delivered")), t.stdout[-300:])
    capsule = sorted((data / "capsules").glob("*.kkg"))[0].read_bytes()
    capsule_text = capsule.decode()

    for bad in (["share"], ["share", job], ["share", job, "0"], ["share", job, "x"], ["share", "review-1", "1"]):
        rc, out, err = k(*bad)
        check(f"kaku {' '.join(bad)} is a usage error", rc == 2 and "usage: kaku share JOB FINDING" in err, f"{rc} {err}")
    rc, out, err = k("share", job, "9")
    check("a finding that is not there is refused in words", rc == 6 and "no finding with that number" in err, f"{rc} {err}")
    out_file = work / "one-finding.kks"
    rc, out, err = k("share", job, "1", "--out", str(out_file))
    check("kaku share writes the file and says what it shows and hides", rc == 0 and out_file.exists() and "It shows:" in out and "It hides:" in out and "kaku verify" in out, f"{rc} {out} {err}")
    rc, out2, err = k("share", job, "1", "--out", str(out_file))
    check("it never writes over a file that exists", rc == 2 and "cannot write" in err, f"{rc} {err}")
    share = out_file.read_bytes()
    pin = re.search(r"--pin (\S+)", out).group(1)

    rc, out, _ = k("verify", str(out_file), "--pin", pin)
    check("anyone with the signer's key verifies the share offline", rc == 0 and out.startswith("VALID SHARE") and "the key you pinned" in out and "finding-1.txt" in out, out[:300])
    m = re.search(r"(\d+) of the release's (\d+) files are shared", out)
    check("it says how many of the release's files it holds", m is not None and int(m.group(1)) < int(m.group(2)), out[:300])
    check("and what it does not establish", "Anything about the release's other files" in out and "Who the release was made for" in out, out[-600:])
    rc, out, _ = k("verify", str(out_file))
    check("without a pinned key it verifies and says the key comes from the share itself", rc == 0 and "key taken from the share" in out, out[:200])
    other = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAE"
    rc, out, _ = k("verify", str(out_file), "--pin", other)
    check("pinned to another key it is refused", rc == 1 and out.startswith("INVALID"), out[:100])

    # what the share holds of the capsule: the finding's own files, and nothing else
    body = json.loads(share)
    names = [l["name"] for l in body["leaves"]]
    check("the share holds the finding's text, reproducer, expected output, sample, its own index entry and the check script", {"finding-1.txt", "expected-1.txt", "sample-1.json", "meta-1.json", "run-check.py"} <= set(names) and any(n.startswith("repro-1.") for n in names), str(names))
    check("and not the capsule's list of all findings", "index.json" not in names)
    all_files = json.loads(capsule_text)["files"]
    leaked = [n for n, c in all_files.items() if n not in names and (n.encode() in share or c.encode() in share)]
    check("no other file of the capsule is in it, by name or by content", leaked == [], str(leaked))
    check("nor the recipient", b"acme-audit" not in share and __import__("base64").urlsafe_b64encode(b"acme-audit").rstrip(b"=") not in share)

    # the shared files can be written out, into a new folder only, and are byte for byte the capsule's
    dest = work / "shared-files"
    rc, out, err = k("verify", str(out_file), "--pin", pin, "--out", str(dest))
    same = all((dest / n).read_bytes() == __import__("base64").urlsafe_b64decode(all_files[n] + "==") for n in names) if dest.exists() else False
    check("--out writes the shared files, and a list of findings that holds only the shared one", rc == 0 and sorted(p.name for p in dest.iterdir()) == sorted(names + ["index.json"]) and same and len(json.loads((dest / "index.json").read_text())["findings"]) == 1, f"{rc} {out[-200:]} {err}")
    run = subprocess.run([sys.executable, str(dest / "run-check.py")], capture_output=True, text=True, timeout=120)
    check("the reader re-runs the finding from the share alone, and it reproduces", run.returncode == 0 and run.stdout.startswith("OK   Token still valid after logout"), f"{run.returncode} {run.stdout} {run.stderr[-300:]}")
    rc, out, err = k("verify", str(out_file), "--out", str(dest))
    check("and never into a folder that exists", rc == 2 and "cannot make the folder" in err, f"{rc} {err}")

    # one changed byte anywhere in the finding's text and the share is refused
    tampered = work / "tampered.kks"
    body["leaves"][names.index("finding-1.txt")]["content"] = __import__("base64").urlsafe_b64encode(b"A finding the owner never made.").rstrip(b"=").decode()
    tampered.write_text(json.dumps(body, separators=(",", ":"), sort_keys=True))
    rc, out, _ = k("verify", str(tampered), "--pin", pin)
    check("a share whose finding was rewritten is refused", rc == 1 and out.startswith("INVALID"), out[:120])
    rc, out, _ = k("verify", str(work / "review.kkg") if (work / "review.kkg").exists() else str(sorted((data / "capsules").glob("*.kkg"))[0]))
    check("a capsule is still checked as a capsule", rc == 0 and out.startswith("VALID") and "SHARE" not in out.splitlines()[0], out[:60])
    # the two forms a check itself prints are accepted as a pin: the key in hex and the "signed by" line
    capsule = str(work / "review.kkg") if (work / "review.kkg").exists() else str(sorted((data / "capsules").glob("*.kkg"))[0])
    hex_key = re.search(r"^signing key ([0-9a-f]{64})", out, re.M).group(1)
    line = re.search(r"^signed by (ed25519:[0-9a-f]{64})", out, re.M).group(1)
    for what, value in (("the key in hex", hex_key), ("the signed-by line", line)):
        rc, out2, _ = k("verify", capsule, "--pin", value)
        check(f"a capsule pinned with {what} is valid and says the key was pinned", rc == 0 and "the key you pinned" in out2, out2[:200])
    for what, value in (("another key in hex", "ab" * 32), ("another signed-by line", "ed25519:" + "ab" * 32)):
        rc, out2, _ = k("verify", capsule, "--pin", value)
        check(f"pinned with {what} it is refused in words", rc == 1 and out2.startswith("INVALID: ") and len(out2.splitlines()) >= 2 and out2.splitlines()[1].endswith("."), out2[:200])
    rc, _, err = k("verify", capsule, "--pin", "ed25519:zz")
    check("a pin that is neither is refused before any check, saying the three forms", rc == 2 and "64 hex digits" in err, err[:200])
finally:
    srv.kill()
bad = results.count(False)
print(f"\n{'all ' + str(len(results)) + ' share checks passed' if not bad else str(bad) + ' FAILED'}")
sys.exit(1 if bad else 0)
