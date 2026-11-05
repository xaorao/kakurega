import test from "node:test";
import assert from "node:assert/strict";
import { draftKey, saveDraft, loadDraft, forgetDraft, clearDrafts, draftCount, stopsReply, rememberSent, sentHistory, forgetSent, retagSent, idleHistory, historyStep, historyKey } from "../src/composer.ts";

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
