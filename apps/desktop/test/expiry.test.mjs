import test from "node:test";
import assert from "node:assert/strict";
import { createApprover, describe } from "../src/approval.mjs";
import { SOON_SECS, deadlineLines, formatSpan, timeLeft } from "../src/expiry.mjs";

const NOW = 1_800_000_000;

test("ex01 time left: valid, soon, the deadline itself and anything past it are told apart, and unreadable is never valid", () => {
  assert.deepEqual(timeLeft(NOW + 86_400, NOW), { state: "valid", secs: 86_400 });
  assert.deepEqual(timeLeft(NOW + SOON_SECS + 1, NOW), { state: "valid", secs: SOON_SECS + 1 });
  assert.deepEqual(timeLeft(NOW + SOON_SECS, NOW), { state: "soon", secs: SOON_SECS });
  assert.deepEqual(timeLeft(NOW + 1, NOW), { state: "soon", secs: 1 });
  assert.deepEqual(timeLeft(NOW, NOW), { state: "expired" }, "the deadline itself counts as expired, as in the service");
  assert.deepEqual(timeLeft(NOW - 1, NOW), { state: "expired" });
  assert.deepEqual(timeLeft(NOW + 0.9, NOW), { state: "expired" }, "less than a whole second left is none");
  for (const bad of [undefined, null, "9999999999", NaN, Infinity, -Infinity, {}, [], true]) {
    assert.deepEqual(timeLeft(bad, NOW), { state: "unknown" }, String(bad));
    assert.deepEqual(timeLeft(NOW + 100, bad), { state: "unknown" }, `now ${String(bad)}`);
  }
});

test("ex02 spans are written with the two largest units and never more", () => {
  for (const [secs, text] of [[0, "0 s"], [40, "40 s"], [60, "1 min"], [252, "4 min 12 s"], [3600, "1 h"], [7500, "2 h 5 min"], [86_340, "23 h 59 min"], [-5, "0 s"], [59.9, "59 s"]]) {
    assert.equal(formatSpan(secs), text, String(secs));
  }
});

test("ex03 the approval window says how long is left, warns near the end, and says plainly when it is over or unreadable", () => {
  assert.deepEqual(deadlineLines({ expires: NOW + 86_340, server_time: NOW }), ["This release can be approved for another 23 h 59 min."]);
  assert.deepEqual(deadlineLines({ expires: NOW + 125, server_time: NOW }), ["This release can be approved for another 2 min 5 s.", "! Only a few minutes are left."]);
  assert.deepEqual(deadlineLines({ expires: NOW, server_time: NOW }), ["This release has expired and can no longer be approved."]);
  for (const a of [{}, null, undefined, { expires: NOW + 100 }, { server_time: NOW }, { expires: "soon", server_time: NOW }]) {
    const lines = deadlineLines(a);
    assert.equal(lines.length, 1);
    assert.ok(/could not be read/.test(lines[0]) && !/can be approved for another/.test(lines[0]), JSON.stringify(a));
  }
});

const sample = { id: "ap1", recipient: "acme-audit", digest: "abcdef0123456789ffff", files: [{ name: "finding-1.txt", bytes: 10, sha256: "a".repeat(64) }], removed: [], not_established: [] };

test("ex04 the text of the approval window puts the deadline straight after the digest, before the files", () => {
  const t = describe({ ...sample, expires: NOW + 7500, server_time: NOW });
  const lines = t.split("\n");
  const at = lines.findIndex((l) => l.startsWith("Digest:"));
  assert.equal(lines[at + 1], "This release can be approved for another 2 h 5 min.");
  assert.ok(t.indexOf("another 2 h 5 min") < t.indexOf("finding-1.txt"));
});

function fakeClient(approvals, calls) {
  return {
    async get(path) {
      if (path === "/admin/approvals") return { status: 200, json: { approvals: approvals.map(({ id }) => ({ id })) } };
      const a = approvals.find((x) => path.endsWith("/" + x.id));
      return a ? { status: 200, json: a } : { status: 404, json: null };
    },
    async post(path, body) {
      calls.push([path, body]);
      return { status: 200, json: {} };
    },
  };
}

test("ex05 an approval that is already expired is denied without showing any window, and one with time left is shown", async () => {
  for (const [offset, shown, outcome] of [[-1, false, "expired"], [0, false, "expired"], [-86_400, false, "expired"], [1, true, "approved"], [600, true, "approved"]]) {
    const calls = [];
    let asked = 0;
    const poll = createApprover({ client: fakeClient([{ ...sample, expires: NOW + offset, server_time: NOW }], calls), confirm: async () => (asked++, true), authenticate: async () => true });
    const done = await poll();
    assert.equal(done[0].outcome, outcome, `offset ${offset}`);
    assert.equal(asked, shown ? 1 : 0, `offset ${offset}`);
    if (!shown) assert.deepEqual(calls, [["/admin/approvals/ap1/deny", undefined]], "it leaves the waiting list and nothing is approved");
  }
});

test("ex06 an unreadable deadline still asks the owner, because the service remains the one that refuses a late approval", async () => {
  for (const extra of [{}, { expires: "tomorrow", server_time: NOW }, { expires: NOW + 100 }, { expires: NaN, server_time: NOW }]) {
    const calls = [];
    let asked = 0;
    const poll = createApprover({ client: fakeClient([{ ...sample, ...extra }], calls), confirm: async () => (asked++, true), authenticate: async () => true });
    assert.equal((await poll())[0].outcome, "approved");
    assert.equal(asked, 1);
  }
});

test("ex07 with a signing key an expired approval is denied before anything is signed", async () => {
  const calls = [];
  let signed = 0;
  const poll = createApprover({
    client: fakeClient([{ ...sample, requires_proof: true, expires: NOW - 5, server_time: NOW }], calls),
    confirm: async () => {
      throw new Error("no window for an expired approval");
    },
    authenticate: async () => true,
    sign: Object.assign(async () => (signed++, "proof"), { ownsDialog: true }),
  });
  assert.equal((await poll())[0].outcome, "expired");
  assert.equal(signed, 0);
  assert.deepEqual(calls.map((c) => c[0]), ["/admin/approvals/ap1/deny"]);
});

test("ex08 'a few minutes left' means ten minutes here and in the web app, and the two word their countdown the same way", async () => {
  const { readFileSync } = await import("node:fs");
  const { dirname, join } = await import("node:path");
  const { fileURLToPath } = await import("node:url");
  assert.equal(SOON_SECS, 600);
  const web = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../../web/src/expiry.ts"), "utf8");
  assert.equal(Number(/export const SOON_SECS = (\d+);/.exec(web)?.[1]), SOON_SECS, "the web app uses the same limit");
  const en = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "../../web/src/locales/en.ts"), "utf8");
  assert.ok(en.includes("\"appr.validFor\": \"This release can be approved for another {time}.\""), "the web app words the countdown the same way");
  assert.equal(deadlineLines({ expires: NOW + 7200, server_time: NOW })[0], "This release can be approved for another 2 h.");
});
