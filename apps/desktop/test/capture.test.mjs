import test from "node:test";
import assert from "node:assert/strict";
import { chatTarget, createCapture, MAX_PICTURE_BYTES, pickSource, pictureName } from "../src/capture.mjs";
import { menuTemplate } from "../src/tray.mjs";
import { translator } from "../src/strings.mjs";

const png = Buffer.from([0x89, 0x50, 0x4e, 0x47, 1, 2, 3]);
const parts = (over = {}) => {
  const seen = { uploads: [], opened: [], asked: 0 };
  const capture = createCapture({
    grab: async () => png,
    confirm: async () => {
      seen.asked++;
      return true;
    },
    upload: async (name, bytes) => {
      seen.uploads.push([name, bytes]);
      return { status: 200, json: { id: "att-0123456789ab" } };
    },
    open: (hash) => seen.opened.push(hash),
    now: () => new Date(2026, 0, 2, 3, 4, 5),
    ...over,
  });
  return { capture, seen };
};

test("cp01 a picture the owner accepts is handed over once, under a dated name, and the chat is opened on it", async () => {
  const { capture, seen } = parts();
  assert.deepEqual(await capture(), { outcome: "attached", id: "att-0123456789ab" });
  assert.deepEqual(seen.uploads, [["screen-20260102-030405.png", png]]);
  assert.deepEqual(seen.opened, ["#/chat/attach/att-0123456789ab"]);
  assert.equal(seen.asked, 1);
});

test("cp02 a picture the owner discards goes nowhere", async () => {
  const { capture, seen } = parts({ confirm: async () => false });
  assert.deepEqual(await capture(), { outcome: "discarded" });
  assert.deepEqual([seen.uploads, seen.opened], [[], []]);
});

test("cp03 no picture, an empty one and one over the limit are never shown to the service", async () => {
  for (const [grab, outcome] of [[async () => null, "no_picture"], [async () => Buffer.alloc(0), "no_picture"], [async () => Buffer.alloc(MAX_PICTURE_BYTES + 1), "too_large"]]) {
    const { capture, seen } = parts({ grab });
    assert.deepEqual(await capture(), { outcome });
    assert.deepEqual([seen.uploads, seen.opened, seen.asked], [[], [], 0]);
  }
});

test("cp04 a service that refuses, or answers with something that is not an attachment id, opens nothing", async () => {
  for (const answer of [{ status: 413, json: null }, { status: 200, json: { id: "../../settings" } }, { status: 200, json: {} }, null]) {
    const { capture, seen } = parts({ upload: async () => answer });
    assert.equal((await capture()).outcome, "refused");
    assert.deepEqual(seen.opened, []);
  }
});

test("cp05 a second choice while the first picture waits for its answer does nothing", async () => {
  let release;
  const { capture, seen } = parts({ confirm: () => new Promise((r) => (release = r)) });
  const first = capture();
  await new Promise((r) => setImmediate(r));
  assert.deepEqual(await capture(), { outcome: "busy" });
  release(true);
  assert.equal((await first).outcome, "attached");
  assert.equal(seen.uploads.length, 1);
  const next = capture();
  await new Promise((r) => setImmediate(r));
  release(true);
  assert.equal((await next).outcome, "attached", "and the next one works again");
});

test("cp06 the address is made only from an attachment id; the display's own source is chosen", () => {
  assert.equal(chatTarget("att-0123456789ab"), "#/chat/attach/att-0123456789ab");
  for (const bad of ["att-0123456789AB", "att-0123456789abc", "x", "", null, 7, "att-0123456789ab/../x"]) assert.equal(chatTarget(bad), null);
  assert.equal(pictureName(new Date(2026, 10, 9, 23, 59, 1)), "screen-20261109-235901.png");
  const sources = [{ display_id: "1", n: "a" }, { display_id: "2", n: "b" }];
  assert.equal(pickSource(sources, 2).n, "b");
  assert.equal(pickSource(sources, 9).n, "a");
  assert.equal(pickSource([], 1), null);
});

test("cp07 the menu offers the picture only while the service runs, in both languages", () => {
  for (const lang of ["en", "hi"]) {
    const t = translator(lang);
    let taken = 0;
    const actions = { open() {}, inbox() {}, setNotify() {}, quit() {}, capture: () => taken++ };
    const item = menuTemplate({ count: 0, stopped: false, notify: true }, t, actions).find((i) => i.id === "capture");
    assert.ok(item && item.label.length > 5 && !item.label.includes("tray.capture"), lang);
    item.click();
    assert.equal(taken, 1);
    assert.equal(menuTemplate({ count: 0, stopped: true, notify: true }, t, actions).find((i) => i.id === "capture"), undefined);
  }
});
