#!/usr/bin/env python3
"""Does the review loop give better findings than asking the same model directly?

The project is fixtures/synthetic-repo, which has one real defect and two things that look like defects and are not
(fixtures/synthetic-repo.truth.json). The same local model is used two ways, several runs each:

  plain     the project's files go into one prompt; the model's list of findings is taken as it is
  kakurega  the real review through the running service: a finding is counted only when its reproducer confirmed it in
            the sandbox

Per run: was the planted defect found, how many false findings were reported, and how long it took. The truth file also
lists one weakness that was not planted as the defect but is real when read on its own; a finding there is counted
apart ("arguable"), neither as a hit nor as false. Reported per arm: every run, and the worst run.

  scripts/eval-quality.py [MODEL] [RUNS]        needs Ollama with MODEL, target/debug/kaku, and the Lima sandbox VM
Writes benchmarks/quality/results.json. The model runs on this computer, so the cost in money is nothing; the tokens are
recorded so a price can be applied.
  scripts/eval-quality.py near:MODEL [RUNS]     the same with a model on NEAR AI Cloud (account key file in
                                                KAKU_TEST_NEAR_KEY_FILE): the plain arm asks it directly, the review runs on
                                                it in confidential hardware; writes benchmarks/quality/results-near.json
LIMITS: one small project with one defect; one model; a handful of runs. It shows whether confirming findings removes
false ones on this project. It is not a measure of how good the reviews are in general.
"""
import http.cookiejar, json, os, re, shutil, subprocess, sys, time, urllib.request
from pathlib import Path

repo = Path(__file__).resolve().parent.parent
MODEL = sys.argv[1] if len(sys.argv) > 1 else "qwen2.5-coder:7b"
RUNS = int(sys.argv[2]) if len(sys.argv) > 2 else 3
truth = json.load(open(repo / "fixtures/synthetic-repo.truth.json"))
REAL = truth["real_defects"][0]
NEAR = MODEL[len("near:"):] if MODEL.startswith("near:") else None
work = repo / ("target/eval-quality-near" if NEAR else "target/eval-quality")
kaku = repo / "target/debug/kaku"
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def near_key():
    """The NEAR AI Cloud account key, from the file KAKU_TEST_NEAR_KEY_FILE names (lines of NAME=value). Never printed."""
    for line in open(os.environ["KAKU_TEST_NEAR_KEY_FILE"]).read().splitlines():
        if line.strip().startswith("NEAR_AI_API_KEY="):
            return line.split("=", 1)[1].strip()
    raise SystemExit("KAKU_TEST_NEAR_KEY_FILE holds no NEAR_AI_API_KEY line")


def near_thinking_off(model):
    """The documented setting that has each model family answer without thinking first (docs.near.ai/cloud/reasoning-models),
    the same one the product's NEAR route sends."""
    m = model.lower()
    if "kimi" in m:
        return {"chat_template_kwargs": {"thinking": False}}
    if "glm-5.3-flash" in m or "gpt-oss" in m:
        return {"reasoning_effort": "low"}
    if m.startswith("qwen/") or m.startswith("zai-org/glm"):
        return {"chat_template_kwargs": {"enable_thinking": False}}
    return {}


def near_ask(model, messages, max_tokens=2048):
    """One answer from the model on NEAR AI Cloud, asked directly over its OpenAI-compatible API: the plain way of using it,
    with none of the product's checks. Returns the text and the token counts."""
    body = {"model": model, "messages": messages, "max_tokens": max_tokens, "temperature": 0.3, **near_thinking_off(model)}
    req = urllib.request.Request("https://cloud-api.near.ai/v1/chat/completions", data=json.dumps(body).encode(),
                                 headers={"content-type": "application/json", "authorization": "Bearer " + near_key()})
    r = json.load(urllib.request.urlopen(req, timeout=300))
    u = r.get("usage") or {}
    return r["choices"][0]["message"].get("content") or "", u.get("prompt_tokens", 0), u.get("completion_tokens", 0)


def project(dest):
    shutil.copytree(repo / "fixtures/synthetic-repo", dest)
    os.rename(dest / "_env", dest / ".env")


def is_real(finding):
    """The real defect: the right file, and a line within a few lines of the faulty one when a line is given."""
    f = str(finding.get("file", "")).lstrip("./")
    line = finding.get("line")
    return f == REAL["file"] and (not isinstance(line, int) or abs(line - REAL["line"]) <= 8)


ARGUABLE = {a["file"] for a in truth.get("arguable", [])}


def score(findings):
    """found_real: the planted defect. arguable: a finding in a file the truth file lists as a real weakness that was not
    the planted one; it is counted apart, neither as found nor as false. Everything else is a false finding."""
    real = any(is_real(f) for f in findings)
    arguable = [f for f in findings if not is_real(f) and str(f.get("file", "")).lstrip("./") in ARGUABLE]
    false = [f for f in findings if not is_real(f) and f not in arguable]
    return {"found_real": real, "arguable_findings": len(arguable), "false_findings": len(false), "false_files": sorted({str(f.get("file", "?")) for f in false}), "reported": len(findings)}


