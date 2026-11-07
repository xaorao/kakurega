import test from "node:test";
import assert from "node:assert/strict";
import { copyNames } from "../src/forgotten.ts";
import { en } from "../src/locales/en.ts";
import { hi } from "../src/locales/hi.ts";

test("fg01 known copies are listed by name, reviews first, and the rest are counted", () => {
  assert.deepEqual(copyNames({ copies_known: { reviews: ["job-1"], exports: ["a.md", "b.md"] } }), { shown: ["job-1", "a.md", "b.md"], more: 0 });
  assert.deepEqual(copyNames({ copies_known: { reviews: [], exports: [] } }), { shown: [], more: 0 });
  assert.deepEqual(copyNames(null), { shown: [], more: 0 });
  assert.deepEqual(copyNames({}), { shown: [], more: 0 });
  const many = { copies_known: { reviews: [], exports: Array.from({ length: 9 }, (_, i) => `f${i}.md`) } };
  assert.deepEqual(copyNames(many, 4), { shown: ["f0.md", "f1.md", "f2.md", "f3.md"], more: 5 });
});

test("fg02 anything that is not a plain short name is dropped, so a hostile file name cannot reach the screen", () => {
  const r = { copies_known: { reviews: ["job-1", 7, null, "x".repeat(121), "a\nb", "<b>x</b>", ""], exports: ["ok.md", "bad\u001b[2J.md"] } };
  assert.deepEqual(copyNames(r), { shown: ["job-1", "ok.md"], more: 0 });
});

test("fg03 the texts exist in both languages", () => {
  for (const k of ["mem.forgotten.copies", "mem.forgotten.more", "mem.forgotten.notReached"]) assert.ok(en[k] && hi[k], k);
});
