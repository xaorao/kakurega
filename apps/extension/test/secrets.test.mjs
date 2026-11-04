import test from "node:test";
import assert from "node:assert/strict";
import { findSecrets } from "../secrets.js";
import { captureSelection } from "../capture.js";

const join = (...p) => p.join("");

test("xs01 provider tokens are found with the same rules as the service", () => {
  assert.deepEqual(findSecrets(join("token ", "ghp", "_", "R7kQ2mZx9LpV4nB8cT1yH6wD3sF5jA0gUe2K")), ["github token"]);
  assert.deepEqual(findSecrets(join("AK", "IAIOSFODNN7ABCDEFG")), ["aws access key id"]);
  assert.deepEqual(findSecrets(join("-----BEGIN ", "RSA PRIVATE KEY-----\nMIIB")), ["private key block"]);
  assert.ok(findSecrets(join("eyJhbGciOiJIUzI1NiJ9", ".", "eyJzdWIiOiIxMjM0NTY3ODkwIn0", ".", "dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U")).includes("jwt"));
});

test("xs02 ordinary text and documented examples are not flagged", () => {
  for (const ok of ["Remember to call Dana about the contract on Friday.", "ghp is short for something", join("AK", "IAIOSFODNN7EXAMPLE"), "see https://example.com/ghp_short", "a_ghp_" + "x".repeat(40), ""]) {
    assert.deepEqual(findSecrets(ok), [], ok);
  }
});

test("xs03 a selection that looks like a secret is never sent, and the message does not repeat it", async () => {
  let sent = 0;
  const token = join("ghp", "_", "R7kQ2mZx9LpV4nB8cT1yH6wD3sF5jA0gUe2K");
  const deps = { fetch: async () => { sent++; return { status: 202, json: async () => ({}), text: async () => "{}" }; }, config: { port: 40000, token: "t" } };
  const r = await captureSelection({ selectionText: `my key ${token}`, pageUrl: "https://example.org/a" }, deps);
  assert.equal(r.ok, false);
  assert.equal(sent, 0, "no request was made");
  assert.ok(r.message.includes("github token") && !r.message.includes(token));
  const fine = await captureSelection({ selectionText: "a normal sentence", pageUrl: "https://example.org/a" }, deps);
  assert.equal(sent, 1);
  void fine;
});
