import test from "node:test";
import assert from "node:assert/strict";
import { baseName, filterFiles, groupByDir, languageOf, lineCount, MAX_SHOWN } from "../src/filetree.ts";

const f = (...paths) => paths.map((path) => ({ path }));

test("ft01 filtering keeps the files whose path holds every word typed, in any case", () => {
  const files = f("src/Payments/refund.py", "src/payments/charge.py", "tests/test_refund.py", "README.md");
  assert.deepEqual(filterFiles(files, "refund").map((x) => x.path), ["src/Payments/refund.py", "tests/test_refund.py"]);
  assert.deepEqual(filterFiles(files, "PAYMENTS refund").map((x) => x.path), ["src/Payments/refund.py"]);
  assert.equal(filterFiles(files, "  ").length, 4);
  assert.equal(filterFiles(files, "").length, 4);
  assert.equal(filterFiles(files, "nothing here").length, 0);
});

test("ft02 grouping puts the top level first, folders in order and files in name order inside each", () => {
  const g = groupByDir(f("b/z.py", "a/y.py", "a/x.py", "top.md", "a/b/c.py"));
  assert.deepEqual(g.map((x) => x.dir), ["", "a", "a/b", "b"]);
  assert.deepEqual(g[1].files.map((x) => x.path), ["a/x.py", "a/y.py"]);
  assert.deepEqual(groupByDir([]), []);
});

test("ft03 a name, an extension and a line count are read without surprises", () => {
  assert.equal(baseName("a/b/c.py"), "c.py");
  assert.equal(baseName("c.py"), "c.py");
  assert.equal(languageOf("src/Main.RS"), "rs");
  assert.equal(languageOf("Makefile"), null);
  assert.equal(languageOf(".env"), null);
  assert.equal(languageOf("file."), null);
  assert.equal(lineCount(""), 0);
  assert.equal(lineCount("a"), 1);
  assert.equal(lineCount("a\n"), 1);
  assert.equal(lineCount("a\nb"), 2);
  assert.equal(lineCount("a\n\nb\n"), 3);
});

test("ft04 a path that looks like an object key or a script cannot break the grouping", () => {
  const nasty = f("__proto__/x", "constructor/y", "a/__proto__", "<img src=x onerror=1>/z");
  const g = groupByDir(nasty);
  assert.equal(g.length, 4);
  assert.deepEqual(filterFiles(nasty, "proto").length, 2);
  assert.ok(MAX_SHOWN >= 100);
});
