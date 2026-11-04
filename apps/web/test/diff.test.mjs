import test from "node:test";
import assert from "node:assert/strict";
import { parseDiff } from "../src/diff.ts";

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
