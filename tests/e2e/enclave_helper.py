#!/usr/bin/env python3
"""The Secure Enclave helper as a trusted approver: it must fetch the release from the kernel itself, refuse anything
inconsistent, and never act as a generic signing oracle. Uses a fake kernel so hostile responses can be served."""
import base64, json, os, re, shutil, subprocess, sys, threading, time
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/enclave-helper-tmp"
shutil.rmtree(work, ignore_errors=True); work.mkdir(parents=True)
helper = str(repo / "target/debug/kaku-enclave-test")
prod = str(repo / "target/debug/kaku-enclave")
# build both from the current source every time: a test that ran whatever binary was lying around could pass against an old build
for flag in (["--test"], []):
    built = subprocess.run(["sh", str(repo / "spikes/enclave/build.sh"), *flag], capture_output=True, text=True)
    if built.returncode != 0:
        print("FAIL  the helper did not build: " + built.stderr[-300:]); sys.exit(1)
blob = str(work / "k.enclave")
results = []
def check(n, ok, d=""):
    results.append(ok); print(("PASS  " if ok else "FAIL  ") + n + (("  " + d) if d else ""))

DIGEST = "ab" * 32
def stmt(**o):
    f = dict(approval="appr-1", digest=DIGEST, expires=int(time.time()) + 3600, job="job-1", nonce="n0nce_value_123456", policy="policy-1", recipient="acme-audit", v=1)
    f.update(o)
    return json.dumps({k: f[k] for k in sorted(f)}, separators=(",", ":"), ensure_ascii=False)

state = {"detail": {}}
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        ok = self.headers.get("Authorization") == "Bearer tok" and self.path.startswith("/admin/approvals/")
        d = state["detail"].get(self.path.rsplit("/", 1)[1]) if ok else None
        self.send_response(200 if d else 404); self.end_headers()
        self.wfile.write(json.dumps(d or {}).encode())
srv = HTTPServer(("127.0.0.1", 0), H); threading.Thread(target=srv.serve_forever, daemon=True).start()
(work / "admin.json").write_text(json.dumps({"port": srv.server_port, "token": "tok"}))
def detail(**o):
    s = o.pop("statement", stmt())
    d = {"statement_canonical": s, "digest": DIGEST, "recipient": "acme-audit", "files": [{"name": "finding-1.txt", "bytes": 10, "sha256": "a" * 64}], "removed": [{"what": "x", "why": "secret"}], "disclosure": {"files_after": 3, "project_files": 10}}
    d.update(o); return d
def run(*args, exe=helper, env=None, timeout=60):
    return subprocess.run([exe, *args], capture_output=True, text=True, timeout=timeout, env=dict(os.environ, **(env or {})))
def approve(aid="appr-1", **kw): return run("approve", "--blob", blob, "--data-dir", str(work), "--approval", aid, **kw)

r = run("enroll", "--blob", blob)
pub = r.stdout.strip()
check("a Secure Enclave key is created once and never overwritten", r.returncode == 0 and len(pub) == 87 and run("enroll", "--blob", blob).returncode == 1)
check("the public key can be read back", run("public", "--blob", blob).stdout.strip() == pub)
check("there is no generic sign command: it is not a signing oracle", run("sign", "--blob", blob, "--type", "x").returncode == 2)

