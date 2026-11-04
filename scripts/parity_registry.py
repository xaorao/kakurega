#!/usr/bin/env python3
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CATALOG = ROOT.parent / "ARCHITECTURE" / "04_FEATURE_CATALOG.md"
REGISTRY = ROOT / "program" / "parity_registry.json"
CLASSES = {"WEB_HOSTED", "WEB_COMPANION", "DESKTOP_NATIVE", "EXTENSION", "CLI_MCP", "MOBILE", "TECHNICALLY_INELIGIBLE", "KERNEL_ALL_SURFACES", "CUT_BY_CATALOG"}
STATUSES = {"NOT_BUILT", "PARTIAL", "BUILT_TESTED", "DEFERRED_WITH_REASON", "CUT_WITH_REASON"}
ROW = re.compile(r"^\|\s*([A-Z]{1,2}\d+|\d+)\s*\|(.*)\|\s*$")


def catalog_rows():
    rows = {}
    for line in CATALOG.read_text().splitlines():
        m = ROW.match(line)
        if not m:
            continue
        fid, rest = m.group(1), [c.strip() for c in m.group(2).split("|")]
        if fid.isdigit():
            fid = "G" + fid
        title = re.sub(r"\*\*|`", "", rest[0])[:140] if rest else ""
        rows.setdefault(fid, {"id": fid, "title": title, "catalog_cells": rest[1:4]})
    return rows


def load():
    return json.loads(REGISTRY.read_text()) if REGISTRY.exists() else {"entries": {}}


def sync():
    reg = load()
    rows = catalog_rows()
    for fid, r in rows.items():
        e = reg["entries"].setdefault(fid, {})
        e["title"] = r["title"]
        e["catalog_cells"] = r["catalog_cells"]
        if rows[fid]["catalog_cells"][1:2] and rows[fid]["catalog_cells"][1].upper().startswith("REJECT") or (len(rows[fid]["catalog_cells"]) > 0 and rows[fid]["catalog_cells"][0].upper().startswith("REJECT")):
            if e.get("surface_class") is None:
                e["surface_class"] = "CUT_BY_CATALOG"
                e["trust_reason"] = "Rejected in the feature catalog with a stated reason: " + " ".join(rows[fid]["catalog_cells"])[:200]
                e["status"] = "CUT_WITH_REASON"
                e["evidence"] = "ARCHITECTURE/04_FEATURE_CATALOG.md"
        for k, v in (("surface_class", None), ("trust_reason", None), ("status", "NOT_BUILT"), ("evidence", None)):
            e.setdefault(k, v)
    for fid in list(reg["entries"]):
        if fid not in rows:
            del reg["entries"][fid]
    REGISTRY.write_text(json.dumps(reg, indent=1, ensure_ascii=False, sort_keys=True) + "\n")
    return reg


def check(reg):
    problems = []
    unclassified = 0
    for fid, e in reg["entries"].items():
        if e["surface_class"] is None or not e["trust_reason"]:
            unclassified += 1
            continue
        if e["surface_class"] not in CLASSES:
            problems.append(f"{fid}: unknown class {e['surface_class']}")
        if e["status"] not in STATUSES:
            problems.append(f"{fid}: unknown status {e['status']}")
        if e["status"] == "BUILT_TESTED" and not e["evidence"]:
            problems.append(f"{fid}: BUILT_TESTED without evidence")
    return unclassified, problems


if __name__ == "__main__":
    reg = sync()
    unclassified, problems = check(reg)
    total = len(reg["entries"])
    built = sum(1 for e in reg["entries"].values() if e["status"] == "BUILT_TESTED")
    print(f"features {total}; classified {total - unclassified}; unclassified {unclassified}; built and tested {built}; problems {len(problems)}")
    for p in problems:
        print("  " + p)
    if problems:
        sys.exit(1)
    print("PARITY REGISTRY COMPLETE" if unclassified == 0 else "PARITY REGISTRY INCOMPLETE")
    sys.exit(0 if unclassified == 0 else 3)
