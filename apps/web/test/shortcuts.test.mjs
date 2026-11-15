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
  for (const e of [ev("k"), ev("k", { ctrlKey: true, shiftKey: true }), ev("k", { ctrlKey: true, altKey: true }), ev("p", { ctrlKey: true }), ev("P", { shiftKey: true }), ev("P", { ctrlKey: true, shiftKey: true, altKey: true }), ev("/", { shiftKey: true }), ev("?", { ctrlKey: true }), ev("a", { metaKey: true })]) {
    assert.equal(appShortcut(e, body), null, JSON.stringify(e));
  }
  assert.equal(appShortcut(ev("K", { metaKey: true }), body)?.id, "palette", "the Command key and capital letters count");
  const send = SHORTCUTS.find((s) => s.id === "send");
  assert.equal(send.matches(ev("Enter", { isComposing: true })), false, "Enter that confirms an input method is not a send");
  assert.equal(send.matches(ev("Enter", { shiftKey: true })), false);
});

test("kb05 keys are written for the computer they are shown on", () => {
  const p = SHORTCUTS.find((s) => s.id === "palette");
  assert.equal(keysFor(p, true), "⌘K, ⇧⌘P");
  assert.equal(keysFor(p, false), "Ctrl+K, Ctrl+Shift+P");
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

test("kb10 a single printable key can be switched off and the same command with a modifier keeps working", () => {
  const body = { tagName: "BODY" };
  const single = SHORTCUTS.filter((s) => s.single);
  assert.deepEqual(single.map((s) => s.id), ["help"], "only the bare ? is a single-key shortcut");
  // every single-key shortcut is a printable key with no modifier; nothing with a modifier is marked single
  for (const s of SHORTCUTS) {
    const bare = s.samples.every((x) => !x.ctrlKey && !x.metaKey && !x.altKey && x.key.length === 1);
    if (s.single) assert.ok(bare, `${s.id} is marked single but its samples are not a bare printable key`);
    if (s.scope === "app" && bare && !s.single) assert.fail(`${s.id} is a bare printable key and is not switchable`);
  }
  assert.equal(appShortcut(ev("?", { shiftKey: true }), body)?.id, "help", "on by default");
  assert.equal(appShortcut(ev("?", { shiftKey: true }), body, true)?.id, "help");
  assert.equal(appShortcut(ev("?", { shiftKey: true }), body, false), null, "switched off: ? does nothing");
  assert.equal(appShortcut(ev("/", { ctrlKey: true }), body, false)?.id, "help-anywhere", "Ctrl+/ still opens the same list");
  assert.equal(appShortcut(ev("k", { ctrlKey: true }), body, false)?.id, "palette");
  assert.equal(appShortcut(ev("P", { ctrlKey: true, shiftKey: true }), body, false)?.id, "palette");
});

test("kb11 a checkbox, radio or button is not a place where text is typed, but every text-like box still is", () => {
  for (const type of ["checkbox", "radio", "button", "submit", "reset", "range", "color", "file", "image", "CHECKBOX"]) {
    assert.equal(typingTarget({ tagName: "INPUT", type }), false, type);
  }
  for (const type of ["text", "search", "email", "password", "number", "url", "tel", "date", "", undefined]) {
    assert.equal(typingTarget({ tagName: "INPUT", type }), true, String(type));
  }
  assert.equal(typingTarget({ tagName: "TEXTAREA", type: "checkbox" }), true, "the type of an input does not change a text area");
  assert.equal(typingTarget({ tagName: "SELECT" }), true);
  assert.equal(typingTarget({ tagName: "DIV", isContentEditable: true }), true);
  const help = appShortcut(ev("?", { shiftKey: true }), { tagName: "INPUT", type: "checkbox" });
  assert.equal(help?.id, "help", "? works while a checkbox has the focus");
  assert.equal(appShortcut(ev("?", { shiftKey: true }), { tagName: "INPUT", type: "text" }), null, "and still not inside a text box");
});

test("kb12 one palette: Cmd or Ctrl with K opens it, Cmd or Ctrl with Shift and P is a second way in, and no other entry answers to either", () => {
  const body = { tagName: "BODY" };
  const box = { tagName: "TEXTAREA" };
  for (const e of [ev("k", { metaKey: true }), ev("k", { ctrlKey: true }), ev("K", { metaKey: true }), ev("P", { metaKey: true, shiftKey: true }), ev("p", { ctrlKey: true, shiftKey: true })]) {
    assert.equal(appShortcut(e, body)?.id, "palette", JSON.stringify(e));
    assert.equal(appShortcut(e, box)?.id, "palette", `${JSON.stringify(e)} also while typing`);
    assert.deepEqual(SHORTCUTS.filter((s) => s.matches(e)).map((s) => s.id), ["palette"], JSON.stringify(e));
  }
  assert.equal(SHORTCUTS.some((s) => s.id === "search"), false, "the side list's filter has no shortcut of its own: Cmd+K is the palette's");
  const p = SHORTCUTS.find((s) => s.id === "palette");
  for (const keys of ["⌘K", "⇧⌘P"]) assert.ok(keysFor(p, true).includes(keys), `the help list names ${keys}`);
  for (const keys of ["Ctrl+K", "Ctrl+Shift+P"]) assert.ok(keysFor(p, false).includes(keys), `the help list names ${keys}`);
});
