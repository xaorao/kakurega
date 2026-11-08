import test from "node:test";
import assert from "node:assert/strict";
import { copyNames, leftBehind } from "../src/forgotten.ts";
import { en } from "../src/locales/en.ts";
import { hi } from "../src/locales/hi.ts";

test("fg01 known copies are listed by name, reviews first, and the rest are counted", () => {
  assert.deepEqual(copyNames({ copies_known: { reviews: ["job-1"], exports: ["a.md", "b.md"] } }), { shown: ["job-1", "a.md", "b.md"], more: 0 });
  assert.deepEqual(copyNames({ copies_known: { reviews: [], exports: [] } }), { shown: [], more: 0 });
  assert.deepEqual(copyNames(null), { shown: [], more: 0 });
  assert.deepEqual(copyNames({}), { shown: [], more: 0 });
  const many = { copies_known: { reviews: [], exports: Array.from({ length: 9 }, (_, i) => `f${i}.md`) } };
  assert.deepEqual(copyNames(many, 4), { shown: ["f0.md", "f1.md", "f2.md", "f3.md"], more: 5 });
});

test("fg02 anything that is not a plain short name is dropped, so a hostile file name cannot reach the screen", () => {
  const r = { copies_known: { reviews: ["job-1", 7, null, "x".repeat(121), "a\nb", "<b>x</b>", ""], exports: ["ok.md", "bad\u001b[2J.md"] } };
  assert.deepEqual(copyNames(r), { shown: ["job-1", "ok.md"], more: 0 });
});

test("fg03 the texts exist in both languages", () => {
  for (const k of ["mem.forgotten.copies", "mem.forgotten.more", "mem.forgotten.notReached"]) assert.ok(en[k] && hi[k], k);
});

test("fg04 conversations and history are read out of the answer, and nothing else", () => {
  const r = { copies_known: { reviews: [], exports: [], conversations: [{ id: "s-1", why: "text" }, { id: "s_2", why: "source" }], conversations_more: 3, prompt_history: 2, search_complete: false } };
  assert.deepEqual(leftBehind(r), { conversations: [{ id: "s-1", why: "text" }, { id: "s_2", why: "source" }], more: 3, history: 2, incomplete: true });
  // nothing left: no card at all
  assert.equal(leftBehind({ copies_known: { reviews: ["job-1"], exports: [], conversations: [], conversations_more: 0, prompt_history: 0, search_complete: true } }), null);
  assert.equal(leftBehind(null), null);
  assert.equal(leftBehind({}), null);
  // an older service that sends none of it
  assert.equal(leftBehind({ forgotten: true, copies_known: { reviews: [], exports: [] } }), null);
  // an incomplete search alone is worth saying
  assert.deepEqual(leftBehind({ copies_known: { search_complete: false } }), { conversations: [], more: 0, history: 0, incomplete: true });
});

test("fg05 hostile ids, reasons and counts are dropped so they cannot reach the page or a request", () => {
  const r = {
    copies_known: {
      conversations: [
        { id: "ok-1", why: "text" },
        { id: "../../etc", why: "text" },
        { id: "a b", why: "text" },
        { id: "x".repeat(65), why: "text" },
        { id: "<b>x</b>", why: "source" },
        { id: "good", why: "mystery" },
        { id: 7, why: "text" },
        { why: "text" },
        null,
        "s-1",
      ],
      conversations_more: -4,
      prompt_history: 1.5,
    },
  };
  assert.deepEqual(leftBehind(r), { conversations: [{ id: "ok-1", why: "text" }], more: 0, history: 0, incomplete: false });
  assert.equal(leftBehind({ copies_known: { conversations: "s-1", conversations_more: "5", prompt_history: "2" } }), null);
  // never more than twenty are kept, however many the answer claims
  const many = { copies_known: { conversations: Array.from({ length: 30 }, (_, i) => ({ id: `s-${i}`, why: "text" })) } };
  assert.equal(leftBehind(many).conversations.length, 20);
});

test("fg06 the card's texts exist in both languages and the Hindi is not left as English", () => {
  const keys = ["mem.left.heading", "mem.left.lead", "mem.left.text", "mem.left.source", "mem.left.untitled", "mem.left.delete", "mem.left.deleteLabel", "mem.left.deleted", "mem.left.history", "mem.left.clearHistory", "mem.left.historyCleared", "mem.left.incomplete", "mem.left.close"];
  for (const k of keys) {
    assert.ok(en[k] && hi[k], k);
    assert.notEqual(en[k], hi[k], k);
  }
  assert.match(en["mem.forgotten.notReached"], /other words/);
});
