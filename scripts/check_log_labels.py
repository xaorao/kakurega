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
KIND = re.compile(r'"([a-z_]+(?:\.[a-z_]+)+)"')


def arguments(text, start):
    """The top-level arguments of the call whose opening parenthesis is at `start`, as source text."""
    out, depth, cur, i, quoted = [], 0, [], start + 1, False
    while i < len(text):
        c = text[i]
        if quoted:
            cur.append(c)
            if c == "\\":
                cur.append(text[i + 1])
                i += 1
            elif c == '"':
                quoted = False
        elif c == '"':
            quoted = True
            cur.append(c)
        elif c in "([{":
            depth += 1
            cur.append(c)
        elif c in ")]}":
            if depth == 0:
                out.append("".join(cur))
                return out
            depth -= 1
            cur.append(c)
        elif c == "," and depth == 0:
            out.append("".join(cur))
            cur = []
        else:
            cur.append(c)
        i += 1
    return out


# the call and which of its arguments is the kind; the kind may be chosen by an if, so every name in that argument counts
CALLS = {"log_event": 0, "session_flag": 4}
kinds = set()
for path in glob.glob(str(repo / "crates/kk-service/src/**/*.rs"), recursive=True) + glob.glob(str(repo / "crates/kk-cli/src/**/*.rs"), recursive=True):
    text = Path(path).read_text()
    # only code that ships: a test module's own log lines are not shown to anyone
    text = text.split("#[cfg(test)]")[0]
    for name, at in CALLS.items():
        for m in re.finditer(r"\b" + name + r"\(", text):
            if re.search(r"fn\s+$", text[max(0, m.start() - 8) : m.start()]):
                continue
            args = arguments(text, m.end() - 1)
            if len(args) > at:
                kinds |= set(KIND.findall(args[at]))
missing = []
for lang in ("en", "hi"):
    body = (repo / f"apps/web/src/locales/{lang}.ts").read_text()
    have = set(re.findall(r'"log\.kind\.([a-z_.]+)"', body))
    missing += [f"{lang}: {k}" for k in sorted(kinds - have)]
if missing:
    print("log entries with no label on the Activity screen:\n  " + "\n  ".join(missing))
    sys.exit(1)
print(f"{len(kinds)} kinds of log entry, each with a label in both languages")
