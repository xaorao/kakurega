#!/usr/bin/env python3
"""Builds the sample the memory recall benchmark runs on, from LongMemEval (MIT, Di Wu et al.).

    python3 scripts/recall-bench-prepare.py [--per-type N] [--dev-per-type N] [--out NAME] [PATH_TO_longmemeval_s_cleaned.json]

The data is downloaded once into target/recall-bench/ (git-ignored) from
https://huggingface.co/datasets/xiaowu0162/longmemeval-cleaned and never committed. The sample follows the reference
retrieval protocol (xiaowu0162/LongMemEval, src/retrieval/run_retrieval.py): the corpus is the user turns of the history
sessions, the query is the question, the ground truth is the user turns marked `has_answer` inside the answer sessions,
abstention questions are skipped, and so is any question with no such turn. From what is left it takes the same number of
questions per question type, in a fixed order, and splits each type into a small development part (to choose settings) and a
larger test part (to report), so a setting is never judged on the questions it was chosen on.
"""
import hashlib
import json
import sys
from pathlib import Path

import argparse

repo = Path(__file__).resolve().parent.parent
ap = argparse.ArgumentParser()
ap.add_argument("--per-type", type=int, default=12)
ap.add_argument("--dev-per-type", type=int, default=4)
ap.add_argument("--out", default="sample.json")
ap.add_argument("source", nargs="?", default=str(repo / "target/recall-bench/longmemeval_s_cleaned.json"))
args = ap.parse_args()
PER_TYPE = args.per_type
DEV_PER_TYPE = args.dev_per_type
src = Path(args.source)
out = repo / "target/recall-bench" / args.out

data = json.load(open(src))
by_type = {}
for q in data:
    if "_abs" in q["question_id"]:
        continue
    answer_sessions = set(q["answer_session_ids"])
    has_target = any(
        t.get("has_answer") and t["role"] == "user"
        for sid, sess in zip(q["haystack_session_ids"], q["haystack_sessions"])
        if sid in answer_sessions
        for t in sess
    )
    if not has_target:
        continue
    by_type.setdefault(q["question_type"], []).append(q)

questions = []
for qtype, items in sorted(by_type.items()):
    # a fixed, content-based order (not the file order), so the sample does not depend on how the file is sorted
    items.sort(key=lambda q: hashlib.sha256(q["question_id"].encode()).hexdigest())
    for n, q in enumerate(items[:PER_TYPE]):
        answer_sessions = set(q["answer_session_ids"])
        sessions = []
        for sid, date, sess in zip(q["haystack_session_ids"], q["haystack_dates"], q["haystack_sessions"]):
            turns = [
                {"i": i + 1, "text": t["content"], "answer": bool(t.get("has_answer")) and sid in answer_sessions}
                for i, t in enumerate(sess)
                if t["role"] == "user"
            ]
            sessions.append({"id": sid, "date": date, "turns": turns})
        questions.append(
            {
                "id": q["question_id"],
                "type": qtype,
                "question": q["question"],
                "split": "dev" if n < DEV_PER_TYPE else "test",
                "answer_sessions": sorted(answer_sessions),
                "sessions": sessions,
            }
        )

out.parent.mkdir(parents=True, exist_ok=True)
json.dump({"dataset": "LongMemEval_S cleaned (MIT)", "questions": questions}, open(out, "w"), ensure_ascii=False)
turns = sum(len(s["turns"]) for q in questions for s in q["sessions"])
print(f"{len(questions)} questions ({sum(q['split']=='dev' for q in questions)} dev, {sum(q['split']=='test' for q in questions)} test), {turns} user turns -> {out}")
for t in sorted(by_type):
    print(f"  {t}: {len(by_type[t])} usable, {min(PER_TYPE, len(by_type[t]))} sampled")
