import test from "node:test";
import assert from "node:assert/strict";
import { STEPS, markWelcomeDone, nextStep, setupSummary, welcomeDone } from "../src/welcome.ts";
import { en } from "../src/locales/en.ts";
import { hi } from "../src/locales/hi.ts";

const mem = () => {
  const m = new Map();
  return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, v) };
};

test("wl01 the welcome is shown until it is finished or skipped, and storage that fails means it shows", () => {
  const s = mem();
  assert.equal(welcomeDone(s), false);
  markWelcomeDone(s);
  assert.equal(welcomeDone(s), true);
  assert.equal(welcomeDone(undefined), false);
  const broken = { getItem() { throw new Error("blocked"); }, setItem() { throw new Error("blocked"); } };
  assert.equal(welcomeDone(broken), false);
  assert.doesNotThrow(() => markWelcomeDone(broken));
  assert.doesNotThrow(() => markWelcomeDone(undefined));
  const other = mem();
  other.setItem("kk-welcome", "yes");
  assert.equal(welcomeDone(other), false, "only the exact value counts");
});

test("wl02 the steps run in order and end after the last", () => {
  assert.deepEqual([...STEPS], ["language", "theme", "setup"]);
  assert.equal(nextStep("language"), "theme");
  assert.equal(nextStep("theme"), "setup");
  assert.equal(nextStep("setup"), null);
  assert.equal(nextStep("nope"), null);
});

test("wl03 the setup summary counts required checks that fail and says nothing when it cannot be read", () => {
  assert.deepEqual(setupSummary([{ ok: true }, { ok: true, required: true }]), { problems: 0 });
  assert.deepEqual(setupSummary([{ ok: false }, { ok: false, required: true }, { ok: false, required: false }, { ok: true }]), { problems: 2 });
  assert.deepEqual(setupSummary([]), { problems: 0 });
  assert.equal(setupSummary(null), null);
  assert.equal(setupSummary(undefined), null);
});

test("wl04 every welcome text exists in both languages", () => {
  for (const k of Object.keys(en).filter((k) => k.startsWith("welcome."))) assert.ok(hi[k], k);
  for (const s of STEPS) assert.ok(en[`welcome.${s}.title`] && hi[`welcome.${s}.title`]);
});
