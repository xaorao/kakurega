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
  const t = describe(sample);
  for (const s of ["acme-audit", "abcdef0123456789ffff", "finding-1.txt", "secret.env (secret)", "model quality"]) assert.ok(t.includes(s), s);
});
