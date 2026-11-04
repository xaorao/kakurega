#!/usr/bin/env python3
import json
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def run_item(item):
    start = time.time()
    p = subprocess.run(item["command"], shell=True, cwd=ROOT, capture_output=True, text=True)
    out = p.stdout + p.stderr
    ok = p.returncode == 0 and item["expect"] in out
    return {"id": item["id"], "status": "PASS" if ok else "FAIL", "seconds": round(time.time() - start), "tail": out.strip().splitlines()[-3:]}


def main():
    manifest = json.loads((ROOT / "program/ACCEPTANCE_MANIFEST.json").read_text())
    milestone = sys.argv[1] if len(sys.argv) > 1 and not sys.argv[1].startswith("--") else "I"
    only = [a.split("=", 1)[1] for a in sys.argv if a.startswith("--only=")]
    skip_heavy = "--skip-heavy" in sys.argv
    items = manifest["milestones"][milestone]["items"]
    results = []
    for item in items:
        if only and item["id"] not in only:
            continue
        if item["kind"] == "machine":
            if skip_heavy and item["command"] != "scripts/test":
                results.append({"id": item["id"], "status": "SKIPPED_BY_FLAG"})
                continue
            results.append(run_item(item))
        elif item["kind"] == "human":
            results.append({"id": item["id"], "status": "NEEDS_HUMAN_REVIEW", "evidence_needed": item["evidence_needed"]})
        elif item["kind"] == "pending":
            results.append({"id": item["id"], "status": "PENDING", "evidence_needed": item["evidence_needed"]})
        else:
            results.append({"id": item["id"], "status": "BLOCKED_HUMAN", "blocked_on": item["blocked_on"], "evidence_needed": item["evidence_needed"]})
    rev = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=ROOT, capture_output=True, text=True).stdout.strip()
    print(json.dumps({"milestone": milestone, "revision": rev, "results": results}, indent=2))
    statuses = {r["status"] for r in results}
    if "FAIL" in statuses:
        print("GATE: FAIL")
        return 1
    if statuses <= {"PASS"}:
        print("GATE: PASS")
        return 0
    if "PENDING" in statuses:
        print("GATE: INCOMPLETE (work not yet done: " + ", ".join(r["id"] for r in results if r["status"] == "PENDING") + ")")
        return 3
    if "SKIPPED_BY_FLAG" in statuses:
        print("GATE: INCOMPLETE (items skipped by flag)")
        return 3
    print("GATE: CONDITIONAL PASS (all machine-checkable items pass; outstanding: " + ", ".join(sorted(statuses - {"PASS"})) + ")")
    return 0


if __name__ == "__main__":
    sys.exit(main())
