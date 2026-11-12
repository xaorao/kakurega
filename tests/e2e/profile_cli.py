#!/usr/bin/env python3
"""Profiles as a person uses them from the terminal: made, listed, chosen, served, kept apart, and removed only by name."""
import http.cookiejar
import json
import os
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/profile-cli-tmp"
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
home = work / "home"
home.mkdir()
kaku = repo / "target/debug/kaku"
env = {**os.environ, "HOME": str(home)}
env.pop("KAKU_PROFILE", None)
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


def k(*args, **extra):
    p = subprocess.run([str(kaku), *args], capture_output=True, text=True, timeout=60, env={**env, **extra})
    return p.returncode, p.stdout, p.stderr


class Client:
    def __init__(self, data):
        admin = json.load(open(data / "admin.json"))
        self.base, self.token, self.csrf = f"http://127.0.0.1:{admin['port']}", admin["token"], None
        self.op = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()), urllib.request.ProxyHandler({}))
        self.csrf = self.req("/api/pair", {"code": self.req("/admin/pairing", {}, admin=True)["code"]})["csrf"]

    def req(self, path, body=None, admin=False):
        h = {"content-type": "application/json"}
        if admin:
            h["authorization"] = "Bearer " + self.token
        else:
            h.update({"Origin": self.base, "Sec-Fetch-Site": "same-origin"})
        if self.csrf:
            h["x-kk-csrf"] = self.csrf
        data = json.dumps(body).encode() if body is not None else None
        return json.load(self.op.open(urllib.request.Request(self.base + path, data=data, headers=h, method="POST" if data is not None else "GET"), timeout=30))


def serve(*args):
    p = subprocess.Popen([str(kaku), "serve", *args, "--root", str(work / "root"), "--scripted", str(repo / "tests/e2e/script.json"), "--ui-dir", str(repo / "apps/web/dist")],
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, env=env)
    end = time.time() + 60
    while time.time() < end:
        line = p.stdout.readline()
        if "#pair=" in line:
            return p
        if line == "" and p.poll() is not None:
            break
    p.kill()
    raise SystemExit("the service did not start")


rc, out, _ = k("profile", "list", "--json")
check("a new installation has one profile, default, and it is the one in use", rc == 0 and [(p["name"], p["current"]) for p in json.loads(out)["profiles"]] == [("default", True)], out)
rc, out, _ = k("profile", "new", "work")
check("a profile is made by name, in its own folder", rc == 0 and (home / ".kakurega/profiles/work/profile.json").is_file(), out)
bad = [k("profile", "new", n)[0] for n in ["Work", "../x", "default", "a b", "work"]]
check("a name that is malformed, kept for the program, or already used is refused", bad == [2, 2, 2, 2, 2], str(bad))
rc, _, err = k("serve", "--profile", "nope")
check("a command given a profile that does not exist stops with the reason and starts nothing", rc == 2 and "no profile named `nope`" in err and not (home / ".kakurega/admin.json").exists(), f"{rc} {err}")
rc, out, _ = k("config", "path", KAKU_PROFILE="work")
by_env = out.strip()
k("profile", "use", "work")
rc2, out2, _ = k("config", "path")
rc3, out3, _ = k("config", "path", "--data-dir", str(work / "elsewhere"))
rc4, out4, _ = k("config", "path", "--profile", "default")
check("the variable, the saved choice, --profile and --data-dir pick the folder in the stated order",
      "profiles/work" in by_env and "profiles/work" in out2 and str(work / "elsewhere") in out3 and "profiles" not in out4, f"{by_env} | {out2} | {out3} | {out4}")
k("profile", "use", "default")

# the flag may stand anywhere and no command sees an extra word among its own
where = [k("--profile", "work", "config", "path")[1], k("config", "--profile", "work", "path")[1], k("config", "path", "--profile", "work")[1]]
check("--profile works before the command, in the middle and at the end", all("profiles/work" in o for o in where), str(where))
rc, out, _ = k("--profile", "work", "profile", "list", "--json")
rows = {p["name"]: p["current"] for p in json.loads(out)["profiles"]} if rc == 0 else {}
check("with --profile the list marks that profile as the one in use", rows == {"default": False, "work": True}, out)
import subprocess as _sp
try:
    p = _sp.run([str(kaku), "telegram", "clear-token", "--profile", "work"], capture_output=True, text=True, timeout=20, env=env)
    quick = (p.returncode, (p.stdout + p.stderr)[:160])
except _sp.TimeoutExpired:
    quick = ("still running", "")
check("a command with its own action word still does that action under --profile (it does not start something else)", quick[0] != "still running", str(quick))
plain = k("log", "verify")
under = k("log", "verify", "--profile", "work")
front = k("--profile", "work", "log", "verify")
check("a command that takes words reads them the same with the flag at either end", plain[0] == under[0] == front[0] and plain[2] == under[2] == front[2], f"{plain[0]} {under[0]} {front[0]} {under[2][:120]}")
rc, _, err = k("config", "path", "--profile")
check("--profile with no name is refused in words", rc == 2 and "needs a profile" in err, err)

a = serve("--profile", "work")
b = serve()
try:
    wdir, ddir = home / ".kakurega/profiles/work", home / ".kakurega"
    cw, cd = Client(wdir), Client(ddir)
    conf_w, conf_d = cw.req("/api/config"), cd.req("/api/config")
    check("the service says which profile it works in; the first folder has no name", conf_w.get("profile") == "work" and conf_d.get("profile") is None, f"{conf_w.get('profile')} {conf_d.get('profile')}")
    check("each profile has its own signing key", conf_w["signer"] != conf_d["signer"] and len(conf_w["signer"]) > 8)
    sid = cw.req("/api/sessions", {"title": "only in work"})["id"]
    in_w = [s["id"] for s in cw.req("/api/sessions")["sessions"]]
    in_d = [s["id"] for s in cd.req("/api/sessions")["sessions"]]
    check("a conversation kept in one profile is not in the other", sid in in_w and sid not in in_d and in_d == [])
    rc, out, _ = k("profile", "list", "--json")
    rows = {p["name"]: p for p in json.loads(out)["profiles"]}
    check("the list shows both, with a service running in each", rows["work"]["running"] and rows["default"]["running"] and rows["default"]["current"], out)
    rc, _, err = k("profile", "remove", "work", "--confirm", "work")
    check("a profile with a running service is not removed", rc == 2 and "running" in err and wdir.is_dir(), f"{rc} {err}")
finally:
    a.kill()
    b.kill()
    a.wait()
    b.wait()
rc, _, err = k("profile", "remove", "work")
check("removing asks for the profile's name; without it nothing is removed", rc == 2 and (home / ".kakurega/profiles/work").is_dir(), err)
k("profile", "use", "work")
rc, out, _ = k("profile", "remove", "work", "--confirm", "work")
rc2, out2, _ = k("profile", "current")
check("with the name it is removed whole, keys included, and the saved choice goes back to default",
      rc == 0 and not (home / ".kakurega/profiles/work").exists() and out2.strip() == "default" and (home / ".kakurega").is_dir(), f"{rc} {out} {out2}")
rc, _, err = k("profile", "remove", "default", "--confirm", "default")
check("the default profile is not removed this way", rc == 2 and (home / ".kakurega").is_dir(), err)

print(f"\n{sum(results)}/{len(results)} checks passed")
sys.exit(0 if all(results) else 1)
