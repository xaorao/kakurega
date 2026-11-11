#!/usr/bin/env python3
"""The read-only GitHub connector, end to end: the real `kaku serve` against tests/e2e/github_fake.py, driven through the
same HTTP interface the web page uses. Sign-in with every polling answer, every read, the token pair kept in one private
file and nowhere else, refresh once and never with a spent refresh token, a rejected refresh signing out, rate limits
reported and not retried, sign-out, and hostile answers and inputs.

    cargo build -p kk-cli && python3 tests/e2e/github_cli.py
"""
import http.client
import json
import os
import shutil
import socket
import stat
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/github-cli-tmp"
subprocess.run(["pkill", "-f", "kaku serve --data-dir " + str(work)], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
kaku = str(repo / "target/debug/kaku")
results = []
bodies = []


def check(name, ok, detail=""):
    results.append(bool(ok))
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


# a second listener a redirect points at: it must never be asked anything, least of all with the token
elsewhere_hits = []


class Elsewhere(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_GET(self):
        elsewhere_hits.append((self.path, self.headers.get("authorization")))
        self.send_response(200)
        self.send_header("content-length", "2")
        self.end_headers()
        self.wfile.write(b"{}")

    do_POST = do_GET


eport = free_port()
threading.Thread(target=ThreadingHTTPServer(("127.0.0.1", eport), Elsewhere).serve_forever, daemon=True).start()
ELSEWHERE = f"http://127.0.0.1:{eport}/steal"

gport = free_port()
fake = subprocess.Popen([sys.executable, str(repo / "tests/e2e/github_fake.py"), str(gport)])
BASE = f"http://127.0.0.1:{gport}"


def fake_call(method, path, body=None):
    c = http.client.HTTPConnection("127.0.0.1", gport, timeout=10)
    c.request(method, path, body=json.dumps(body) if body is not None else None, headers={"content-type": "application/json"})
    r = c.getresponse()
    out = json.loads(r.read() or b"null")
    c.close()
    return out


def control(**kw):
    fake_call("POST", "/_control", kw)


def fake_log():
    return fake_call("GET", "/_log")


class Session:
    def __init__(self, data):
        for _ in range(300):
            if (data / "admin.json").exists():
                break
            time.sleep(0.1)
        adm = json.loads((data / "admin.json").read_text())
        self.port = adm["port"]
        code = self.raw("POST", "/admin/pairing", {}, {"authorization": "Bearer " + adm["token"]})[1]["code"]
        st, v, headers = self.raw("POST", "/api/pair", {"code": code}, {"origin": self.origin()}, want_headers=True)
        self.cookie = headers["set-cookie"].split(";")[0]
        self.csrf = v["csrf"]

    def origin(self):
        return f"http://127.0.0.1:{self.port}"

    def raw(self, method, path, body=None, headers=None, want_headers=False):
        c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=90)
        h = {"content-type": "application/json", **(headers or {})}
        c.request(method, path, body=json.dumps(body) if body is not None else None, headers=h)
        r = c.getresponse()
        text = r.read().decode()
        bodies.append(text)
        hs = {k.lower(): v for k, v in r.getheaders()}
        c.close()
        try:
            v = json.loads(text)
        except ValueError:
            v = None
        return (r.status, v, hs) if want_headers else (r.status, v)

    def get(self, path):
        return self.raw("GET", path, None, {"cookie": self.cookie})

    def post(self, path, body=None):
        return self.raw("POST", path, body or {}, {"cookie": self.cookie, "origin": self.origin(), "x-kk-csrf": self.csrf})


def serve(data, base, out):
    env = dict(os.environ, KAKU_GITHUB_TEST_BASE=base)
    return subprocess.Popen([kaku, "serve", "--data-dir", str(data), "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"),
                             "--ui-dir", str(repo / "apps/web/dist")], stdout=out, stderr=subprocess.STDOUT, env=env)


def code_of(v):
    return (v or {}).get("error", {}).get("code")


def mode(path):
    return stat.S_IMODE(os.stat(path).st_mode)


