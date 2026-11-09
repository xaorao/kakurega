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
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
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
BAD = "111111111:revoked-token-for-tests-BBBB"
CONFLICT = "222222222:second-poller-for-tests-CCCC"
CONFLICT2 = "333333333:no-webhook-for-tests-DDDD"
STORED = "444444444:stored-in-the-vault-for-tests-EEEE"

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
        try:
            self.send_response(200)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            # the bridge was stopped while its poll was held open: nobody is left to answer
            pass

    def do_GET(self):
        self.path = self.path.replace(STORED, TOKEN)
        if self.path.startswith(f"/bot{TOKEN}/getMe"):
            self.reply({"ok": True, "result": {"username": "kakurega_test_bot"}})
            return
        for tok, hook in ((CONFLICT, "https://example.test/hook/SECRETPART"), (CONFLICT2, "")):
            if self.path.startswith(f"/bot{tok}/getWebhookInfo"):
                self.reply({"ok": True, "result": {"url": hook}})
                return
        for tok, code in ((BAD, 401), (CONFLICT, 409), (CONFLICT2, 409)):
            if self.path.startswith(f"/bot{tok}/getUpdates"):
                body = json.dumps({"ok": False, "description": "test"}).encode()
                self.send_response(code)
                self.send_header("content-length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
        if not self.path.startswith(f"/bot{TOKEN}/getUpdates"):
            self.send_response(404); self.end_headers(); return
        hold = int(self.path.split("timeout=")[1].split("&")[0]) if "timeout=" in self.path else 0
        with lock:
            polls.append((time.time(), hold))
        # like Telegram: a poll with nothing to return is held open until a message arrives or the timeout passes
        end = time.time() + hold
        while True:
            with lock:
                offset = int(self.path.split("offset=")[1].split("&")[0]) if "offset=" in self.path else 0
                # updates below the requested offset are confirmed and gone
                queue[:] = [u for u in queue if u["update_id"] >= offset]
                ups = list(queue)
            if ups or time.time() >= end or closing[0]:
                break
            time.sleep(0.05)
        self.reply({"ok": True, "result": ups})

    def do_POST(self):
        self.path = self.path.replace(STORED, TOKEN)
        n = int(self.headers.get("content-length", 0))
        body = json.loads(self.rfile.read(n) or b"{}")
        if self.path == f"/bot{TOKEN}/setMyCommands":
            with lock:
                menus.append(body)
            self.reply({"ok": True, "result": True})
        elif self.path == f"/bot{TOKEN}/sendMessage" and send_mode[0] != "ok":
            with lock:
                attempts.append((time.time(), send_mode[0], body.get("text", "")[:40]))
                mode = send_mode[0]
                if mode == "429-once":
                    send_mode[0] = "ok"
            if mode == "500":
                out = json.dumps({"ok": False, "error_code": 500, "description": "Internal Server Error"}).encode()
                code = 500
            else:
                out = json.dumps({"ok": False, "error_code": 429, "description": "Too Many Requests: retry after 2", "parameters": {"retry_after": 2}}).encode()
                code = 429
            self.send_response(code)
            self.send_header("content-type", "application/json")
            self.send_header("content-length", str(len(out)))
            self.end_headers()
            self.wfile.write(out)
        elif self.path == f"/bot{TOKEN}/sendMessage":
            with lock:
                msg_ids[0] += 1
                body["_message_id"] = msg_ids[0]
                body["_t"] = time.time()
                sent.append(body)
            self.reply({"ok": True, "result": {"message_id": msg_ids[0]}})
        elif self.path == f"/bot{TOKEN}/deleteWebhook":
            with lock:
                deletes.append(body)
            self.reply({"ok": True, "result": True})
        elif self.path == f"/bot{TOKEN}/editMessageText":
            with lock:
                edits.append(body)
            self.reply({"ok": True, "result": {}})
        else:
            self.send_response(404); self.end_headers()


send_mode = ["ok"]
attempts = []
menus = []
polls = []
closing = [False]
edits = []
deletes = []
msg_ids = [100]
fake = ThreadingHTTPServer(("127.0.0.1", 0), H)
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
    # pairing names the bot; on a terminal it also draws the link as a QR code, in a pipe it does not
    import pty
    master, slave = pty.openpty()
    pp = subprocess.Popen([kaku, "telegram", "pair", "--data-dir", str(data), "--api-base", api], stdout=slave, stderr=subprocess.PIPE, text=True, env=env)
    os.close(slave)
    chunks = b""
    import select
    end = time.time() + 30
    # a terminal's output queue is small, so it is read while the program runs, as a terminal would
    while time.time() < end:
        if select.select([master], [], [], 0.2)[0]:
            try:
                got = os.read(master, 65536)
            except OSError:
                break
            if not got:
                break
            chunks += got
        elif pp.poll() is not None:
            break
    pp.wait(timeout=10)
    os.close(master)
    shown = chunks.decode("utf8", "replace")
    qr_rows = [l for l in shown.splitlines() if len(l) > 20 and set(l.strip("\r")) <= set("█▀▄ ")]
    check("on a terminal the pairing link is also drawn as a QR code of block characters", "https://t.me/kakurega_test_bot" in shown and "scan this" in shown and len(qr_rows) >= 12 and len({len(l) for l in qr_rows}) == 1, f"{len(qr_rows)} rows; {shown[:200]!r}")
    piped = subprocess.run([kaku, "telegram", "pair", "--data-dir", str(data), "--api-base", api], capture_output=True, text=True, env=env, timeout=30)
    check("into a pipe the pairing output has no QR code, only the link and the command", "https://t.me/kakurega_test_bot" in piped.stdout and not any(c in piped.stdout for c in "█▀▄"), piped.stdout[:200])
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
    check("every poll asks Telegram to hold it open for 25 seconds", len(polls) >= 1 and all(h == 25 for _, h in polls), str(polls[:3]))
    check("the phone's command menu is set once to exactly the four commands the bridge answers", len(menus) >= 1 and [c["command"] for c in menus[0]["commands"]] == ["status", "reviews", "mute", "help"], str(menus)[:120])
    st = data / "telegram-status.json"
    for _ in range(100):
        if st.exists(): break
        time.sleep(0.2)
    check("the bridge writes a status file readable only by the owner, without chat text or the token", st.exists() and (st.stat().st_mode & 0o077) == 0 and TOKEN not in st.read_text() and set(json.loads(st.read_text()).keys()) == {"updated", "paired", "last_poll_ok", "last_send_ok", "queued", "last_error"})
    r = subprocess.run([kaku, "telegram", "status", "--data-dir", str(data)], capture_output=True, text=True)
    check("`kaku telegram status` says the bridge is running", r.returncode == 0 and "The bridge is running" in r.stdout, r.stdout[:120])
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
    # Telegram delivering the same update twice (a lost confirmation) is answered once
    with lock:
        dup = next_id[0]
        next_id[0] += 1
        for _ in range(2):
            queue.append({"update_id": dup, "message": {"chat": {"id": 111, "type": "private"}, "text": "/reviews"}})
    wait_sent(lambda m: "No reviews yet" in m["text"])
    time.sleep(3)
    answers = [m for m in sent if "No reviews yet" in m["text"]]
    check("the same update delivered twice is answered once", len(answers) == 1, str(len(answers)))
    time.sleep(1)
    rp = data / "telegram-receipts.json"
    check("the handled updates are remembered in an owner-only file that holds ids and times, never text", rp.exists() and (rp.stat().st_mode & 0o777) == 0o600 and "reviews" not in rp.read_text() and str(dup) in rp.read_text(), rp.read_text()[:120] if rp.exists() else "missing")
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
    t_ap = time.time()
    ap = adm(f"/admin/jobs/{job}/approval", {})
    m = wait_sent(lambda m: "approval for " + job in m["text"])
    check("a notice is not delayed by the open poll: it is sent within 6 seconds of the approval being requested", m is not None and time.time() - t_ap < 6, f"{time.time() - t_ap:.1f}s")
    check("a waiting approval is announced and says to decide on the computer", m is not None and "Nothing is sent until you approve there" in m["text"])

    aid = ap.get("id") or ap.get("approval") or ap.get("approval_id")
    adm(f"/admin/approvals/{aid}/deny", {})
    e = None
    end = time.time() + 20
    while time.time() < end and e is None:
        with lock:
            e = next((x for x in edits if x.get("message_id") == m["_message_id"]), None)
        time.sleep(0.2)
    check("when the approval stops waiting, the earlier notice is edited in place, not followed by a new message", e is not None and "no longer waiting" in e["text"] and "approve there" not in e["text"], str(e)[:140])
    time.sleep(3)
    with lock:
        again = [x for x in edits if x.get("message_id") == m["_message_id"]]
    check("and it is edited only once", len(again) == 1, str(len(again)))
    b1.terminate(); b1.wait(timeout=10)
    n_before = len(sent)
    b2 = start_bridge()
    time.sleep(2)
    t_say = time.time()
    say(111, "/reviews")
    m = wait_sent(lambda m: m["text"].startswith(job))
    check("after a restart the bridge still serves the same chat without pairing", m is not None)
    check("a message that arrives while a poll is held open is answered at once, not after the hold", m is not None and time.time() - t_say < 3, f"{time.time() - t_say:.1f}s")
    time.sleep(4)
    with lock:
        mark = len(polls)
    time.sleep(12)
    with lock:
        idle = len(polls) - mark
    check("an idle bridge makes at most one request in twelve seconds, not one every two", idle <= 1, f"{idle} requests")
    with lock:
        replay = [x for x in sent[n_before:] if "finished" in x["text"] or "approval for" in x["text"]]
    check("and does not replay old notifications", not replay, str(replay)[:100])

    # /mute end to end: notices carry disable_notification while muted, the reply to /mute itself does not, and /mute off ends it
    def review_notice(recipient="tg-recipient-Acme"):
        start = len(sent)
        j = adm("/admin/jobs", {"repo": str(work / "root/payments-service"), "recipient": recipient})["id"]
        m = wait_sent(lambda m: j in m["text"] and "finished" in m["text"], timeout=120)
        return j, m

    n0 = len(sent)
    say(111, "/mute 30m")
    mm = wait_sent(lambda m: "without a sound for" in m["text"])
    check("/mute 30m is answered, and that answer itself is not quiet", mm is not None and not mm.get("disable_notification"), str(mm)[:120])
    j1, n1 = review_notice()
    check("a notice sent while muted is quiet on the wire (disable_notification true) and still arrives", n1 is not None and n1.get("disable_notification") is True, str(n1)[:160])
    say(111, "/mute off")
    mo = wait_sent(lambda m: "Sound is back on" in m["text"])
    check("/mute off is answered", mo is not None)
    j2, n2 = review_notice()
    check("after /mute off the next notice carries no quiet flag", n2 is not None and not n2.get("disable_notification"), str(n2)[:160])

    # Telegram's flood control (429 with retry_after): the notice is held for that long, then delivered once
    send_mode[0] = "429-once"
    t429 = time.time()
    j3, n3 = review_notice()
    with lock:
        tries = [a for a in attempts if a[1] == "429-once"]
    gap = ((n3 or {}).get("_t", 0) - tries[0][0]) if tries else 0
    check("after a 429 the bridge tried once, waited the 2 seconds Telegram named, and then delivered the notice", n3 is not None and len(tries) == 1 and 1.9 <= gap < 15, f"tries={len(tries)} gap={gap:.1f}s")
    check("and the notice was delivered once, not twice", len([x for x in sent if j3 in x["text"] and "finished" in x["text"]]) == 1)

    # a burst: eight reviews finish while the bridge is not looking (paused), so its next look sees eight new notices at once
    import signal
    os.kill(b2.pid, signal.SIGSTOP)
    burst = []
    for batch in range(2):
        ids = [adm("/admin/jobs", {"repo": str(work / "root/payments-service"), "recipient": f"burst-{batch}-{i}"})["id"] for i in range(4)]
        end = time.time() + 120
        while time.time() < end:
            states = {x["id"]: x["state"] for x in adm("/admin/jobs")["jobs"]}
            if all(states.get(i) not in (None, "running") for i in ids):
                break
            time.sleep(0.5)
        burst += ids
    with lock:
        n_pre = len(sent)
    os.kill(b2.pid, signal.SIGCONT)
    fold = wait_sent(lambda m: "more things happened at once" in m["text"], timeout=60)
    time.sleep(4)
    with lock:
        after = sent[n_pre:]
    singles = [x for x in after if any(j in x["text"] for j in burst)]
    folds = [x for x in after if "more things happened at once" in x["text"]]
    check("eight reviews finishing at once become three notices and one line with the count", fold is not None and len(singles) == 3 and len(folds) == 1 and folds[0]["text"].startswith("5 more things happened at once"), f"singles={len(singles)} folds={[x['text'][:40] for x in folds]}")
    import re as _re
    nums = [int(_re.search(r"job-(\d+)", x["text"]).group(1)) for x in singles]
    check("and the three shown are the three newest reviews, newest first, none is repeated, and the count line comes last", len({x["text"] for x in singles}) == 3 and nums == sorted((int(j.split("-")[1]) for j in burst), reverse=True)[:3] and after[-1]["text"].startswith("5 more"), f"{nums} {sorted((int(j.split('-')[1]) for j in burst), reverse=True)[:3]}")

    # an outage with the bridge restarted in the middle: the notice waits in the owner-only outbox file and arrives once afterwards
    b2.terminate(); b2.wait(timeout=10)
    send_mode[0] = "500"
    b3 = start_bridge()
    time.sleep(2)
    j4 = adm("/admin/jobs", {"repo": str(work / "root/payments-service"), "recipient": "tg-recipient-Acme"})["id"]
    ob = data / "telegram-outbox.json"
    for _ in range(300):
        if ob.exists() and j4 in ob.read_text():
            break
        time.sleep(0.2)
    check("while Telegram is failing the notice waits in an owner-only outbox file", ob.exists() and j4 in ob.read_text() and (ob.stat().st_mode & 0o077) == 0, "")
    check("and nothing was delivered yet", not any(j4 in x["text"] for x in sent))
    b3.terminate(); b3.wait(timeout=10)
    send_mode[0] = "ok"
    b4 = start_bridge()
    got = wait_sent(lambda m: j4 in m["text"] and "finished" in m["text"], timeout=60)
    check("after a restart, with Telegram working again, the waiting notice is delivered", got is not None)
    time.sleep(4)
    check("and delivered exactly once", len([x for x in sent if j4 in x["text"] and "finished" in x["text"]]) == 1)
    check("the outbox file is empty or gone afterwards", (not ob.exists()) or j4 not in ob.read_text())
    b2 = b4

    everything = json.dumps(sent)
    for marker in ["tg-recipient", "Acme", "payments", "session.py", "Token still valid", "hunter2", "/Users/", ".env"]:
        check(f"no message ever contained `{marker}`", marker not in everything)

    # a token Telegram no longer accepts stops the bridge with a clear message; a second poller (409) does not stop it
    bad = subprocess.Popen([kaku, "telegram", "--data-dir", str(data), "--api-base", api], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=dict(env, TELEGRAM_BOT_TOKEN=BAD))
    bridges.append(bad)
    try:
        bad.wait(timeout=15)
    except subprocess.TimeoutExpired:
        pass
    bout, berr = (bad.communicate(timeout=5) if bad.poll() is not None else ("", ""))
    check("a token that Telegram rejects (401) stops the bridge with a message that says so", bad.returncode == 1 and "rejected the bot token (401)" in berr and BAD not in berr and BAD.split(":")[1] not in berr, f"rc={bad.returncode} {berr[:120]}")
    conflict = subprocess.Popen([kaku, "telegram", "--data-dir", str(data), "--api-base", api], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=dict(env, TELEGRAM_BOT_TOKEN=CONFLICT))
    bridges.append(conflict)
    time.sleep(4)
    alive = conflict.poll() is None
    conflict.terminate()
    cout, cerr = conflict.communicate(timeout=10)
    check("a conflict (409) is explained once, not over and over, and does not stop the bridge", alive and "conflict (409)" in cerr and cerr.count("conflict (409)") <= 1, f"alive={alive} said {cerr.count(chr(99) + 'onflict (409)')} times {cerr[:120]}")
    check("with a webhook set, the 409 message says so, names the way out and never prints the webhook address", "a webhook is set" in cerr and "--clear-webhook" in cerr and "SECRETPART" not in cerr and "example.test" not in cerr, cerr[:300])
    check("the bridge did not remove a webhook on its own", deletes == [], str(deletes))
    nohook = subprocess.Popen([kaku, "telegram", "--data-dir", str(data), "--api-base", api], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=dict(env, TELEGRAM_BOT_TOKEN=CONFLICT2))
    bridges.append(nohook)
    time.sleep(4)
    nohook.terminate()
    _, nerr = nohook.communicate(timeout=10)
    check("with no webhook set, the 409 message says another program is polling", "no webhook is set" in nerr and "another program is polling" in nerr and "--clear-webhook" not in nerr, nerr[:300])
    clr = subprocess.Popen([kaku, "telegram", "--clear-webhook", "--data-dir", str(data), "--api-base", api], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=dict(env, TELEGRAM_BOT_TOKEN=TOKEN))
    bridges.append(clr)
    time.sleep(4)
    clr.terminate()
    cl_out, _ = clr.communicate(timeout=10)
    check("--clear-webhook removes the webhook once, keeps queued messages, and says so", len(deletes) == 1 and deletes[0].get("drop_pending_updates") is False and "webhook for this bot was removed" in cl_out, f"{deletes} {cl_out[:200]}")
    # the token kept in the encrypted store instead of a file or the environment
    env_plain = {k: v for k, v in env.items() if k != "TELEGRAM_BOT_TOKEN"}
    r = subprocess.run([kaku, "telegram", "--data-dir", str(data), "--api-base", api], capture_output=True, text=True, env=env_plain, timeout=30)
    check("with no token anywhere the bridge says how to give it one, and does not start", r.returncode == 1 and "set-token" in r.stderr, f"{r.returncode} {r.stderr[:200]}")
    r = subprocess.run([kaku, "telegram", "set-token", "--data-dir", str(data)], input="not a token\n", capture_output=True, text=True, env=env_plain, timeout=30)
    check("a value that is not shaped like a token is refused (2) and says why", r.returncode == 2 and "does not look like a bot token" in r.stderr, f"{r.returncode} {r.stderr[:200]}")
    import pty
    master, slave = pty.openpty()
    pr = subprocess.run([kaku, "telegram", "set-token", "--data-dir", str(data)], stdin=slave, capture_output=True, text=True, env=env_plain, timeout=30)
    os.close(master)
    os.close(slave)
    check("from a terminal, set-token refuses to read the token (it would be shown on the screen) and says to pipe it", pr.returncode == 2 and "Pipe the token in" in pr.stderr, f"{pr.returncode} {pr.stderr[:200]}")
    r = subprocess.run([kaku, "telegram", "set-token", "--data-dir", str(data)], input="", capture_output=True, text=True, env=env_plain, timeout=30)
    check("an empty standard input is a usage error (2)", r.returncode == 2, f"{r.returncode} {r.stderr[:200]}")
    r = subprocess.run([kaku, "telegram", "set-token", "--data-dir", str(data)], input=STORED + "\n", capture_output=True, text=True, env=env_plain, timeout=30)
    check("set-token stores the token from standard input and never prints it", r.returncode == 0 and STORED not in r.stdout + r.stderr and STORED.split(":")[1] not in r.stdout + r.stderr, f"{r.returncode} {r.stdout[:200]} {r.stderr[:200]}")
    found = [str(f) for f in data.rglob("*") if f.is_file() and STORED.split(":")[1].encode() in f.read_bytes()]
    check("the token is in no readable file under the data folder (the store is encrypted)", not found, str(found))
    menus_before = len(menus)
    stored_bridge = subprocess.Popen([kaku, "telegram", "--data-dir", str(data), "--api-base", api], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env_plain)
    bridges.append(stored_bridge)
    time.sleep(5)
    alive = stored_bridge.poll() is None
    stored_bridge.terminate()
    sout, serr = stored_bridge.communicate(timeout=10)
    check("a bridge with no environment variable and no file starts with the stored token and reaches Telegram", alive and len(menus) > menus_before and "running" in sout, f"alive={alive} menus {menus_before}->{len(menus)} {sout[:100]} {serr[:100]}")
    check("it prints the token nowhere", STORED.split(":")[1] not in sout + serr)
    r = subprocess.run([kaku, "telegram", "--data-dir", str(data), "--api-base", api, "--token-file", str(work / "no-such.env")], capture_output=True, text=True, env=env_plain, timeout=30)
    check("a token file that is named and missing is an error, not a quiet switch to the stored token", r.returncode == 1 and "cannot read the token file" in r.stderr, f"{r.returncode} {r.stderr[:200]}")
    r = subprocess.run([kaku, "telegram", "clear-token", "--data-dir", str(data)], capture_output=True, text=True, env=env_plain, timeout=30)
    check("clear-token removes it", r.returncode == 0 and "removed" in r.stdout, f"{r.stdout} {r.stderr}")
    r = subprocess.run([kaku, "telegram", "--data-dir", str(data), "--api-base", api], capture_output=True, text=True, env=env_plain, timeout=30)
    check("after clearing, the bridge no longer starts without a token", r.returncode == 1, f"{r.returncode}")
    closing[0] = True
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
