import { test } from "node:test";
import assert from "node:assert/strict";
import { when, whenToday } from "../src/when.ts";

test("a date names its month, so 12 November cannot be read as 11 December", () => {
  const at = Date.UTC(2026, 10, 12, 14, 5) / 1000;
  const en = when(at, "en");
  assert.match(en, /Nov/);
  assert.match(en, /2026/);
  assert.doesNotMatch(en, /\d{1,2}\/\d{1,2}\/\d{4}/);
  assert.doesNotMatch(when(at, "en", { dateOnly: true }), /:/);
  assert.match(when(at, "hi"), /[ऀ-ॿ]/);
});

test("earlier today shows the time alone, another day shows the date too", () => {
  const now = Date.UTC(2026, 10, 12, 18, 0);
  const earlier = new Date(now);
  earlier.setHours(earlier.getHours() - 1);
  assert.doesNotMatch(whenToday(earlier.getTime() / 1000, "en", now), /2026/);
  assert.match(whenToday((now - 3 * 86400 * 1000) / 1000, "en", now), /2026/);
});
