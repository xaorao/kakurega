import test from "node:test";
import assert from "node:assert/strict";
import { parseDiff, markChanges } from "../src/diff.ts";

test("df01 a unified diff keeps old and new line numbers on each side", () => {
  const lines = parseDiff("--- a/x.py\n+++ b/x.py\n@@ -3,4 +3,3 @@\n keep\n-gone\n-gone too\n+new\n tail\n");
  assert.deepEqual(lines.map((l) => l.kind), ["file", "file", "hunk", "ctx", "del", "del", "add", "ctx"]);
  assert.deepEqual([lines[3].old, lines[3].now], [3, 3]);
  assert.deepEqual([lines[4].old, lines[4].now], [4, undefined]);
  assert.deepEqual([lines[6].old, lines[6].now], [undefined, 4]);
  assert.deepEqual([lines[7].old, lines[7].now], [6, 5]);
});

test("df02 odd input never throws and keeps its text", () => {
  for (const d of ["", "garbage", "@@ nonsense @@\n+x", "+", "-", "\n\n\n"]) parseDiff(d);
  assert.equal(parseDiff("just text")[0].text, "just text");
  assert.equal(parseDiff("+<script>alert(1)</script>")[0].text, "<script>alert(1)</script>", "angle brackets stay text; the screen escapes them");
});

test("df03 a removed or added line that looks like a file header is still a changed line, and numbering stays right", () => {
  const d = "--- a/q.sql\n+++ b/q.sql\n@@ -1,3 +1,3 @@\n select 1;\n--- a comment\n+++ a new comment\n tail\n";
  const lines = parseDiff(d);
  assert.deepEqual(lines.map((l) => l.kind), ["file", "file", "hunk", "ctx", "del", "add", "ctx"]);
  assert.equal(lines[4].text, "-- a comment");
  assert.equal(lines[5].text, "++ a new comment");
  assert.deepEqual([lines[4].old, lines[5].now, lines[6].old, lines[6].now], [2, 2, 3, 3]);
});

test("df04 the no-newline marker is not a line and does not move the numbers", () => {
  const lines = parseDiff("--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n");
  assert.deepEqual(lines.map((l) => l.kind), ["file", "file", "hunk", "ctx", "del", "meta", "add", "meta"]);
  assert.deepEqual([lines[4].old, lines[6].now], [2, 2]);
  assert.equal(lines[5].old, undefined);
});

test("df05 several files in one diff each get their header and their own numbering", () => {
  const d = "--- a/one.py\n+++ b/one.py\n@@ -5,2 +5,2 @@\n x\n-y\n+z\n--- a/two.py\n+++ b/two.py\n@@ -1,1 +1,1 @@\n-p\n+q\n";
  const lines = parseDiff(d);
  assert.deepEqual(lines.filter((l) => l.kind === "file").map((l) => l.text), ["--- a/one.py", "+++ b/one.py", "--- a/two.py", "+++ b/two.py"]);
  const del = lines.filter((l) => l.kind === "del");
  assert.deepEqual(del.map((l) => l.old), [6, 1]);
});

test("df06 a hunk header without counts means one line, and a new file starts at line one", () => {
  const lines = parseDiff("--- /dev/null\n+++ b/new\n@@ -0,0 +1,2 @@\n+a\n+b\n");
  assert.deepEqual(lines.filter((l) => l.kind === "add").map((l) => l.now), [1, 2]);
  const one = parseDiff("@@ -7 +7 @@\n-x\n+y\n");
  assert.deepEqual([one[1].old, one[2].now], [7, 7]);
});

const parts = (text, ranges) => (ranges ?? []).map(([a, b]) => text.slice(a, b));

test("df07 one changed word in a replaced line is marked on both sides", () => {
  const lines = markChanges(parseDiff("@@ -1,1 +1,1 @@\n-    return a == b\n+    return a != b\n"));
  const del = lines.find((l) => l.kind === "del");
  const add = lines.find((l) => l.kind === "add");
  assert.deepEqual(parts(del.text, del.marks), ["=="]);
  assert.deepEqual(parts(add.text, add.marks), ["!="]);
});

