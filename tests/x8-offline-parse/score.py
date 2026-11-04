import json
import sys
from collections import defaultdict

truth = json.load(open(sys.argv[1]))
out = json.load(open(sys.argv[2]))["docs"]

def norm(s):
    return " ".join(str(s).split())

stats = defaultdict(lambda: {"total": 0, "text_found": 0, "anchor_ok": 0})
misses = []
for t in truth:
    items = out[t["doc"]]["items"]
    key = (t["doc"].rsplit(".", 1)[1], t["kind"])
    s = stats[key]
    s["total"] += 1
    if t["kind"] == "text":
        hit = [i for i in items if i["kind"] == "text" and t["text"] in norm(i["text"])]
        if hit:
            s["text_found"] += 1
            if "page" not in t or any(i["page"] == t["page"] for i in hit):
                s["anchor_ok"] += 1
            else:
                misses.append((t, "wrong page", [i["page"] for i in hit]))
        else:
            misses.append((t, "text not found", None))
    else:
        want = norm(t["text"])
        cands = [i for i in items if i["kind"] == "cell" and norm(i["text"]) == want]
        if cands:
            s["text_found"] += 1
            if any(i["row"] == t["row"] and i["col"] == t["col"] for i in cands):
                s["anchor_ok"] += 1
            else:
                misses.append((t, "wrong cell position", [(i["row"], i["col"]) for i in cands]))
        else:
            # a bare number may be rendered as text item rather than cell
            misses.append((t, "cell text not found", None))
print(f"{'format':6} {'kind':5} {'items':>5} {'text found':>10} {'anchor correct':>14}")
tot = {"total": 0, "text_found": 0, "anchor_ok": 0}
for (fmt, kind), s in sorted(stats.items()):
    print(f"{fmt:6} {kind:5} {s['total']:5} {s['text_found']:10} {s['anchor_ok']:14}")
    for k in tot:
        tot[k] += s[k]
print(f"ALL    all   {tot['total']:5} {tot['text_found']:10} {tot['anchor_ok']:14}")
for t, why, extra in misses[:12]:
    print("MISS", t["doc"], t["kind"], repr(t["text"])[:60], why, extra)
