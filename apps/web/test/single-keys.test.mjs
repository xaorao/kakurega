import assert from "node:assert/strict";
import test from "node:test";

function install(store) {
  globalThis.localStorage = store;
}

test("sk01 single-key shortcuts are on until turned off, the choice is kept, and a store that fails changes nothing", async () => {
  const m = new Map();
  install({ getItem: (k) => m.get(k) ?? null, setItem: (k, v) => m.set(k, String(v)), removeItem: (k) => m.delete(k) });
  const mod = await import("../src/single-keys.ts?a");
  assert.equal(mod.singleKeys(), true, "on by default");
  mod.chooseSingleKeys(false);
  assert.equal(mod.singleKeys(), false);
  assert.equal(m.get("kk-single-keys"), "off");
  mod.chooseSingleKeys(true);
  assert.equal(mod.singleKeys(), true);
  assert.equal(m.has("kk-single-keys"), false, "turning it back on leaves no stored value");
  // a later visit reads what was kept
  m.set("kk-single-keys", "off");
  const again = await import("../src/single-keys.ts?b");
  assert.equal(again.singleKeys(), false);
  // a stored value that is anything else counts as on
  m.set("kk-single-keys", "garbage");
  const odd = await import("../src/single-keys.ts?c");
  assert.equal(odd.singleKeys(), true);
  // storage that throws: on, and choosing still works for this visit
  install({ getItem() { throw new Error("blocked"); }, setItem() { throw new Error("blocked"); }, removeItem() { throw new Error("blocked"); } });
  const broken = await import("../src/single-keys.ts?d");
  assert.equal(broken.singleKeys(), true);
  broken.chooseSingleKeys(false);
  assert.equal(broken.singleKeys(), false);
});
