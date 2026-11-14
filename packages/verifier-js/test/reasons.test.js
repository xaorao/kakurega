import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

test("every reason the verifier can give has words on the check page", () => {
  const src = readFileSync(new URL("../src/index.js", import.meta.url), "utf8");
  const page = readFileSync(new URL("../../../apps/verifier-page/main.js", import.meta.url), "utf8");
  const given = new Set([...src.matchAll(/stop\("([a-z_]+)"/g), ...src.matchAll(/[?:] "([a-z]+(?:_[a-z]+)+)"\)/g)].map((m) => m[1]));
  const worded = new Set([...page.matchAll(/^  ([a-z_]+): "/gm)].map((m) => m[1]));
  assert.ok(given.size > 30, "the reasons were found in the verifier");
  assert.deepEqual([...given].filter((r) => !worded.has(r)), []);
});
