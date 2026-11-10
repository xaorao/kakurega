import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { verifyCapsule, b64urlDecode } from "../src/index.js";
import { parseStrict } from "@kakurega/canon";

const doc = parseStrict(readFileSync(new URL("../../../fixtures/capsule/vectors.json", import.meta.url)));
const vectors = doc.get("vectors");

test("the vector file has the expected breadth", () => {
  assert.ok(vectors.length >= 45, `only ${vectors.length} vectors`);
});

for (const v of vectors) {
  const name = v.get("name");
  test(`capsule vector: ${name}`, async () => {
    const opts = v.get("options");
    const pinned = opts.get("pinned");
    const result = await verifyCapsule(b64urlDecode(v.get("capsule")), {
      pinned: pinned === null ? null : pinned.map((k) => b64urlDecode(k)),
      now: Number(opts.get("now")),
      recipient: opts.get("recipient"),
    });
    assert.equal(result.valid, v.get("valid"), `${name}: ${JSON.stringify(result)}`);
    assert.equal(result.reason, v.get("reason"), name);
    if (v.get("valid")) assert.equal(result.signer, v.get("signer"));
    assert.equal((result.receipts ?? []).length, Number(v.get("receipts")), `${name}: receipts`);
  });
}
