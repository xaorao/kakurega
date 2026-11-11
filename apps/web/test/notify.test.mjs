import test from "node:test";
import assert from "node:assert/strict";
import { createNotifier, NOTIFY_KEY, CLAIMED_KEY, MAX_CLAIMED } from "../src/notify.ts";

/** A Web Locks stand-in with the semantics the notifier relies on: shared locks coexist, an exclusive lock needs the name to
 * be free, and `ifAvailable` answers null at once when the lock cannot be granted now. */
function fakeLocks() {
  const held = new Map(); // name -> { shared: number, exclusive: boolean }
  const state = (n) => held.get(n) ?? held.set(n, { shared: 0, exclusive: false }).get(n);
  const waiting = [];
  const grantable = (n, mode) => (mode === "shared" ? !state(n).exclusive : !state(n).exclusive && state(n).shared === 0);
  const drain = () => {
    for (const w of [...waiting]) {
      if (grantable(w.name, w.mode)) {
        waiting.splice(waiting.indexOf(w), 1);
        w.run();
      }
    }
  };
  return {
    async request(name, a, b) {
      const opts = typeof a === "function" ? {} : a;
      const cb = typeof a === "function" ? a : b;
      const mode = opts.mode ?? "exclusive";
      const run = async () => {
        const s = state(name);
        if (mode === "shared") s.shared++;
        else s.exclusive = true;
        try {
          return await cb({ name, mode });
        } finally {
          if (mode === "shared") s.shared--;
          else s.exclusive = false;
          drain();
        }
      };
      if (grantable(name, mode)) return run();
      if (opts.ifAvailable) return cb(null);
      return new Promise((resolve, reject) => waiting.push({ name, mode, run: () => run().then(resolve, reject) }));
    },
  };
}

function fakeStorage(initial = {}) {
  const m = new Map(Object.entries(initial));
  return { getItem: (k) => m.get(k) ?? null, setItem: (k, v) => void m.set(k, String(v)), raw: m };
}

const text = (kind) => ({ approval_waiting: "Something is waiting for your approval.", ready_to_review: "A review is ready.", failed: "A review stopped before it finished." })[kind];

/** One browser tab. */
function tab(shared, over = {}) {
  const shown = [];
  let focused = false;
  const env = {
    hasFocus: () => focused,
    permission: () => over.permission ?? "granted",
    show: (title, body, tag) => {
      if (over.showThrows) throw new Error("the system refused");
      shown.push({ title, body, tag });
    },
    locks: "locks" in over ? over.locks : shared.locks,
    storage: "storage" in over ? over.storage : shared.storage,
  };
  const n = createNotifier(env, text);
  return {
    n,
    shown,
    focus: async (v) => {
      focused = v;
      await n.focusChanged();
    },
  };
}

const world = () => ({ locks: fakeLocks(), storage: fakeStorage() });
const item = (kind, job, since, extra = {}) => ({ kind, job, since, ...extra });

test("n01 it is off until the owner turns it on, whatever else is true", async () => {
  const w = world();
  const t = tab(w);
  assert.equal(t.n.enabled(), false);
  await t.n.observe([]);
  assert.equal(await t.n.observe([item("approval_waiting", "job-1", 10)]), 0);
  assert.deepEqual(t.shown, []);
});

test("n02 existing items at start are only a baseline; a new item while unfocused gives one generic notification", async () => {
  const w = world();
  const t = tab(w);
  t.n.setEnabled(true);
  assert.equal(await t.n.observe([item("approval_waiting", "job-1", 10)]), 0, "already waiting when the page opened");
  assert.equal(await t.n.observe([item("approval_waiting", "job-1", 10), item("approval_waiting", "job-2", 20)]), 1);
  assert.equal(t.shown.length, 1);
  assert.deepEqual(t.shown[0].title, "Kakurega");
  assert.equal(t.shown[0].body, "Something is waiting for your approval.");
  // the same list again says nothing more
  assert.equal(await t.n.observe([item("approval_waiting", "job-1", 10), item("approval_waiting", "job-2", 20)]), 0);
  assert.equal(t.shown.length, 1);
});

test("n03 a window that has focus is never interrupted, and what it saw is not announced later", async () => {
  const w = world();
  const t = tab(w);
  t.n.setEnabled(true);
  await t.n.observe([]);
  await t.focus(true);
  assert.equal(await t.n.observe([item("failed", "job-3", 30)]), 0);
  await t.focus(false);
  assert.equal(await t.n.observe([item("failed", "job-3", 30)]), 0, "already seen while the owner was looking");
  assert.deepEqual(t.shown, []);
});

test("n04 no permission, no support: nothing is shown and nothing throws", async () => {
  for (const permission of ["denied", "default", "unsupported"]) {
    const t = tab(world(), { permission });
    t.n.setEnabled(true);
    await t.n.observe([]);
    assert.equal(await t.n.observe([item("approval_waiting", "job-1", 1)]), 0, permission);
    assert.deepEqual(t.shown, []);
  }
});

