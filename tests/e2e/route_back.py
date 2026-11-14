#!/usr/bin/env python3
"""A service started on NEAR with a key file can go to the model on this computer and come back without the key being
typed again: the key it was started with is still in its memory. Nothing is asked of NEAR here (the key is made up)."""
import http.cookiejar, json, os, subprocess, sys, tempfile, time, urllib.request
from pathlib import Path

repo = Path(__file__).resolve().parents[2]
work = Path(tempfile.mkdtemp(prefix="kaku-route-back-"))
data = work / "data"
(work / "root").mkdir()
key = "-".join(["sk", "test", "back", "0123456789abcdefghij"])
key_file = work / "near.env"
key_file.write_text(f"NEAR_AI_API_KEY={key}\n")
key_file.chmod(0o600)
bad = 0


def check(name, ok, detail=""):
    global bad
    bad += 0 if ok else 1
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""))


env = {k: v for k, v in os.environ.items() if k != "NEAR_AI_API_KEY"}
serve = subprocess.Popen([str(repo / "target/debug/kaku"), "serve", "--data-dir", str(data), "--root", str(work / "root"), "--route", "near", "--near-model", "Qwen/Qwen3.8-27B", "--near-key-file", str(key_file)], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, env=env)
try:
    for _ in range(300):
        if (data / "admin.json").exists():
            break
        time.sleep(0.1)
    admin = json.loads((data / "admin.json").read_text())
    base = f"http://127.0.0.1:{admin['port']}"
    opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

    def call(method, path, body=None, headers=None):
        own = {"content-type": "application/json"}
        if not path.startswith("/admin/"):
            own.update({"Origin": base, "Sec-Fetch-Site": "same-origin"})
        req = urllib.request.Request(base + path, method=method, data=None if body is None else json.dumps(body).encode(), headers={**own, **(headers or {})})
        try:
            with opener.open(req, timeout=30) as r:
                return r.status, json.loads(r.read() or b"null")
        except urllib.error.HTTPError as e:
            return e.code, json.loads(e.read() or b"null")

    _, pairing = call("POST", "/admin/pairing", {}, {"Authorization": f"Bearer {admin['token']}"})
    _, session = call("POST", "/api/pair", {"code": pairing["code"]})
    csrf = {"x-kk-csrf": session["csrf"]}
    st, cfg = call("GET", "/api/config")
    check("it starts on NEAR's confidential hardware", st == 200 and cfg["route"]["class"] == "verified_tee", json.dumps(cfg.get("route"))[:200])
    st, body = call("POST", "/api/route/choose", {"route": "local"}, csrf)
    st2, cfg = call("GET", "/api/config")
    check("the owner goes to the model on this computer", st == 200 and cfg["route"]["class"] == "device_only", f"{st} {json.dumps(body)[:200]}")
    st, body = call("POST", "/api/route/choose", {"route": "near", "model": "Qwen/Qwen3.8-27B"}, csrf)
    st2, cfg = call("GET", "/api/config")
    check("and comes back to NEAR without typing the key again", st == 200 and cfg["route"]["class"] == "verified_tee", f"{st} {json.dumps(body)[:200]}")
    held = [str(p.relative_to(data)) for p in data.rglob("*") if p.is_file() and key.encode() in p.read_bytes()]
    check("the key it was started with was written nowhere in the data folder", held == [], str(held))
finally:
    serve.terminate()
    serve.wait(timeout=10)
print("all route-back checks passed" if bad == 0 else f"{bad} FAILED")
sys.exit(1 if bad else 0)
