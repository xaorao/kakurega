import test from "node:test";
import assert from "node:assert/strict";
import { SETTINGS, SECTIONS, searchSettings } from "../src/settings-catalog.ts";
import { en } from "../src/locales/en.ts";
import { hi } from "../src/locales/hi.ts";

const say = (k) => en[k] ?? k;
const ids = (list) => list.map((s) => s.id);

test("st01 every setting is named and explained in both languages, has a section and a unique id", () => {
  assert.equal(new Set(SETTINGS.map((s) => s.id)).size, SETTINGS.length);
  for (const s of SETTINGS) {
    for (const key of [s.label, s.help]) {
      assert.ok(en[key], `${key} in English`);
      assert.ok(hi[key], `${key} in Hindi`);
    }
    assert.ok(SECTIONS.includes(s.section), s.id);
    assert.ok(s.keywords.length >= 1, `${s.id} has words people use for it`);
    if (s.kind === "link") assert.match(s.target ?? "", /^#\/[a-z]+$/, s.id);
  }
  for (const sec of SECTIONS) assert.ok(en[`set.section.${sec}`] && hi[`set.section.${sec}`], sec);
});

test("st02 searching finds a setting by its name, its help or a word people use for it", () => {
  assert.ok(ids(searchSettings(SETTINGS, "notify", say)).includes("notifications"), "notify finds notifications");
  assert.deepEqual(ids(searchSettings(SETTINGS, "dark", say)), ["theme"]);
  assert.ok(ids(searchSettings(SETTINGS, "telemetry", say)).includes("telemetry"));
  assert.ok(ids(searchSettings(SETTINGS, "analytics", say)).includes("telemetry"), "a word that is only a keyword");
  assert.ok(ids(searchSettings(SETTINGS, "forget", say)).includes("memory"));
  assert.ok(ids(searchSettings(SETTINGS, "hindi", say)).includes("language"));
  assert.deepEqual(ids(searchSettings(SETTINGS, "THEME", say)), ["theme"].concat(ids(searchSettings(SETTINGS, "THEME", say)).slice(1)), "case does not matter");
});

test("st03 every word must be there, a name beats a mention, nothing typed lists everything, nonsense lists nothing", () => {
  assert.deepEqual(ids(searchSettings(SETTINGS, "", say)), ids(SETTINGS));
  assert.deepEqual(ids(searchSettings(SETTINGS, "   ", say)), ids(SETTINGS));
  assert.deepEqual(searchSettings(SETTINGS, "zxqvj", say), []);
  assert.deepEqual(searchSettings(SETTINGS, "theme zxqvj", say), []);
  const found = ids(searchSettings(SETTINGS, "usage", say));
  assert.equal(found[0], "usage", "the setting named Model usage comes first: " + found.join(","));
  assert.ok(found.length >= 1);
});

test("st04 each privacy setting says in words what it changes or means, and a link goes to a real page", () => {
  const privacy = SETTINGS.filter((s) => s.section === "privacy");
  assert.ok(privacy.length >= 4);
  for (const s of privacy) assert.ok(en[s.help].length > 30, s.id);
  assert.ok(privacy.some((s) => s.kind === "info" && s.id === "telemetry"));
});

test("st05 a question made only of symbols is not a pattern and finds nothing", () => {
  for (const q of ["%", "_", ".*", "(", "[a-z]", "\\"]) assert.deepEqual(searchSettings(SETTINGS, q, say), [], q);
});
