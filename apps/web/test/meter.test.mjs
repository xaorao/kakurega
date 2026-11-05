import test from "node:test";
import assert from "node:assert/strict";
import { meterLine } from "../src/meter.ts";

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
