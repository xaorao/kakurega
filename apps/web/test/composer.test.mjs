import test from "node:test";
import assert from "node:assert/strict";
import { draftKey, saveDraft, loadDraft, forgetDraft, clearDrafts, draftCount, stopsReply, rememberSent, sentHistory, forgetSent, retagSent, idleHistory, historyStep, historyKey, historyMatches, opensHistoryPicker, exchangeWasSaved } from "../src/composer.ts";

test("cm01 each conversation keeps its own draft and an empty draft is not kept", () => {
  clearDrafts();
  saveDraft(draftKey(null), "unkept text");
  saveDraft(draftKey("s1"), "kept text");
  assert.equal(loadDraft("new"), "unkept text");
  assert.equal(loadDraft("kept:s1"), "kept text");
  assert.equal(loadDraft("kept:s2"), "");
  saveDraft("kept:s1", "");
  assert.equal(draftCount(), 1, "an emptied draft is dropped, not stored");
  forgetDraft("new");
  assert.equal(draftCount(), 0);
});

test("cm02 at most 20 drafts are kept and the oldest untouched one goes first", () => {
  clearDrafts();
  for (let i = 0; i < 21; i++) saveDraft(`kept:${i}`, `d${i}`);
  assert.equal(draftCount(), 20);
  assert.equal(loadDraft("kept:0"), "", "the oldest was dropped");
  saveDraft("kept:1", "touched again");
  saveDraft("kept:99", "new one");
  assert.equal(loadDraft("kept:1"), "touched again", "touching a draft makes it recent");
  assert.equal(loadDraft("kept:2"), "", "the next oldest went instead");
});

test("cm03 sign-out leaves nothing typed behind", () => {
  saveDraft("new", "secret plan");
  clearDrafts();
  assert.equal(draftCount(), 0);
  assert.equal(loadDraft("new"), "");
});

test("cm04 Escape stops a running reply, never while composing, never with a modifier, never when idle", () => {
  const k = (over = {}) => ({ key: "Escape", ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, isComposing: false, ...over });
  assert.equal(stopsReply(k(), true), true);
  assert.equal(stopsReply(k(), false), false);
  assert.equal(stopsReply(k({ isComposing: true }), true), false);
  assert.equal(stopsReply(k({ shiftKey: true }), true), false);
  assert.equal(stopsReply(k({ metaKey: true }), true), false);
  assert.equal(stopsReply(k({ key: "Enter" }), true), false);
});

test("cm05 Up from an empty box walks back through sent messages and Down walks forward to what was being written", () => {
  const entries = ["first", "second\nline", "third"];
  let r = historyStep(entries, idleHistory(), "up", "", 0);
  assert.deepEqual(r, { state: { index: 0, saved: "" }, text: "third", cursor: 0 });
  r = historyStep(entries, r.state, "up", r.text, r.cursor);
  assert.equal(r.text, "second\nline");
  r = historyStep(entries, r.state, "up", r.text, 0);
  assert.equal(r.text, "first");
  assert.equal(historyStep(entries, r.state, "up", r.text, 0), null, "nothing older");
  r = historyStep(entries, r.state, "down", r.text, 0);
  assert.deepEqual([r.text, r.cursor], ["second\nline", "second\nline".length]);
  r = historyStep(entries, r.state, "down", r.text, r.cursor);
  assert.equal(r.text, "third");
  r = historyStep(entries, r.state, "down", r.text, r.cursor);
  assert.deepEqual(r, { state: { index: -1, saved: "" }, text: "", cursor: 0 });
  assert.equal(historyStep(entries, r.state, "down", "", 0), null, "Down outside history does nothing");
});

test("cm06 Up and Down never take the caret from a message being written", () => {
  const entries = ["older"];
  assert.equal(historyStep(entries, idleHistory(), "up", "abc", 0), null);
  assert.equal(historyStep(entries, idleHistory(), "up", "abc", 3), null);
  assert.equal(historyStep([], idleHistory(), "up", "", 0), null, "no history");
  const inside = historyStep(entries, idleHistory(), "up", "", 0);
  assert.equal(historyStep(entries, inside.state, "down", inside.text, 2), null, "inside a recalled message the caret moves");
});

test("cm07 sent messages are kept once each, at most 100, and Clear or sign-out forgets them", () => {
  clearDrafts();
  rememberSent("  a  ");
  rememberSent("a");
  rememberSent("");
  rememberSent("b");
  assert.deepEqual([...sentHistory()], ["a", "b"]);
  for (let i = 0; i < 120; i++) rememberSent(`m${i}`);
  assert.equal(sentHistory().length, 100);
  assert.equal(sentHistory()[99], "m119");
  forgetSent();
  assert.equal(sentHistory().length, 0);
  rememberSent("x");
  clearDrafts();
  assert.equal(sentHistory().length, 0, "sign-out forgets them too");
});

test("cm08 only a plain Up or Down outside an input-method composition moves through history", () => {
  const k = (over = {}) => ({ key: "ArrowUp", ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, isComposing: false, ...over });
  assert.equal(historyKey(k()), "up");
  assert.equal(historyKey(k({ key: "ArrowDown" })), "down");
  assert.equal(historyKey(k({ isComposing: true })), null);
  assert.equal(historyKey(k({ shiftKey: true })), null);
  assert.equal(historyKey(k({ altKey: true })), null);
  assert.equal(historyKey(k({ key: "ArrowLeft" })), null);
});

