const CLOSED = 2;

export interface SourceLike {
  readonly readyState: number;
  onopen: ((ev: Event) => void) | null;
  onmessage: ((ev: MessageEvent) => void) | null;
  onerror: ((ev: Event) => void) | null;
  addEventListener(type: string, listener: () => void): void;
  close(): void;
}

export interface LiveOptions {
  open: () => SourceLike;
  /** Something changed, or the stream came back after a gap in which changes may have been missed. */
  onChange: () => void;
  now?: () => number;
  /** A stream that has been silent this long is dropped and opened again. Three server pings. */
  idleMs?: number;
  /** When the page returns to the foreground, a stream silent for longer than this is opened again. */
  quietMs?: number;
  /** Waits before opening again after the browser gives up on a stream (a refusal, not a dropped connection). */
  retryMs?: number[];
}

export interface Live {
  start(): void;
  stop(): void;
  /** Call every few seconds. Opens a stream that is due and replaces one that went silent. */
  check(): void;
  /** The page became visible again. */
  foreground(): void;
  /** The network came back. */
  online(): void;
}

/**
 * The page's live connection to the service. The browser reconnects a dropped stream by itself, but not a stream the
 * service refused (too many open, session ended), and it never notices a stream that went half-open while the computer
 * slept. The service sends a named "ping" event every 15 seconds, which the page cannot see as a message but does see as
 * activity, so silence longer than three pings means the connection is dead.
 */
export function createLive(o: LiveOptions): Live {
  const now = o.now ?? Date.now;
  const idleMs = o.idleMs ?? 45_000;
  const quietMs = o.quietMs ?? 20_000;
  const delays = o.retryMs ?? [1_000, 2_000, 5_000, 10_000, 30_000];
  let src: SourceLike | null = null;
  let active = false;
  let last = 0;
  let opened = false;
  let failures = 0;
  let reopenAt = 0;

  const drop = () => {
    const s = src;
    src = null;
    s?.close();
  };

  const connect = () => {
    const s = o.open();
    src = s;
    last = now();
    s.onopen = () => {
      if (src !== s) return;
      last = now();
      // The first open of a clean start needs no catch-up (the page has just loaded). Any later open follows a gap.
      const gap = opened || failures > 0;
      failures = 0;
      opened = true;
      if (gap) o.onChange();
    };
    s.onmessage = () => {
      if (src !== s) return;
      last = now();
      o.onChange();
    };
    s.addEventListener("ping", () => {
      if (src === s) last = now();
    });
    s.onerror = () => {
      if (src !== s || s.readyState !== CLOSED) return;
      drop();
      reopenAt = now() + (delays[Math.min(failures, delays.length - 1)] ?? 30_000);
      failures += 1;
    };
  };

  const reopen = () => {
    drop();
    connect();
  };

  return {
    start() {
      if (active) return;
      active = true;
      failures = 0;
      connect();
    },
    stop() {
      active = false;
      opened = false;
      drop();
    },
    check() {
      if (!active) return;
      if (!src) {
        if (now() >= reopenAt) connect();
        return;
      }
      if (now() - last > idleMs) reopen();
    },
    foreground() {
      if (!active) return;
      if (!src || now() - last >= quietMs) reopen();
    },
    online() {
      if (!active) return;
      reopen();
    },
  };
}
