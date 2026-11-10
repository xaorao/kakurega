import test from "node:test";
import assert from "node:assert/strict";
import { pluralKey } from "../src/plural.ts";

const has = (k) => ["a.entries.one", "b.one"].includes(k);

test("pl01 exactly one uses the singular text when there is one", () => {
  assert.equal(pluralKey("a.entries", { count: 1 }, has), "a.entries.one");
  assert.equal(pluralKey("b", { n: 1 }, has), "b.one");
  assert.equal(pluralKey("b", { n: "1" }, has), "b.one");
});

test("pl02 any other count, a missing count or a key with no singular keeps the key", () => {
  for (const n of [0, 2, 11, 101, -1, 1.5, "01", "1.0", "", "x"]) assert.equal(pluralKey("b", { n }, has), "b", String(n));
  assert.equal(pluralKey("b", {}, has), "b");
  assert.equal(pluralKey("c", { n: 1 }, has), "c");
});
