import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { LANGUAGES, STRINGS, pickLanguage, translator } from "../src/strings.mjs";
import { describe } from "../src/approval.mjs";
import { deadlineLines } from "../src/expiry.mjs";
import { menuTemplate } from "../src/tray.mjs";

const src = join(dirname(fileURLToPath(import.meta.url)), "../src");
const placeholders = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

test("st01 every key exists in English and Hindi, nothing is empty, and both fill in the same placeholders", () => {
  assert.deepEqual(LANGUAGES, ["en", "hi"]);
  const en = Object.keys(STRINGS.en).sort();
  assert.deepEqual(Object.keys(STRINGS.hi).sort(), en);
  for (const k of en) {
    for (const lang of LANGUAGES) assert.ok(typeof STRINGS[lang][k] === "string" && STRINGS[lang][k].trim() !== "", `${lang} ${k}`);
    assert.deepEqual(placeholders(STRINGS.hi[k]), placeholders(STRINGS.en[k]), k);
  }
});

test("st02 the Hindi text is Hindi (not English left in place), apart from names and commands", () => {
  const sameOnPurpose = new Set();
  for (const k of Object.keys(STRINGS.en)) {
    if (sameOnPurpose.has(k)) continue;
    assert.notEqual(STRINGS.hi[k], STRINGS.en[k], k);
    assert.match(STRINGS.hi[k], /[ऀ-ॿ]/, `${k} has Devanagari`);
  }
});

test("st03 every key the shell asks for exists, so no dialog ever shows a raw key", () => {
  const used = new Set();
  for (const file of ["main.mjs", "approval.mjs", "expiry.mjs", "tray.mjs", "inbox.mjs"]) {
    for (const m of readFileSync(join(src, file), "utf8").matchAll(/\bt\("([a-zA-Z.]+)"/g)) used.add(m[1]);
  }
  assert.ok(used.size >= 30, `found ${used.size} uses`);
  for (const k of used) assert.ok(Object.hasOwn(STRINGS.en, k), k);
  for (const kind of ["approval_waiting", "ready_to_review", "failed", "team_question"]) assert.ok(Object.hasOwn(STRINGS.en, `notify.${kind}`), kind);
  assert.ok(!/title: "[A-Z]|message: "[A-Z]|showErrorBox\("/.test(readFileSync(join(src, "main.mjs"), "utf8")), "main.mjs has no English dialog text of its own left");
});

test("st04 the language follows the system: the first preferred language that is ours, else English", () => {
  for (const [list, want] of [
    [["hi-IN", "en-US"], "hi"],
    [["hi"], "hi"],
    [["en-GB", "hi-IN"], "en"],
    [["fr-FR", "hi-IN"], "hi"],
    [["de-DE", "fr"], "en"],
    [["HI_in"], "hi"],
    [[], "en"],
    [undefined, "en"],
    [[null, 3, "hi"], "hi"],
  ]) assert.equal(pickLanguage(list), want, JSON.stringify(list));
});

test("st05 placeholders are filled, a missing value stays visible, and an unknown language is English", () => {
  const hi = translator("hi");
  assert.equal(hi("tray.waiting.many", { n: 4 }), "4 चीज़ें आपकी प्रतीक्षा में हैं");
  assert.equal(translator("en")("tray.waiting.many"), "{n} things are waiting for you");
  assert.equal(translator("xx")("tray.open"), "Open Kakurega");
  assert.equal(hi("no.such.key"), "no.such.key");
  assert.equal(translator("en")("approve.reason", { digest: "{recipient}", recipient: "r" }), "Approve release {recipient} to r", "a value is never filled in twice");
});

test("st06 the approval window and its deadline read in Hindi when the shell is in Hindi, and stay word for word in English otherwise", () => {
  const a = { recipient: "acme", digest: "abc", files: [{ name: "f.txt", bytes: 3, sha256: "a".repeat(64) }], removed: [], not_established: [], expires: 1000 + 7500, server_time: 1000 };
  const en = describe(a);
  assert.ok(en.startsWith("Recipient: acme\nDigest: abc\nThis release can be approved for another 2 h 5 min.\n"));
  assert.ok(en.includes("  f.txt  3 bytes  sha256 aaaaaaaaaaaaaaaa") && en.includes("Withheld and never released:\n  (none)"));
  const hi = describe(a, translator("hi"));
  assert.ok(hi.includes("प्राप्तकर्ता: acme") && hi.includes("यह रिलीज़ अभी 2 h 5 min और स्वीकृत की जा सकती है।") && hi.includes("f.txt  3 बाइट"), hi);
  assert.deepEqual(deadlineLines({ expires: 5, server_time: 5 }, translator("hi")), [STRINGS.hi["deadline.expired"]]);
  const web = readFileSync(join(src, "../../web/src/locales/hi.ts"), "utf8");
  assert.ok(web.includes(`"appr.validFor": "${STRINGS.hi["deadline.validFor"]}"`), "the countdown is worded as the web app words it in Hindi");
});

test("st07 the menu-bar item's menu is in the shell's language", () => {
  const items = menuTemplate({ count: 2, notify: true }, translator("hi"), { open() {}, inbox() {}, setNotify() {}, quit() {} });
  const labels = items.filter((i) => i.label).map((i) => i.label);
  assert.deepEqual(labels, [STRINGS.hi["tray.running"], "2 चीज़ें आपकी प्रतीक्षा में हैं", STRINGS.hi["tray.open"], "आपकी प्रतीक्षा में (2)", STRINGS.hi["tray.notify"], STRINGS.hi["tray.quit"]]);
});