S0 = stmt()
state["detail"]["appr-1"] = detail(statement=S0)
r = approve()
proof = json.loads(r.stdout) if r.returncode == 0 else {}
check("a consistent kernel release is shown and signed exactly as the kernel gave it", r.returncode == 0 and base64.urlsafe_b64decode(proof["payload"] + "==").decode() == S0 and proof["type"] == "application/vnd.kakurega.approval+json", r.stderr[:80])
shown = r.stderr
check("the dialog showed the recipient, the full fingerprint, the file, the withheld count and the cumulative figure", all(x in shown for x in ["acme-audit", DIGEST, "finding-1.txt", "Withheld: 1", "3 of 10"]))
open(work / "p.json", "w").write(r.stdout)
check("the dialog says how long the release can still be approved, without a warning when there is plenty of time", re.search(r"This release can be approved for another (1 h|59 min( \d+ s)?)\.\n", shown) is not None and "Only a few minutes" not in shown, shown[:200])
for secs, pattern, warn in [(300, r"another (4 min( \d+ s)?|5 min)\.", True), (7500, r"another 2 h [45] min\.", False), (620, r"another 10 min( \d+ s)?\.", False), (590, r"another 9 min( \d+ s)?\.", True), (20, r"another (1[0-9]|20) s\.", True)]:
    state["detail"]["appr-1"] = detail(statement=stmt(expires=int(time.time()) + secs))
    rr = approve()
    check(f"the dialog counts down for a release with {secs} seconds left, and warns only near the end", rr.returncode == 0 and re.search(pattern, rr.stderr) is not None and (("Only a few minutes are left." in rr.stderr) == warn), rr.stderr[:160])
state["detail"]["appr-1"] = detail(statement=S0)
v = subprocess.run(["cargo", "test", "-p", "kk-evidence", "hw02", "--", "--ignored"], cwd=repo, capture_output=True, text=True, env=dict(os.environ, KK_SE_PROOF=str(work / "p.json")))
check("the Rust verifier accepts the enclave signature", "1 passed" in v.stdout, v.stdout[-120:] if "1 passed" not in v.stdout else "")

bad = {
  "statement for another approval": detail(statement=stmt(approval="appr-9")),
  "kernel digest differs from the statement": detail(digest="cd" * 32),
  "kernel recipient differs from the statement": detail(recipient="someone-else"),
  "recipient with a quote": detail(statement=stmt(recipient='a"b'), recipient='a"b'),
  "recipient with a backslash": detail(statement=stmt(recipient="a\\b"), recipient="a\\b"),
  "recipient with non-ASCII look-alike letters": detail(statement=stmt(recipient="acme-аudit"), recipient="acme-аudit"),
  "recipient with a bidi control": detail(statement=stmt(recipient="acme‮-audit"), recipient="acme‮-audit"),
  "statement with spaces (not canonical)": detail(statement=stmt().replace(",", ", ")),
  "statement with fields in another order": detail(statement='{"digest":"%s","approval":"appr-1","expires":%d,"job":"j","nonce":"n0nce_value_123456","policy":"p","recipient":"r","v":1}' % (DIGEST, int(time.time()) + 99)),
  "statement with an extra field": detail(statement=stmt().replace('"v":1', '"v":1,"extra":1')),
  "statement with another version": detail(statement=stmt().replace('"v":1', '"v":2')),
  "statement that is not a string": detail(statement=None),
}
for name, d in bad.items():
    state["detail"]["appr-1"] = d
    r = approve()
    check(f"refused: {name}", r.returncode != 0 and r.stdout.strip() == "" and "SHOWN" not in r.stderr, f"exit {r.returncode}")
state["detail"]["appr-1"] = detail(statement=stmt(expires=int(time.time()) - 5))
r = approve(); check("refused: an expired approval is never shown or signed", r.returncode == 9 and r.stdout == "" and "SHOWN" not in r.stderr, f"exit {r.returncode}")
state["detail"]["appr-1"] = detail()
r = approve(env={"KAKU_TEST_DECISION": "refuse"}); check("cancel: choosing Refuse signs nothing", r.returncode == 5 and r.stdout == "")
r = approve("appr-404"); check("an approval the kernel does not know is refused", r.returncode == 7 and r.stdout == "")
for evil in ["../x", "a/b", "a b", "", "a;b", "x" * 80]:
    r = approve(evil)
    check(f"a hostile approval id is refused: {evil[:12]!r}", r.returncode in (2, 7) and r.stdout == "")
(work / "admin.json").rename(work / "admin.off")
r = approve(); check("without the kernel's admin file nothing is signed", r.returncode != 0 and r.stdout == "")
(work / "admin.off").rename(work / "admin.json")

