import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { verifyShare, b64urlDecode } from "../src/index.js";
import { parseStrict } from "@kakurega/canon";

const doc = parseStrict(readFileSync(new URL("../../../fixtures/share/vectors.json", import.meta.url)));
const vectors = doc.get("vectors");

test("the share vector file has the expected breadth", () => {
  assert.ok(vectors.length >= 55, `only ${vectors.length} vectors`);
});

for (const v of vectors) {
  const name = v.get("name");
  test(`share vector: ${name}`, async () => {
    const pinned = v.get("pinned");
    const result = await verifyShare(b64urlDecode(v.get("share")), { pinned: pinned === null ? null : pinned.map((k) => b64urlDecode(k)) });
    assert.equal(result.valid, v.get("valid"), `${name}: ${JSON.stringify({ ...result, files: undefined })}`);
    assert.equal(result.reason, v.get("reason"), name);
    assert.deepEqual(result.files.map((f) => f.name), v.get("files"), name);
    if (v.get("valid")) {
      assert.equal(result.signer, v.get("signer"));
      assert.equal(result.totalFiles, Number(v.get("total_files")));
    }
  });
}
