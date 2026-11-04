import test from "node:test";
import assert from "node:assert/strict";
import { matchItems, wordScore } from "../src/palette-match.ts";

const items = [
  { id: "a", label: "New review", group: "Go to" },
  { id: "b", label: "Memory", group: "Go to" },
  { id: "c", label: "Review a project", group: "Actions" },
  { id: "d", label: "Documents", group: "Go to" },
  { id: "e", label: "Check a capsule", group: "Go to" },
  { id: "f", label: "Inbox", group: "Review queue" },
];
const ids = (q) => matchItems(items, q).map((i) => i.id);

test("pm01 no query keeps the given order and every item", () => {
  assert.deepEqual(ids(""), ["a", "b", "c", "d", "e", "f"]);
  assert.deepEqual(ids("   "), ["a", "b", "c", "d", "e", "f"]);
});

test("pm02 a label that starts with the word beats one that only contains it, and a label match beats a group match", () => {
  assert.deepEqual(ids("review").slice(0, 3), ["c", "a", "f"]);
  assert.equal(ids("review").at(-1), "f", "group-only match is last");
});

test("pm03 every typed word must fit, in any order", () => {
  assert.deepEqual(ids("project review"), ["c"]);
  assert.deepEqual(ids("zzz"), []);
  assert.deepEqual(ids("memory zzz"), []);
});

test("pm04 letters typed in order find an item (an abbreviation); letters missing or out of order do not", () => {
  assert.ok(ids("mmry").includes("b"));
  assert.ok(ids("ckcps").includes("e"));
  assert.deepEqual(ids("z"), []);
  assert.deepEqual(ids("mq"), [], "letters out of order or missing do not match");
});

test("pm05 ties keep the original order, and case does not matter", () => {
  assert.deepEqual(ids("GO TO").slice(0, 4), ["a", "b", "d", "e"]);
});

test("pm06 scores are ordered the way the ranking promises, and other scripts work", () => {
  assert.ok(wordScore("memory", "mem") > wordScore("my memory", "mem") && wordScore("my memory", "mem") > wordScore("amem", "mem") && wordScore("amem", "mem") > wordScore("m a e m", "mem"));
  assert.equal(wordScore("abc", "x"), -1);
  assert.ok(wordScore("नई समीक्षा", "समी") > 0);
  assert.ok(wordScore("नई समीक्षा", "नसम") > 0);
});
