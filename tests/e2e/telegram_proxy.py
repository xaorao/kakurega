#!/usr/bin/env python3
"""The Telegram bridge reaches Telegram through the proxy the environment names, and not around it; the local service never goes through one."""
import json
import os
import shutil
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/telegram-proxy-tmp"
subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work}"], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
data = work / "data"
kaku = str(repo / "target/debug/kaku")
TOKEN = "123456789:FAKE-token-for-proxy-test-AAAA"
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


connects = []
srv = socket.socket()
srv.bind(("127.0.0.1", 0))
srv.listen(8)


def proxy():
    while True:
        try:
            c, _ = srv.accept()
        except OSError:
            return
        try:
            c.settimeout(3)
            head = c.recv(4096).decode("latin1", "replace")
            connects.append(head.split("\r\n")[0])
            c.sendall(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\n\r\n")
        except Exception:
            pass
        finally:
            c.close()


threading.Thread(target=proxy, daemon=True).start()
service = subprocess.Popen([kaku, "serve", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    for _ in range(150):
        if (data / "admin.json").exists():
            break
        time.sleep(0.1)
    base_env = {k: v for k, v in os.environ.items() if "proxy" not in k.lower()}
    base_env["TELEGRAM_BOT_TOKEN"] = TOKEN
    proxy_url = f"http://127.0.0.1:{srv.getsockname()[1]}"

    def bridge(extra, seconds=5):
        p = subprocess.Popen([kaku, "telegram", "--data-dir", str(data)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=dict(base_env, **extra))
        time.sleep(seconds)
        p.terminate()
        out, err = p.communicate(timeout=15)
        return out, err

    connects.clear()
    out, err = bridge({"HTTPS_PROXY": proxy_url})
    check("with HTTPS_PROXY set the bridge asks the proxy for a tunnel to api.telegram.org:443", any(c.startswith("CONNECT api.telegram.org:443") for c in connects), str(connects))
    check("the token is not in the tunnel request or in the bridge's output", all(TOKEN.split(":")[1] not in c for c in connects) and TOKEN.split(":")[1] not in out + err, str(connects) + err[:200])
    connects.clear()
    out, err = bridge({"HTTPS_PROXY": proxy_url, "NO_PROXY": "api.telegram.org"})
    check("NO_PROXY for the Telegram host sends nothing to the proxy", not connects, str(connects))
    connects.clear()
    out, err = bridge({"HTTPS_PROXY": proxy_url, "ALL_PROXY": proxy_url}, seconds=3)
    check("the bridge never sends the local service's traffic through the proxy", not any("127.0.0.1" in c for c in connects), str(connects))
finally:
    service.kill()
    subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work}"], capture_output=True)
    srv.close()

bad = results.count(False)
print(f"\nall {len(results)} proxy checks passed" if not bad else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
