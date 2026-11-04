#!/usr/bin/env python3
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
reg_path = ROOT / "program" / "parity_registry.json"
reg = json.loads(reg_path.read_text())
for f in sorted((ROOT / "program" / "parity").glob("*.json")):
    d = json.loads(f.read_text())
    for g in d["groups"]:
        for i in g["ids"]:
            if i not in reg["entries"]:
                sys.exit(f"{f.name}: {i} is not in the catalog")
            e = reg["entries"][i]
            e["surface_class"], e["trust_reason"] = g["surface_class"], g["trust_reason"]
    for i, b in d.get("built", {}).items():
        reg["entries"][i]["status"], reg["entries"][i]["evidence"] = b["status"], b["evidence"]
reg_path.write_text(json.dumps(reg, indent=1, ensure_ascii=False, sort_keys=True) + "\n")
