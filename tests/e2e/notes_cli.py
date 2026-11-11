#!/usr/bin/env python3
"""`kaku notes` as a script uses it: a transcript file in, notes out, each point with the line its quote is on.
The model is the stand-in server of tests/e2e/team_script.py, reached through the real local-model route."""
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/notes-cli-tmp"
subprocess.run(["pkill", "-f", "kaku serve --data-dir " + str(work)], capture_output=True)
shutil.rmtree(work, ignore_errors=True)
(work / "root").mkdir(parents=True)
data = work / "data"
kaku = repo / "target/debug/kaku"
port = 20000 + os.getpid() % 1000
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


key = "-".join(["sk-ant", "api03-abcdefghijklmnopqrstuvwxyz0123456789"])
transcript = work / "sync.txt"
transcript.write_text(
    "Asha Rao: Then we ship the new pricing page on Friday. Everyone agreed?\n"
    "Cara Lind: Agreed.\n"
    f"Ben Okafor: the deploy key is {key}\n"
    "Cara Lind: I'll send the addendum to their counsel at legal@meridian.example by Wednesday.\n"
)
model = subprocess.Popen([sys.executable, str(repo / "tests/e2e/team_script.py"), str(port)])
time.sleep(0.6)
common = ["--data-dir", str(data), "--root", str(work / "root"), "--model", "team-script", "--ollama", f"http://127.0.0.1:{port}"]
server = subprocess.Popen([str(kaku), "serve", *common, "--ui-dir", str(repo / "apps/web/dist")], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def k(*args, stdin=None):
    p = subprocess.run([str(kaku), "notes", *args, *common], capture_output=True, text=True, timeout=120, input=stdin)
    return p.returncode, p.stdout, p.stderr


try:
    deadline = time.time() + 30
    while not (data / "admin.json").exists() and time.time() < deadline:
        time.sleep(0.1)
    rc, out, err = k()
    check("without a file it prints the usage, exit 2", rc == 2 and "usage: kaku notes FILE" in err, f"{rc} {err}")
    rc, out, err = k(str(work / "nope.txt"))
    check("a file that is not there is said in words, exit 2", rc == 2 and "cannot read" in err, f"{rc} {err}")
    short = work / "short.txt"
    short.write_text("hello\n")
    rc, out, err = k(str(short))
    check("too little text is refused by the service in words, exit 6", rc == 6 and "too little text" in err, f"{rc} {err}")
    rc, out, err = k(str(transcript), "--title", "Weekly sync")
    check("the notes are printed as text with the meeting's name", rc == 0 and out.startswith("# Weekly sync\n"), f"{rc} {out[:200]} {err}")
    check("each checked point names the line its quote is on", "- Ship the new pricing page on Friday [line 1]" in out and "- Cara Lind: Send the addendum to counsel (due: by Wednesday) [line 4]" in out, out)
    facts, _, guesses = out.partition("## Not backed by the transcript")
    check("the invented action is under its own heading, not among the facts", "Hire two engineers" in guesses and "Hire two engineers" not in facts, out)
    check("the counts, where the model runs and the withheld line are said on the error stream", "2 checked against the transcript, 1 not backed by it." in err and "runs on this computer" in err and "1 line that holds a secret was not sent." in err, err)
    check("the key is nowhere in what is printed", key not in out and key not in err and "sk-ant" not in out)
    rc, out, err = k(str(transcript), "--json")
    v = json.loads(out)
    check("--json prints one object with the checked points and their lines", rc == 0 and [p["line"] for p in v["checked"]] == [1, 4] and v["withheld_lines"] == 1 and v["route"]["stays_here"] is True, out[:300])
    target = work / "notes.md"
    rc, out, err = k(str(transcript), "--out", str(target))
    check("--out writes the notes to a file", rc == 0 and target.read_text().startswith("# Meeting notes\n") and out.strip() == f"Wrote {target}", f"{rc} {out} {err}")
    rc, out, err = k(str(transcript), "--out", str(target))
    check("--out never writes over a file that exists", rc == 2 and "cannot write" in err, f"{rc} {err}")
    rc, out, err = k("-", stdin=transcript.read_text())
    check("- reads the transcript from standard input", rc == 0 and "[line 1]" in out, f"{rc} {out[:200]} {err}")
finally:
    server.terminate()
    model.terminate()
    subprocess.run(["pkill", "-f", "kaku serve --data-dir " + str(work)], capture_output=True)

print(f"{sum(results)}/{len(results)} passed")
sys.exit(0 if all(results) else 1)