test("n05 two tabs announce a new item once, and a focused sibling tab keeps the other quiet", async () => {
  const w = world();
  const a = tab(w);
  const b = tab(w);
  for (const t of [a, b]) {
    t.n.setEnabled(true);
    await t.n.observe([]);
  }
  const items = [item("ready_to_review", "job-4", 40)];
  const [x, y] = await Promise.all([a.n.observe(items), b.n.observe(items)]);
  assert.equal(x + y, 1, "exactly one of the two announced it");
  assert.equal(a.shown.length + b.shown.length, 1);
  // a sibling that is focused means somebody is looking: no notification from the other
  const c = tab(w);
  const d = tab(w);
  for (const t of [c, d]) {
    t.n.setEnabled(true);
    await t.n.observe([]);
  }
  await d.focus(true);
  assert.equal(await c.n.observe([item("approval_waiting", "job-5", 50)]), 0);
  assert.equal(c.shown.length, 0);
  await d.focus(false);
  assert.equal(await c.n.observe([item("approval_waiting", "job-6", 60)]), 1, "once nobody is looking it is announced");
});

test("n06 a reloaded page does not repeat what an earlier page announced", async () => {
  const w = world();
  const first = tab(w);
  first.n.setEnabled(true);
  await first.n.observe([]);
  assert.equal(await first.n.observe([item("failed", "job-7", 70)]), 1);
  const reloaded = tab(w);
  assert.equal(reloaded.n.enabled(), true, "the choice is remembered");
  assert.equal(await reloaded.n.observe([item("failed", "job-7", 70)]), 0);
  assert.equal(await reloaded.n.observe([item("failed", "job-7", 70), item("failed", "job-8", 80)]), 1);
  // even if the baseline were missed, the stored record stops a repeat of the same event
  const raced = tab(w);
  raced.n.setEnabled(true);
  await raced.n.observe([]);
  assert.equal(await raced.n.observe([item("failed", "job-7", 70)]), 0);
});

test("n07 the text never carries anything about the work, whatever the items contain", async () => {
  const w = world();
  const t = tab(w);
  t.n.setEnabled(true);
  await t.n.observe([]);
  const hostile = {
    recipient: "Acme <b>Audit</b> secret-recipient",
    what: "release 3 files of payments/auth/session.py to acme",
    title: "Token not revoked in payments",
    repo: "/Users/p/secret-project",
  };
  await t.n.observe([
    item("approval_waiting", "job-9", 90, hostile),
    item("ready_to_review", "secret-project-job", 91, hostile),
    item("failed", "job-10", 92, hostile),
  ]);
  assert.equal(t.shown.length, 3);
  const all = JSON.stringify(t.shown);
  for (const leak of ["Acme", "secret", "payments", "session.py", "Token", "/Users", "job-", "files", "release"]) {
    assert.equal(all.includes(leak), false, `the notification text leaked ${leak}: ${all}`);
  }
  // the tag only groups notifications by kind; it carries no job id
  assert.ok(t.shown.every((s) => /^kk-[a-z_]+$/.test(s.tag)), JSON.stringify(t.shown.map((s) => s.tag)));
});

test("n08 only the kinds that need the owner notify", async () => {
  const w = world();
  const t = tab(w);
  t.n.setEnabled(true);
  await t.n.observe([]);
  const n = await t.n.observe([item("suggestions", "job-1", 1), item("ready_to_deliver", "job-2", 2), item("approval_waiting", "job-3", 3)]);
  assert.equal(n, 1);
  assert.equal(t.shown[0].body, "Something is waiting for your approval.");
});

test("n09 a failure in showing, in locks or in storage never breaks the page and never repeats", async () => {
  const t = tab(world(), { showThrows: true });
  t.n.setEnabled(true);
  await t.n.observe([]);
  assert.equal(await t.n.observe([item("failed", "job-1", 1)]), 0);
  // locks that throw (an iframe, an odd origin) fall back to the stored record: still announced, still once
  const brokenLocks = tab(world(), { locks: { request: async () => { throw new Error("locks broke"); } } });
  brokenLocks.n.setEnabled(true);
  await brokenLocks.n.observe([]);
  assert.equal(await brokenLocks.n.observe([item("failed", "job-1", 1)]), 1);
  assert.equal(await brokenLocks.n.observe([item("failed", "job-1", 1)]), 0);
  // no Web Locks at all (an old browser): the stored record still prevents repeats, and it still notifies
  const w = world();
  const plain = tab(w, { locks: undefined });
  plain.n.setEnabled(true);
  await plain.n.observe([]);
  assert.equal(await plain.n.observe([item("failed", "job-1", 1)]), 1);
  assert.equal(await plain.n.observe([item("failed", "job-1", 1)]), 0);
  // storage that refuses writes or reads
  const hostileStorage = { getItem() { throw new Error("blocked"); }, setItem() { throw new Error("blocked"); } };
  const noStore = tab(world(), { storage: hostileStorage });
  noStore.n.setEnabled(true);
  await noStore.n.observe([]);
  assert.equal(await noStore.n.observe([item("failed", "job-1", 1)]), 1, "in-memory record still works");
  assert.equal(await noStore.n.observe([item("failed", "job-1", 1)]), 0);
});

