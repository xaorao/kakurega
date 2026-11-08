import assert from "node:assert/strict";
import test from "node:test";
import { visible } from "../src/visible.ts";
import { parse } from "../src/markdown.ts";
import { markTerms } from "../src/terms.ts";

const HIDDEN = ["​", "‌", "‍", "‎", "‏", "‪", "‫", "‬", "‭", "‮", "⁦", "⁧", "⁨", "⁩", " ", " ", "﻿"];
const has = (s) => HIDDEN.some((c) => s.includes(c));

test("vi01 every direction, zero-width and separator character becomes the replacement mark, and ordinary text is untouched", () => {
  for (const c of HIDDEN) assert.equal(visible(`a${c}b`), "a�b", `U+${c.codePointAt(0).toString(16)}`);
  const plain = "line one\nline two\ttabbed é 日本語 😀 العربية עברית";
  assert.equal(visible(plain), plain, "real right-to-left words are left alone; only the control characters that reorder text go");
  assert.equal(visible(""), "");
});

test("vi02 an answer never keeps one of them anywhere in the tree the screen is built from", () => {
  const text = "safe‮gnirts‬ zero​width [li‮nk](https://example.com/) `co⁦de⁩`\n\n```\nfen‮ce\n```\n\n| a‮ | b |\n|---|---|\n| x​ | y |\n\n- item two\n> quote﻿";
  const tree = JSON.stringify(parse(text));
  for (const c of HIDDEN) assert.ok(!tree.includes(c), `U+${c.codePointAt(0).toString(16)} is in the tree`);
  assert.ok(tree.includes("�") || tree.includes("\\ufffd") || tree.includes("�"), "the replacement mark shows something was there");
});

test("vi03 a note is shown without them too, and the matched words are still found around the replaced marks", () => {
  const pieces = markTerms("pay‮ the invoice​ now", ["invoice"]);
  assert.ok(!has(pieces.map((p) => p.text).join("")));
  assert.deepEqual(pieces.filter((p) => p.hit).map((p) => p.text), ["invoice"]);
});
