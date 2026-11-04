#!/usr/bin/env python3
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def load_layers():
    return json.loads((ROOT / "scripts" / "layers.json").read_text())["layers"]


def violations(metadata, layers):
    found = []
    names = {p["name"] for p in metadata["packages"] if p["name"].startswith("kk-")}
    for pkg in metadata["packages"]:
        name = pkg["name"]
        if not name.startswith("kk-"):
            continue
        if name not in layers:
            found.append("%s has no layer assigned in scripts/layers.json" % name)
            continue
        for dep in pkg["dependencies"]:
            dname = dep["name"]
            if dname == name or dname not in names:
                continue
            if dname not in layers:
                found.append("%s depends on %s which has no layer" % (name, dname))
            elif layers[dname] >= layers[name]:
                found.append(
                    "%s (layer %d) depends on %s (layer %d)"
                    % (name, layers[name], dname, layers[dname])
                )
    for name in layers:
        if name not in names:
            found.append("layers.json lists %s but no such crate exists" % name)
    return found


def selftest():
    layers = {"kk-types": 0, "kk-vault": 1, "kk-kernel": 1}
    ok = {"packages": [
        {"name": "kk-types", "dependencies": []},
        {"name": "kk-vault", "dependencies": [{"name": "kk-types"}]},
        {"name": "kk-kernel", "dependencies": [{"name": "kk-types"}, {"name": "serde"}]},
    ]}
    up = {"packages": [
        {"name": "kk-types", "dependencies": [{"name": "kk-vault"}]},
        {"name": "kk-vault", "dependencies": []},
        {"name": "kk-kernel", "dependencies": []},
    ]}
    sideways = {"packages": [
        {"name": "kk-types", "dependencies": []},
        {"name": "kk-vault", "dependencies": [{"name": "kk-kernel"}]},
        {"name": "kk-kernel", "dependencies": []},
    ]}
    unassigned = {"packages": [{"name": "kk-new", "dependencies": []}]}
    assert violations(ok, layers) == [], "clean graph must pass"
    assert violations(up, layers), "upward dependency must fail"
    assert violations(sideways, layers), "same-layer dependency must fail"
    selfdep = {"packages": [{"name": "kk-types", "dependencies": [{"name": "kk-types"}]}]}
    assert violations(selfdep, {"kk-types": 0}) == [], "a crate enabling its own features is not an edge"
    assert violations(unassigned, {}), "unassigned crate must fail"
    print("check_layers selftest: ok (5 cases)")


def main():
    if "--selftest" in sys.argv:
        selftest()
        return 0
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout
    found = violations(json.loads(out), load_layers())
    if found:
        print("AR01 FAIL")
        for line in found:
            print("  " + line)
        return 1
    print("AR01 ok: layer rule holds")
    return 0


if __name__ == "__main__":
    sys.exit(main())
