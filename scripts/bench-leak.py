#!/usr/bin/env python3
"""Leak benchmark: does a secret that sits in the owner's material reach the model, or the disk in plain form?

Every case plants one fresh, random secret of a realistic shape somewhere the owner would really have it (a file they
attach, a folder they ask about, a meeting transcript, a note marked restricted) and then uses the product the ordinary
way. The model is an adversary that knows the planted secrets and repeats any of them it is ever shown
(benchmarks/leak/adversary_model.py); its own request log is the independent record of what reached it.

Two arms, same model, same material, same question:
  plain     the usual way to build an assistant: the material is put into the prompt as it is
  kakurega  the real service, driven over its own HTTP interface, with the model on this computer. (The local-model
            route refuses an address that is not this computer, so the "model elsewhere" arm cannot be run this way; it
            is the test `bench_elsewhere` in crates/kk-service/tests/flow.rs, which writes the same kind of record.)

Per case the record says: did any form of the secret (as written, base64, hex, URL-encoded) reach the model; is it in
anything the service wrote under its data folder; did the answer returned to the owner carry it.

  scripts/bench-leak.py [--only DOOR] [--out FILE]      needs target/debug/kaku and apps/web/dist built
Results go to benchmarks/leak/results.json (unsigned); `kaku bench sign` signs them and `kaku bench verify` checks them.
The plain arm leaks by construction with this model: it is the control that shows the adversary and the detector of
leaks both work, not a measurement of other products. What is measured is the kakurega arm.
"""
import base64, hashlib, http.cookiejar, json, os, secrets, shutil, string, subprocess, sys, time, urllib.parse, urllib.request
from pathlib import Path

repo = Path(__file__).resolve().parent.parent
work = repo / "target/bench-leak"
kaku = repo / "target/debug/kaku"
ALNUM = string.ascii_letters + string.digits


def rnd(n, alphabet=ALNUM):
    return "".join(secrets.choice(alphabet) for _ in range(n))


def pem():
    body = base64.b64encode(secrets.token_bytes(600)).decode()
    lines = "\n".join(body[i:i + 64] for i in range(0, len(body), 64))
    kind = "RSA " + "PRIV" + "ATE KEY"  # written in two pieces so this file itself holds nothing shaped like a key block
    return f"-----BEGIN {kind}-----\n{lines}\n-----END {kind}-----"


def jwt():
    seg = lambda b: base64.urlsafe_b64encode(b).decode().rstrip("=")
    return f"{seg(json.dumps({'alg': 'HS256', 'typ': 'JWT'}).encode())}.{seg(json.dumps({'sub': rnd(12), 'iat': 1790000000}).encode())}.{seg(secrets.token_bytes(32))}"


# name, what a person would call it, how a fresh one is made, the part that must not travel (for a key block: any 40 characters of it)
SHAPES = [
    ("anthropic-key", "an Anthropic API key", lambda: "sk-ant-" + "api03-" + rnd(93, ALNUM + "-_") + "AA"),
    ("openai-key", "an OpenAI API key", lambda: "sk-" + "proj-" + rnd(48)),
    ("aws-secret", "an AWS secret access key", lambda: rnd(40, ALNUM + "+/")),
    ("github-token", "a GitHub token", lambda: "ghp" + "_" + rnd(36)),
    ("stripe-key", "a Stripe live key", lambda: "sk_" + "live_" + rnd(24)),
    ("slack-token", "a Slack bot token", lambda: "xoxb" + "-" + rnd(12, string.digits) + "-" + rnd(12, string.digits) + "-" + rnd(24)),
    ("private-key", "a private key block", pem),
    ("jwt", "a signed session token", jwt),
    ("db-password", "a database password in a URL", lambda: rnd(24)),
    ("generic-password", "a password in a settings line", lambda: rnd(20, ALNUM + "!@#%")),
    ("bearer", "a bearer token in a header", lambda: rnd(40)),
]

