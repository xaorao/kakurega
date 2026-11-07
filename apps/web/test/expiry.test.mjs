import test from "node:test";
import assert from "node:assert/strict";
import { SOON_SECS, clockOffset, formatSpan, timeLeft } from "../src/expiry.ts";

test("ex01 a deadline is valid, soon, expired, or unknown, and unknown is never shown as valid", () => {
  assert.deepEqual(timeLeft(1000, 0), { state: "valid", secs: 1000 });
  assert.deepEqual(timeLeft(1000, 1000 - SOON_SECS), { state: "soon", secs: SOON_SECS });
  assert.deepEqual(timeLeft(1000, 1000 - SOON_SECS - 1), { state: "valid", secs: SOON_SECS + 1 });
  assert.deepEqual(timeLeft(1000, 999.4), { state: "expired" }, "less than a whole second left counts as none");
  assert.deepEqual(timeLeft(1000, 1000), { state: "expired" }, "the deadline itself is already too late, as in the kernel");
  assert.deepEqual(timeLeft(1000, 5000), { state: "expired" });
  for (const bad of [undefined, null, NaN, Infinity, "1000", {}]) assert.deepEqual(timeLeft(bad, 0), { state: "unknown" }, String(bad));
  assert.deepEqual(timeLeft(1000, NaN), { state: "unknown" });
});

test("ex02 spans are the two largest units, never more", () => {
  assert.equal(formatSpan(0), "0 s");
  assert.equal(formatSpan(59), "59 s");
  assert.equal(formatSpan(60), "1 min");
  assert.equal(formatSpan(61), "1 min 1 s");
  assert.equal(formatSpan(3599), "59 min 59 s");
  assert.equal(formatSpan(3600), "1 h");
  assert.equal(formatSpan(3660), "1 h 1 min");
  assert.equal(formatSpan(86399), "23 h 59 min");
  assert.equal(formatSpan(90000), "25 h");
  assert.equal(formatSpan(-5), "0 s");
  assert.equal(formatSpan(12.9), "12 s");
});

test("ex03 the clock offset is the service's clock minus the page's, or nothing", () => {
  assert.equal(clockOffset(1000, 940), 60);
  assert.equal(clockOffset(900, 1000), -100);
  assert.equal(clockOffset(undefined, 1000), 0);
  assert.equal(clockOffset(NaN, 1000), 0);
  assert.equal(clockOffset(1000, NaN), 0);
});
