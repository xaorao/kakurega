import test from "node:test";
import assert from "node:assert/strict";
import { findMatches, stepTo, MAX_MATCHES, MAX_QUERY } from "../src/find.ts";

test("fd01 every occurrence is found in order, ignoring case, per message", () => {
  const m = findMatches(["The Deadline moved", "no match here", "deadline, DEADLINE"], "deadline");
  assert.deepEqual(m, [
    { turn: 0, start: 4, end: 12 },
    { turn: 2, start: 0, end: 8 },
    { turn: 2, start: 10, end: 18 },
  ]);
});

test("fd02 occurrences do not overlap and an empty or blank query finds nothing", () => {
  assert.deepEqual(findMatches(["aaaa"], "aa").map((x) => x.start), [0, 2]);
  assert.deepEqual(findMatches(["abc"], ""), []);
  assert.deepEqual(findMatches(["a b c"], "   "), []);
  assert.deepEqual(findMatches([], "x"), []);
});

test("fd03 a query is plain text, never a pattern", () => {
  assert.deepEqual(findMatches(["a.c abc"], "a.c").map((x) => x.start), [0]);
  assert.deepEqual(findMatches(["(x) [y] *"], "(x)").length, 1);
  assert.deepEqual(findMatches(["anything"], ".*"), []);
});

test("fd04 the number of matches and the query are bounded", () => {
  assert.equal(findMatches(["a ".repeat(2000)], "a").length, MAX_MATCHES);
  const long = "x".repeat(MAX_QUERY + 50);
  assert.equal(findMatches(["x".repeat(2 * MAX_QUERY)], long).length, 2, "a longer query is cut to the limit, so it matches twice in a text twice the limit");
});

test("fd05 a text whose lower case has another length is skipped, so offsets never point at the wrong place", () => {
  assert.deepEqual(findMatches(["İstanbul istanbul"], "istanbul").map((x) => x.turn), [], "dotted capital I grows when lower-cased");
});

test("fd06 stepping wraps in both directions and handles none", () => {
  assert.equal(stepTo(0, null, 1), null);
  assert.equal(stepTo(3, null, 1), 0);
  assert.equal(stepTo(3, null, -1), 2);
  assert.equal(stepTo(3, 2, 1), 0);
  assert.equal(stepTo(3, 0, -1), 2);
  assert.equal(stepTo(1, 0, 1), 0);
});
