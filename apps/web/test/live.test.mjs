import test from "node:test";
import assert from "node:assert/strict";
import { createLive } from "../src/live.ts";

function harness() {
  let t = 0;
  let changes = 0;
  const sources = [];
  const open = () => {
    const listeners = {};
    const s = {
      readyState: 0,
      onopen: null,
      onmessage: null,
      onerror: null,
      closed: false,
      addEventListener(type, fn) {
        (listeners[type] ??= []).push(fn);
      },
      close() {
        this.closed = true;
        this.readyState = 2;
      },
      ping() {
        (listeners.ping ?? []).forEach((f) => f());
      },
      opened() {
        this.readyState = 1;
        this.onopen?.({});
      },
      message() {
        this.onmessage?.({});
      },
      fail(browserGaveUp) {
        this.readyState = browserGaveUp ? 2 : 0;
        this.onerror?.({});
      },
    };
    sources.push(s);
    return s;
  };
  const live = createLive({ open, onChange: () => changes++, now: () => t });
  return { live, sources, advance: (ms) => (t += ms), changes: () => changes };
}

test("lv01 the first open needs no catch-up, a message refreshes, and the browser's own reconnect catches up", () => {
  const h = harness();
  h.live.start();
  assert.equal(h.sources.length, 1);
  h.sources[0].opened();
  assert.equal(h.changes(), 0);
  h.sources[0].message();
  assert.equal(h.changes(), 1);
  h.sources[0].fail(false);
  assert.equal(h.sources.length, 1, "a dropped connection is left to the browser, which retries by itself");
  assert.equal(h.sources[0].closed, false);
  h.sources[0].opened();
  assert.equal(h.changes(), 2, "changes made while the connection was down are fetched when it returns");
});

test("lv02 pings keep a quiet stream alive and a silent one is replaced", () => {
  const h = harness();
  h.live.start();
  h.sources[0].opened();
  h.advance(40_000);
  h.sources[0].ping();
  h.advance(40_000);
  h.live.check();
  assert.equal(h.sources.length, 1, "80 seconds in, but the last ping was 40 seconds ago");
  h.advance(6_000);
  h.live.check();
  assert.equal(h.sources.length, 2, "46 seconds of silence: the half-open stream is dropped and a new one opened");
  assert.equal(h.sources[0].closed, true);
  h.sources[1].opened();
  assert.equal(h.changes(), 1, "and the page catches up on what it may have missed");
});

test("lv03 a refused stream is retried with growing waits and recovers", () => {
  const h = harness();
  h.live.start();
  h.sources[0].fail(true);
  h.live.check();
  assert.equal(h.sources.length, 1, "no retry before the first wait is over");
  h.advance(999);
  h.live.check();
  assert.equal(h.sources.length, 1);
  h.advance(1);
  h.live.check();
  assert.equal(h.sources.length, 2, "retried after 1 second");
  h.sources[1].fail(true);
  h.advance(1_999);
  h.live.check();
  assert.equal(h.sources.length, 2);
  h.advance(1);
  h.live.check();
  assert.equal(h.sources.length, 3, "the second wait is 2 seconds");
  h.sources[2].opened();
  assert.equal(h.changes(), 1, "the first successful open after refusals catches up");
  h.sources[2].fail(true);
  h.advance(1_000);
  h.live.check();
  assert.equal(h.sources.length, 4, "a success resets the waits to 1 second");
});

test("lv04 coming back to the page or the network replaces a stream that went quiet", () => {
  const h = harness();
  h.live.start();
  h.sources[0].opened();
  h.advance(10_000);
  h.live.foreground();
  assert.equal(h.sources.length, 1, "10 seconds of quiet is normal between pings");
  h.advance(15_000);
  h.live.foreground();
  assert.equal(h.sources.length, 2, "25 seconds of quiet after the computer slept: open a new one");
  assert.equal(h.sources[0].closed, true);
  h.live.online();
  assert.equal(h.sources.length, 3, "the network came back");
  assert.equal(h.sources[1].closed, true);
  h.live.stop();
  assert.equal(h.sources[2].closed, true);
  h.live.foreground();
  h.live.online();
  h.live.check();
  assert.equal(h.sources.length, 3, "nothing opens while stopped");
});

test("lv05 a replaced stream can no longer cause refreshes or reopen anything", () => {
  const h = harness();
  h.live.start();
  const old = h.sources[0];
  h.live.online();
  assert.equal(h.sources.length, 2);
  old.message();
  old.opened();
  old.fail(true);
  assert.equal(h.changes(), 0);
  h.advance(60_000);
  h.sources[1].ping();
  h.live.check();
  assert.equal(h.sources.length, 2, "the failure of the old stream did not drop the new one");
});

test("lv06 starting twice opens one stream, and a restart after stop begins clean", () => {
  const h = harness();
  h.live.start();
  h.live.start();
  assert.equal(h.sources.length, 1);
  h.sources[0].opened();
  h.live.stop();
  assert.equal(h.sources[0].closed, true);
  h.live.start();
  assert.equal(h.sources.length, 2);
  h.sources[1].opened();
  assert.equal(h.changes(), 0, "signing in again is a fresh start, not a gap");
});
