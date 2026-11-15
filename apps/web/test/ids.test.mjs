import test from "node:test";
import assert from "node:assert/strict";
import { shortId, HEAD, TAIL } from "../src/ids.ts";

const digest = "3f9a2c1d5be04477a1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1a07b42";

test("id01 a long digest is shown by its first 8 and last 6 characters, with one mark between", () => {
  assert.equal(HEAD, 8);
  assert.equal(TAIL, 6);
  assert.equal(shortId(digest), "3f9a2c1d…a07b42");
  assert.ok(digest.startsWith(shortId(digest).split("…")[0]) && digest.endsWith(shortId(digest).split("…")[1]), "both ends are the value's own");
});

test("id02 a value no longer than its two ends and the mark is shown whole; nothing is ever lengthened", () => {
  for (const v of ["", "appr-7", "job-12", "abcdefghijklmno", "0123456789abcde"]) assert.equal(shortId(v), v, v);
  assert.equal(shortId("0123456789abcdef"), "01234567…abcdef", "one character more than fits is shortened");
  for (const v of [digest, "x".repeat(200)]) assert.ok(shortId(v).length < v.length);
});

test("id04 a key that names its kind keeps the kind and shows the ends of the key itself", () => {
  assert.equal(shortId(`ed25519:${digest}`), "ed25519:3f9a2c1d…a07b42");
  assert.equal(shortId("ed25519:abc"), "ed25519:abc");
  // only a short lower-case kind counts: a colon later in a value, or a long word before one, is part of the value
  assert.equal(shortId(`Digest:${digest}`), "Digest:3…a07b42");
  assert.equal(shortId(`averyveryverylongkind:${digest}`), "averyver…a07b42");
});

test("id03 characters outside the basic plane are counted as characters, never cut in half", () => {
  const v = "🔑".repeat(10) + "abcdefgh" + "🔐".repeat(10);
  const s = shortId(v);
  assert.equal(s, "🔑".repeat(8) + "…" + "🔐".repeat(6));
  assert.equal([...s].includes("�"), false);
});
