import test from "node:test";
import assert from "node:assert/strict";
import { inflateSync } from "node:zlib";
import { menuTemplate, statusLine, trayIconPng, trayTitle, trayToolTip } from "../src/tray.mjs";
import { translator } from "../src/strings.mjs";

const t = translator("en");

function decode(png) {
  assert.deepEqual([...png.subarray(0, 8)], [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  let o = 8;
  const chunks = {};
  while (o < png.length) {
    const len = png.readUInt32BE(o);
    const type = png.toString("ascii", o + 4, o + 8);
    chunks[type] = Buffer.concat([chunks[type] ?? Buffer.alloc(0), png.subarray(o + 8, o + 8 + len)]);
    o += 12 + len;
  }
  const w = chunks.IHDR.readUInt32BE(0);
  const raw = inflateSync(chunks.IDAT);
  const px = (x, y) => [...raw.subarray(y * (w * 4 + 1) + 1 + x * 4, y * (w * 4 + 1) + 1 + x * 4 + 4)];
  return { w, h: chunks.IHDR.readUInt32BE(4), depth: chunks.IHDR[8], colour: chunks.IHDR[9], px, end: "IEND" in chunks };
}

test("tr01 the icon is a valid PNG at 16 and 32 pixels, black with transparency only, as macOS template images must be", () => {
  for (const size of [16, 32]) {
    const img = decode(trayIconPng(size));
    assert.deepEqual([img.w, img.h, img.depth, img.colour, img.end], [size, size, 8, 6, true]);
    let opaque = 0;
    for (let y = 0; y < size; y++)
      for (let x = 0; x < size; x++) {
        const [r, g, b, a] = img.px(x, y);
        assert.deepEqual([r, g, b], [0, 0, 0]);
        if (a > 200) opaque++;
      }
    assert.ok(opaque > size * size * 0.2 && opaque < size * size * 0.7, `a visible mark, not a block: ${opaque}`);
    assert.equal(img.px(0, 0)[3], 0, "the corners are clear");
    assert.ok(img.px(size / 2, size / 2)[3] > 200, "the bar crosses the middle");
  }
});

test("tr02 the title next to the icon is the count, or nothing", () => {
  assert.deepEqual([0, 1, 12, -1, 1.5, null, undefined, "3"].map(trayTitle), ["", "1", "12", "", "", "", "", ""]);
});

test("tr03 the status says what waits, and that the app stopped when it did", () => {
  assert.equal(statusLine({ count: 0 }, t), "Nothing is waiting for you");
  assert.equal(statusLine({ count: 1 }, t), "1 thing is waiting for you");
  assert.equal(statusLine({ count: 3 }, t), "3 things are waiting for you");
  assert.equal(statusLine({ count: 3, stopped: true }, t), "Kakurega has stopped. Quit and open it again.");
  assert.equal(trayToolTip({ count: 2 }, t), "Kakurega is running · 2 things are waiting for you");
});

test("tr04 the menu: running, what waits, open, waiting for you, notifications, quit; the status lines cannot be clicked", () => {
  const calls = [];
  const actions = { open: () => calls.push("open"), inbox: () => calls.push("inbox"), setNotify: (on) => calls.push(`notify:${on}`), quit: () => calls.push("quit") };
  const items = menuTemplate({ count: 2, notify: true }, t, actions);
  assert.deepEqual(items.filter((i) => i.id).map((i) => i.id), ["running", "status", "open", "inbox", "notify", "quit"]);
  assert.equal(items.find((i) => i.id === "inbox").label, "Waiting for you (2)");
  assert.equal(items.find((i) => i.id === "running").enabled, false);
  assert.equal(items.find((i) => i.id === "status").enabled, false);
  const notify = items.find((i) => i.id === "notify");
  assert.deepEqual([notify.type, notify.checked], ["checkbox", true]);
  for (const id of ["open", "inbox", "quit"]) items.find((i) => i.id === id).click();
  notify.click({ checked: false });
  assert.deepEqual(calls, ["open", "inbox", "quit", "notify:false"]);
  const none = menuTemplate({ count: 0, notify: false }, t, actions);
  assert.equal(none.find((i) => i.id === "inbox").label, "Waiting for you");
  assert.equal(none.find((i) => i.id === "notify").checked, false);
  const stopped = menuTemplate({ count: 2, stopped: true, notify: true }, t, actions);
  assert.equal(stopped.find((i) => i.id === "status"), undefined);
  assert.ok(stopped.some((i) => i.id === "quit"), "Quit is always there");
});

test("tr05 the capture entry shows its shortcut when the app holds it, and the menu does not take the keys itself", () => {
  const t = (k) => k;
  const actions = { open() {}, inbox() {}, capture() {}, setNotify() {}, quit() {} };
  const held = menuTemplate({ count: 0, stopped: false, notify: true, shortcut: "CommandOrControl+Alt+K" }, t, actions).find((i) => i.id === "capture");
  assert.equal(held.accelerator, "CommandOrControl+Alt+K");
  assert.equal(held.registerAccelerator, false);
  const free = menuTemplate({ count: 0, stopped: false, notify: true, shortcut: null }, t, actions).find((i) => i.id === "capture");
  assert.equal(free.accelerator, undefined, "keys another app holds are not shown as if they worked");
});
