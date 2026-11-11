#!/usr/bin/env python3
"""`kaku bench sign` and `kaku bench verify` as a reader of the repository uses them: results are signed, a signed file
checks against the pinned key, and a file that was changed, signed by someone else, or does not add up is refused."""
import base64
import json
import shutil
import subprocess
import sys
from pathlib import Path

repo = Path(__file__).resolve().parent.parent.parent
work = repo / "target/bench-cli-tmp"
shutil.rmtree(work, ignore_errors=True)
work.mkdir(parents=True)
kaku = repo / "target/debug/kaku"
results = []


def check(name, ok, detail=""):
    results.append(ok)
    print(("PASS  " if ok else "FAIL  ") + name + (f"  {detail}" if detail and not ok else ""), flush=True)


def k(*args):
    p = subprocess.run([str(kaku), "bench", *args], capture_output=True, text=True, timeout=60)
    return p.returncode, p.stdout, p.stderr


def case(name, door, status, asked, reached, verdict):
    return {"case": name, "door": door, "group": "known shapes", "http_status": status, "model_was_asked": asked, "reached_model": reached, "on_disk_plain": [], "plain_arm_leaked": True, "verdict": verdict}


def b64d(s):
    return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))


good = {"type": "kakurega-leak-benchmark", "version": 1, "commit": "0" * 40, "tree_had_uncommitted_changes": False, "model": "adversary",
        "cases": [case("attach/a", "attach", 200, True, False, "held"), case("ask/a", "ask", 200, True, True, "LEAK"), case("note/a", "note", 422, False, False, "refused")]}
f = work / "results.json"
f.write_text(json.dumps(good, indent=1))
key, other = work / "key", work / "other-key"

rc, out, err = k()
check("without words it prints the usage, exit 2", rc == 2 and "kaku bench sign" in err and "kaku bench verify" in err, err)
rc, out, err = k("sign", str(f), "--key", str(key))
check("it does not make a key unless asked", rc == 2 and "--new-key" in err and not key.exists(), err)
rc, out, err = k("sign", str(f), "--key", str(key), "--new-key")
signed = work / "results.signed.json"
pub = out.strip().splitlines()[-1].split(": ")[-1] if rc == 0 else ""
check("signing writes the signed file and prints the public key", rc == 0 and signed.exists() and len(b64d(pub)) == 32, f"{rc} {out} {err}")
check("the key file can be read by its owner only", oct(key.stat().st_mode & 0o777) == "0o600")
check("the private key is not in the signed file or the output", key.read_text().strip() not in signed.read_text() + out)

rc, out, err = k("verify", str(signed))
check("verifying without a pinned key is refused", rc == 2 and "--pin" in err, err)
rc, out, err = k("verify", str(signed), "--pin", pub)
check("the signed file checks, and the counts and the leak are printed from the cases", rc == 0 and "3 recorded cases" in out and "Held 1, refused before any model 1" in out and "leaked 1." in out and "leaked: ask/a" in out, f"{rc} {out} {err}")
check("it says what a signature does not show", "does not show the run was honest" in out)
rc, out, err = k("verify", str(signed), "--pin", pub, "--json")
v = json.loads(out) if rc == 0 else {}
check("--json gives the same as one object", v.get("signature") == "ok" and v["summary"]["groups"]["known shapes"]["LEAK"] == 1 and v["problems"] == [], out)

env = json.loads(signed.read_text())
payload = json.loads(b64d(env["payload"]))
payload["cases"][1]["verdict"], payload["cases"][1]["reached_model"] = "held", False
raw = json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()
enc = (base64.urlsafe_b64encode if ("-" in env["payload"] or "_" in env["payload"]) else base64.b64encode)(raw).decode()
edited = dict(env, payload=enc if env["payload"].endswith("=") or len(b64d(env["payload"])) % 3 == 0 else enc.rstrip("="))
(work / "edited.json").write_text(json.dumps(edited, sort_keys=True, separators=(",", ":")))
rc, out, err = k("verify", str(work / "edited.json"), "--pin", pub)
check("a leak edited out after signing is refused", rc == 2 and "NOT VERIFIED" in err and "signature" in err, f"{rc} {out} {err}")

rc, out2, err = k("sign", str(f), "--key", str(other), "--new-key", "--out", str(work / "other.signed.json"))
rc, out, err = k("verify", str(work / "other.signed.json"), "--pin", pub)
check("the same results signed by another key do not check against the pinned key", rc == 2 and "NOT VERIFIED" in err, f"{rc} {out} {err}")
rc, out, err = k("verify", str(signed), "--pin", "not-a-key")
check("a pin that is not a key is said in words", rc == 2 and "not a public key" in err, err)

bad = json.loads(json.dumps(good))
bad["cases"][1]["verdict"] = "held"
(work / "bad.json").write_text(json.dumps(bad))
rc, out, err = k("sign", str(work / "bad.json"), "--key", str(key))
check("results whose verdict does not follow from what was observed are not signed", rc == 2 and "observations give LEAK" in err and not (work / "bad.signed.json").exists(), f"{rc} {err}")
nocontrol = json.loads(json.dumps(good))
nocontrol["cases"][0]["plain_arm_leaked"] = False
(work / "nocontrol.json").write_text(json.dumps(nocontrol))
rc, out, err = k("sign", str(work / "nocontrol.json"), "--key", str(key))
check("results whose control failed are not signed", rc == 2 and "control" in err, f"{rc} {err}")
(work / "float.json").write_text('{"type": "kakurega-leak-benchmark", "version": 1, "score": 0.5, "cases": []}')
rc, out, err = k("sign", str(work / "float.json"), "--key", str(key))
check("a file this cannot sign is said in words", rc == 2 and "whole numbers" in err, f"{rc} {err}")
rc, out, err = k("verify", str(f), "--pin", pub)
check("an unsigned results file is not a signed one", rc == 2 and "not a signed results file" in err, f"{rc} {err}")

print(f"{sum(results)}/{len(results)} passed")
sys.exit(0 if all(results) else 1)