test("df08 only lines that replace each other one for one are compared", () => {
  const unequal = markChanges(parseDiff("@@ -1,2 +1,1 @@\n-if x == 1:\n-    pass\n+if x == 2:\n"));
  assert.ok(unequal.every((l) => !l.marks), "two removed lines against one added line are not paired");
  const onlyAdd = markChanges(parseDiff("@@ -1,1 +1,2 @@\n ctx\n+new line\n"));
  assert.ok(onlyAdd.every((l) => !l.marks));
  // a run of two against two pairs first with first and second with second
  const two = markChanges(parseDiff("@@ -1,2 +1,2 @@\n-a = 1\n-b = 2\n+a = 10\n+b = 20\n"));
  const marks = two.filter((l) => l.marks).map((l) => parts(l.text, l.marks));
  assert.deepEqual(marks, [["1"], ["2"], ["10"], ["20"]]);
});

test("df09 a line that is mostly different is not marked: the whole line already says it changed", () => {
  const lines = markChanges(parseDiff("@@ -1,1 +1,1 @@\n-return compute(a)\n+raise ValueError(\"x\")\n"));
  assert.ok(lines.every((l) => !l.marks));
});

test("df10 marks fall on whole words, work in any script, and cover a change in only an indent", () => {
  const w = markChanges(parseDiff("@@ -1,1 +1,1 @@\n-total = price_sum + tax\n+total = price_total + tax\n"));
  assert.deepEqual(parts(w.find((l) => l.kind === "del").text, w.find((l) => l.kind === "del").marks), ["price_sum"], "a changed identifier is marked whole, not a fragment");
  const h = markChanges(parseDiff("@@ -1,1 +1,1 @@\n-नाम = \"राम\" और\n+नाम = \"श्याम\" और\n"));
  assert.deepEqual(parts(h.find((l) => l.kind === "add").text, h.find((l) => l.kind === "add").marks), ["श्याम"]);
  const i = markChanges(parseDiff("@@ -1,1 +1,1 @@\n-  x = 1\n+    x = 1\n"));
  assert.ok(i.find((l) => l.kind === "add").marks?.length === 1, "an indent change is shown");
});

test("df11 marking never changes the text or the numbers, and odd input stays safe", () => {
  const input = "--- a/x\n+++ b/x\n@@ -5,1 +5,1 @@\n-<b>old</b> & more\n+<b>new</b> & more\n\\ No newline at end of file\n";
  const before = parseDiff(input);
  const after = markChanges(before);
  assert.deepEqual(after.map(({ kind, text, old, now }) => ({ kind, text, old, now })), before.map(({ kind, text, old, now }) => ({ kind, text, old, now })));
  const m = after.find((l) => l.kind === "add");
  assert.deepEqual(parts(m.text, m.marks), ["new"]);
  for (const junk of ["", "@@", "-\n+\n", "@@ -1 +1 @@\n-\n+x\n", "@@ -1 +1 @@\n-x\n+\n"]) assert.doesNotThrow(() => markChanges(parseDiff(junk)));
  const long = "a ".repeat(50_000);
  assert.doesNotThrow(() => markChanges(parseDiff(`@@ -1 +1 @@\n-${long}x\n+${long}y\n`)), "a very long line is handled");
});

test("df12 the input is never changed, and a line over the limit is shown whole while one under it is compared", () => {
  const input = parseDiff("@@ -1,1 +1,1 @@\n-x = old\n+x = new\n");
  const out = markChanges(input);
  assert.ok(out.some((l) => l.marks) && input.every((l) => !l.marks), "marks go on a copy");
  const pad = (n) => "w ".repeat(n / 2);
  const under = markChanges(parseDiff(`@@ -1 +1 @@\n-${pad(4000)}one\n+${pad(4000)}two\n`));
  assert.ok(under.some((l) => l.marks), "a 4000-character line is compared");
  const over = markChanges(parseDiff(`@@ -1 +1 @@\n-${pad(5200)}one\n+${pad(5200)}two\n`));
  assert.ok(over.every((l) => !l.marks), "a line over 5000 characters is not compared word by word");
});
