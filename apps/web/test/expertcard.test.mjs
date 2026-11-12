import { test } from "node:test";
import assert from "node:assert/strict";
import { initials, hueOf } from "../src/monogram.ts";

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
