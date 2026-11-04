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
