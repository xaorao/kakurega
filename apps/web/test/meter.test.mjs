import test from "node:test";
import assert from "node:assert/strict";
import { answerLine, meterLine, noteNotes, pageNotes, savedLine, summaryNotes } from "../src/meter.ts";

test("mt01 the model's own count is shown and labelled as counted, with the window and percent", () => {
  assert.deepEqual(meterLine({ context_tokens: 4321, context_tokens_estimate: 5000, context_window: 16384, estimate: false }), { key: "chat.contextCountedWindow", n: 4321, max: 16384, pct: 26, near: false });
});

test("mt02 an estimate is labelled as one", () => {
  assert.deepEqual(meterLine({ context_tokens: 900, context_tokens_estimate: 900, context_window: 16384, estimate: true }), { key: "chat.contextWindow", n: 900, max: 16384, pct: 5, near: false });
});

test("mt03 no window is invented when the service sent none, and an older service's reply still shows a figure", () => {
  assert.deepEqual(meterLine({ context_tokens: 4321, context_tokens_estimate: 5000, context_window: null, estimate: false }), { key: "chat.contextCounted", n: 4321, near: false });
  assert.deepEqual(meterLine({ context_tokens_estimate: 1240 }), { key: "chat.context", n: 1240, near: false });
  assert.equal(meterLine({ context_tokens_estimate: 10, context_window: 0 }).max, undefined, "a zero window is no window");
});

test("mt04 a count of zero is not presented as the model's own", () => {
  assert.equal(meterLine({ context_tokens: 0, context_tokens_estimate: 70, context_window: 100, estimate: false }).key, "chat.contextWindow");
});

test("mt05 near the limit is flagged from 80%, and the percent is capped", () => {
  assert.equal(meterLine({ context_tokens: 79, context_tokens_estimate: 79, context_window: 100, estimate: false }).near, false);
  assert.equal(meterLine({ context_tokens: 80, context_tokens_estimate: 80, context_window: 100, estimate: false }).near, true);
  assert.equal(meterLine({ context_tokens: 10 ** 9, context_tokens_estimate: 1, context_window: 100, estimate: false }).pct, 999);
});

test("mt06 the footer names pages used, cut short and left out, and says nothing about pages when none were chosen", () => {
  assert.deepEqual(pageNotes({ web_sources_used: 2, web_sources_trimmed: 1, web_sources_left_out: 3 }), [
    { key: "web.used", n: 2 },
    { key: "web.trimmed", n: 1 },
    { key: "web.leftOut", n: 3 },
  ]);
  assert.deepEqual(pageNotes({ web_sources_used: 0 }), []);
  assert.deepEqual(pageNotes({ web_sources_used: 0, web_sources_left_out: 2 }), [{ key: "web.leftOut", n: 2 }], "every page left out is still reported");
});

test("mt07 the footer says how many matching notes were left out, and nothing when none were", () => {
  assert.deepEqual(noteNotes({ notes_left_out: 3 }), [{ key: "chat.notesLeftOut", n: 3 }]);
  assert.deepEqual(noteNotes({ notes_left_out: 0 }), []);
  assert.deepEqual(noteNotes({}), [], "an older service that sends no count shows nothing");
});

test("mt08 the footer names the real reason an exchange was not saved", () => {
  assert.equal(savedLine({ saved: true }), "chat.saved");
  assert.equal(savedLine({ saved: false, not_saved: "full" }), "chat.notSaved");
  assert.equal(savedLine({ saved: false, not_saved: "forgotten" }), "chat.notSavedForgotten");
  assert.equal(savedLine({ saved: false, not_saved: "error" }), "chat.notSavedError");
  assert.equal(savedLine({ saved: false }), "chat.notSaved", "an older service that gives no reason keeps the old wording");
});

test("mt09 the footer says when older messages are replaced by a summary, when one was just written, and when none could be", () => {
  assert.deepEqual(summaryNotes({ history_turns_summarized: 12, compacted_now: true }), [{ key: "chat.summarisedNow", n: 12 }]);
  assert.deepEqual(summaryNotes({ history_turns_summarized: 12, compacted_now: false }), [{ key: "chat.summarised", n: 12 }]);
  assert.deepEqual(summaryNotes({ history_turns_summarized: 0, compaction_failed: "unavailable" }), [{ key: "chat.summaryFailed", n: 0 }]);
  assert.deepEqual(summaryNotes({}), [], "an older service or an unkept chat says nothing");
});

test("am01 the answer line gives about how long the answer is and how long it took, with a whole-answer speed only when it means something", () => {
  assert.deepEqual(answerLine({ elapsed_ms: 3200, reply_tokens_estimate: 140 }), { key: "chat.answerSpeed", tokens: 140, took: "3.2 s", per: 43 });
  assert.deepEqual(answerLine({ elapsed_ms: 500, reply_tokens_estimate: 10 }), { key: "chat.answerSpeed", tokens: 10, took: "500 ms", per: 20 });
  assert.deepEqual(answerLine({ elapsed_ms: 499, reply_tokens_estimate: 10 }), { key: "chat.answer", tokens: 10, took: "499 ms" }, "too short for a speed to mean anything");
  assert.deepEqual(answerLine({ elapsed_ms: 0, reply_tokens_estimate: 5 }), { key: "chat.answer", tokens: 5, took: "0 ms" });
  assert.equal(answerLine({ elapsed_ms: 999, reply_tokens_estimate: 10 }).took, "999 ms");
  assert.equal(answerLine({ elapsed_ms: 1000, reply_tokens_estimate: 10 }).took, "1.0 s");
  assert.deepEqual(answerLine({ elapsed_ms: 1500, reply_tokens_estimate: 10 }), { key: "chat.answerSpeed", tokens: 10, took: "1.5 s", per: 6 });
  assert.deepEqual(answerLine({ elapsed_ms: 600000, reply_tokens_estimate: 10 }), { key: "chat.answer", tokens: 10, took: "600.0 s" }, "a speed that rounds to zero is not shown");
});

test("am02 bad or missing figures give no answer line, and huge ones do not break", () => {
  for (const [ms, tokens] of [[1000, 0], [undefined, 10], [1000, undefined], ["1000", 10], [1.5, 10], [-5, 10], [1000, -1], [[1000], 10], [NaN, 10], [Infinity, 10], [1000, NaN], [null, 10], [1000, null], [2 ** 60, 10], [1000, 2 ** 60]]) {
    assert.equal(answerLine({ elapsed_ms: ms, reply_tokens_estimate: tokens }), null, `${String(ms)} ${String(tokens)}`);
  }
  assert.equal(answerLine({}), null, "an older service that sends no time gives no line");
  assert.equal(answerLine({ elapsed_ms: 1000, reply_tokens_estimate: Number.MAX_SAFE_INTEGER }).key, "chat.answerSpeed");
});
