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
    mcp = subprocess.Popen([kaku, "mcp", "--data-dir", str(data)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1, cwd=str(work))
    n = 0

    def rpc(method, params=None):
        global n
        n += 1
        mcp.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n, "method": method, "params": params or {}}) + "\n")
        mcp.stdin.flush()
        line = mcp.stdout.readline()
        transcript.append(line)
        return json.loads(line)

    schemas = {}

    def conforms(v, sch):
        t = sch.get("type")
        types = {"object": dict, "array": list, "string": str, "boolean": bool, "integer": int, "number": (int, float)}
        if t and not (isinstance(v, types[t]) and not (t in ("integer", "number") and isinstance(v, bool))):
            return False
        if t == "object":
            props = sch.get("properties", {})
            if any(k not in v for k in sch.get("required", [])):
                return False
            if sch.get("additionalProperties") is False and any(k not in props for k in v):
                return False
            return all(conforms(v[k], props[k]) for k in v if k in props)
        if t == "array" and "items" in sch:
            return all(conforms(x, sch["items"]) for x in v)
        return True

    def tool(name, **args):
        r = rpc("tools/call", {"name": name, "arguments": args})["result"]
        if r.get("structuredContent") is not None and name in schemas:
            check(f"{name}: the structured output satisfies the tool's own output schema", conforms(r["structuredContent"], schemas[name]), json.dumps(r["structuredContent"])[:100])
        return r

    init = rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test-agent", "version": "0"}})
    check("initialize answers with the protocol version", init["result"]["protocolVersion"] == "2025-06-18")
    tools = rpc("tools/list")["result"]["tools"]
    check("seven typed tools are offered", len(tools) == 7)
    schemas.update({t["name"]: t["outputSchema"] for t in tools})
    o = tool("overview")
    check("overview tells the agent whether approvals need a signature", isinstance(o["structuredContent"].get("approvals_require_signature"), bool))
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

    # leased mode against the live service: the owner's lease, not the agent, decides whether a review can be started
    def owner(*args):
        return subprocess.run([kaku, "agent", *args, "--data-dir", str(data)], capture_output=True, text=True)

    lm = subprocess.Popen([kaku, "mcp", "--leased", "--data-dir", str(data)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1, cwd=str(work))
    ln = 0

    def lrpc(method, params=None):
        global ln
        ln += 1
        lm.stdin.write(json.dumps({"jsonrpc": "2.0", "id": ln, "method": method, "params": params or {}}) + "\n")
        lm.stdin.flush()
        return json.loads(lm.stdout.readline())

    def lnames():
        return [t["name"] for t in lrpc("tools/list")["result"]["tools"]]

    lrpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}})
    start_args = {"repo": str(work / "root/payments-service"), "recipient": "Acme Audit"}

    def lstart():
        return lrpc("tools/call", {"name": "start_review", "arguments": start_args})["result"]

    check("leased: with no lease the tools that act are not offered", "start_review" not in lnames() and "request_approval" not in lnames() and "overview" in lnames())
    r0 = lstart()
    check("leased: a start without a lease is refused and says how to get one", r0["isError"] is True and "kaku agent grant start_review" in r0["content"][0]["text"], r0["content"][0]["text"][:120])
    g = owner("grant", "start_review", "--uses", "1", "--minutes", "5")
    check("the owner grants one use from the terminal", g.returncode == 0 and "1 time(s)" in g.stdout, g.stdout + g.stderr)
    check("leased: the granted tool appears at once, the other does not", "start_review" in lnames() and "request_approval" not in lnames())
    r1 = lstart()
    check("leased: the leased start really starts a review", r1["isError"] is False and r1["structuredContent"]["id"].startswith("job-"), json.dumps(r1)[:160])
    r2 = lstart()
    check("leased: the second start is refused because the single use is spent", r2["isError"] is True and "no uses left" in r2["content"][0]["text"], r2["content"][0]["text"][:120])
    st = owner("status")
    check("the owner sees that no lease is left", st.returncode == 0 and "No lease is active" in st.stdout, st.stdout)
    owner("grant", "start_review", "--uses", "5", "--minutes", "5")
    rv = owner("revoke", "start_review")
    check("the owner revokes", rv.returncode == 0 and "Revoked 1" in rv.stdout, rv.stdout)
    r3 = lstart()
    check("leased: after the revoke the very next call is refused", r3["isError"] is True and "not allowed it" in r3["content"][0]["text"], r3["content"][0]["text"][:120])
    bad = owner("grant", "overview")
    check("a lease for a tool that only reads is refused", bad.returncode == 2, bad.stderr)
    exp = subprocess.run([kaku, "log", "export", "--out", str(work / "lease-log.json"), "--data-dir", str(data)], capture_output=True, text=True)
    logtext = (work / "lease-log.json").read_text()
    check("the signed log records the grant, the use, the refusals and the revoke", exp.returncode == 0 and all(k in logtext for k in ["agent.lease.granted", "agent.lease.used", "agent.lease.refused", "agent.lease.revoked"]))
    lm.stdin.close()
    lm.wait(timeout=10)
finally:
    srv.kill()

bad = results.count(False)
print(f"\n{len(results) - bad}/{len(results)} MCP checks passed" if bad == 0 else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
