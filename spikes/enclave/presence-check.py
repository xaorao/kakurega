#!/usr/bin/env python3
"""One-time owner check of the Secure Enclave approver with user presence required.
Creates a presence-gated enclave key in a temporary folder, serves one fake release from a local stand-in for the kernel,
runs the shipped helper (native dialog, then Touch ID or your account password), and verifies the signature with the
same Rust verifier the service uses. Nothing is sent anywhere and nothing in your real data folder is touched."""
import base64, json, os, shutil, subprocess, sys, tempfile, threading, time
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

repo = Path(__file__).resolve().parents[2]
helper = repo / "target/debug/kaku-enclave"
if not helper.exists():
    subprocess.run([str(repo / "spikes/enclave/build.sh")], check=True, stdout=subprocess.DEVNULL)
work = Path(tempfile.mkdtemp(prefix="kk-presence-"))
D = "ab" * 32
st = json.dumps(dict(sorted(dict(approval="appr-1", digest=D, expires=int(time.time()) + 3600, job="job-1", nonce="n0nce_value_123456", policy="policy-1", recipient="acme-audit", v=1).items())), separators=(",", ":"))
detail = {"statement_canonical": st, "digest": D, "recipient": "acme-audit", "files": [{"name": "finding-1.txt", "bytes": 10}], "removed": [], "disclosure": {"files_after": 3, "project_files": 10}}
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_GET(self):
        self.send_response(200); self.end_headers(); self.wfile.write(json.dumps(detail).encode())
srv = HTTPServer(("127.0.0.1", 0), H); threading.Thread(target=srv.serve_forever, daemon=True).start()
(work / "admin.json").write_text(json.dumps({"port": srv.server_port, "token": "t"}))
blob = work / "k.enclave"
try:
    e = subprocess.run([str(helper), "enroll", "--blob", str(blob), "--presence"], capture_output=True, text=True)
    if e.returncode != 0:
        print("FAIL: could not create the presence-gated key:", e.stderr.strip()); sys.exit(1)
    print("A dialog titled 'Approve this release to acme-audit?' will appear. Click Approve..., then Touch ID or type your account password.")
    r = subprocess.run([str(helper), "approve", "--blob", str(blob), "--data-dir", str(work), "--approval", "appr-1"], capture_output=True, text=True, timeout=300)
    if r.returncode != 0:
        print(f"NOT APPROVED (exit {r.returncode}): {r.stderr.strip()[:200]}"); sys.exit(1)
    (work / "p.json").write_text(r.stdout)
    v = subprocess.run(["cargo", "test", "-p", "kk-evidence", "hw02", "--", "--ignored"], cwd=repo, capture_output=True, text=True, env=dict(os.environ, KK_SE_PROOF=str(work / "p.json")))
    ok = "1 passed" in v.stdout
    print("PASS: the presence-gated enclave key signed after your approval and the Rust verifier accepts the signature." if ok else "FAIL: the signature did not verify:\n" + v.stdout[-300:])
    sys.exit(0 if ok else 1)
finally:
    shutil.rmtree(work, ignore_errors=True)
