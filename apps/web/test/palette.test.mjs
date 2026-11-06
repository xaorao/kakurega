import test from "node:test";
import assert from "node:assert/strict";
import { matchItems, wordScore, sectionHeads, moveSelection, moveSection } from "../src/palette-match.ts";

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

const grouped = [
  { id: "a", label: "A", group: "Go to" },
  { id: "b", label: "B", group: "Go to" },
  { id: "c", label: "C", group: "Go to" },
  { id: "d", label: "D", group: "Theme" },
  { id: "e", label: "E", group: "Theme" },
  { id: "f", label: "F", group: "Account" },
];

test("pm07 while browsing a header is shown where the group changes, and never while searching", () => {
  assert.deepEqual(sectionHeads(grouped, ""), [0, 3, 5]);
  assert.deepEqual(sectionHeads(grouped, "  "), [0, 3, 5], "blank counts as browsing");
  assert.deepEqual(sectionHeads(grouped, "a"), []);
  assert.deepEqual(sectionHeads([], ""), []);
  // the same group appearing again later starts a new section: headers follow the order given
  assert.deepEqual(sectionHeads([{ id: "x", label: "X", group: "G" }, { id: "y", label: "Y", group: "H" }, { id: "z", label: "Z", group: "G" }], ""), [0, 1, 2]);
});

test("pm08 page moves stop at the ends; arrows still wrap", () => {
  assert.equal(moveSelection(0, 6, "pageDown"), 5);
  assert.equal(moveSelection(2, 30, "pageDown"), 12);
  assert.equal(moveSelection(25, 30, "pageDown"), 29, "stops at the last item");
  assert.equal(moveSelection(4, 30, "pageUp"), 0, "stops at the first item");
  assert.equal(moveSelection(15, 30, "pageUp"), 5);
  assert.equal(moveSelection(5, 6, "down"), 0, "down from the last wraps");
  assert.equal(moveSelection(0, 6, "up"), 5, "up from the first wraps");
  for (const m of ["down", "up", "pageDown", "pageUp"]) assert.equal(moveSelection(0, 0, m), 0, `nothing to move in: ${m}`);
});

test("pm09 Alt+Up and Alt+Down jump between groups and wrap", () => {
  assert.equal(moveSection(grouped, "", 0, 1), 3);
  assert.equal(moveSection(grouped, "", 3, 1), 5);
  assert.equal(moveSection(grouped, "", 5, 1), 0, "past the last group wraps to the first");
  assert.equal(moveSection(grouped, "", 4, -1), 3, "from inside a group, up goes to that group's first item");
  assert.equal(moveSection(grouped, "", 3, -1), 0);
  assert.equal(moveSection(grouped, "", 0, -1), 5, "before the first group wraps to the last");
});

test("pm10 while searching there are no sections, so the section keys do nothing", () => {
  assert.equal(moveSection(grouped, "a", 2, 1), 2);
  assert.equal(moveSection([], "", 0, 1), 0);
});
