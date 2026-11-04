#!/usr/bin/env python3
"""Runs the real `kaku telegram` bridge against a real service and a fake Telegram server."""
import json
import os
import shutil
import subprocess
import sys
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/telegram-tmp"
subprocess.run(["pkill", "-f", f"kaku serve --data-dir {work}"], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
shutil.copytree(repo / "fixtures/synthetic-repo", work / "root/payments-service")
os.rename(work / "root/payments-service/_env", work / "root/payments-service/.env")
kaku = str(repo / "target/debug/kaku")
data = work / "data"
TOKEN = "123456789:FAKE-token-for-tests-AAAA"

results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (("  " + detail) if detail else ""))


lock = threading.Lock()
queue = []
sent = []
next_id = [1]


class H(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def reply(self, obj):
        body = json.dumps(obj).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if not self.path.startswith(f"/bot{TOKEN}/getUpdates"):
            self.send_response(404); self.end_headers(); return
        with lock:
            offset = int(self.path.split("offset=")[1].split("&")[0]) if "offset=" in self.path else 0
            # like Telegram, updates below the requested offset are confirmed and gone
            queue[:] = [u for u in queue if u["update_id"] >= offset]
            ups = list(queue)
        time.sleep(0.2)
        self.reply({"ok": True, "result": ups})

    def do_POST(self):
        n = int(self.headers.get("content-length", 0))
        body = json.loads(self.rfile.read(n) or b"{}")
        if self.path == f"/bot{TOKEN}/setMyCommands":
            with lock:
                menus.append(body)
            self.reply({"ok": True, "result": True})
        elif self.path == f"/bot{TOKEN}/sendMessage":
            with lock:
                sent.append(body)
            self.reply({"ok": True, "result": {}})
        else:
            self.send_response(404); self.end_headers()


menus = []
fake = HTTPServer(("127.0.0.1", 0), H)
threading.Thread(target=fake.serve_forever, daemon=True).start()
api = f"http://127.0.0.1:{fake.server_port}"


def say(chat, text, kind="private"):
    with lock:
        queue.append({"update_id": next_id[0], "message": {"chat": {"id": chat, "type": kind}, "text": text}})
        next_id[0] += 1


def wait_sent(pred, timeout=20):
    end = time.time() + timeout
    while time.time() < end:
        with lock:
            for m in sent:
                if pred(m):
                    return m
        time.sleep(0.2)
    return None


srv = subprocess.Popen([kaku, "serve", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
bridges = []
try:
    for _ in range(100):
        if (data / "admin.json").exists():
            break
        time.sleep(0.1)
    admin = json.loads((data / "admin.json").read_text())
    base = f"http://127.0.0.1:{admin['port']}"
    adm = lambda path, body=None: json.loads(urllib.request.urlopen(urllib.request.Request(base + path, data=None if body is None else json.dumps(body).encode(), headers={"Authorization": "Bearer " + admin["token"], "content-type": "application/json"}), timeout=60).read())
    env = dict(os.environ, TELEGRAM_BOT_TOKEN=TOKEN)
    pair = subprocess.run([kaku, "telegram", "pair", "--data-dir", str(data)], capture_output=True, text=True, env=env)
    code = pair.stdout.split("/pair ")[1].split()[0]
    check("`kaku telegram pair` prints a one-time code", len(code) == 12, "")

    def start_bridge():
        b = subprocess.Popen([kaku, "telegram", "--data-dir", str(data), "--api-base", api], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
        bridges.append(b)
        return b

    b1 = start_bridge()
    say(222, f"/pair {code}", "group")
    say(111, "/status")
    time.sleep(2)
    check("the phone's command menu is set once to exactly the three commands the bridge answers", len(menus) >= 1 and [c["command"] for c in menus[0]["commands"]] == ["status", "reviews", "help"], str(menus)[:120])
    check("an unpaired chat and a group get no reply at all", not sent)
    say(111, "/pair 00000000")
    time.sleep(1.5)
    check("a wrong code during an open pairing window is explained, never accepted", len(sent) == 1 and "wrong or has expired" in sent[0]["text"])
    say(111, f"/pair {code}")
    m = wait_sent(lambda m: m["chat_id"] == 111 and "Paired" in m["text"])
    check("the right code in a private chat pairs it", m is not None)
    say(111, "/status")
    m = wait_sent(lambda m: "Reviews:" in m["text"])
    check("/status answers with counts", m is not None and "Setup:" in m["text"], (m or {}).get("text", "")[:100])
    say(333, "/status")
    time.sleep(2)
    check("another chat is still ignored after pairing", not any(x["chat_id"] == 333 for x in sent))
    say(111, "/approve job-1")
    m = wait_sent(lambda m: "cannot approve or send anything" in m["text"])
    check("there is no approve command, and the bot says so", m is not None)
    check("the pairing was saved so a restart does not need it", (data / "telegram.json").exists() and not (data / "telegram.pairing").exists())

    started = adm("/admin/jobs", {"repo": str(work / "root/payments-service"), "recipient": "tg-recipient-Acme"})
    job = started["id"]
    m = wait_sent(lambda m: job in m["text"] and "finished" in m["text"], timeout=120)
    check("a finished review is announced with its id and counts", m is not None, (m or {}).get("text", "")[:120])
    ap = adm(f"/admin/jobs/{job}/approval", {})
    m = wait_sent(lambda m: "approval for " + job in m["text"])
    check("a waiting approval is announced and says to decide on the computer", m is not None and "Nothing is sent until you approve there" in m["text"])

    b1.terminate(); b1.wait(timeout=10)
    n_before = len(sent)
    b2 = start_bridge()
    say(111, "/reviews")
    m = wait_sent(lambda m: m["text"].startswith(job))
    check("after a restart the bridge still serves the same chat without pairing", m is not None)
    time.sleep(4)
    with lock:
        replay = [x for x in sent[n_before:] if "finished" in x["text"] or "approval for" in x["text"]]
    check("and does not replay old notifications", not replay, str(replay)[:100])

    everything = json.dumps(sent)
    for marker in ["tg-recipient", "Acme", "payments", "session.py", "Token still valid", "hunter2", "/Users/", ".env"]:
        check(f"no message ever contained `{marker}`", marker not in everything)

    fake.shutdown()
    fake.server_close()
    time.sleep(6)
    b2.terminate()
    out, err = b2.communicate(timeout=10)
    check("with Telegram unreachable the bridge keeps running and says so", "not reachable" in err or "could not" in err, err.strip()[:100])
    check("the bot token never appears in the bridge's output or errors", TOKEN not in out and TOKEN not in err and TOKEN.split(":")[1] not in err)
    check("nor in any file under the data directory", all(TOKEN not in p.read_text(errors="ignore") for p in data.rglob("*") if p.is_file() and p.stat().st_size < 5_000_000 and p.suffix in (".json", ".pairing", ".log")))
finally:
    for b in bridges:
        if b.poll() is None:
            b.kill()
    srv.kill()

bad = results.count(False)
print(f"\n{len(results) - bad}/{len(results)} Telegram checks passed" if bad == 0 else f"\n{bad} FAILED")
sys.exit(1 if bad else 0)