# Secrets with nothing about them that says "secret": no known prefix, no label next to them. A detector that works by
# shape and context has little to go on, so these rows say how far the protection really reaches.
HARD = [
    ("bare-token", "a random 32-character token in a plain sentence", lambda: rnd(32)),
    ("human-password", "a password a person made up, said in a sentence", lambda: secrets.choice(["Tr0ub4dor", "correct-horse", "Blue42Harbor", "winter"]) + "&" + rnd(3, string.digits)),
    ("pin", "a 6-digit code said in a sentence", lambda: rnd(6, string.digits)),
]

# how the secret is written in the material: name, the text around it
FORMS = {
    "env": lambda name, s: {"aws-secret": f"AWS_ACCESS_KEY_ID=AKIA{rnd(16, string.ascii_uppercase + string.digits)}\nAWS_SECRET_ACCESS_KEY={s}", "db-password": f"DATABASE_URL=postgres://app:{s}@db.internal:5432/main", "generic-password": f"ADMIN_PASSWORD={s}", "bearer": f"Authorization: Bearer {s}", "private-key": s}.get(name, f"API_KEY={s}"),
    "json": lambda name, s: json.dumps({"service": "billing", "credentials": {"secret": s}}, indent=1),
    "yaml": lambda name, s: f"billing:\n  secret: \"{s}\"\n" if "\n" not in s else "billing:\n  key: |\n" + "\n".join("    " + l for l in s.splitlines()),
    "code": lambda name, s: f"client = Client(token=\"{s}\")" if "\n" not in s else f"KEY = \"\"\"{s}\"\"\"",
    "prose": lambda name, s: f"For the record, the credential we use is {s} and it must not be shared.",
    # for the shapeless ones: said with the word for what it is, and said with no such word
    "named": lambda name, s: {"human-password": f"The staging server password is still {s} from the demo.", "pin": f"The door code is {s}, please do not pass it on."}.get(name, f"For the record, the credential we use is {s} and it must not be shared."),
    "unnamed": lambda name, s: {"human-password": f"Ben said to type {s} when the staging box asks.", "pin": f"Punch in {s} at the side entrance."}.get(name, f"Use {s} when you call the export job."),
    "url": lambda name, s: f"https://api.internal.example/v1/export?token={urllib.parse.quote(s, safe='')}&format=csv",
}
# the secret written another way before it is planted
ENCODINGS = {
    "base64": lambda s: base64.b64encode(s.encode()).decode(),
    "hex": lambda s: s.encode().hex(),
}
FILLER = "Quarterly notes for the billing service.\nThe export job runs every night and writes a summary for finance.\n"
QUESTION = "Summarise this for me and list anything that looks important, word for word."


def needles(secret):
    """Every form of the secret whose arrival at the model counts as a leak."""
    core = [secret]
    if "PRIV" + "ATE KEY" in secret:
        body = "".join(secret.splitlines()[1:-1])
        core = [body[40:80], secret.splitlines()[3]]
    out = []
    for c in core:
        out += [c, base64.b64encode(c.encode()).decode(), c.encode().hex(), urllib.parse.quote(c, safe="")]
    return sorted(set(out))


class Client:
    def __init__(self, base, token):
        self.base, self.token, self.csrf = base, token, None
        self.op = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()), urllib.request.ProxyHandler({}))
        code = self.req("/admin/pairing", {}, admin=True)[1]["code"]
        self.csrf = self.req("/api/pair", {"code": code})[1]["csrf"]

    def req(self, path, body=None, admin=False, raw=None):
        h = {"content-type": "application/json"}
        if admin:
            h["authorization"] = "Bearer " + self.token
        else:
            h["Origin"] = self.base
            h["Sec-Fetch-Site"] = "same-origin"
        if self.csrf:
            h["x-kk-csrf"] = self.csrf
        data = raw if raw is not None else (json.dumps(body).encode() if body is not None else None)
        if raw is not None:
            h["content-type"] = "application/octet-stream"
        r = urllib.request.Request(self.base + path, data=data, headers=h, method="POST" if data is not None else "GET")
        try:
            resp = self.op.open(r, timeout=300)
            text = resp.read().decode("utf-8", "replace")
            try:
                return resp.status, json.loads(text)
            except Exception:
                # a stream of lines (a roundtable) is kept as the text it was
                return resp.status, {"_text": text}
        except urllib.error.HTTPError as e:
            text = e.read().decode("utf-8", "replace")
            try:
                return e.code, json.loads(text)
            except Exception:
                return e.code, {"_text": text}


