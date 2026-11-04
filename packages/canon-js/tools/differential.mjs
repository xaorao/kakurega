import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import canonicalizeNpm from "canonicalize";
import { canonicalize, CanonError } from "../src/index.js";

let seed = Number(process.env.SEED || 20260101);
const rnd = () => { seed = (seed * 1664525 + 1013904223) >>> 0; return seed / 2 ** 32; };
const pick = (a) => a[Math.floor(rnd() * a.length)];
const chars = ["a", "b", "Z", "é", "\u0000", "\u001f", "\u007f", '"', "\\", "/", "\n", "\t", "", "￿", "😀", "\u{10000}", "́", " ", "ß"];
const str = () => Array.from({ length: Math.floor(rnd() * 6) }, () => pick(chars)).join("");
const num = () => pick([0, 1, -1, 7, 255, 65536, -123456, 9007199254740991, -9007199254740991, Math.floor(rnd() * 1e9)]);
function gen(d = 0) {
  const r = rnd();
  if (d > 4 || r < 0.35) return pick([null, true, false, num(), str(), num(), str()]);
  if (r < 0.6) return Array.from({ length: Math.floor(rnd() * 4) }, () => gen(d + 1));
  const o = {};
  for (let k = 0; k < Math.floor(rnd() * 5); k++) o[str()] = gen(d + 1);
  return o;
}

const N = Number(process.env.N || 3000);
const values = Array.from({ length: N }, () => gen());
const cases = values.map((v) => JSON.stringify(v));
const proc = spawnSync("cargo", ["run", "-q", "-p", "kk-evidence", "--example", "canon_lines"], {
  input: cases.join("\n") + "\n", encoding: "utf8", maxBuffer: 1 << 28,
});
if (proc.status !== 0) { console.error(proc.stderr); process.exit(2); }
const lines = proc.stdout.split("\n").filter(Boolean);
let bad = 0;
cases.forEach((text, k) => {
  const [tag, hash, canon] = lines[k].split("\t");
  const js = canonicalize(text);
  const npm = canonicalizeNpm(values[k]);
  const jsHash = createHash("sha256").update(js, "utf8").digest("hex");
  if (tag !== "OK" || canon !== js || js !== npm || hash !== jsHash) {
    if (bad++ < 3) { let x = 0; while (js[x] === npm[x]) x++; console.error("MISMATCH at", x, JSON.stringify(js.slice(Math.max(0,x-20), x+25)), JSON.stringify(npm.slice(Math.max(0,x-20), x+25)), "rust==js", canon === js); }
  }
});
console.log(`differential: ${N} random documents, rust == js == npm canonicalize: ${bad === 0 ? "all identical" : bad + " mismatches"}`);
process.exit(bad === 0 ? 0 : 1);
