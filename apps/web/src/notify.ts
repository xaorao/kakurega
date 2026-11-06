// Desktop notifications for the things that wait for the owner.
//
// Off until the owner turns it on. A notification is shown only when no window of the app has focus, at most once per event
// however many tabs are open or how often the page is reloaded, and at most three at a time. The text says that something
// waits and never what, for whom or in which project: it goes through the operating system, which shows it on a locked
// screen and keeps it in a notification history that this program cannot clear.

export const NOTIFY_KEY = "kk.notify";
export const CLAIMED_KEY = "kk.notify.claimed";
export const MAX_CLAIMED = 200;
export const MAX_PER_UPDATE = 3;

const FOCUS_LOCK = "kk:notify-focus";

/** What needs the owner. The other kinds of inbox items are listed on the Inbox screen and not announced. */
export type Kind = "approval_waiting" | "ready_to_review" | "ready_to_deliver" | "failed" | "suggestions";
const ANNOUNCED: ReadonlySet<string> = new Set(["approval_waiting", "ready_to_review", "failed"]);

export interface Item {
  kind: Kind;
  job: string;
  since: number;
}

export interface LocksLike {
  request(name: string, a: unknown, b?: unknown): Promise<unknown>;
}

export interface StorageLike {
  getItem(k: string): string | null;
  setItem(k: string, v: string): void;
}

export interface Env {
  hasFocus(): boolean;
  permission(): "granted" | "denied" | "default" | "unsupported";
  show(title: string, body: string, tag: string): void;
  locks: LocksLike | undefined;
  storage: StorageLike | undefined;
}

export interface Notifier {
  enabled(): boolean;
  setEnabled(on: boolean): void;
  /** The inbox as it is now. Returns how many notifications were shown. Never throws. */
  observe(items: readonly { kind: string; job: string; since: number }[]): Promise<number>;
  /** Call when the window gained or lost focus, so other tabs know somebody is looking. */
  focusChanged(): Promise<void>;
}

const keyOf = (i: { kind: string; job: string; since: number }) => `${i.kind}:${i.job}:${i.since}`;

export function createNotifier(env: Env, text: (kind: string) => string): Notifier {
  const seen = new Set<string>();
  const claimedHere = new Set<string>();
  let baseline = false;
  let holding = false;
  let pending = false;
  let release: (() => void) | undefined;

  const read = (k: string): string | null => {
    try {
      return env.storage?.getItem(k) ?? null;
    } catch {
      return null;
    }
  };
  const write = (k: string, v: string) => {
    try {
      env.storage?.setItem(k, v);
    } catch {
      /* the in-memory record still prevents repeats in this page */
    }
  };
  const claimedList = (): string[] => {
    try {
      const v: unknown = JSON.parse(read(CLAIMED_KEY) ?? "[]");
      return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
    } catch {
      return [];
    }
  };
  const isClaimed = (key: string) => claimedHere.has(key) || claimedList().includes(key);
  const claim = (key: string) => {
    claimedHere.add(key);
    write(CLAIMED_KEY, JSON.stringify([...claimedList(), key].slice(-MAX_CLAIMED)));
  };

  // the choice lives in storage so every tab and reload agrees; where storage refuses, this page still honours it
  let chosen: boolean | null = null;
  const enabled = () => {
    const stored = read(NOTIFY_KEY);
    return stored === null ? chosen === true : stored === "1";
  };

  /** Shows one notification if nobody is looking. Returns whether it was shown. */
  const announce = async (item: { kind: string; job: string; since: number }): Promise<boolean> => {
    const key = keyOf(item);
    const run = async (): Promise<boolean> => {
      if (isClaimed(key) || env.hasFocus()) return false;
      const show = () => {
        try {
          env.show("Kakurega", text(item.kind), `kk-${item.kind}`);
          return true;
        } catch {
          return false;
        }
      };
      let shown: boolean;
      const locks = env.locks;
      if (!locks) shown = show();
      else {
        try {
          // exclusive only if no window of the app holds the shared focus lock: nobody is looking
          shown = (await locks.request(FOCUS_LOCK, { mode: "exclusive", ifAvailable: true }, async (lock: unknown) =>
            lock ? show() : false,
          )) as boolean;
        } catch {
          shown = show();
        }
      }
      if (shown) claim(key);
      return shown;
    };
    const locks = env.locks;
    if (!locks) return run();
    try {
      // one event is decided by one tab at a time
      return (await locks.request(`kk:notify:${key}`, run)) as boolean;
    } catch {
      return run();
    }
  };

  return {
    enabled,
    setEnabled(on) {
      chosen = on;
      write(NOTIFY_KEY, on ? "1" : "0");
    },
    async observe(items) {
      let shown = 0;
      try {
        const fresh = items.filter((i) => !seen.has(keyOf(i)));
        for (const i of fresh) seen.add(keyOf(i));
        if (!baseline) {
          baseline = true;
          return 0;
        }
        if (!enabled() || env.permission() !== "granted") return 0;
        for (const i of fresh) {
          if (shown >= MAX_PER_UPDATE) break;
          if (!ANNOUNCED.has(i.kind)) continue;
          if (await announce(i)) shown++;
        }
      } catch {
        /* a notification must never break the page */
      }
      return shown;
    },
    async focusChanged() {
      const locks = env.locks;
      if (!locks) return;
      try {
        if (env.hasFocus()) {
          if (holding || pending) return;
          pending = true;
          await new Promise<void>((granted) => {
            void Promise.resolve(
              locks.request(FOCUS_LOCK, { mode: "shared" }, () => {
                holding = true;
                pending = false;
                granted();
                return new Promise<void>((res) => {
                  release = res;
                });
              }),
            ).catch(() => {
              pending = false;
              granted();
            });
          });
        } else {
          release?.();
          release = undefined;
          holding = false;
        }
      } catch {
        pending = false;
      }
    },
  };
}

/** The real browser, or nothing where the pieces are missing. */
export function browserEnv(): Env {
  const hasNotification = typeof Notification !== "undefined";
  return {
    hasFocus: () => typeof document !== "undefined" && document.hasFocus(),
    permission: () => (hasNotification ? Notification.permission : "unsupported"),
    show: (title, body, tag) => {
      const n = new Notification(title, { body, tag });
      n.onclick = () => {
        window.focus();
        location.hash = "#/inbox";
        n.close();
      };
    },
    locks: typeof navigator !== "undefined" && "locks" in navigator ? (navigator.locks as unknown as LocksLike) : undefined,
    storage: (() => {
      try {
        return typeof localStorage !== "undefined" ? localStorage : undefined;
      } catch {
        return undefined;
      }
    })(),
  };
}