# ---- the doors: each plants the material and uses the product; returns (status, the answer the owner got, as text) ----

def door_attach(c, root, material, n):
    s, v = c.req(f"/api/attachments?name=notes-{n}.txt", raw=material.encode())
    if s not in (200, 201):
        return s, json.dumps(v)
    fid = v["id"]
    s, v = c.req("/api/chat", {"message": QUESTION, "attachments": [fid]})
    c.req(f"/api/attachments/{fid}/remove", {})
    return s, json.dumps(v)


def door_attach_path(c, root, material, n):
    p = root / f"config-{n}.txt"
    p.write_text(material)
    s, v = c.req("/api/attachments/path", {"path": str(p)})
    if s not in (200, 201):
        return s, json.dumps(v)
    fid = v["id"]
    s, v = c.req("/api/chat", {"message": QUESTION, "attachments": [fid]})
    c.req(f"/api/attachments/{fid}/remove", {})
    return s, json.dumps(v)


def door_ask(c, root, material, n):
    d = root / f"folder-{n}"
    d.mkdir()
    (d / "settings.txt").write_text(material)
    (d / "readme.md").write_text(FILLER)
    s, v = c.req("/api/ask", {"folder": str(d), "question": "What credential does the billing service use? Quote it exactly."})
    return s, json.dumps(v)


def door_meeting(c, root, material, n):
    lines = [f"Asha: {l}" for l in material.splitlines()]
    text = "Asha: Let us go through the billing service settings line by line.\n" + "\n".join(lines) + "\nBen: Noted, I will rotate that this week.\n"
    s, v = c.req("/api/meeting-notes", {"text": text})
    return s, json.dumps(v)


def door_note(c, root, material, n):
    s, v = c.req("/api/memories", {"text": material[:3800], "level": "restricted", "reason": "bench"})
    if s not in (200, 201):
        # refusing to keep a secret as a note is the protection working; nothing reached a model
        return s, json.dumps(v)
    s, v = c.req("/api/chat", {"message": "What do my notes say about the billing service credential? Quote it exactly."})
    return s, json.dumps(v)


def door_roundtable(c, root, material, n):
    s, v = c.req(f"/api/attachments?name=brief-{n}.txt", raw=material.encode())
    if s not in (200, 201):
        return s, json.dumps(v)
    fid = v["id"]
    s, v = c.req("/api/chat/roundtable", {"message": QUESTION, "experts": ["security-reviewer", "plain-writer"], "attachments": [fid]})
    c.req(f"/api/attachments/{fid}/remove", {})
    return s, json.dumps(v)


def door_typed(c, root, material, n):
    s, v = c.req("/api/chat", {"message": "Here is my settings file, tidy it up:\n" + material})
    return s, json.dumps(v)


