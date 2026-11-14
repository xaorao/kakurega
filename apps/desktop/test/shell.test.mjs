import test from "node:test";
import assert from "node:assert/strict";
import { isAppUrl, allowPermission, mayOpenExternally, windowOptions } from "../src/policy.mjs";
import { createApprover, describe } from "../src/approval.mjs";

const origin = "http://127.0.0.1:5000";

test("ds01 only the companion origin is an app url; look-alikes are not", () => {
  assert.equal(isAppUrl(origin + "/x", origin), true);
  for (const u of ["http://127.0.0.1:5001/", "http://127.0.0.1.evil.com:5000/", "https://127.0.0.1:5000/", "http://evil.com/#http://127.0.0.1:5000", "javascript:alert(1)", "file:///etc/passwd", "not a url", ""])
    assert.equal(isAppUrl(u, origin), false, u);
});

test("ds02 external opening is https only and carries no credentials", () => {
  assert.equal(mayOpenExternally("https://docs.near.org/"), true);
  for (const u of ["http://a.com", "file:///x", "javascript:1", "https://user:pw@a.com", "ssh://a", "x"]) assert.equal(mayOpenExternally(u), false, u);
  assert.equal(allowPermission(), false);
});

test("ds03 the window is sandboxed with no node, no webview tag", () => {
  const p = windowOptions().webPreferences;
  assert.deepEqual([p.contextIsolation, p.nodeIntegration, p.sandbox, p.webviewTag, p.webSecurity], [true, false, true, false, true]);
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
const sample = { id: "ap1", recipient: "acme-audit", digest: "abcdef0123456789ffff", files: [{ name: "finding-1.txt", bytes: 10, sha256: "a".repeat(64) }], removed: [{ what: "secret.env", why: "secret" }], not_established: ["model quality"] };

test("ds04 approval needs the human yes AND the operating system proof; either missing denies", async () => {
  for (const [yes, proof, outcome, path] of [
    [true, true, "approved", "/admin/approvals/ap1/approve"],
    [false, true, "denied", "/admin/approvals/ap1/deny"],
    [true, false, "authentication_failed", "/admin/approvals/ap1/deny"],
  ]) {
    const calls = [];
    const poll = createApprover({ client: fakeClient([sample], calls), confirm: async () => yes, authenticate: async () => proof });
    const out = await poll();
    assert.equal(out[0].outcome, outcome);
    assert.equal(calls[0][0], path);
    if (outcome === "approved") assert.deepEqual(calls[0][1], { ack: "abcdef012345" });
  }
});

test("ds04b with approvals closed to anything but an enrolled key and none enrolled, the owner is told instead of asked", async () => {
  const calls = [];
  let asked = 0;
  const poll = createApprover({ client: fakeClient([{ ...sample, approver_missing: true }], calls), confirm: async () => (asked++, true), authenticate: async () => true });
  const out = await poll();
  assert.deepEqual(out.map((o) => o.outcome), ["needs_enrolled_key"]);
  assert.equal(asked, 0, "no confirmation is asked for something that cannot succeed");
  assert.equal(calls.length, 0, "nothing is posted: the approval stays waiting");
  assert.deepEqual(await poll(), [], "and it is reported once, not every second");
  // when nothing is missing the flag is false, and the ordinary journey runs
  const calls2 = [];
  const poll2 = createApprover({ client: fakeClient([{ ...sample, id: "ap2", approver_missing: false }], calls2), confirm: async () => true, authenticate: async () => true });
  assert.equal((await poll2())[0].outcome, "approved");
});

test("ds05 an approval is shown once, and a hostile id is never used in a path", async () => {
  const calls = [];
  let asked = 0;
  const client = fakeClient([sample, { ...sample, id: "../../admin/x" }], calls);
  const poll = createApprover({ client, confirm: async () => (asked++, false), authenticate: async () => true });
  assert.equal((await poll()).length, 1);
  assert.equal((await poll()).length, 0);
  assert.equal(asked, 1);
  assert.ok(calls.every(([p]) => !p.includes("..")));
});

test("ds06 the approval text lists exact files, withheld items and what is not established", () => {
  const t = describe({ ...sample, disclosure: { files_after: 5, project_files: 11, releases_before: 2, reasons: ["over the review threshold"], not_checked: ["combined excerpts"] } });
  for (const s of ["5 of 11 project files", "2 earlier releases", "over the review threshold", "combined excerpts"]) assert.ok(t.includes(s), s);
  for (const s of ["acme-audit", "abcdef0123456789ffff", "finding-1.txt", "secret.env (secret)", "model quality"]) assert.ok(t.includes(s), s);
});

test("ds-proof when the service requires a proof the shell hands the signer only the approval id, shows no dialog of its own, and refuses if it cannot sign", async () => {
  const calls = [];
  const a = { ...sample, requires_proof: true, statement_canonical: '{"approval":"ap1"}' };
  const signed = [];
  const poll = createApprover({ client: fakeClient([a], calls), confirm: async () => { throw new Error("the helper owns the dialog"); }, authenticate: async () => { throw new Error("must not prompt twice"); }, sign: Object.assign(async (...args) => (signed.push(args), "PROOF"), { ownsDialog: true }) });
  assert.equal((await poll())[0].outcome, "approved");
  assert.deepEqual(signed[0], ["ap1"], "no bytes, no recipient: only the id");
  assert.ok(calls.some((c) => c[0].endsWith("/approve") && c[1]?.proof === "PROOF"));
  const calls2 = [];
  const noSigner = createApprover({ client: fakeClient([{ ...a }], calls2), confirm: async () => true, authenticate: async () => true });
  assert.equal((await noSigner())[0].outcome, "signing_failed");
  assert.ok(!calls2.some((c) => c[0].endsWith("/approve")), "nothing is approved without a signature");
  const calls3 = [];
  const refuses = createApprover({ client: fakeClient([{ ...a }], calls3), confirm: async () => true, authenticate: async () => true, sign: async () => { throw new Error("denied"); } });
  assert.equal((await refuses())[0].outcome, "signing_failed");
  assert.ok(calls3.some((c) => c[0].endsWith("/deny")));
});

test("ds-sw a signer that does not own the dialog gets the shell's confirmation first, and a refusal signs nothing", async () => {
  const a = { ...sample, requires_proof: true };
  const asked = [];
  const signer = Object.assign(async (id) => (asked.push(id), "PROOF"), { ownsDialog: false });
  const callsYes = [];
  const yes = createApprover({ client: fakeClient([a], callsYes), confirm: async () => (asked.push("confirm"), true), authenticate: async () => true, sign: signer });
  assert.equal((await yes())[0].outcome, "approved");
  assert.deepEqual(asked, ["confirm", "ap1"], "dialog before signature");
  const callsNo = [];
  const asked2 = [];
  const no = createApprover({ client: fakeClient([{ ...a }], callsNo), confirm: async () => false, authenticate: async () => true, sign: Object.assign(async (id) => (asked2.push(id), "P"), { ownsDialog: false }) });
  assert.equal((await no())[0].outcome, "denied");
  assert.deepEqual(asked2, [], "nothing was signed after a refusal");
});

import { createRestartPolicy, rendererCrashed } from "../src/recovery.mjs";

test("ds10 a crashing part is restarted a few times with growing waits, then reported instead of looping", () => {
  const p = createRestartPolicy({ max: 3, windowMs: 1000, baseDelayMs: 100 });
  assert.deepEqual([p.next(0), p.next(10), p.next(20)].map((d) => d.delayMs), [100, 200, 400]);
  assert.equal(p.next(30).restart, false);
  assert.equal(p.next(900).restart, false, "still inside the window");
  assert.equal(p.next(1021).restart, true, "old crashes age out, so a part that was fine for a while gets another chance");
});

test("ds11 only a real crash counts: a window closing normally does not", () => {
  assert.equal(rendererCrashed({ reason: "clean-exit" }), false);
  for (const reason of ["crashed", "oom", "killed", "launch-failed", "integrity-failure", "abnormal-exit"]) assert.equal(rendererCrashed({ reason }), true, reason);
  assert.equal(rendererCrashed(undefined), false);
});

import { randomBytes } from "node:crypto";
import { newShellSecret, withShellSecret, SHELL_HEADER } from "../src/policy.mjs";

test("ds20 a shell secret is long, fresh each time and safe in a header", () => {
  const seen = new Set();
  for (let i = 0; i < 50; i++) {
    const s = newShellSecret(randomBytes);
    assert.match(s, /^[A-Za-z0-9_-]{43}$/);
    seen.add(s);
  }
  assert.equal(seen.size, 50);
});

test("ds21 the secret goes only to the companion's own origin and replaces anything a page set", () => {
  const secret = "s".repeat(43);
  const h = { Accept: "*/*" };
  assert.deepEqual(withShellSecret(origin + "/api/health", h, origin, secret), { Accept: "*/*", [SHELL_HEADER]: secret });
  for (const u of ["http://127.0.0.1:5001/", "http://127.0.0.1.evil.com:5000/", "https://127.0.0.1:5000/", "http://evil.com/#http://127.0.0.1:5000/", "http://127.0.0.1:5000@evil.com/", "not a url", ""]) {
    assert.equal(withShellSecret(u, h, origin, secret), h, u);
  }
  assert.equal(withShellSecret(origin + "/", h, origin, null), h, "no secret, nothing added");
  assert.equal(withShellSecret(origin + "/", h, origin, ""), h);
  const sneaky = withShellSecret(origin + "/", { "X-KK-Shell": "page-supplied", Accept: "*/*" }, origin, secret);
  assert.deepEqual(sneaky, { Accept: "*/*", [SHELL_HEADER]: secret }, "a page-supplied value never survives");
});

test("on a computer with no Touch ID the owner's answer in the native window approves; a failed or cancelled fingerprint still refuses", async () => {
  const none = [];
  const without = createApprover({ client: fakeClient([sample], none), confirm: async () => true, authenticate: async () => "unavailable" });
  assert.deepEqual((await without()).map((d) => d.outcome), ["approved"]);
  assert.ok(none.some((c) => /\/approve$/.test(c.path ?? c[1] ?? "")) || JSON.stringify(none).includes("/approve"));
  const failed = [];
  const cancelled = createApprover({ client: fakeClient([{ ...sample, id: "ap9" }], failed), confirm: async () => true, authenticate: async () => false });
  assert.deepEqual((await cancelled()).map((d) => d.outcome), ["authentication_failed"]);
  assert.ok(JSON.stringify(failed).includes("/deny") && !JSON.stringify(failed).includes("/approve"));
  // nothing but true or "unavailable" counts as proof
  for (const odd of [undefined, null, "yes", 1, {}]) {
    const calls = [];
    const poll = createApprover({ client: fakeClient([{ ...sample, id: "ap10" }], calls), confirm: async () => true, authenticate: async () => odd });
    assert.deepEqual((await poll()).map((d) => d.outcome), ["authentication_failed"], String(odd));
  }
});
