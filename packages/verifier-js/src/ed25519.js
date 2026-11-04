const P = (1n << 255n) - 19n;
const D = (((-121665n % P) + P) % P) * modInv(121666n) % P;
const SQRT_M1 = modPow(2n, (P - 1n) / 4n, P);

function mod(a) {
  const r = a % P;
  return r >= 0n ? r : r + P;
}

function modPow(base, exp, m) {
  let result = 1n;
  let b = ((base % m) + m) % m;
  let e = exp;
  while (e > 0n) {
    if (e & 1n) result = (result * b) % m;
    b = (b * b) % m;
    e >>= 1n;
  }
  return result;
}

function modInv(a) {
  return modPow(a, P - 2n, P);
}

function decompress(bytes) {
  if (bytes.length !== 32) return null;
  let y = 0n;
  for (let i = 31; i >= 0; i--) y = (y << 8n) | BigInt(i === 31 ? bytes[i] & 0x7f : bytes[i]);
  const sign = (bytes[31] >> 7) & 1;
  y = mod(y);
  const y2 = mod(y * y);
  const u = mod(y2 - 1n);
  const v = mod(D * y2 + 1n);
  const v3 = mod(v * v * v);
  const v7 = mod(v3 * v3 * v);
  let x = mod(u * v3 * modPow(mod(u * v7), (P - 5n) / 8n, P));
  const vx2 = mod(v * x * x);
  if (vx2 !== u) {
    if (vx2 === mod(-u)) x = mod(x * SQRT_M1);
    else return null;
  }
  if (x === 0n && sign === 1) return null;
  if (Number(x & 1n) !== sign) x = mod(-x);
  return [x, y];
}

function add([x1, y1], [x2, y2]) {
  const dxy = mod(D * x1 * x2 * y1 * y2);
  const x3 = mod((x1 * y2 + x2 * y1) * modInv(mod(1n + dxy)));
  const y3 = mod((y1 * y2 + x1 * x2) * modInv(mod(1n - dxy)));
  return [x3, y3];
}

export function classifyPublicKey(bytes) {
  const p = decompress(bytes);
  if (p === null) return "invalid";
  let q = p;
  for (let i = 0; i < 3; i++) q = add(q, q);
  return q[0] === 0n && q[1] === 1n ? "small_order" : "ok";
}

export async function verifySignature(publicKey, message, signature) {
  const key = await crypto.subtle.importKey("raw", publicKey, { name: "Ed25519" }, false, ["verify"]);
  return crypto.subtle.verify("Ed25519", key, signature, message);
}
