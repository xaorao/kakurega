import test from "node:test";
import assert from "node:assert/strict";
import { fileNotes, room, sizeText, MAX_FILES, MAX_BYTES } from "../src/attach.ts";
import { en } from "../src/locales/en.ts";
import { hi } from "../src/locales/hi.ts";

const full = { used: 2, trimmed: 1, left_out: 1, withheld_instructions: 3, withheld_secrets: 4 };

test("at01 nothing is said when no file went with the question", () => {
  assert.deepEqual(fileNotes(undefined), []);
  assert.deepEqual(fileNotes({ used: 0, trimmed: 0, left_out: 0, withheld_instructions: 0, withheld_secrets: 0 }), []);
});

test("at02 every kind of thing that happened to the files is said, with its number, in a fixed order", () => {
  assert.deepEqual(fileNotes(full), [
    { key: "files.used", n: 2 },
    { key: "files.trimmed", n: 1 },
    { key: "files.leftOut", n: 1 },
    { key: "files.withheldInstructions", n: 3 },
    { key: "files.withheldSecrets", n: 4 },
  ]);
  assert.deepEqual(fileNotes({ used: 1, trimmed: 0, left_out: 0, withheld_instructions: 0, withheld_secrets: 2 }), [
    { key: "files.used", n: 1 },
    { key: "files.withheldSecrets", n: 2 },
  ]);
  // files that were all left out still say so
  assert.deepEqual(fileNotes({ used: 0, trimmed: 0, left_out: 2, withheld_instructions: 0, withheld_secrets: 0 }), [{ key: "files.leftOut", n: 2 }]);
});

test("at03 every key used exists in both languages", () => {
  for (const n of fileNotes(full)) {
    assert.ok(en[n.key], n.key);
    assert.ok(hi[n.key], n.key);
    assert.ok(en[n.key].includes("{n}") && hi[n.key].includes("{n}"), n.key);
  }
});

test("at04 no more than five files go with one message, and the rest are counted", () => {
  const f = (n) => Array.from({ length: n }, (_, i) => ({ name: `f${i}` }));
  assert.equal(MAX_FILES, 5);
  assert.deepEqual(room(0, f(3)), { take: f(3), over: 0 });
  assert.deepEqual(room(4, f(3)).take, f(1));
  assert.equal(room(4, f(3)).over, 2);
  assert.deepEqual(room(5, f(2)), { take: [], over: 2 });
  assert.deepEqual(room(9, f(1)), { take: [], over: 1 });
});

test("at05 sizes read the way people read them", () => {
  assert.equal(sizeText(0), "0 B");
  assert.equal(sizeText(1023), "1023 B");
  assert.equal(sizeText(1024), "1 KB");
  assert.equal(sizeText(1536), "1.5 KB");
  assert.equal(sizeText(1024 * 1024), "1 MB");
  assert.equal(sizeText(8 * 1024 * 1024), "8 MB");
});

test("at06 the size limit is the service's, and a file at the limit is allowed while one byte over is not", () => {
  assert.equal(MAX_BYTES, 8 * 1024 * 1024);
  assert.ok(MAX_BYTES <= MAX_BYTES);
  assert.ok(MAX_BYTES + 1 > MAX_BYTES);
});
