let seed = 20260101;
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
// check: does JSON.parse(JSON.stringify(v)) round-trip every key exactly?
let bad = 0;
const keysOf = (x, out) => { if (x && typeof x === "object") for (const k of Object.keys(x)) { out.push(k); keysOf(x[k], out); } return out; };
for (let n = 0; n < 3000; n++) {
  const v = gen();
  const back = JSON.parse(JSON.stringify(v));
  const a = keysOf(v, []).sort().join("\u0001"), b = keysOf(back, []).sort().join("\u0001");
  if (a !== b) { bad++; if (bad === 1) console.log("first round-trip key mismatch at doc", n); }
}
console.log(process.version, "round-trip key mismatches:", bad);
seed = 20260101;
for (let n = 0; n < 1073; n++) {
  const v = gen();
  if (n === 1072) {
    const find = (x, tag) => { if (x && typeof x === "object") for (const [k, val] of Object.entries(x)) { if (val === 771713056) console.log(tag, "key cps", [...k].map(c => c.codePointAt(0))); find(val, tag); } };
    find(v, "original");
    const t = JSON.stringify(v);
    const i = t.indexOf("771713056");
    console.log("text key chars:", [...t.slice(i - 6, i)].map(c => c.codePointAt(0)));
    find(JSON.parse(t), "parsed");
    console.log("count of 771713056 in text:", t.split("771713056").length - 1);
  }
}
