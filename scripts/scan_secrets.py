#!/usr/bin/env python3
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

PATTERNS = {
    "private key block": re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
    "telegram bot token": re.compile(r"\b\d{8,10}:[A-Za-z0-9_-]{35}\b"),
    "api key (sk- prefix)": re.compile(r"\bsk-[A-Za-z0-9_-]{24,}\b"),
    "huggingface token": re.compile(r"\bhf_[A-Za-z0-9]{30,}\b"),
    "github token": re.compile(r"\b(?:ghp|gho|ghs|github_pat)_[A-Za-z0-9_]{30,}\b"),
    "stripe live key": re.compile(r"\bsk_live_[A-Za-z0-9]{16,}\b"),
    "aws access key": re.compile(r"\bAKIA[0-9A-Z]{16}\b"),
    "env assignment of a secret": re.compile(
        r"(?im)^\s*(?:export\s+)?[A-Z0-9_]*(?:API_KEY|SECRET|TOKEN|PASSWORD|SEED|MNEMONIC)[A-Z0-9_]*\s*=\s*['\"]?[A-Za-z0-9+/_\-]{16,}"
    ),
}
BLOCKED_PATH = re.compile(r"(^|/)(\.secrets/|\.env($|\.)|.*\.pem$|.*\.key$|id_rsa|id_ed25519)")
SELF = {"scripts/scan_secrets.py"}
FAKE_FIXTURE_PREFIX = "fixtures/synthetic-repo/"
MARKER_FILES = {
    "crates/kk-review/src/import.rs": {"private key block"},
    "crates/kk-secrets/src/lib.rs": {"private key block"},
    "crates/kk-secrets/tests/corpus.rs": {"private key block"},
    "apps/extension/formats.js": {"private key block"},
    "crates/kk-review/tests/release.rs": {"private key block"},
    "crates/kk-review/tests/import.rs": {"private key block"},
}


def scan_text(path, text):
    hits = []
    if BLOCKED_PATH.search(path):
        hits.append((path, "blocked file name"))
    if path in SELF:
        return hits
    for label, rx in PATTERNS.items():
        if label in MARKER_FILES.get(path, set()):
            continue
        for m in rx.finditer(text):
            if path.startswith(FAKE_FIXTURE_PREFIX) and "fake" in m.group(0).lower():
                continue
            hits.append((path, label))
            break
    return hits


def tracked_or_staged(staged_only):
    cmd = ["git", "diff", "--cached", "--name-only", "--diff-filter=ACM"] if staged_only \
        else ["git", "ls-files", "--cached", "--others", "--exclude-standard"]
    out = subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True, check=True).stdout
    return [p for p in out.splitlines() if p]


def selftest():
    fake_tg = "123456789:" + "A" * 35
    fake_sk = "sk-" + "a" * 30
    assert scan_text("x.txt", "token " + fake_tg)
    assert scan_text("x.txt", fake_sk)
    assert scan_text("x.txt", "-----BEGIN " + "PRIVATE KEY-----")
    assert scan_text("config/.env", "A=b")
    assert scan_text("x.txt", "NEAR_AI_API_KEY=" + "q" * 20)
    assert not scan_text("x.txt", "ordinary text with no secrets")
    assert not scan_text("fixtures/synthetic-repo/_env", "PAYMENTS_API_KEY=sk_live_51NfakeFAKEfakeFAKE0000000000")
    assert scan_text("fixtures/synthetic-repo/_env", "PAYMENTS_API_KEY=" + "Zq9" * 8)
    assert scan_text("elsewhere.rs", "-----BEGIN " + "PRIVATE KEY-----")
    assert not scan_text("crates/kk-review/src/import.rs", "-----BEGIN " + "PRIVATE KEY-----")
    assert scan_text("elsewhere/_env", "PAYMENTS_API_KEY=sk_live_51NfakeFAKEfakeFAKE0000000000")
    print("scan_secrets selftest: ok (11 cases)")


def main():
    if "--selftest" in sys.argv:
        selftest()
        return 0
    staged = "--staged" in sys.argv
    bad = []
    for rel in tracked_or_staged(staged):
        p = ROOT / rel
        if not p.is_file():
            continue
        try:
            text = p.read_text(errors="ignore") if p.stat().st_size < 2_000_000 else ""
        except OSError:
            continue
        bad.extend(scan_text(rel, text))
    if bad:
        print("SECRET SCAN FAIL")
        for path, label in bad:
            print("  %s: %s" % (path, label))
        return 1
    print("secret scan ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
