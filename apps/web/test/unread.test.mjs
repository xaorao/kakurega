import assert from "node:assert/strict";
import test from "node:test";
import { MAX_REMEMBERED, baseline, loadSeen, markSeen, saveSeen, unreadIds } from "../src/unread.ts";

const mem = () => {
  const m = new Map();
  return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => m.set(k, v), raw: m };
};

test("un01 on the first visit nothing is new, a later change or a new conversation is, and the open one never is", () => {
  const list = [{ id: "a", updated: 10 }, { id: "b", updated: 20 }];
  const seen = baseline(list);
  assert.equal(unreadIds(list, seen, null).size, 0);
  const later = [{ id: "a", updated: 10 }, { id: "b", updated: 25 }, { id: "c", updated: 30 }];
  assert.deepEqual([...unreadIds(later, seen, null)].sort(), ["b", "c"]);
  assert.deepEqual([...unreadIds(later, seen, "b")].sort(), ["c"], "the one that is open is not marked");
  assert.equal(unreadIds([{ id: "a", updated: 5 }], { a: 10 }, null).size, 0, "an older time than seen is not a change");
});

test("un02 looking at a conversation clears it and never moves the memory backwards", () => {
  let seen = baseline([{ id: "a", updated: 10 }]);
  seen = markSeen(seen, "a", 40);
  assert.equal(seen.a, 40);
  seen = markSeen(seen, "a", 12);
  assert.equal(seen.a, 40, "a stale time cannot make a seen conversation new again");
  assert.equal(markSeen(seen, "z", 3).z, 3);
});

test("un03 the memory is kept in storage, bounded, and damaged or missing storage reads as nothing remembered", () => {
  const s = mem();
  assert.equal(loadSeen(s), null);
  saveSeen(s, { a: 1, b: 2 });
  assert.deepEqual(loadSeen(s), { a: 1, b: 2 });
  const big = Object.fromEntries(Array.from({ length: MAX_REMEMBERED + 50 }, (_, i) => [`c${i}`, i]));
  saveSeen(s, big);
  const back = loadSeen(s);
  assert.equal(Object.keys(back).length, MAX_REMEMBERED);
  assert.ok(back[`c${MAX_REMEMBERED + 49}`] !== undefined && back.c0 === undefined, "the most recently seen are kept");
  for (const junk of ["", "not json", "[]", "5", "null", '{"a":"x"}']) {
    s.raw.set("kk-seen", junk);
    const v = loadSeen(s);
    assert.ok(v === null || Object.keys(v).length === 0, junk);
  }
  s.raw.set("kk-seen", JSON.stringify({ ["x".repeat(81)]: 1, ok: 2, nan: null }));
  assert.deepEqual(loadSeen(s), { ok: 2 });
  assert.equal(loadSeen({ getItem: () => { throw new Error("blocked"); } }), null);
  saveSeen({ setItem: () => { throw new Error("full"); } }, { a: 1 });
  assert.equal(loadSeen(undefined), null);
});
