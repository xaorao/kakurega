import test from "node:test";
import assert from "node:assert/strict";
import { en } from "../src/locales/en.ts";
import { hi } from "../src/locales/hi.ts";

const slots = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
const tags = (s) => [...s.matchAll(/<\/?([bcea])>/g)].map((m) => m[0]).sort();

test("i18n01 every language has exactly the English keys", () => {
  assert.deepEqual(Object.keys(hi).sort(), Object.keys(en).sort());
});

test("i18n02 placeholders and markup tags match the English text in every key", () => {
  for (const k of Object.keys(en)) {
    assert.deepEqual(slots(hi[k]), slots(en[k]), `placeholders differ in ${k}`);
    assert.deepEqual(tags(hi[k]), tags(en[k]), `tags differ in ${k}`);
  }
});

test("i18n03 no translation is empty or left identical to English except names and codes", () => {
  const same = Object.keys(en).filter((k) => hi[k] === en[k]);
  assert.deepEqual(same.filter((k) => hi[k].trim() === ""), []);
  // names that are the same in every language: a search example, the names of the programming languages, and a check
  // mark before a folder's own name
  const allowed = new Set(["nav.search.placeholder", "find.lang.python", "find.lang.javascript", "find.lang.typescript", "find.lang.rust", "folder.chosen"]);
  assert.deepEqual(same.filter((k) => !allowed.has(k)), []);
});
