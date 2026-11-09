#!/usr/bin/env python3
"""Fails when an error or hint message in the review and sandbox crates interpolates program output or file text.

A message that quotes what a test printed or what a file holds could show a viewer text their clearance does not allow
(the rule is OpenCode's: a hint that shows file contents only when reading that file is already allowed). In these crates the
messages are fixed words, counts and identifiers that were checked character by character; anything else must go through the
label check first, and a deliberate exception is named in ALLOWED below with the reason.

    python3 scripts/check_error_text.py
"""
import re
import sys
from pathlib import Path

repo = Path(__file__).resolve().parent.parent
CRATES = ["kk-review", "kk-worker", "kk-sandbox", "kk-run", "kk-kernel"]
RISKY = r"(stdout|stderr|output|content|contents|text|source|body|snippet|diff|file_text)"
# every `{name}` or `{name:...}` inside a format-like macro call, across lines
CALL = re.compile(r"(format|write|writeln|println|eprintln|panic|anyhow|bail)!\s*\((?P<args>(?:[^()]|\((?:[^()]|\([^()]*\))*\))*)\)", re.S)
HOLE = re.compile(r"\{" + RISKY + r"(?::[^}]*)?\}")
ALLOWED: dict[str, str] = {
    "crates/kk-review/src/ask.rs:137": "not a message: the block of a file's own text handed to the model as labelled data, after the label check and the secret withholding",
    "crates/kk-review/src/ask.rs:141": "not a message: the block of a file's own text handed to the model as labelled data, after the label check and the secret withholding",
}

def selftest():
    bad = [
        'let e = format!("failed: {stderr}");',
        'return Err(format!("could not apply\n{content}"));',
        'Failed(format!("{text:?} was wrong"))',
        'panic!("bad {output}")',
    ]
    good = [
        'let e = format!("{n} test(s) failed");',
        'format!("the package {pkg} is missing")',
        'let x = stderr.len();',
        'println!("{}", count)',
    ]
    for b in bad:
        assert any(HOLE.search(m.group("args")) for m in CALL.finditer(b)), b
    for g in good:
        assert not any(HOLE.search(m.group("args")) for m in CALL.finditer(g)), g
    print("error-text check selftest: ok (4 violations found, 4 clean lines passed)")


if "--selftest" in sys.argv:
    selftest()
    sys.exit(0)

hits = []
for crate in CRATES:
    for path in sorted((repo / "crates" / crate / "src").rglob("*.rs")):
        text = path.read_text()
        # tests and test-only modules may quote what they check
        text = text.split("#[cfg(test)]")[0]
        for m in CALL.finditer(text):
            if HOLE.search(m.group("args")):
                line = text.count("\n", 0, m.start()) + 1
                key = f"{path.relative_to(repo)}:{line}"
                if key not in ALLOWED:
                    hits.append(f"{key}: {' '.join(m.group('args').split())[:110]}")

if hits:
    print("FAIL  a message interpolates program output or file text:")
    print("\n".join("  " + h for h in hits))
    sys.exit(1)
print(f"error-text check: no message in {', '.join(CRATES)} interpolates program output or file text")