def plain(dest):
    parts = []
    for p in sorted(dest.rglob("*")):
        if p.is_file() and p.suffix in (".py", ".md", ".txt", "") and p.stat().st_size < 40_000:
            try:
                parts.append(f"=== {p.relative_to(dest)} ===\n{p.read_text()}")
            except UnicodeDecodeError:
                pass
    prompt = ("Review this project for security defects. Reply with one JSON array and nothing else: "
              "[{\"file\": path, \"line\": number, \"summary\": text}]. Report only real defects.\n\n" + "\n\n".join(parts))
    started = time.time()
    if NEAR:
        text, tin, tout = near_ask(NEAR, [{"role": "user", "content": prompt}])
        r = {"message": {"content": text}, "prompt_eval_count": tin, "eval_count": tout}
    else:
        r = json.load(opener.open(urllib.request.Request("http://127.0.0.1:11434/api/chat", data=json.dumps({"model": MODEL, "stream": False, "options": {"temperature": 0.3, "num_ctx": 16384}, "messages": [{"role": "user", "content": prompt}]}).encode(), headers={"content-type": "application/json"}), timeout=1800))
    text = r["message"]["content"]
    m = re.search(r"\[.*\]", text, re.S)
    try:
        findings = [f for f in json.loads(m.group(0)) if isinstance(f, dict)] if m else []
    except Exception:
        findings = []
    env = (dest / ".env").read_text()
    secrets = [l.split("=", 1)[1] for l in env.splitlines() if "=" in l and len(l.split("=", 1)[1]) > 8]
    out = score(findings)
    out.update({"seconds": round(time.time() - started), "tokens_in": r.get("prompt_eval_count", 0), "tokens_out": r.get("eval_count", 0), "answer_held_a_secret": any(s in text for s in secrets), "readable": m is not None})
    return out


class Client:
    def __init__(self, base, token):
        self.base, self.token, self.csrf = base, token, None
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
        return json.load(self.op.open(urllib.request.Request(self.base + path, data=json.dumps(body).encode() if body is not None else None, headers=h, method="POST" if body is not None else "GET"), timeout=300))


def kakurega(c, dest):
    started = time.time()
    job = c.req("/api/jobs", {"repo": str(dest), "recipient": "external-auditor"})["id"]
    state = None
    while time.time() - started < 3600:
        state = next((x for x in c.req("/api/jobs")["jobs"] if x["id"] == job), {}).get("state")
        if state in ("ready", "nothing_to_release", "failed", "rejected", "delivered"):
            break
        time.sleep(5)
    found = c.req(f"/api/jobs/{job}/findings")["findings"]
    confirmed = [f for f in found if f.get("status") == "reproduced"]
    out = score(confirmed)
    release = json.dumps(c.req(f"/api/jobs/{job}/release"))
    env = (dest / ".env").read_text()
    secrets = [l.split("=", 1)[1] for l in env.splitlines() if "=" in l and len(l.split("=", 1)[1]) > 8]
    out.update({"seconds": round(time.time() - started), "state": state, "proposed": len(found), "not_confirmed": len(found) - len(confirmed), "confirmed_files": sorted({f["file"] for f in confirmed}), "release_held_a_secret": any(s in release for s in secrets)})
    return out


def main():
    shutil.rmtree(work, ignore_errors=True)
    (work / "root").mkdir(parents=True)
    rows = {"plain": [], "kakurega": []}
    for i in range(RUNS):
        dest = work / "root" / f"plain-{i}"
        project(dest)
        rows["plain"].append(plain(dest))
        print("plain   ", i, rows["plain"][-1], flush=True)
    route = ["--route", "near", "--near-model", NEAR, "--near-key-file", os.environ["KAKU_TEST_NEAR_KEY_FILE"]] if NEAR else ["--model", MODEL]
    server = subprocess.Popen([str(kaku), "serve", "--data-dir", str(work / "data"), "--root", str(work / "root"), *route, "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    try:
        base = None
        end = time.time() + 60
        while time.time() < end and base is None:
            line = server.stdout.readline()
            if "#pair=" in line:
                base = "http://" + line.split("http://", 1)[1].split("/#pair=")[0]
        if base is None:
            raise SystemExit("the service did not start")
        c = Client(base, json.load(open(work / "data/admin.json"))["token"])
        for i in range(RUNS):
            dest = work / "root" / f"review-{i}"
            project(dest)
            rows["kakurega"].append(kakurega(c, dest))
            print("kakurega", i, rows["kakurega"][-1], flush=True)
        usage = c.req("/api/usage")
    finally:
        server.kill()
    summary = {}
    for arm, rs in rows.items():
        summary[arm] = {"runs": len(rs), "found_real_in": sum(r["found_real"] for r in rs), "false_findings_total": sum(r["false_findings"] for r in rs), "arguable_findings_total": sum(r["arguable_findings"] for r in rs), "worst_run_false_findings": max(r["false_findings"] for r in rs), "found_real_in_every_run": all(r["found_real"] for r in rs), "seconds_slowest": max(r["seconds"] for r in rs), "seconds_fastest": min(r["seconds"] for r in rs)}
    commit = subprocess.run(["git", "-C", str(repo), "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    out = repo / ("benchmarks/quality/results-near.json" if NEAR else "benchmarks/quality/results.json")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({"type": "kakurega-quality-eval", "version": 1, "model": MODEL, "commit": commit, "ran_at": int(time.time()), "project": "fixtures/synthetic-repo", "real_defect": REAL, "runs": rows, "summary": summary, "kakurega_usage": usage, "money": "the model runs on this computer: no charge"}, indent=1, sort_keys=True) + "\n")
    print(json.dumps(summary, indent=1))
    print("wrote", out)


if __name__ == "__main__":
    main()
