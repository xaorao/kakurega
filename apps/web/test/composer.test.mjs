import test from "node:test";
import assert from "node:assert/strict";
import { draftKey, saveDraft, loadDraft, forgetDraft, clearDrafts, draftCount, stopsReply } from "../src/composer.ts";

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
