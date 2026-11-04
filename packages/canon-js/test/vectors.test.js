import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { canonicalize, digestHex, CanonError } from "../src/index.js";

const vectors = JSON.parse(readFileSync(new URL("../../../fixtures/canon/vectors.json", import.meta.url), "utf8"));

for (const v of vectors.accept) {
  test("accept: " + v.name, async () => {
    assert.equal(canonicalize(v.input), v.canonical);
    assert.equal(await digestHex(v.input), v.sha256);
  });
}

for (const v of vectors.reject) {
  test("reject: " + v.name, () => {
    const input = v.input_hex ? Uint8Array.from(Buffer.from(v.input_hex, "hex")) : v.input;
    assert.throws(
      () => canonicalize(input),
      (e) => e instanceof CanonError && e.code === v.code,
    );
  });
}