test("n10 turning it off stops it at once, and the choice and the record are stored under their own keys", async () => {
  const w = world();
  const t = tab(w);
  t.n.setEnabled(true);
  assert.equal(w.storage.raw.get(NOTIFY_KEY), "1");
  await t.n.observe([]);
  t.n.setEnabled(false);
  assert.equal(await t.n.observe([item("failed", "job-1", 1)]), 0);
  assert.equal(w.storage.raw.has(NOTIFY_KEY) ? w.storage.raw.get(NOTIFY_KEY) : null, "0");
  t.n.setEnabled(true);
  assert.equal(await t.n.observe([item("failed", "job-1", 1)]), 0, "seen while it was off; not announced when it comes back on");
});

test("n11 the stored record of announced events stays bounded and is never trusted blindly", async () => {
  const w = world();
  w.storage.setItem(CLAIMED_KEY, "not json");
  const t = tab(w);
  t.n.setEnabled(true);
  await t.n.observe([]);
  let announced = 0;
  for (let i = 0; i < MAX_CLAIMED + 50; i++) announced += await t.n.observe([item("failed", `job-${i}`, i)]);
  assert.equal(announced, MAX_CLAIMED + 50);
  const stored = JSON.parse(w.storage.getItem(CLAIMED_KEY));
  assert.equal(stored.length, MAX_CLAIMED);
  assert.ok(stored.every((s) => typeof s === "string"));
  w.storage.setItem(CLAIMED_KEY, JSON.stringify([1, null, { a: 1 }, "ok"]));
  const t2 = tab(w);
  t2.n.setEnabled(true);
  await t2.n.observe([]);
  assert.equal(await t2.n.observe([item("failed", "job-new", 1)]), 1);
});

test("n12 a flood of new items cannot flood the system: at most three are announced at once", async () => {
  const w = world();
  const t = tab(w);
  t.n.setEnabled(true);
  await t.n.observe([]);
  const burst = Array.from({ length: 12 }, (_, i) => item("approval_waiting", `job-${i}`, i + 1));
  assert.equal(await t.n.observe(burst), 3);
  assert.equal(t.shown.length, 3);
  assert.equal(await t.n.observe(burst), 0, "the rest were seen and are not announced later");
});

test("n13 without Web Locks a focused window is still never interrupted", async () => {
  const w = world();
  const t = tab(w, { locks: undefined });
  t.n.setEnabled(true);
  await t.n.observe([]);
  await t.focus(true);
  assert.equal(await t.n.observe([item("failed", "job-1", 1)]), 0);
  assert.deepEqual(t.shown, []);
});

test("n14 two tabs racing on one event show it once even when both find nobody looking", async () => {
  // the focus lock is always free here (both tabs think nobody is looking); only the per-event lock can stop a double notice
  const base = fakeLocks();
  const locks = {
    request(name, a, b) {
      if (name === "kk:notify-focus" && typeof a === "object" && a.ifAvailable) return Promise.resolve(b({ name }));
      return base.request(name, a, b);
    },
  };
  const w = { locks, storage: fakeStorage() };
  const a = tab(w);
  const b = tab(w);
  for (const t of [a, b]) {
    t.n.setEnabled(true);
    await t.n.observe([]);
  }
  const items = [item("approval_waiting", "job-9", 9)];
  const [x, y] = await Promise.all([a.n.observe(items), b.n.observe(items)]);
  assert.equal(x + y, 1);
  assert.equal(a.shown.length + b.shown.length, 1);
});

test("n14 an expert's question on a team run is announced once per question, and a plan to start is not", async () => {
  const w = world();
  const t = tab(w);
  t.n.setEnabled(true);
  await t.n.observe([]);
  const q = (task, since) => ({ kind: "team_question", team: "team-0123456789ab", task, since });
  assert.equal(await t.n.observe([{ kind: "team_planned", team: "team-0123456789ab", since: 1 }, { kind: "team_interrupted", team: "team-aaaaaaaaaaaa", since: 1 }]), 0);
  assert.equal(await t.n.observe([q(1, 5)]), 1);
  assert.equal(await t.n.observe([q(1, 5)]), 0, "the same question is not repeated");
  // two tasks of one run asking at the same moment are two questions
  assert.equal(await t.n.observe([q(1, 5), q(2, 5)]), 1);
  assert.equal(t.shown.length, 2);
  assert.ok(!/team-0123456789ab/.test(JSON.stringify(t.shown)), "nothing about the work is in the notification");
});
