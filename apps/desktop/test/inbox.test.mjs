import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { ANNOUNCED, MAX_PER_UPDATE, clean, createNotifier, fromJobs, keyOf, readInbox, targetFor } from "../src/inbox.mjs";
import { STRINGS, translator } from "../src/strings.mjs";

const web = join(dirname(fileURLToPath(import.meta.url)), "../../web/src");

function harness({ focus = false, on = true } = {}) {
  const shown = [];
  const env = { focus, on };
  const n = createNotifier({ show: (title, body, target, kind) => shown.push({ title, body, target, kind }), hasFocus: () => env.focus, enabled: () => env.on, text: translator("en") });
  return { n, shown, env };
}

const review = (kind, job, since = 100) => ({ kind, job, since });

test("in01 what was already waiting when the shell started is not announced, only what arrives after", () => {
  const { n, shown } = harness();
  assert.equal(n.observe([review("ready_to_review", "job-1")]), 0);
  assert.equal(n.observe([review("ready_to_review", "job-1"), review("ready_to_review", "job-2")]), 1);
  assert.deepEqual(shown.map((s) => s.target), ["#/job/job-2"]);
});

test("in02 each item is announced once however often the inbox is read", () => {
  const { n, shown } = harness();
  n.observe([]);
  for (let i = 0; i < 5; i++) n.observe([review("approval_waiting", "job-3")]);
  assert.equal(shown.length, 1);
});

test("in03 at most three at a time, and the rest are not saved up for later", () => {
  const { n, shown } = harness();
  n.observe([]);
  const many = ["a", "b", "c", "d", "e"].map((j) => review("ready_to_review", `job-${j}`));
  assert.equal(n.observe(many), MAX_PER_UPDATE);
  assert.equal(n.observe(many), 0);
  assert.equal(shown.length, 3);
});

test("in04 nothing is shown while the window is in front, and an item seen then is not shown later either", () => {
  const { n, shown, env } = harness({ focus: true });
  n.observe([]);
  assert.equal(n.observe([review("ready_to_review", "job-4")]), 0);
  env.focus = false;
  assert.equal(n.observe([review("ready_to_review", "job-4")]), 0, "the owner was looking when it arrived");
  assert.equal(n.observe([review("ready_to_review", "job-4"), review("failed", "job-5")]), 1);
  assert.deepEqual(shown.map((s) => s.kind), ["failed"]);
});

test("in05 only the four kinds that need the owner are announced", () => {
  const { n, shown } = harness();
  n.observe([]);
  const quiet = ["ready_to_deliver", "suggestions", "team_planned", "team_interrupted"].map((k, i) => review(k, `job-q${i}`));
  n.observe([...quiet, review("approval_waiting", "job-a"), review("ready_to_review", "job-r")]);
  n.observe([review("failed", "job-f"), { kind: "team_question", team: "team-1", task: 2, since: 5 }]);
  assert.deepEqual(shown.map((s) => s.kind).sort(), [...ANNOUNCED].sort());
  assert.equal(shown.length, 4);
});

test("in06 turned off means nothing is shown, and what arrived meanwhile is not announced when it is turned back on", () => {
  const { n, shown, env } = harness({ on: false });
  n.observe([]);
  n.observe([review("ready_to_review", "job-6")]);
  env.on = true;
  n.observe([review("ready_to_review", "job-6")]);
  assert.equal(shown.length, 0);
});

test("in07 hostile: the text of a notification never carries the recipient, the project, a team's goal, an expert's question or an id", () => {
  const { n, shown } = harness();
  n.observe([]);
  const secret = "ACME-Merger-Plan-7731";
  const raw = [
    { kind: "ready_to_review", job: "job-77", recipient: secret, since: 1, what: `about ${secret}` },
    { kind: "team_question", team: "team-77", task: 1, title: secret, since: 2, what: `Lawyer asks: is ${secret} legal?` },
    { kind: "approval_waiting", job: "job-78", recipient: secret, since: 3, what: secret },
  ];
  n.observe(raw.map(clean));
  assert.equal(shown.length, 3);
  for (const s of shown) {
    const words = `${s.title} ${s.body}`;
    assert.ok(!words.includes(secret) && !/job-7|team-7/.test(words), words);
    assert.equal(s.title, "Kakurega");
  }
  for (const item of raw.map(clean)) for (const v of Object.values(item)) assert.ok(!String(v).includes(secret), "clean() keeps only kinds and ids");
});

