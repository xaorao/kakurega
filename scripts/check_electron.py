#!/usr/bin/env python3
"""Is the pinned Electron still a version the Electron team fixes security bugs in?

Electron's support policy (electronjs.org/docs/latest/tutorial/electron-timelines): the latest three stable major versions are
supported, and only the newest release of each of them gets a fix. So a pinned version is
  - current   when it is the newest release of the newest major,
  - supported when its major is one of the newest three and it is the newest release of that major,
  - behind    when its major is supported but a newer release of it exists (a security fix may be in it),
  - unsupported when its major is older than the newest three.

    python3 scripts/check_electron.py [--strict] [--selftest]

Asks the npm registry; with no network it says it could not check and exits 0 unless --strict. Exits 1 for `unsupported`, and
for `behind` as well with --strict. Meant to be run before a release and on a schedule, not on every build.
"""
import json
import re
import sys
import urllib.request
from pathlib import Path

repo = Path(__file__).resolve().parent.parent


def parse(v):
    m = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", v)
    return tuple(int(x) for x in m.groups()) if m else None


def classify(pinned, releases):
    """`releases`: every stable version string. Returns (status, newest_of_pinned_major, newest_overall)."""
    stable = sorted(t for t in (parse(r) for r in releases) if t)
    p = parse(pinned)
    if p is None or not stable:
        return ("unknown", None, None)
    newest = stable[-1]
    majors = sorted({t[0] for t in stable})
    supported = majors[-3:]
    same = [t for t in stable if t[0] == p[0]]
    newest_same = same[-1] if same else None
    if p[0] not in supported:
        return ("unsupported", newest_same, newest)
    if newest_same and p < newest_same:
        return ("behind", newest_same, newest)
    return ("current" if p[0] == newest[0] else "supported", newest_same, newest)


def selftest():
    rel = ["43.0.0", "43.1.2", "44.0.0", "44.2.1", "45.0.0", "45.2.2", "45.2.3", "46.0.0"]
    assert classify("46.0.0", rel)[0] == "current"
    assert classify("45.2.3", rel)[0] == "supported"
    assert classify("45.2.2", rel) == ("behind", (45, 2, 3), (46, 0, 0))
    assert classify("44.2.1", rel)[0] == "supported"
    assert classify("44.0.0", rel)[0] == "behind"
    assert classify("43.1.2", rel)[0] == "unsupported", "four majors back"
    assert classify("not-a-version", rel)[0] == "unknown"
    assert classify("45.2.2", [])[0] == "unknown"
    assert classify("45.2.2", ["45.2.3-beta.1", "bad"])[0] == "unknown", "prereleases and junk are not releases"
    print("electron check selftest: ok")


def main():
    if "--selftest" in sys.argv:
        selftest()
        return 0
    pkg = json.loads((repo / "apps/desktop/package.json").read_text())
    pinned = pkg["devDependencies"]["electron"] if "electron" in pkg.get("devDependencies", {}) else pkg["dependencies"]["electron"]
    try:
        with urllib.request.urlopen("https://registry.npmjs.org/electron", timeout=20) as r:
            releases = list(json.load(r)["versions"].keys())
    except Exception as e:  # no network is not a failure of the pin
        print(f"electron {pinned}: could not ask the registry ({type(e).__name__}); not checked")
        return 1 if "--strict" in sys.argv else 0
    status, newest_same, newest = classify(pinned, releases)
    fmt = lambda t: ".".join(map(str, t)) if t else "?"
    print(f"electron {pinned}: {status} (newest of its line {fmt(newest_same)}, newest overall {fmt(newest)})")
    if status == "unsupported" or (status in ("behind", "unknown") and "--strict" in sys.argv):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
