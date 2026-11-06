import test from "node:test";
import assert from "node:assert/strict";
import { editorLocation } from "../src/cite.ts";

test("ct01 the location is folder, file and line in the form editors open (path:line)", () => {
  assert.equal(editorLocation("/work/acme", "src/pay.py", 42), "/work/acme/src/pay.py:42");
  assert.equal(editorLocation("/work/acme/", "pay.py", 7), "/work/acme/pay.py:7", "no doubled slash");
  assert.equal(editorLocation("/work/my docs", "a b.md", 3), "/work/my docs/a b.md:3", "spaces stay as they are");
});

test("ct02 a Windows folder keeps its own separator", () => {
  assert.equal(editorLocation("C:\\work\\acme", "src/pay.py", 5), "C:\\work\\acme\\src\\pay.py:5");
  assert.equal(editorLocation("C:\\work\\acme\\", "pay.py", 1), "C:\\work\\acme\\pay.py:1");
});

test("ct03 a line that is not a whole number of at least one, or a file that climbs out of the folder, gives no location", () => {
  for (const line of [0, -1, 1.5, NaN, Infinity, undefined, null]) assert.equal(editorLocation("/w", "a.py", line), null, String(line));
  for (const file of ["../secret", "a/../../b", "/etc/passwd", "C:\\x", "", "a\\..\\b", "a//b.py", "a/", "a\\\\b.py"]) assert.equal(editorLocation("/w", file, 3), null, file);
  assert.equal(editorLocation("", "a.py", 3), null, "no folder, no location");
});
