import assert from "node:assert/strict";
import test from "node:test";
import { markTerms } from "../src/terms.ts";

const joined = (p) => p.map((x) => x.text).join("");
const hits = (p) => p.filter((x) => x.hit).map((x) => x.text);

test("tm01 matched words are marked in the note's own spelling and the pieces join back to the exact text", () => {
  const text = "Acme pays invoices on the 20th. ACME also pays late.";
  const p = markTerms(text, ["acme", "pays"]);
  assert.equal(joined(p), text);
  assert.deepEqual(hits(p), ["Acme", "pays", "ACME", "pays"]);
});

test("tm02 terms are literal text, never a pattern", () => {
  const text = "version 1.2 (final) costs $5 + tax [draft] a|b";
  for (const term of ["1.2", "(final)", "$5", "+ tax", "[draft]", "a|b", ".*", "\\"]) {
    const p = markTerms(text, [term]);
    assert.equal(joined(p), text, term);
    const expected = text.toLowerCase().split(term.toLowerCase()).length - 1;
    assert.equal(hits(p).length, expected, `${term}: marks only where the text itself occurs`);
  }
  assert.deepEqual(hits(markTerms("axc a.c", ["a.c"])), ["a.c"], "a dot is a dot, not any character");
});

test("tm03 nothing becomes markup: the pieces are plain strings whatever the note or the search says", () => {
  const text = '<img src=x onerror="alert(1)"> and <b>bold</b>';
  const p = markTerms(text, ["<b>", "onerror"]);
  assert.equal(joined(p), text);
  assert.ok(p.every((x) => typeof x.text === "string" && typeof x.hit === "boolean"));
  assert.deepEqual(hits(p), ["onerror", "<b>"]);
});

test("tm04 no terms, empty terms, an empty note and over-long terms mark nothing", () => {
  assert.deepEqual(markTerms("hello", []), [{ text: "hello", hit: false }]);
  assert.deepEqual(markTerms("hello", ["", "   "]), [{ text: "hello", hit: false }]);
  assert.deepEqual(markTerms("", ["a"]), [{ text: "", hit: false }]);
  assert.deepEqual(markTerms("x".repeat(200), ["x".repeat(81)]), [{ text: "x".repeat(200), hit: false }]);
});

test("tm05 the longest term wins where two overlap, and touching marks join into one", () => {
  assert.deepEqual(hits(markTerms("payments-service", ["pay", "payments"])), ["payments"]);
  assert.deepEqual(markTerms("abcd", ["ab", "cd"]), [{ text: "abcd", hit: true }]);
  assert.deepEqual(markTerms("ab cd", ["ab", "cd"]).map((x) => x.hit), [true, false, true]);
});

test("tm06 only the first twelve distinct terms are used", () => {
  const terms = Array.from({ length: 20 }, (_, i) => `term${String.fromCharCode(97 + i)}`);
  const text = terms.join(" ");
  const marked = hits(markTerms(text, terms));
  assert.equal(marked.length, 12);
  assert.deepEqual(hits(markTerms("same same", ["same", "same", "SAME"])), ["same", "same"]);
});

test("tm07 other scripts and accented letters match without case trouble", () => {
  const text = "Ünïcode ünïcode 東京の天気 Straße";
  assert.deepEqual(hits(markTerms(text, ["ünïcode"])), ["Ünïcode", "ünïcode"]);
  assert.deepEqual(hits(markTerms(text, ["東京"])), ["東京"]);
  assert.equal(joined(markTerms(text, ["straße", "東京"])), text);
});

test("tm08 a long note with many matches stays quick", () => {
  const text = "pay ".repeat(50_000);
  const t0 = Date.now();
  const p = markTerms(text, ["pay"]);
  assert.equal(joined(p), text);
  assert.ok(Date.now() - t0 < 2000);
});

test("tm09 spaces around a term are not part of it", () => {
  assert.deepEqual(hits(markTerms("we pay now", [" pay "])), ["pay"]);
});
