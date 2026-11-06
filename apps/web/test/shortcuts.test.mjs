import test from "node:test";
import assert from "node:assert/strict";
import { SHORTCUTS, appShortcut, typingTarget, keysFor } from "../src/shortcuts.ts";
import { stopsReply, historyKey, opensHistoryPicker } from "../src/composer.ts";

const ev = (key, mods = {}) => ({ key, ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...mods });

test("kb01 every shortcut has its own id, both spellings of its keys, a label and an event that matches it", () => {
  assert.equal(new Set(SHORTCUTS.map((s) => s.id)).size, SHORTCUTS.length);
  for (const s of SHORTCUTS) {
    assert.ok(s.mac && s.other && /^kb\.[a-zA-Z]+$/.test(s.label), s.id);
    assert.ok(s.samples.length >= 1);
    for (const sample of s.samples) assert.equal(s.matches(sample), true, `${s.id} matches its own sample`);
    assert.ok(["app", "chat"].includes(s.scope));
  }
});

test("kb02 a sample matches its own shortcut and no other that shares its scope", () => {
  for (const s of SHORTCUTS) {
    for (const sample of s.samples) {
      const others = SHORTCUTS.filter((o) => o.id !== s.id && o.scope === s.scope && o.matches(sample));
      assert.deepEqual(others.map((o) => o.id), [], `${s.id} is also ${others.map((o) => o.id).join(", ")}`);
    }
  }
});

test("kb03 page-wide shortcuts are found through the table, and a bare key typed in a text box is never a command", () => {
  const body = { tagName: "BODY" };
  const box = { tagName: "TEXTAREA" };
  for (const s of SHORTCUTS.filter((x) => x.scope === "app")) {
    for (const sample of s.samples) {
      assert.equal(appShortcut(sample, body)?.id, s.id, s.id);
      if (s.editable) assert.equal(appShortcut(sample, box)?.id, s.id, `${s.id} works inside a text box`);
      else assert.equal(appShortcut(sample, box), null, `${s.id} does not fire inside a text box`);
    }
  }
  for (const t of [{ tagName: "INPUT" }, { tagName: "select" }, { tagName: "DIV", isContentEditable: true }]) assert.equal(typingTarget(t), true);
  for (const t of [null, undefined, {}, { tagName: "BUTTON" }, { tagName: "DIV" }]) assert.equal(typingTarget(t), false);
  assert.equal(appShortcut(ev("?", { shiftKey: true }), { tagName: "INPUT" }), null, "a question mark typed in a field is a question mark");
  assert.equal(appShortcut(ev("?", { shiftKey: true }), { tagName: "BUTTON" })?.id, "help");
});

test("kb04 near misses are not shortcuts", () => {
  const body = { tagName: "BODY" };
  for (const e of [ev("k"), ev("k", { ctrlKey: true, shiftKey: true }), ev("k", { ctrlKey: true, altKey: true }), ev("p", { ctrlKey: true }), ev("P", { shiftKey: true }), ev("/", { shiftKey: true }), ev("?", { ctrlKey: true }), ev("a", { metaKey: true })]) {
    assert.equal(appShortcut(e, body), null, JSON.stringify(e));
  }
  assert.equal(appShortcut(ev("K", { metaKey: true }), body)?.id, "search", "the Command key and capital letters count");
  const send = SHORTCUTS.find((s) => s.id === "send");
  assert.equal(send.matches(ev("Enter", { isComposing: true })), false, "Enter that confirms an input method is not a send");
  assert.equal(send.matches(ev("Enter", { shiftKey: true })), false);
});

test("kb05 keys are written for the computer they are shown on", () => {
  const p = SHORTCUTS.find((s) => s.id === "palette");
  assert.equal(keysFor(p, true), "⌘⇧P");
  assert.equal(keysFor(p, false), "Ctrl+Shift+P");
});

const full = (e) => ({ isComposing: false, ...e });

test("kb06 the chat entries in the table agree with the code that really handles those keys", () => {
  const byId = (id) => SHORTCUTS.find((s) => s.id === id);
  for (const sample of byId("stop").samples) assert.equal(stopsReply(full(sample), true), true, JSON.stringify(sample));
  for (const sample of byId("history").samples) assert.notEqual(historyKey(full(sample)), null);
  for (const sample of byId("picker").samples) assert.equal(opensHistoryPicker(full(sample), "", 0, 3), true);
  // and what the table says is not a shortcut is not one for the code either
  assert.equal(stopsReply(full(ev("a")), true), false);
  assert.equal(historyKey(full(ev("ArrowUp", { shiftKey: true }))), null);
  assert.equal(byId("stop").matches(ev("g")), false, "G alone is a letter");
  assert.equal(byId("stop").matches(ev("Escape", { isComposing: true })), false);
});