DOORS = {
    "attach": ("a file attached to a chat message", door_attach),
    "attach-path": ("a file in the owner's folder, attached by its path", door_attach_path),
    "ask": ("a folder asked about", door_ask),
    "meeting": ("a meeting transcript", door_meeting),
    "note": ("a note marked restricted, then asked about in chat", door_note),
    "roundtable": ("a file given to a roundtable of experts", door_roundtable),
    "typed": ("typed by the owner into the chat box", door_typed),
}
# Where the owner themself hands the secret to the model on their own computer, its arrival there is the product doing
# what it was asked, not a leak. It is recorded as "sent_by_owner" and counted apart. (With a model elsewhere the same
# case must be held; that is in the other arm.)
BY_OWNER = {"typed"}
# A note the owner marked restricted is, by that label, for the model on this computer and for no other. Its arrival
# there is the label doing its job; it is counted apart as "allowed_on_this_computer".
BY_LABEL = {"note"}


def cases():
    out = []
    for door in DOORS:
        for name, _, _ in SHAPES:
            out.append((door, name, "env", None))
    for door in DOORS:
        for form in ["json", "yaml", "code", "prose", "url"]:
            out.append((door, "anthropic-key", form, None))
        for enc in ENCODINGS:
            out.append((door, "github-token", "env", enc))
        for name, _, _ in HARD:
            out.append((door, name, "named", None))
            out.append((door, name, "unnamed", None))
    return out


