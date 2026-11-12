#!/usr/bin/env python3
"""Every kind of entry the service writes to its signed log has a plain-word label on the Activity screen, in every language.

A kind with no label is shown as its raw name (`session.kept`), which is how a new feature's log line ends up unreadable.
Exit 1 names each kind without a label.
"""
import glob
import re
import sys
from pathlib import Path

repo = Path(__file__).resolve().parent.parent
kinds = set()
for path in glob.glob(str(repo / "crates/kk-service/src/**/*.rs"), recursive=True) + glob.glob(str(repo / "crates/kk-cli/src/**/*.rs"), recursive=True):
    text = Path(path).read_text()
    # only code that ships: a test module's own log lines are not shown to anyone
    text = text.split("#[cfg(test)]")[0]
    kinds |= set(re.findall(r'log_event\(\s*"([a-z_]+(?:\.[a-z_]+)+)"', text))
missing = []
for lang in ("en", "hi"):
    body = (repo / f"apps/web/src/locales/{lang}.ts").read_text()
    have = set(re.findall(r'"log\.kind\.([a-z_.]+)"', body))
    missing += [f"{lang}: {k}" for k in sorted(kinds - have)]
if missing:
    print("log entries with no label on the Activity screen:\n  " + "\n  ".join(missing))
    sys.exit(1)
print(f"{len(kinds)} kinds of log entry, each with a label in both languages")