test("in08 the words are the web app's, in both languages", () => {
  for (const lang of ["en", "hi"]) {
    const src = readFileSync(join(web, `locales/${lang}.ts`), "utf8");
    for (const kind of ANNOUNCED) {
      const m = new RegExp(`"notify\\.${kind}": "([^"]*)"`).exec(src);
      assert.ok(m, `${lang} notify.${kind} exists in the web app`);
      assert.equal(STRINGS[lang][`notify.${kind}`], m[1], `${lang} ${kind}`);
    }
  }
  const notify = readFileSync(join(web, "notify.ts"), "utf8");
  const announced = /const ANNOUNCED[^=]*= new Set\(\[([^\]]*)\]\)/.exec(notify)[1].match(/"[a-z_]+"/g).map((s) => s.slice(1, -1));
  assert.deepEqual([...ANNOUNCED].sort(), announced.sort(), "the same kinds are announced");
  assert.equal(Number(/export const MAX_PER_UPDATE = (\d+);/.exec(notify)[1]), MAX_PER_UPDATE);
});

test("in09 the key that makes an item 'the same item' is the web app's", () => {
  assert.equal(keyOf({ kind: "ready_to_review", job: "job-1", since: 5 }), "ready_to_review:job-1:5");
  assert.equal(keyOf({ kind: "team_question", team: "t1", task: 3, since: 9 }), "team_question:t1#3:9");
  const notify = readFileSync(join(web, "notify.ts"), "utf8");
  assert.ok(notify.includes("`${i.kind}:${i.job ?? `${i.team ?? \"\"}#${i.task ?? 0}`}:${i.since}`"));
});

test("in10 a click opens the item's own screen; a hostile id opens the inbox instead of a crafted address", () => {
  assert.equal(targetFor({ job: "job-12" }), "#/job/job-12");
  assert.equal(targetFor({ team: "team-3", task: 1 }), "#/team/team-3");
  for (const bad of ["../admin", "job-1/../../x", "a b", "", "x".repeat(65), "job?x=1", "javascript:1"]) {
    assert.equal(targetFor({ job: bad }), "#/inbox", bad);
    assert.equal(clean({ kind: "failed", job: bad, since: 1 }), null, bad);
  }
  assert.equal(clean({ kind: "not_a_kind", job: "job-1" }), null);
  assert.equal(clean(null), null);
});

test("in11 the reviews' part of the inbox is worked out from the admin view by the service's own rules", () => {
  const jobs = [
    { id: "job-1", state: "ready", approval: null, delivered: null },
    { id: "job-2", state: "ready", approval: "waiting", delivered: null },
    { id: "job-3", state: "ready", approval: "approved", delivered: null },
    { id: "job-4", state: "ready", approval: "denied", delivered: null },
    { id: "job-5", state: "ready", approval: "approved", delivered: { digest: "d" } },
    { id: "job-6", state: "failed", note: "the worker crashed" },
    { id: "job-7", state: "failed", note: "stopped by you" },
    { id: "job-8", state: "running" },
    { id: "job-9", state: "nothing_to_release" },
    { id: "../x", state: "failed" },
  ];
  assert.deepEqual(fromJobs(jobs).map((i) => `${i.kind}:${i.job}`), ["ready_to_review:job-1", "approval_waiting:job-2", "ready_to_deliver:job-3", "ready_to_review:job-4", "failed:job-6"]);
  const review = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../../../crates/kk-review/src/job.rs"), "utf8");
  assert.ok(review.includes('pub const STOPPED_BY_OWNER: &str = "stopped by you";'), "the service still words a stopped review this way");
});

test("in12 the inbox route is used when the service has it, the reviews are used when it answers 404, and anything else is an error", async () => {
  const client = (routes) => ({ get: async (p) => routes[p] ?? { status: 500, json: null } });
  const full = await readInbox(client({ "/admin/inbox": { status: 200, json: { items: [{ kind: "team_question", team: "t1", task: 1, since: 3, what: "secret" }], count: 1 } } }));
  assert.deepEqual(full, { items: [{ kind: "team_question", team: "t1", task: 1, since: 3 }], complete: true });
  const partial = await readInbox(client({ "/admin/inbox": { status: 404, json: null }, "/admin/jobs": { status: 200, json: { jobs: [{ id: "job-1", state: "ready" }] } } }));
  assert.deepEqual(partial, { items: [{ kind: "ready_to_review", job: "job-1", since: 0 }], complete: false });
  await assert.rejects(readInbox(client({ "/admin/inbox": { status: 401, json: null } })));
  await assert.rejects(readInbox(client({ "/admin/inbox": { status: 404, json: null }, "/admin/jobs": { status: 401, json: null } })));
});

test("in13 a show that throws does not stop the others or break the update", () => {
  let calls = 0;
  const n = createNotifier({ show: () => { calls++; if (calls === 1) throw new Error("no notification centre"); }, hasFocus: () => false, enabled: () => true, text: translator("en") });
  n.observe([]);
  assert.equal(n.observe([review("failed", "job-a"), review("failed", "job-b")]), 1);
  assert.equal(n.observe("not a list"), 0);
});