data = work / "data"
out_path = work / "serve-output.txt"
out = open(out_path, "wb")
srv = serve(data, BASE, out)
token_file = data / "github-token.json"
try:
    time.sleep(0.3)
    s = Session(data)
    G = "/api/connectors/github"
    st, v = s.get(G + "/status")
    check("status before anything: available, no client id, not connected", st == 200 and v["available"] and not v["client_set"] and not v["connected"] and v["test_hosts"], f"{st} {v}")
    st, v = s.post(G + "/read", {"action": "whoami"})
    check("a read before signing in is refused in words", st == 409 and code_of(v) == "SIGNED_OUT" and "sign in" in v["error"]["message"], f"{st} {v}")
    st, v = s.post(G + "/start")
    check("a sign-in without a client id is refused in words", st == 409 and code_of(v) == "NO_CLIENT", f"{st} {v}")
    st, v = s.post(G + "/client", {"client_id": "Iv23-not-a-client-id!"})
    check("a client id of the wrong shape is refused", st == 422 and code_of(v) == "BAD_CLIENT", f"{st} {v}")
    st, v = s.post(G + "/client", {"client_id": "Iv23liFakeApp0123456"})
    check("the client id is kept in a private file", st == 200 and v["client_set"] and mode(data / "github-app.json") == 0o600, f"{st} {v}")

    # hostile answers to starting a sign-in
    for m, want in [("redirect", "redirect"), ("oversized", "larger"), ("wrong_uri", "expected form"), ("bad_device_code", "expected form"), ("bad_user_code", "expected form")]:
        control(start_mode=m, elsewhere=ELSEWHERE)
        st, v = s.post(G + "/start")
        check(f"a start answer that is hostile ({m}) is refused and nothing waits", st == 502 and want in v["error"]["message"] and s.post(G + "/poll")[0] == 409, f"{st} {v}")
    check("the redirect was not followed: the other address was asked nothing", elsewhere_hits == [], str(elsewhere_hits))
    control(start_mode="normal")

    # the sign-in: pending, then slow_down, then authorized, one poll per interval
    control(poll_script=["pending", "slow_down", "authorized"], next_expires_in=299)
    st, v = s.post(G + "/start")
    check("a sign-in starts: the code and GitHub's page, never the device code", st == 200 and v["user_code"] == "WDJB-MJHT" and v["verification_uri"] == BASE + "/login/device" and v["interval"] == 1 and "device_code" not in json.dumps(v), f"{st} {v}")
    st, v = s.post(G + "/poll")
    check("a poll before the interval asks GitHub nothing", st == 200 and v["state"] == "wait" and fake_log()["polls"] == 0, f"{st} {v}")
    time.sleep(1.2)
    st, v = s.post(G + "/poll")
    check("authorization_pending: still waiting", st == 200 and v["state"] == "waiting" and not v["done"] and fake_log()["polls"] == 1, f"{st} {v}")
    time.sleep(1.2)
    st, v = s.post(G + "/poll")
    check("slow_down: the new interval is used", st == 200 and v["state"] == "slow_down" and v["interval"] == 6, f"{st} {v}")
    st, v = s.post(G + "/poll")
    check("and nothing is asked before it has passed", st == 200 and v["state"] == "wait" and v["next_poll_in"] >= 5 and fake_log()["polls"] == 2, f"{st} {v}")
    st, v = s.get(G + "/status")
    check("status shows the code still waiting", v["pending"]["user_code"] == "WDJB-MJHT" and not v["connected"], str(v))
    time.sleep(6.2)
    st, v = s.post(G + "/poll")
    check("authorized: connected as the account", st == 200 and v["state"] == "authorized" and v["account"]["login"] == "octo-fake", f"{st} {v}")
    st, v = s.get(G + "/status")
    check("status shows the login and no pending sign-in", v["connected"] and v["account"] == {"login": "octo-fake", "id": 4242} and v["pending"] is None, str(v))
    check("the token file is readable by the owner only", token_file.exists() and mode(token_file) == 0o600, oct(mode(token_file)) if token_file.exists() else "missing")
    first_access = fake_log()["issued"][0]
    check("the token file holds the issued token", first_access in token_file.read_text())

    # the first token ran out within five minutes: the first read refreshes it once
    st, v = s.post(G + "/read", {"action": "whoami"})
    lg = fake_log()
    check("a token close to its end is refreshed once before the read", st == 200 and v["account"]["login"] == "octo-fake" and lg["refreshes"] == 1, f"{st} {v} {lg['refreshes']}")
    s.post(G + "/read", {"action": "whoami"})
    check("a fresh token is not refreshed again", fake_log()["refreshes"] == 1)
    check("the file now holds the new token, not the spent one", first_access not in token_file.read_text() and fake_log()["issued"][2] in token_file.read_text())

    # every read
    st, v = s.post(G + "/read", {"action": "repos"})
    check("repos: the first page and that there is more", st == 200 and len(v["items"]) == 2 and v["has_more"] and v["untrusted"] and v["note"], f"{st} {v}")
    st, v = s.post(G + "/read", {"action": "repos", "page": 2})
    check("repos: the last page says there is no more", st == 200 and v["page"] == 2 and not v["has_more"], f"{st} {v}")
    st, v = s.post(G + "/read", {"action": "issues", "owner": "acme", "repo": "widgets", "state": "all"})
    check("issues: issues and pull requests, each said which", st == 200 and [i["is_pull_request"] for i in v["items"]] == [False, True], f"{st} {v}")
    st, v = s.post(G + "/read", {"action": "pulls", "owner": "acme", "repo": "widgets"})
    check("pulls: head and base", st == 200 and v["items"][0]["head"] == "fix-retry" and v["items"][0]["base"] == "main", f"{st} {v}")
    st, v = s.post(G + "/read", {"action": "issue", "owner": "acme", "repo": "widgets", "number": 7})
    check("issue: the issue with its comments", st == 200 and v["issue"]["title"] == "Payments retry storm" and v["comments_shown"] == 2 and not v["comments_cut"], f"{st} {v}")
    st, v = s.post(G + "/read", {"action": "issue", "owner": "acme", "repo": "widgets", "number": 7, "attach": True})
    att = (v or {}).get("attachment", {})
    check("issue made into an attachment: the planted key and the order are flagged", st == 200 and att.get("id", "").startswith("att-") and att["flagged"]["secret"] >= 1 and att["flagged"]["instruction_like"] >= 1, f"{st} {att}")
    st, shown = s.get(f"/api/attachments/{att.get('id', 'att-0')}")
    lines = {l["text"]: l["flagged"] for p in (shown or {}).get("read", []) for l in p["lines"]}
    check("and they are marked as left out where the owner reads it", any(f == "secret" and "sk-ant" in t for t, f in lines.items()) and any(f == "instruction_like" and "Ignore all previous" in t for t, f in lines.items()), str(lines)[:400])
    st, v = s.post("/api/chat", {"message": "Summarise the attached issue.", "attachments": [att.get("id")]})
    check("asking about it says both were withheld from the model", st == 200 and v["files"]["withheld_secrets"] >= 1 and v["files"]["withheld_instructions"] >= 1, f"{st} {v and v.get('files')}")
    st, sent = s.get("/api/chat/sent")
    sent_text = json.dumps(sent)
    check("what the model was sent holds neither", "sk-ant" not in sent_text and "Ignore all previous" not in sent_text and "Checkout retries forever" in sent_text, sent_text[:300])
    st, v = s.post(G + "/read", {"action": "file", "owner": "acme", "repo": "widgets", "path": "src/app.py", "ref": "main"})
    check("file: a text file at a ref", st == 200 and v["file"]["text"].startswith("def retry():") and v["file"]["ref"] == "main", f"{st} {v}")
    st, v = s.post(G + "/read", {"action": "file", "owner": "acme", "repo": "widgets", "path": "logo.png"})
    check("file: a binary file is refused in words", st == 422 and code_of(v) == "BINARY", f"{st} {v}")
    st, v = s.post(G + "/read", {"action": "file", "owner": "acme", "repo": "widgets", "path": "big.bin"})
    check("file: one over 1 MB is refused in words", st == 422 and code_of(v) == "TOO_LARGE" and "1 MB" in v["error"]["message"], f"{st} {v}")
    st, v = s.post(G + "/read", {"action": "file", "owner": "acme", "repo": "widgets", "path": "src"})
    check("file: a folder is said to be a folder", st == 422 and code_of(v) == "NOT_A_FILE", f"{st} {v}")
    st, v = s.post(G + "/read", {"action": "file", "owner": "acme", "repo": "widgets", "path": "nope.txt"})
    check("file: nothing there is a 404 in words", st == 404 and code_of(v) == "NOT_FOUND", f"{st} {v}")
    st, v = s.post(G + "/read", {"action": "search", "owner": "acme", "repo": "widgets", "query": "retry"})
    check("search: inside the one repository", st == 200 and v["total_count"] == 1 and v["items"][0]["number"] == 7, f"{st} {v}")
    gets = [r for r in fake_log()["requests"] if r["method"] == "GET" and r["path"] != "/_log"]
    check("every read carried the API version and GitHub's media type", gets and all(r["api_version"] == "2026-03-10" and r["accept"] == "application/vnd.github+json" for r in gets), str(gets[:2]))
    check("the token was never sent to the sign-in endpoints", fake_log()["bearer_seen"] == [], str(fake_log()["bearer_seen"]))

    # hostile input never reaches GitHub
    before = fake_log()["api_calls"]
    for body in [{"action": "issues", "owner": "..", "repo": "widgets"}, {"action": "issues", "owner": "acme", "repo": "a..b"},
                 {"action": "file", "owner": "acme", "repo": "widgets", "path": "../../etc/passwd"},
                 {"action": "file", "owner": "acme", "repo": "widgets", "path": "src/app.py", "ref": "main..evil"},
                 {"action": "search", "owner": "acme", "repo": "widgets", "query": "x repo:other/secret"}]:
        st, v = s.post(G + "/read", body)
        check(f"refused before anything is asked: {json.dumps(body)[:70]}", st == 422 and code_of(v) == "REFUSED", f"{st} {v}")
    check("and GitHub was asked nothing for any of them", fake_log()["api_calls"] == before)

    # rate limits are reported, never retried
    before = fake_log()["api_calls"]
    control(api_mode="primary")
    st, v = s.post(G + "/read", {"action": "whoami"})
    check("primary rate limit: reported with when it resets", st == 429 and code_of(v) == "RATE_LIMITED" and v["error"]["retry_at"] == 1900000000, f"{st} {v}")
    check("and asked exactly once", fake_log()["api_calls"] == before + 1, str(fake_log()["api_calls"] - before))
    control(api_mode="secondary")
    st, v = s.post(G + "/read", {"action": "whoami"})
    check("secondary rate limit: retry-after is honoured in the answer", st == 429 and abs(v["error"]["retry_at"] - time.time() - 42) < 5 and fake_log()["api_calls"] == before + 2, f"{st} {v}")
    control(api_mode="oversized")
    st, v = s.post(G + "/read", {"action": "whoami"})
    check("an API answer over 2 MB is refused, none of it used", st == 502 and code_of(v) == "ANSWER_TOO_LARGE", f"{st} {v}")
    control(api_mode="redirect", elsewhere=ELSEWHERE)
    st, v = s.post(G + "/read", {"action": "whoami"})
    check("an API redirect is not followed and the token goes nowhere else", st == 502 and "redirect" in v["error"]["message"] and elsewhere_hits == [], f"{st} {v} {elsewhere_hits}")

    # GitHub refuses the access token: one refresh, then the read again; a refresh token is never used twice
    control(expire_access=True)
    st, v = s.post(G + "/read", {"action": "whoami"})
    lg = fake_log()
    check("a refused token is refreshed once and the read succeeds", st == 200 and lg["refreshes"] == 2, f"{st} {v} {lg['refreshes']}")
    check("no refresh token was presented twice", lg["refresh_reused"] == 0 and len(set(lg["refresh_presented"])) == len(lg["refresh_presented"]), str(lg["refresh_presented"]))

    # a rejected refresh signs out
    control(expire_access=True, reject_refresh=True)
    st, v = s.post(G + "/read", {"action": "whoami"})
    check("a rejected refresh signs out, in words", st == 409 and code_of(v) == "SIGNED_OUT" and "sign in again" in v["error"]["message"], f"{st} {v}")
    check("and removes the token file", not token_file.exists())
    st, v = s.get(G + "/status")
    check("status says not connected", st == 200 and not v["connected"], str(v))

    # sign in again, then the file's permission is checked on every use
    control(reject_refresh=False, poll_script=["authorized"], next_expires_in=299)
    s.post(G + "/start")
    time.sleep(1.2)
    st, v = s.post(G + "/poll")
    check("signing in again works", st == 200 and v["state"] == "authorized", f"{st} {v}")
    # three reads at once find the new token close to its end: it is refreshed once, not three times
    before = fake_log()["refreshes"]
    got = []
    threads = [threading.Thread(target=lambda: got.append(s.post(G + "/read", {"action": "whoami"})[0])) for _ in range(3)]
    [t.start() for t in threads]
    [t.join() for t in threads]
    lg = fake_log()
    check("reads at the same moment share one refresh", got == [200, 200, 200] and lg["refreshes"] == before + 1 and lg["refresh_reused"] == 0, f"{got} {lg['refreshes'] - before}")
    os.chmod(token_file, 0o644)
    st, v = s.get(G + "/status")
    check("a token file others can read is not used", not v["connected"] and "other users" in (v["problem"] or ""), str(v))
    st, v = s.post(G + "/read", {"action": "whoami"})
    check("and reads are refused while it is", st == 409 and code_of(v) == "SIGNED_OUT", f"{st} {v}")
    os.chmod(token_file, 0o600)
    check("made private again, it is used", s.get(G + "/status")[1]["connected"])

    # the tokens are in the token file and nowhere else
    issued = fake_log()["issued"]
    leaks = []
    for p in data.rglob("*"):
        if p.is_file() and p.name != "github-token.json":
            b = p.read_bytes()
            leaks += [f"{p.name}" for t in issued if t.encode() in b]
    check("no token is in any other file of the data folder", not leaks, str(leaks))
    check("no token is in any answer the service gave", not any(t in b for b in bodies for t in issued))
    st, logtext = s.get("/api/log/export")
    logtext = json.dumps(logtext)
    check("the event log has the reads by kind, with no repository, title or account", "connector.read" in logtext and not any(w in logtext for w in ["acme", "widgets", "Payments retry", "octo-fake"]), logtext[:300])

    # sign-out
    st, v = s.post(G + "/signout")
    check("sign-out removes the file and says how to revoke at GitHub", st == 200 and v["signed_out"] and "revoke" in v["revoke"] and not token_file.exists(), f"{st} {v}")
    st, v = s.post(G + "/read", {"action": "repos"})
    check("after sign-out reads are refused", st == 409 and code_of(v) == "SIGNED_OUT", f"{st} {v}")
finally:
    srv.terminate()
    srv.wait(timeout=20)
    out.close()

served = out_path.read_bytes()
check("no token is in the service's output", not any(t.encode() in served for t in fake_log()["issued"]))

# a test address that is not this computer is refused, not used
data2 = work / "data2"
out2 = open(work / "serve2-output.txt", "wb")
srv = serve(data2, "http://evil.example:80", out2)
try:
    s = Session(data2)
    st, v = s.get(G + "/status")
    check("KAKU_GITHUB_TEST_BASE that is not 127.0.0.1 turns the connector off, in words", st == 200 and not v["available"] and "KAKU_GITHUB_TEST_BASE" in (v["problem"] or ""), str(v))
    s.post(G + "/client", {"client_id": "Iv23liFakeApp0123456"})
    st, v = s.post(G + "/start")
    check("and a sign-in is refused instead of going there", st == 503 and code_of(v) == "CONFIG", f"{st} {v}")
finally:
    srv.terminate()
    srv.wait(timeout=20)
    out2.close()
    fake.terminate()

bad = results.count(False)
print(f"\n{len(results) - bad}/{len(results)} GitHub connector checks passed")
sys.exit(1 if bad else 0)
