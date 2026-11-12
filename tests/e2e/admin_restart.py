#!/usr/bin/env python3
"""A program that outlives one service keeps working with the next: the tool server for agents (`kaku mcp`) answers again
after the service was stopped and started, without being started again itself."""
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/admin-restart-tmp"
subprocess.run(["pkill", "-f", "kaku serve --data-dir " + str(work)], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
data = work / "data"
kaku = str(repo / "target/debug/kaku")
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


def serve():
    p = subprocess.Popen([kaku, "serve", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    end = time.time() + 30
    while time.time() < end:
        try:
            if f'"pid":{p.pid}' in (data / "admin.json").read_text().replace(" ", ""):
                return p
        except OSError:
            pass
        time.sleep(0.1)
    raise SystemExit("the service did not start")


service = serve()
mcp = subprocess.Popen([kaku, "mcp", "--data-dir", str(data)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1, cwd=str(work))
n = 0


def rpc(method, params=None):
    global n
    n += 1
    mcp.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n, "method": method, "params": params or {}}) + "\n")
    mcp.stdin.flush()
    return json.loads(mcp.stdout.readline())


try:
    rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}})
    first = rpc("tools/call", {"name": "overview", "arguments": {}})["result"]
    check("the tool server answers on the running service", first.get("isError") is False, json.dumps(first)[:200])
    token_before = json.loads((data / "admin.json").read_text())["token"]
    service.kill()
    service.wait()
    down = rpc("tools/call", {"name": "overview", "arguments": {}})["result"]
    check("with the service stopped the tool says so and does not hang", down.get("isError") is True, json.dumps(down)[:200])
    service = serve()
    check("the new service has a new token", json.loads((data / "admin.json").read_text())["token"] != token_before)
    again = rpc("tools/call", {"name": "overview", "arguments": {}})["result"]
    check("after the restart the same tool server answers again", again.get("isError") is False, json.dumps(again)[:300])
finally:
    mcp.kill()
    service.kill()

print(f"{sum(results)}/{len(results)} checks passed")
sys.exit(0 if all(results) else 1)
