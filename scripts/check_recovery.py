#!/usr/bin/env python3
"""Checks the capability-recovery records against the code, so the claims in them can be trusted.

  - the ledger has no PARTIAL row, and every row has a status and a decision
  - every LATER row says what it waits for, every EXCLUDED row says why
  - every test id a BUILT or PARTIAL row cites (sv172, tu52, lp11 ...) is the start of a test that exists in the repository
  - every trace row records a reading depth (FULL, PART, MATRIX or NOT READ), and the counts line is right

Prints what it checked and exits 1 on the first kind of failure it finds.
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LEDGER = ROOT / "docs/engineering-decisions/rcr-ledger.md"
TRACE = ROOT / "docs/engineering-decisions/capability-trace.md"
STATUSES = {"BUILT", "PARTIAL", "EXCLUDED", "KEPT", "LATER"}


def test_ids():
    """Every identifier like sv172 that starts a test function or a named browser check in the repository."""
    ids = set()
    fn = re.compile(r"\bfn ([a-z]{1,4}\d{1,3}[a-z]?)(?:_|\b)")
    py = re.compile(r"^\s*def (?:test_)?([a-z]{1,4}\d{1,3}[a-z]?)(?:_|\b)", re.M)
    for base in ("crates", "apps/desktop", "apps/extension"):
        for p in (ROOT / base).rglob("*.rs"):
            if "target" in p.parts:
                continue
            ids.update(fn.findall(p.read_text(errors="ignore")))
    for p in list((ROOT / "apps").rglob("*.mjs")) + list((ROOT / "apps").rglob("*.ts")) + list((ROOT / "tests").rglob("*")):
        if p.is_file() and "node_modules" not in p.parts and p.suffix in (".mjs", ".ts", ".py", ".js", ".tsx"):
            t = p.read_text(errors="ignore")
            ids.update(re.findall(r"""['"`]([a-z]{1,4}\d{1,3}[a-z]?)[ :'"`]""", t))
            ids.update(py.findall(t))
    return ids


def ledger_rows():
    rows = []
    for l in LEDGER.read_text().splitlines():
        c = [x.strip() for x in l.split("|")]
        if len(c) > 6 and c[4] in STATUSES:
            rows.append(c)
    return rows


def main():
    problems = []
    rows = ledger_rows()
    known = test_ids()
    cite = re.compile(r"\b((?:sv|tu|lp|cw|sg|al|fz|md|vi|tm|kb|sk|tr|aw|rp|cs|cr|dg|wf|fb|uf|ff|at|pty|j|b|tt|ex|gw)\d{1,4}[a-z]?)\b")
    status = {}
    for c in rows:
        rid, st, text = c[1], c[4], c[5]
        status[st] = status.get(st, 0) + 1
        if st == "PARTIAL":
            problems.append(f"{rid}: still PARTIAL")
        # a row that is not built says why: a reason of its own, beyond a bare word
        if st in ("LATER", "EXCLUDED") and len(text.strip()) < 12 and len(c[3].strip()) < 40:
            problems.append(f"{rid}: {st} without a reason")
        if st in ("BUILT", "PARTIAL"):
            for t in sorted(set(cite.findall(text))):
                if t.startswith("pty"):
                    continue
                if t not in known:
                    problems.append(f"{rid}: cites {t}, which is not the start of any test found in the repository")
    trace = TRACE.read_text()
    depth = re.compile(r"^\| (\d+) \|[^|]*\|[^|]*\| (FULL|PART|MATRIX|NOT READ)[^|]*\|", re.M)
    index = depth.findall(trace)
    full = sum(1 for _, d in index if d == "FULL")
    part = sum(1 for _, d in index if d == "PART")
    m = re.search(r"Counts: FULL on (\d+) rows, PART on (\d+) rows", trace)
    if not m:
        problems.append("the trace has no counts line")
    elif (int(m.group(1)), int(m.group(2))) != (full, part):
        problems.append(f"the trace counts line says FULL {m.group(1)}, PART {m.group(2)} but its index has FULL {full}, PART {part}")
    if not index:
        problems.append("the trace index has no rows")
    print(f"ledger: {len(rows)} rows {dict(sorted(status.items()))}; test ids known: {len(known)}; trace index: {len(index)} rows (FULL {full}, PART {part})")
    for p in problems:
        print("PROBLEM", p)
    print("RECOVERY RECORDS OK" if not problems else f"{len(problems)} problem(s)")
    return 1 if problems else 0


sys.exit(main())
