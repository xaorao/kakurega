#!/usr/bin/env python3
"""The scripting commands (review, jobs, findings, chat) against a real service: output shapes, exit codes, and that none of them can approve."""
import json
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/script-cli-tmp"
subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work}"], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
shutil.copytree(repo / "fixtures/synthetic-repo", work / "root/payments-service")
os.rename(work / "root/payments-service/_env", work / "root/payments-service/.env")
data = work / "data"
kaku = str(repo / "target/debug/kaku")
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


def k(*args, stdin=None, timeout=300):
    p = subprocess.run([kaku, *args, "--data-dir", str(data)], input=stdin, capture_output=True, text=True, timeout=timeout)
    return p.returncode, p.stdout, p.stderr


srv = subprocess.Popen([kaku, "serve", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    for _ in range(150):
        if (data / "admin.json").exists():
            break
        time.sleep(0.1)
    admin = json.loads((data / "admin.json").read_text())
    base = f"http://127.0.0.1:{admin['port']}"
    adm = lambda path: json.load(urllib.request.urlopen(urllib.request.Request(base + path, headers={"Authorization": "Bearer " + admin["token"]})))

    rc, out, err = k("jobs", "--json")
    check("jobs --json on a fresh service is an empty list", rc == 0 and json.loads(out) == {"jobs": []}, f"{rc} {out} {err}")
    rc, out, _ = k("jobs")
    check("jobs without --json says so in words", rc == 0 and out.strip() == "no reviews yet", out)

    rc, out, err = k("review", str(work / "root/payments-service"), "--for", "acme-audit", "--json")
    events = [json.loads(l) for l in out.strip().splitlines()]
    check("review --json prints one JSON object per line", rc == 0 and len(events) >= 3 and all(isinstance(e, dict) for e in events), f"{rc} {out[:200]} {err}")
    check("it starts with `started`, reports stages, and ends at awaiting_approval", events[0]["event"] == "started" and any(e["event"] == "stage" for e in events) and events[-1]["event"] == "awaiting_approval" and events[-1]["next"] == "kaku approve", str(events[-1:]))
    job = events[0]["job"]
    check("no approval was requested or given by the command", adm("/admin/approvals")["approvals"] == [] and not adm(f"/admin/jobs/{job}").get("delivered"), str(adm("/admin/approvals")))
    check("a script gets the instruction, not a bypass", "cannot approve" in events[-1]["note"])

    # a question about the project: the line the scripted agent made up is not found, so the command says so and exits 3
    rc, out, err = k("explore", str(work / "root/payments-service"), "Where", "is", "the", "session", "checked?", "--depth", "quick", "--json")
    lines = [json.loads(l) for l in out.strip().splitlines()] if out.strip() else []
    fin = lines[-1]["question"] if lines and lines[-1].get("event") == "finished" else {}
    a = fin.get("answer", {})
    check("explore --json follows the question to its answer, with each line checked", rc == 3 and lines[0]["event"] == "started" and a.get("checked") == 1 and a.get("unchecked") == 1, f"{rc} {out[-400:]} {err[-200:]}")
    check("the withheld .env file was neither read nor given to the script", ".env" not in a.get("files_read", []) and "no .env file is visible" in a["checks"][0]["run"]["output"], json.dumps(a)[:400])
    qid = fin.get("id", "")
    rc, out, _ = k("explore", "show", qid)
    check("explore show in words marks the made-up line as not in the file", rc == 3 and "NOT in the file; the agent wrote:" in out and "1 of the 2 lines it rests on were found:" in out, out[-500:])
    rc, out, _ = k("explore", "show", "../../etc/passwd")
    check("a question's name is checked before anything is asked", rc == 2, out)
    rc, out, _ = k("explore", "forget", qid)
    rc2, out2, _ = k("explore", "list", "--json")
    check("explore forget removes it", rc == 0 and qid not in out2, out + out2)
    check("explore list --json is one object, like the other lists", rc2 == 0 and isinstance(json.loads(out2).get("questions"), list), out2)
    rc, out, _ = k("explore", "list")
    check("an empty list says so in words", rc == 0 and out.strip() == "no questions yet", repr(out))
    rc, out, err = k("explore", str(work / "root/payments-service"), "Where?", "--depth", "deep")
    check("a wrong depth names the words it takes", rc == 2 and "--depth is quick, normal or thorough" in out + err, out + err)
    rc, out, err = k("jobs", "everything")
    check("a stray word is refused with the usage, not ignored", rc == 2 and "kaku jobs takes no" in out + err, out + err)
    p = subprocess.run([kaku, "chat", "--help"], capture_output=True, text=True, timeout=30)
    check("chat --help is chat's own usage, not a message", p.returncode == 0 and "kaku chat MESSAGE" in p.stdout and "kaku review" not in p.stdout, p.stdout[:300] + p.stderr)

    rc, out, _ = k("jobs", "--json")
    listed = json.loads(out)["jobs"]
    check("jobs lists the review with its state", rc == 0 and listed and listed[0]["id"] == job and listed[0]["state"] == "ready", out)
    rc, out, _ = k("jobs")
    check("jobs in words is one line per review", rc == 0 and re.fullmatch(rf"{job}  ready  for acme-audit\n", out) is not None, repr(out))

    rc, out, _ = k("findings", job, "--json")
    f = json.loads(out)
    check("findings --json lists what the kernel confirmed", rc == 0 and f["job"] == job and f["findings"] and f["findings"][0]["status"] == "reproduced", out[:200])
    rc, out, _ = k("findings", job)
    check("findings in words names status, place and title", rc == 0 and re.search(rf"{job}  reproduced  payments/auth/session\.py:\d+  ", out) is not None, out)

    rc, out, err = k("roundtable", "--list")
    check("roundtable --list names the experts with what each does", rc == 0 and "security-reviewer" in out and "Security reviewer (Engineering)" in out and len(out.splitlines()) >= 10, f"{rc} {out[:200]} {err}")
    rc, out, err = k("roundtable", "vault or code?", "--experts", "security-reviewer,data-analyst")
    check("a roundtable prints each expert's answer under its name, then the resolution", rc == 0 and "== Security reviewer ==" in out and "== Data analyst ==" in out and out.index("== Resolution ==") > out.index("== Data analyst =="), f"{rc} {out[:300]} {err}")
    rc, out, err = k("roundtable", "vault or code?", "--experts", "security-reviewer,data-analyst", "--json")
    rt_events = [json.loads(l) for l in out.splitlines() if l.strip()]
    check("with --json each event is one line: whole answers, the resolution, then done, and no pieces", rc == 0 and [e.get("expert") for e in rt_events if "answer" in e] == ["security-reviewer", "data-analyst"] and any("resolution" in e for e in rt_events) and rt_events[-1] == {"done": True, "unresolved": False} and not any("delta" in e for e in rt_events), f"{rc} {out[:300]}")
    rc, out, err = k("roundtable", "q", "--experts", "security-reviewer")
    check("one expert is refused in words, exit 6", rc == 6 and "2 to 5 experts" in err, f"{rc} {err}")
    rc, out, err = k("roundtable", "q", "--experts", "security-reviewer,the-boss")
    check("an unknown expert is refused in words", rc == 6 and "no expert with that name" in err, f"{rc} {err}")
    rc, out, err = k("roundtable")
    check("without a question it prints the usage, exit 2", rc == 2 and "usage: kaku roundtable" in err, f"{rc} {err}")
    rc, out, err = k("chat", "hello there", "--json")
    c = json.loads(out)
    check("chat --json prints the reply and the route class", rc == 0 and "scripted reply" in c["reply"] and c["route"]["class"] == "device_only", out[:200])
    rc, out, _ = k("chat", "-", stdin="a message piped in\n")
    check("chat reads the message from standard input", rc == 0 and "received 18 characters" in out, out)

    rc, out, err = k("follow", job, "--json")
    fol = [json.loads(l) for l in out.strip().splitlines()]
    check("follow on a finished review prints its steps once and ends at awaiting_approval (0)", rc == 0 and [e["event"] for e in fol][-1] == "awaiting_approval" and sum(1 for e in fol if e["event"] == "stage") == sum(1 for e in events if e["event"] == "stage"), f"{rc} {out[:300]} {err}")
    rc, _, err = k("follow", "job-999999")
    check("follow of an unknown review is refused (6)", rc == 6, f"{rc} {err}")
    rc, _, err = k("follow", "../x")
    check("follow of a bad name is a usage error (2)", rc == 2, err)
    rc, _, err = k("follow")
    check("follow with no name is a usage error (2)", rc == 2 and "usage: kaku follow" in err, err)

    rc, out, err = k("ask", "When do sessions expire?", "--folder", str(work / "root/payments-service"), "--json")
    ans = json.loads(out)
    check("ask against the real service: the checker marks the real quote verified and the invented one not found, so it exits 3; withheld files and the hostile README are reported", rc == 3 and ans["checkable"] is True and [c["verdict"] for c in ans["citations"]] == ["verified", "not_found"] and ans["files_withheld"] >= 1 and ans["injection_suspected"] is True, f"{rc} {out[:400]} {err}")
    rc, out, _ = k("ask", "When do sessions expire?", "--folder", str(work / "root/payments-service"))
    check("found in real use: in plain text too, ask says that a file holds text written like orders to a model, and names it", "Careful:" in out and "README" in out and "as data" in out, out[-300:])
    check("in words the invented quote says not to rely on it and the real one is verified with its line", rc == 3 and "(verified, line 1)" in out and "NOT FOUND in that file: do not rely on it" in out, out[:300])
    rc, _, err = k("ask", "q", "--folder", "/no/such/folder")
    check("ask of a folder that does not exist is a usage error (2)", rc == 2, err)
    rc, _, err = k("ask", "q")
    check("ask without a folder is a usage error (2)", rc == 2 and "usage: kaku ask" in err, err)
    rc, _, err = k("ask", "x" * 2001, "--folder", str(work / "root/payments-service"))
    check("a question over 2000 characters is a usage error (2)", rc == 2, err)
    rc, _, err = k("ask", "q", "--folder", str(work))
    check("ask of a folder outside the allowed roots is refused by the service (6)", rc == 6, f"{rc} {err}")

    # kept conversations: start one, continue it, list it
    rc, out, err = k("chat", "remember this thread", "--keep", "--json")
    kept = json.loads(out)
    sid = kept["session"]
    check("--keep starts a kept conversation and reports its id", rc == 0 and re.fullmatch(r"chat-[0-9a-f]+", sid or "") is not None, f"{rc} {out[:200]} {err}")
    rc, out, _ = k("sessions", "--json")
    listed = json.loads(out)["sessions"]
    mine = [x for x in listed if x["id"] == sid]
    check("sessions lists the kept conversation with its two messages", rc == 0 and len(mine) == 1 and mine[0]["messages"] == 2, out[:300])
    rc, out, err = k("chat", "and one more", "--session", sid, "--json")
    check("--session continues it and says which conversation", rc == 0 and json.loads(out)["session"] == sid, f"{rc} {out[:200]} {err}")
    rc, out, _ = k("sessions", "--json")
    check("the continued conversation now holds four messages", [x["messages"] for x in json.loads(out)["sessions"] if x["id"] == sid] == [4], out[:300])
    rc, out, _ = k("sessions")
    check("sessions in words is one line per conversation", rc == 0 and re.search(rf"^{sid}  4  ", out, re.M) is not None, out)
    for bad in ["../../etc", "chat-1/2", "a" * 41, "chat 1"]:
        rc, _, err = k("chat", "x", "--session", bad)
        check(f"the conversation name {bad[:12]!r} (slash, dots, spaces or too long) is a usage error (2)", rc == 2, err)
    rc, out, _ = k("sessions", "export", sid)
    check("export prints the conversation as Markdown with both sides", rc == 0 and out.startswith(f"# Conversation {sid}\n") and out.count("## You") == 2 and out.count("## Assistant") == 2 and "remember this thread" in out, out[:300])
    target = work / "exported.md"
    rc, _, err = k("sessions", "export", sid, "--out", str(target))
    check("export --out writes a new file readable by the owner only", rc == 0 and target.read_text().startswith("# Conversation") and (target.stat().st_mode & 0o777) == 0o600, err)
    rc, _, err = k("sessions", "export", sid, "--out", str(target))
    check("export --out never overwrites an existing file (exit 6)", rc == 6 and "already exists" in err, f"{rc} {err}")
    link = work / "link.md"
    link.symlink_to(work / "elsewhere.md")
    rc, _, err = k("sessions", "export", sid, "--out", str(link))
    check("export --out through a symbolic link is refused and creates nothing at its target", rc != 0 and not (work / "elsewhere.md").exists(), f"{rc} {err}")
    rc, _, err = k("sessions", "export", "../x")
    check("export of a bad name is a usage error (2)", rc == 2, err)
    rc, _, err = k("sessions", "export")
    check("export with no name is a usage error (2)", rc == 2 and "usage: kaku sessions export" in err, err)
    rc, out, err = k("sessions", "export", "chat-0000000000")
    check("export of an unknown conversation is refused (6) and writes nothing", rc == 6 and out == "", f"{rc} {out} {err}")
    rc, _, err = k("chat", "x", "--session", sid, "--keep")
    check("--session together with --keep is a usage error (2)", rc == 2 and "use one of them" in err, err)
    rc, out, _ = k("chat", "x", "--session", "chat-0000000000", "--json")
    check("an unknown conversation is refused (6) with a JSON error and nothing is created", rc == 6 and json.loads(out)["event"] == "error" and all(x["id"] != "chat-0000000000" for x in json.loads(k("sessions", "--json")[1])["sessions"]), out[:200])

    # wrong use and refusals
    rc, out, err = k("review", "/no/such/folder", "--for", "x")
    check("a folder that does not exist is a usage error (2)", rc == 2 and "does not exist" in err, f"{rc} {err}")
    rc, _, err = k("review", str(work / "root/payments-service"))
    check("a review with no recipient is a usage error (2) that shows the usage", rc == 2 and "usage: kaku review" in err, err)
    rc, _, err = k("review", str(work), "--for", "x")
    check("a folder outside the allowed roots is refused by the service (6)", rc == 6, f"{rc} {err}")
    rc, _, err = k("findings", "not-a-job")
    check("a bad review name is a usage error (2)", rc == 2, err)
    rc, out, err = k("findings", "job-999999", "--json")
    e = json.loads(out)
    check("an unknown review is refused (6) with a JSON error", rc == 6 and e["event"] == "error" and e["code"] == 6, f"{rc} {out}")
    rc, _, err = k("chat", "")
    check("an empty message is a usage error (2)", rc == 2)
    # no control characters ever reach a script's output
    allout = "".join(k("jobs")[1] + k("findings", job)[1] for _ in range(1))
    check("no control character in any output", not re.search(r"[\x00-\x08\x0b-\x1f\x7f]", allout))
    # the other ways a review can end, against a stand-in service that answers as the real one does
    import threading
    from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
    mode = ["nothing"]
    ask_mode = ["good"]

    class H(BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def send(self, code, obj):
            body = json.dumps(obj).encode()
            self.send_response(code)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            n = int(self.headers.get("content-length", 0))
            self.rfile.read(n)
            if self.path == "/admin/pairing":
                self.send(200, {"code": "c"})
            elif self.path == "/api/pair":
                self.send(200, {"csrf": "x"})
            elif self.path == "/api/jobs":
                self.send(202, {"id": "job-1"})
            elif self.path == "/api/ask":
                good = {"checkable": True, "answer": "Sessions expire at once.", "citations": [{"file": "a.py", "quote": "del s", "verdict": "verified", "line": 3}], "files_used": ["a.py"], "files_withheld": [], "files_skipped": []}
                bad = dict(good, citations=[{"file": "a.py", "quote": "made up", "verdict": "not_found", "line": 0}, {"file": "b.py", "quote": "x", "verdict": "unknown_file", "line": 0}])
                loose = {"checkable": False, "answer": "must not be shown", "unchecked_text": "free text\u001b[2J here", "citations": [], "files_used": [], "files_withheld": [], "files_skipped": []}
                self.send(200, {"good": good, "bad": bad, "loose": loose}[ask_mode[0]])
            elif self.path == "/api/chat":
                self.send(200, {"reply": "fine\u001b[2Jgone\u0007", "route": {"class": "device_only", "simulated": True}, "notes_used": 0})
            else:
                self.send(404, {})

        def do_GET(self):
            if self.path == "/api/jobs/job-1":
                state = {"nothing": "nothing_to_release", "failed": "failed", "running": "running"}[mode[0]]
                self.send(200, {"state": state, "note": "the agent stopped\u001b[2J early", "events": [{"stage": "import", "detail": "reading"}]})
            else:
                self.send(404, {})

    fake = ThreadingHTTPServer(("127.0.0.1", 0), H)
    threading.Thread(target=fake.serve_forever, daemon=True).start()
    fdata = work / "fake-data"
    fdata.mkdir()
    (fdata / "admin.json").write_text(json.dumps({"port": fake.server_port, "token": "t"}))
    kf = lambda *a, timeout=120: subprocess.run([kaku, *a, "--data-dir", str(fdata)], capture_output=True, text=True, timeout=timeout)
    mode[0] = "nothing"
    p = kf("review", str(work / "root/payments-service"), "--for", "x", "--json")
    last = json.loads(p.stdout.strip().splitlines()[-1])
    check("a review with nothing that can be released ends with exit 3 and says so", p.returncode == 3 and last["state"] == "nothing_to_release", f"{p.returncode} {p.stdout}")
    mode[0] = "nothing"
    p = kf("follow", "job-1", "--json")
    check("follow ends 3 when nothing can be released", p.returncode == 3 and json.loads(p.stdout.strip().splitlines()[-1])["state"] == "nothing_to_release", f"{p.returncode} {p.stdout}")
    mode[0] = "failed"
    p = kf("review", str(work / "root/payments-service"), "--for", "x")
    check("a review that failed ends with exit 4 and its reason, cleaned of control characters", p.returncode == 4 and "the agent stopped" in p.stdout and "\x1b" not in p.stdout, f"{p.returncode} {p.stdout!r}")
    mode[0] = "failed"
    p = kf("follow", "job-1")
    check("follow ends 4 with the reason when the review failed", p.returncode == 4 and "the agent stopped" in p.stdout, f"{p.returncode} {p.stdout!r}")
    mode[0] = "running"
    p = kf("follow", "job-1", "--timeout", "10", timeout=60)
    check("follow ends 5 and says it is still running when the time given runs out", p.returncode == 5 and "still running" in p.stderr, f"{p.returncode} {p.stderr}")
    mode[0] = "running"
    p = kf("review", str(work / "root/payments-service"), "--for", "x", "--timeout", "10", timeout=60)
    check("a review that does not finish in the time given ends with exit 5 and says it is still running", p.returncode == 5 and "still running" in p.stderr, f"{p.returncode} {p.stderr}")
    ask_mode[0] = "good"
    p = kf("ask", "q", "--folder", str(work / "root/payments-service"))
    check("a checkable answer whose quotes were all found is exit 0 with its citation marked verified", p.returncode == 0 and "Sessions expire at once." in p.stdout and "(verified, line 3)" in p.stdout, f"{p.returncode} {p.stdout!r}")
    ask_mode[0] = "bad"
    p = kf("ask", "q", "--folder", str(work / "root/payments-service"), "--json")
    parsed = json.loads(p.stdout)
    check("a quote that was not found, or names a file the model was not shown, makes it exit 3 and is reported as such", p.returncode == 3 and [c["verdict"] for c in parsed["citations"]] == ["not_found", "unknown_file"] and parsed["checkable"] is True, f"{p.returncode} {p.stdout[:300]}")
    ask_mode[0] = "loose"
    p = kf("ask", "q", "--folder", str(work / "root/payments-service"), "--json")
    loose_json = json.loads(p.stdout)
    check("an answer that is not checkable exits 3, shows no answer, and carries the unchecked text apart", p.returncode == 3 and loose_json["checkable"] is False and loose_json["answer"] is None and "free text" in loose_json["unchecked_text"], f"{p.returncode} {p.stdout[:300]}")
    p = kf("ask", "q", "--folder", str(work / "root/payments-service"))
    check("in words the unchecked text is introduced as untrusted and stripped of control characters", "trust none of it" in p.stdout and "\x1b" not in p.stdout and "must not be shown" not in p.stdout, repr(p.stdout))
    ask_mode[0] = "bad"
    p = kf("ask", "q", "--folder", str(work / "root/payments-service"))
    check("in words each failed quote says not to rely on it", "NOT FOUND in that file: do not rely on it" in p.stdout and "NOT A FILE the model was shown: do not rely on it" in p.stdout, p.stdout)
    p = kf("chat", "hello")
    check("a reply with control characters is cleaned before it reaches the terminal", p.returncode == 0 and not re.search(r"[\x00-\x08\x0b-\x1f\x7f]", p.stdout) and "fine" in p.stdout and "gone" in p.stdout, repr(p.stdout))
    fake.shutdown()
    # with no service at all the command starts one for its own use and stops it again; with no model behind it the answer is exit 5
    d2 = work / "own-service"
    p = subprocess.run([kaku, "chat", "hi", "--data-dir", str(d2), "--ollama", "http://127.0.0.1:9", "--root", str(work / "root")], capture_output=True, text=True, timeout=120)
    check("with no service running the command starts its own, and a model that cannot be reached is exit 5", p.returncode == 5, f"{p.returncode} {p.stderr[:200]}")
    left = subprocess.run(["pgrep", "-f", f"kaku serve --data-dir {d2}"], capture_output=True, text=True)
    check("and it stops that service when it is done", left.returncode != 0, left.stdout)

    # a second service on the same data folder says which process has it and what to do, in words
    second = subprocess.run([kaku, "serve", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json")], capture_output=True, text=True, timeout=60)
    check("a second service on the same data folder names the running one and what to do", second.returncode == 1 and f"already running for this data folder (process {srv.pid})" in second.stderr and "--data-dir" in second.stderr, second.stderr)
    # a model address that is not this computer is refused at the start, saying what to use instead
    remote = subprocess.run([kaku, "serve", "--data-dir", str(work / "data-remote"), "--root", str(work / "root"), "--ollama", "http://0.0.0.0:11434"], capture_output=True, text=True, timeout=60)
    check("a model address elsewhere is refused when the service starts, naming the routes for a model elsewhere", remote.returncode == 1 and "is not on this computer" in remote.stderr and "--route near" in remote.stderr, remote.stderr)
    # with the service gone, a command says it is not running and how to start it, not a socket error
    srv.kill()
    srv.wait(timeout=10)
    mcp = subprocess.run([kaku, "mcp", "--data-dir", str(data)], input="\n".join(json.dumps(m) for m in [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "list_reviews", "arguments": {}}},
    ]) + "\n", capture_output=True, text=True, timeout=60)
    last = json.loads(mcp.stdout.strip().splitlines()[-1]) if mcp.stdout.strip() else {}
    said = json.dumps(last)
    check("an agent calling a tool with the service stopped is told it is not running and how to start it", last.get("result", {}).get("isError") is True and "not answering" in said and "kaku serve" in said and "os error" not in said, said[:300])
finally:
    srv.kill()
    subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work}"], capture_output=True)

bad = results.count(False)
print(f"\nall {len(results)} scripting checks passed" if not bad else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
