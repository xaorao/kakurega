#!/usr/bin/env python3
"""Drives `kaku mcp` as an agent client would, against a live service with a real sandboxed review."""
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/mcp-live-tmp"
subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work}"], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
shutil.copytree(repo / "fixtures/synthetic-repo", work / "root/payments-service")
os.rename(work / "root/payments-service/_env", work / "root/payments-service/.env")
kaku = str(repo / "target/debug/kaku")
data = work / "data"
srv = subprocess.Popen([kaku, "serve", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (("  " + detail) if detail else ""))


transcript = []
try:
    for _ in range(100):
        if (data / "admin.json").exists():
            break
        time.sleep(0.1)
    mcp = subprocess.Popen([kaku, "mcp", "--data-dir", str(data)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1)
    n = 0

    def rpc(method, params=None):
        global n
        n += 1
        mcp.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n, "method": method, "params": params or {}}) + "\n")
        mcp.stdin.flush()
        line = mcp.stdout.readline()
        transcript.append(line)
        return json.loads(line)

    def tool(name, **args):
        r = rpc("tools/call", {"name": name, "arguments": args})["result"]
        return r

    init = rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test-agent", "version": "0"}})
    check("initialize answers with the protocol version", init["result"]["protocolVersion"] == "2025-06-18")
    tools = rpc("tools/list")["result"]["tools"]
    check("seven typed tools are offered", len(tools) == 7)
    o = tool("overview")
    check("overview returns structured counts", o["isError"] is False and "reviews" in o["structuredContent"] and o["structuredContent"]["ready"] in (True, False), json.dumps(o["structuredContent"])[:140])
    started = tool("start_review", repo=str(work / "root/payments-service"), recipient="mcp-agent")
    check("an agent can start a review", started["isError"] is False and started["structuredContent"]["id"].startswith("job-"))
    job = started["structuredContent"]["id"]
    state = None
    for _ in range(300):
        st = tool("review_status", id=job)["structuredContent"]
        state = st["state"]
        if state != "running":
            break
        time.sleep(0.4)
    check("the review finishes and status shows counts only", state == "ready" and st["findings_confirmed"] == 1 and st["files_withheld"] == 1, json.dumps(st)[:200])
    ap = tool("request_approval", id=job)
    check("the agent can only ASK for approval and is told where the owner decides", ap["isError"] is False and "kaku approve" in ap["structuredContent"]["how"])
    st2 = tool("review_status", id=job)["structuredContent"]
    check("nothing was delivered by asking", st2["delivered"] is None)
    again = tool("request_approval", id=job)
    check("asking twice is refused politely", again["isError"] is True)
    lst = tool("list_reviews")["structuredContent"]["jobs"]
    check("list_reviews shows the review without text", len(lst) == 1 and set(lst[0].keys()) >= {"id", "state"} and "summary" not in lst[0])
    export = subprocess.run([kaku, "log", "export", "--data-dir", str(data), "--out", str(work / "log.json")], capture_output=True, text=True)
    vl = tool("verify_log", path=str(work / "log.json"))
    check("verify_log checks a saved log offline", vl["isError"] is False and vl["structuredContent"]["valid"] is True, export.stdout.strip()[:100])
    pk = json.loads((work / "log.json").read_text())["public_key"]
    vl2 = tool("verify_log", path=str(work / "log.json"), pin=pk)
    check("and with the right pinned key", vl2["structuredContent"]["valid"] is True and vl2["structuredContent"]["signer_pinned"] is True)
    (work / "bad.json").write_text((work / "log.json").read_text().replace("job.finished", "job.finishes"))
    vl3 = tool("verify_log", path=str(work / "bad.json"))
    check("a changed log is reported invalid", vl3["structuredContent"]["valid"] is False)
    everything = "".join(transcript)
    for marker in ["session.py", "Token still valid", "hunter2", "payments/auth", "logout", "in-process dictionary", "Always release"]:
        check(f"no response ever contained `{marker}`", marker not in everything)
    mcp.stdin.close()
    mcp.wait(timeout=10)
finally:
    srv.kill()

bad = results.count(False)
print(f"\n{len(results) - bad}/{len(results)} MCP checks passed" if bad == 0 else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