# concurrency and crash recovery
p1 = subprocess.Popen([helper, "approve", "--blob", blob, "--data-dir", str(work), "--approval", "appr-1"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=dict(os.environ, KAKU_TEST_HOLD_MS="4000"))
time.sleep(1.5)
r2 = approve()
check("a second approval while one is on screen is refused (one dialog at a time)", r2.returncode == 6 and r2.stdout == "", f"exit {r2.returncode}")
p1.kill(); p1.wait()
r3 = approve()
check("after the helper is killed mid-dialog the next attempt works and the key is intact", r3.returncode == 0 and json.loads(r3.stdout)["public"] == pub)

# key lifecycle and the change command
sub = "p256:" + "ab" * 32
r = run("change", "--blob", blob, "--data-dir", str(work), "--action", "enroll", "--subject", sub)
ch = json.loads(r.stdout) if r.returncode == 0 else {}
pl = base64.urlsafe_b64decode(ch.get("payload", "") + "==").decode() if ch else ""
check("an approver change is signed over its own action, subject and time, built by the helper", ch.get("type") == "application/vnd.kakurega.approver-action+json" and '"action":"enroll"' in pl and sub in pl and pl.endswith('"v":1}'), pl[:90])
check("the change dialog names the key and the effect", "ADD this key" in r.stderr and sub in r.stderr)
for a, s_ in [("delete", sub), ("enroll", "p256:xyz"), ("enroll", 'p256:' + 'ab' * 32 + '"x'), ("revoke", "")]:
    r = run("change", "--blob", blob, "--data-dir", str(work), "--action", a, "--subject", s_)
    check(f"a hostile change request is refused: {a} {s_[:14]!r}", r.returncode == 2 and r.stdout == "")
(work / "damaged.enclave").write_bytes(b"junk")
check("a damaged key blob signs nothing", run("public", "--blob", str(work / "damaged.enclave")).returncode != 0)
good = open(blob, "rb").read()
alts = {"truncated": good[:-3], "first byte flipped": bytes([good[0] ^ 1]) + good[1:], "middle byte flipped": good[:100] + bytes([good[100] ^ 1]) + good[101:], "last byte flipped": good[:-1] + bytes([good[-1] ^ 1]), "empty": b""}
for name, data in alts.items():
    q = work / "alt.enclave"; q.write_bytes(data)
    r = run("public", "--blob", str(q))
    check(f"an altered key blob ({name}) fails closed or is still the same key, never another key", r.returncode != 0 or r.stdout.strip() == pub, f"exit {r.returncode}")
    r2 = run("approve", "--blob", str(q), "--data-dir", str(work), "--approval", "appr-1")
    check(f"and it cannot produce a signature by another key ({name})", r2.returncode != 0 or json.loads(r2.stdout)["public"] == pub)
check("the shipped helper has no automatic-approval switch (the env var is ignored and it needs a real dialog)", b"KAKU_TEST_DECISION" not in open(prod, "rb").read() and b"KAKU_TEST_HOLD_MS" not in open(prod, "rb").read())
# presence enforcement by the Secure Enclave itself: with no way to ask the user, a presence-gated key must refuse to sign
pblob = str(work / "p.enclave")
pp = run("enroll", "--blob", pblob, "--presence")
check("a presence-gated key can be created", pp.returncode == 0)
state["detail"]["appr-1"] = detail(statement=S0)
r = run("approve", "--blob", pblob, "--data-dir", str(work), "--approval", "appr-1", env={"KAKU_TEST_NOINTERACT": "1"})
check("a presence-gated key refuses to sign when the user cannot be asked (the enclave enforces presence, not our code)", r.returncode == 4 and r.stdout == "", f"exit {r.returncode} {r.stderr.strip()[-80:]}")
r = run("approve", "--blob", blob, "--data-dir", str(work), "--approval", "appr-1", env={"KAKU_TEST_NOINTERACT": "1"})
check("control: the key made without presence signs under the same conditions", r.returncode == 0 and r.stdout.strip() != "")
bad = results.count(False)
print(f"\n{len(results) - bad}/{len(results)} enclave helper checks passed" if bad == 0 else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
