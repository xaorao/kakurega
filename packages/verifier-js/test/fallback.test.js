import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// this file runs in its own process: Web Crypto here behaves like a browser that has no Ed25519, so every vector is
// checked by the fallback
const realImport = crypto.subtle.importKey.bind(crypto.subtle);
let refused = 0;
crypto.subtle.importKey = (format, key, algorithm, ...rest) => {
  const name = typeof algorithm === "string" ? algorithm : algorithm?.name;
  if (name === "Ed25519") {
    refused += 1;
    return Promise.reject(new DOMException("Ed25519 is not supported", "NotSupportedError"));
  }
  return realImport(format, key, algorithm, ...rest);
};
const { verifyCapsule, verifyShare, b64urlDecode } = await import("../src/index.js");
const { parseStrict } = await import("@kakurega/canon");

const capsules = parseStrict(readFileSync(new URL("../../../fixtures/capsule/vectors.json", import.meta.url))).get("vectors");
for (const v of capsules) {
  test(`without Web Crypto Ed25519, capsule vector: ${v.get("name")}`, async () => {
    const opts = v.get("options");
    const pinned = opts.get("pinned");
    const result = await verifyCapsule(b64urlDecode(v.get("capsule")), {
      pinned: pinned === null ? null : pinned.map((k) => b64urlDecode(k)),
      now: Number(opts.get("now")),
      recipient: opts.get("recipient"),
    });
    assert.equal(result.valid, v.get("valid"), `${v.get("name")}: ${JSON.stringify(result)}`);
    assert.equal(result.reason, v.get("reason"));
  });
}

const shares = parseStrict(readFileSync(new URL("../../../fixtures/share/vectors.json", import.meta.url))).get("vectors");
for (const v of shares) {
  test(`without Web Crypto Ed25519, share vector: ${v.get("name")}`, async () => {
    const pinned = v.get("pinned");
    const result = await verifyShare(b64urlDecode(v.get("share")), { pinned: pinned === null ? null : pinned.map((k) => b64urlDecode(k)) });
    assert.equal(result.valid, v.get("valid"), v.get("name"));
    assert.equal(result.reason, v.get("reason"), v.get("name"));
  });
}

// signatures made by hand, so the fallback is held to the same rule as the Rust checker and OpenSSL on the cases where
// Ed25519 implementations differ
const { verifySignature } = await import("../src/ed25519.js");
const { Point } = await import("@noble/ed25519");
const { createHash, createPublicKey, verify: nodeVerify, randomBytes } = await import("node:crypto");
const L = 2n ** 252n + 27742317777372353535851937790883648493n;
const le = (b) => b.reduceRight((n, x) => (n << 8n) | BigInt(x), 0n);
const toLe = (n) => Uint8Array.from({ length: 32 }, (_, i) => Number((n >> BigInt(8 * i)) & 255n));
const scalar = () => (le(randomBytes(64)) % (L - 1n)) + 1n;
const challenge = (R, A, m) => le(createHash("sha512").update(R).update(A).update(m).digest()) % L;
const openssl = (A, m, sig) => nodeVerify(null, m, createPublicKey({ key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), A]), format: "der", type: "spki" }), sig);
const msg = new TextEncoder().encode("the release statement");
const a = scalar();
const A = Point.BASE.multiply(a).toBytes();
const sign = (Rpoint, r) => {
  const R = Rpoint.toBytes();
  const s = (r + challenge(R, A, msg) * a) % L;
  return Uint8Array.from([...R, ...toLe(s)]);
};
const r = scalar();
const good = sign(Point.BASE.multiply(r), r);
// a point of order 8 (from the list of small-order encodings libsodium refuses)
const T = Point.fromBytes(Buffer.from("c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a", "hex"), false);

test("a signature made by hand verifies, in the fallback and in OpenSSL", async () => {
  assert.equal(await verifySignature(A, msg, good), true);
  assert.equal(openssl(A, msg, good), true);
});
test("s plus the group order is refused, as OpenSSL refuses it", async () => {
  const s = le(good.subarray(32)) + L;
  const bad = Uint8Array.from([...good.subarray(0, 32), ...toLe(s)]);
  assert.equal(await verifySignature(A, msg, bad), false);
  assert.equal(openssl(A, msg, bad), false);
});
test("a nonce point with a small-order part added verifies only under the cofactored rule: refused, as OpenSSL refuses it", async () => {
  const mixed = sign(Point.BASE.multiply(r).add(T), r);
  assert.equal(await verifySignature(A, msg, mixed), false);
  assert.equal(openssl(A, msg, mixed), false);
});
test("a small-order nonce point is refused", async () => {
  const bad = Uint8Array.from([...T.toBytes(), ...good.subarray(32)]);
  assert.equal(await verifySignature(A, msg, bad), false);
});
test("a changed message is refused", async () => {
  assert.equal(await verifySignature(A, new TextEncoder().encode("another statement"), good), false);
});

test("the browser's Ed25519 was asked once, and every signature after it went to the fallback", () => {
  assert.equal(refused, 1);
});
