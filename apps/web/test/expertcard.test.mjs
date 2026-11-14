import { test } from "node:test";
import assert from "node:assert/strict";
import { initials, hueOf, marks } from "../src/monogram.ts";

test("an expert's monogram is two letters from the name, and the hue is the same every time", () => {
  assert.equal(initials("Security reviewer"), "SR");
  assert.equal(initials("Lawyer"), "LA");
  assert.equal(initials("the-boss"), "TB");
  assert.equal(initials("सुरक्षा समीक्षक"), "सस");
  assert.equal(initials("  "), "");
  assert.equal(hueOf("security-reviewer"), hueOf("security-reviewer"));
  const hues = new Set(["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"].map(hueOf));
  assert.ok([...hues].every((h) => h >= 1 && h <= 6) && hues.size >= 3);
});

test("in one list no two experts share a monogram, and the first keeps the plain one", () => {
  assert.deepEqual(marks(["Data analyst", "Devil's advocate", "Security reviewer"]), ["DA", "DE", "SR"]);
  assert.deepEqual(marks(["Data analyst", "Data architect", "Data auditor"]), ["DA", "DR", "DU"]);
  const all = marks(["Security reviewer", "Software architect", "Sceptical tester", "Performance engineer", "Product manager", "UX designer", "Plain-language writer", "Data analyst", "Devil's advocate", "Finance and operations", "Contract and risk reader", "Privacy officer", "Project reviewer", "Code reader"]);
  assert.equal(new Set(all).size, all.length);
  assert.deepEqual(marks(["Lawyer", "Lawyer"]), ["LA", "LR"]);
  assert.deepEqual(marks([]), []);
});