def start(route, host):
    d = work / route
    shutil.rmtree(d, ignore_errors=True)
    (d / "root").mkdir(parents=True)
    canary_file, model_log = d / "canaries.json", d / "model.log"
    canary_file.write_text('{"needles": []}')
    model_log.write_text("")
    port = 21000 + (os.getpid() % 500) + (0 if route == "here" else 500)
    model = subprocess.Popen([sys.executable, str(repo / "benchmarks/leak/adversary_model.py"), "0.0.0.0" if route != "here" else "127.0.0.1", str(port), str(canary_file), str(model_log)])
    time.sleep(0.6)
    server = subprocess.Popen([str(kaku), "serve", "--data-dir", str(d / "data"), "--root", str(d / "root"), "--model", "adversary", "--ollama", f"http://{host}:{port}", "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    base = None
    end = time.time() + 40
    while time.time() < end and base is None:
        line = server.stdout.readline()
        if "#pair=" in line:
            base = line.split("http://", 1)[1].split("/#pair=")[0]
            base = "http://" + base
    if base is None:
        raise SystemExit("the service did not start")
    token = json.load(open(d / "data/admin.json"))["token"]
    return d, model, server, Client(base, token), canary_file, model_log, port


def in_tree(folder, pins):
    hits = []
    for p in folder.rglob("*"):
        if p.is_file():
            try:
                b = p.read_bytes()
            except Exception:
                continue
            if any(x.encode() in b for x in pins):
                hits.append(str(p.relative_to(folder)))
    return hits


def run_arm(route, host, chosen):
    d, model, server, c, canary_file, model_log, port = start(route, host)
    rows = []
    try:
        for n, (door, shape, form, enc) in enumerate(chosen):
            make = next(m for name, _, m in SHAPES + HARD if name == shape)
            secret = make()
            planted = ENCODINGS[enc](secret) if enc else secret
            material = FILLER + FORMS[form](shape, planted) + "\n" + FILLER
            pins = needles(secret) + ([planted] if enc else [])
            canary_file.write_text(json.dumps({"needles": pins}))
            before = model_log.stat().st_size
            status, answer = DOORS[door][1](c, d / "root", material, n)
            time.sleep(0.05)
            sent = model_log.read_text()[before:]
            reached = sorted({("as written" if p == secret else "another form") for p in pins if p in sent})
            # the control: the same material in a plain prompt to the same model
            plain = json.dumps({"model": "adversary", "stream": False, "messages": [{"role": "system", "content": "You are a helpful assistant."}, {"role": "user", "content": QUESTION + "\n\n" + material}]}).encode()
            pr = json.load(urllib.request.build_opener(urllib.request.ProxyHandler({})).open(urllib.request.Request(f"http://127.0.0.1:{port}/api/chat", data=plain, headers={"content-type": "application/json"}), timeout=60))
            plain_leak = any(p in pr["message"]["content"] for p in pins)
            disk = in_tree(d / "data", pins)
            rows.append({
                "case": f"{door}/{shape}/{form}" + (f"/{enc}" if enc else ""),
                "group": "beyond the detector" if any(shape == h[0] for h in HARD) else "known shapes",
                "door": door, "shape": shape, "form": form, "encoding": enc, "route": route,
                "secret_sha256": hashlib.sha256(secret.encode()).hexdigest(),
                "http_status": status,
                "reached_model": bool(reached), "reached_as": reached,
                "in_answer": any(p in answer for p in pins),
                "on_disk_plain": disk,
                "model_was_asked": len(sent) > 0,
                "plain_arm_leaked": plain_leak,
            })
            r = rows[-1]
            # held: the product was used and the secret stayed out. refused: the product said no before any model was
            # asked. not_run: something failed, which proves nothing.
            r["verdict"] = "LEAK" if r["on_disk_plain"] or (r["reached_model"] and door not in BY_OWNER and door not in BY_LABEL) else "sent_by_owner" if r["reached_model"] and door in BY_OWNER else "allowed_on_this_computer" if r["reached_model"] and door in BY_LABEL else "held" if status in (200, 201) and r["model_was_asked"] else "refused" if status in (409, 422) and not r["model_was_asked"] else "not_run"
            r["said"] = "" if status == 200 else str(json.loads(answer).get("error", {}).get("code", "")) if answer.strip().startswith("{") else ""
            print(f"{r['verdict']:8} {r['case']:44} status {status} {r['said']}", flush=True)
    finally:
        server.kill()
        model.kill()
    return rows


def main():
    only = sys.argv[sys.argv.index("--only") + 1] if "--only" in sys.argv else None
    out = Path(sys.argv[sys.argv.index("--out") + 1]) if "--out" in sys.argv else repo / "benchmarks/leak/results.json"
    chosen = [c for c in cases() if only in (None, c[0])]
    lan = subprocess.run(["sh", "-c", "ipconfig getifaddr en0 || ipconfig getifaddr en1 || hostname -I | cut -d' ' -f1"], capture_output=True, text=True).stdout.strip()
    rows = run_arm("here", "127.0.0.1", chosen)
    # the local-model route must refuse a model that is not on this computer: checked once, as a case of its own
    refused_remote = None
    if lan:
        d, model, server, c, _, model_log, _ = start("lan", lan)
        try:
            status, v = c.req("/api/chat", {"message": "hello"})
            refused_remote = {"http_status": status, "model_was_asked": model_log.stat().st_size > 0, "said": str(v.get("error", {}).get("code"))}
            print(f"local route pointed at another address: status {status}, model asked: {refused_remote['model_was_asked']}")
        finally:
            server.kill()
            model.kill()
    commit = subprocess.run(["git", "-C", str(repo), "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    dirty = bool(subprocess.run(["git", "-C", str(repo), "status", "--porcelain"], capture_output=True, text=True).stdout.strip())
    doc = {"type": "kakurega-leak-benchmark", "version": 1, "commit": commit, "tree_had_uncommitted_changes": dirty, "ran_at": int(time.time()),
           "model": "adversary (repeats any planted secret it is shown)", "local_route_pointed_elsewhere": refused_remote, "cases": rows}
    out.write_text(json.dumps(doc, indent=1, sort_keys=True) + "\n")
    for group in ["known shapes", "beyond the detector"]:
        rs = [r for r in rows if r["group"] == group]
        tally = {v: sum(r["verdict"] == v for r in rs) for v in ["held", "refused", "sent_by_owner", "allowed_on_this_computer", "LEAK", "not_run"]}
        print(f"\n{group}: {len(rs)} cases: {tally}; plain arm leaked {sum(r['plain_arm_leaked'] for r in rs)}/{len(rs)}; on disk in plain: {sum(bool(r['on_disk_plain']) for r in rs)}")
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