test("cm09 forgetting one conversation removes only its messages, and keeping a chat carries its messages along", () => {
  clearDrafts();
  rememberSent("in the open chat", "new");
  rememberSent("in kept one", "kept:1");
  rememberSent("in kept two", "kept:2");
  forgetSent("kept:1");
  assert.deepEqual(sentHistory(), ["in the open chat", "in kept two"]);
  retagSent("new", "kept:3");
  forgetSent("kept:3");
  assert.deepEqual(sentHistory(), ["in kept two"], "the chat that became kept took its messages with it");
  rememberSent("same", "kept:2");
  rememberSent("same", "kept:4");
  assert.equal(sentHistory().length, 3, "the same words in another conversation are their own entry");
  forgetSent("kept:4");
  assert.deepEqual(sentHistory(), ["in kept two", "same"]);
});

const key = (over = {}) => ({ key: "x", ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, isComposing: false, ...over });

test("cp20 Ctrl+G stops a reply being written, like Escape, and only then", () => {
  assert.equal(stopsReply(key({ key: "g", ctrlKey: true }), true), true);
  assert.equal(stopsReply(key({ key: "G", ctrlKey: true }), true), true);
  assert.equal(stopsReply(key({ key: "g", ctrlKey: true }), false), false, "idle: Ctrl+G does nothing");
  assert.equal(stopsReply(key({ key: "g" }), true), false, "a plain g is text");
  assert.equal(stopsReply(key({ key: "g", ctrlKey: true, shiftKey: true }), true), false);
  assert.equal(stopsReply(key({ key: "g", ctrlKey: true, metaKey: true }), true), false);
  assert.equal(stopsReply(key({ key: "g", ctrlKey: true, isComposing: true }), true), false);
});

test("cp21 Shift+Up opens the picker only from an empty box with the caret at the start", () => {
  const up = key({ key: "ArrowUp", shiftKey: true });
  assert.equal(opensHistoryPicker(up, "", 0, 3), true);
  assert.equal(opensHistoryPicker(up, "", 0, 0), false, "nothing was sent yet");
  assert.equal(opensHistoryPicker(up, "draft", 0, 3), false, "never takes selection keys from text being written");
  assert.equal(opensHistoryPicker(key({ key: "ArrowUp" }), "", 0, 3), false, "plain Up is the history step");
  assert.equal(opensHistoryPicker(key({ key: "ArrowDown", shiftKey: true }), "", 0, 3), false);
  assert.equal(opensHistoryPicker(key({ key: "ArrowUp", shiftKey: true, ctrlKey: true }), "", 0, 3), false);
  assert.equal(opensHistoryPicker(key({ key: "ArrowUp", shiftKey: true, isComposing: true }), "", 0, 3), false);
});

test("cp22 the picker lists the newest first, once each, and narrows by every typed word", () => {
  const sent = ["summarise the audit", "what changed in billing", "summarise the audit", "draft a reply to Acme about billing"];
  assert.deepEqual(historyMatches(sent, ""), ["draft a reply to Acme about billing", "summarise the audit", "what changed in billing"]);
  assert.deepEqual(historyMatches(sent, "billing"), ["draft a reply to Acme about billing", "what changed in billing"]);
  assert.deepEqual(historyMatches(sent, "BILLING acme"), ["draft a reply to Acme about billing"], "every word, any case, any order");
  assert.deepEqual(historyMatches(sent, "zzz"), []);
  assert.deepEqual(historyMatches([], ""), []);
});

test("cp23 the picker shows at most fifty and ignores blank entries", () => {
  const many = Array.from({ length: 80 }, (_, i) => `message number ${i}`);
  const got = historyMatches(many, "");
  assert.equal(got.length, 50);
  assert.equal(got[0], "message number 79");
  assert.deepEqual(historyMatches(["", "  ", "real"], ""), ["real"]);
});

const turn = (role, content) => ({ role, content });

test("cp24 a failed send counts as saved only when the kept conversation holds exactly this exchange as its two newest turns", () => {
  const before = [turn("user", "hello"), turn("assistant", "hi")];
  const ok = [...before, turn("user", "what changed?"), turn("assistant", "Two things.")];
  assert.equal(exchangeWasSaved(2, "what changed?", ok), true);
  assert.equal(exchangeWasSaved(2, "  what changed?  ", ok), true, "the box text is trimmed before it is sent");
  assert.equal(exchangeWasSaved(2, "what changed?", before), false, "nothing was added");
  assert.equal(exchangeWasSaved(2, "what changed?", [...before, turn("user", "what changed?")]), false, "a message without its reply is not an exchange");
  assert.equal(exchangeWasSaved(2, "what changed?", [...ok, turn("user", "x"), turn("assistant", "y")]), false, "another device added more: do not guess");
  assert.equal(exchangeWasSaved(2, "something else", ok), false, "a different message");
  assert.equal(exchangeWasSaved(2, "what changed?", [...before, turn("assistant", "Two things."), turn("user", "what changed?")]), false, "wrong order");
  assert.equal(exchangeWasSaved(2, "what changed?", [...before, turn("user", "what changed?"), turn("assistant", "  ")]), false, "an empty reply");
  assert.equal(exchangeWasSaved(2, "what changed?", [...before, turn("assistant", "what changed?"), turn("assistant", "Two things.")]), false, "the first new turn must be the owner's");
  assert.equal(exchangeWasSaved(2, "what changed?", [...before, turn("user", "what changed?"), turn("user", "Two things.")]), false, "the second new turn must be the reply");
  assert.equal(exchangeWasSaved(0, "first", [turn("user", "first"), turn("assistant", "ok")]), true);
  assert.equal(exchangeWasSaved(0, "first", []), false);
});
