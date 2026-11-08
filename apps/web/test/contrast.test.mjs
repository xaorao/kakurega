import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const css = readFileSync(new URL("../src/styles.css", import.meta.url), "utf8");

const lum = (hex) => {
  const v = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2];
};
const ratio = (a, b) => {
  const [x, y] = [lum(a), lum(b)].sort((m, n) => n - m);
  return (x + 0.05) / (y + 0.05);
};

/** Every block that sets --redline, with the text colour a danger button gets there: the block's own --on-redline, else the one the page starts with. */
function redlineBlocks() {
  const base = css.match(/--on-redline:\s*(#[0-9a-fA-F]{6})/)?.[1];
  assert.ok(base, "the page defines a default --on-redline");
  const out = [];
  for (const part of css.split("}")) {
    const red = part.match(/--redline:\s*(#[0-9a-fA-F]{6})/)?.[1];
    if (!red) continue;
    const own = part.match(/--on-redline:\s*(#[0-9a-fA-F]{6})/)?.[1];
    out.push({ red, on: own ?? base, selector: part.split("{")[0].trim().replace(/\s+/g, " ").slice(-60) });
  }
  return out;
}

test("ct01 the text on a danger button reads at 4.5 to 1 or better in every theme block", () => {
  const blocks = redlineBlocks();
  assert.ok(blocks.length >= 6, `light, dark (two spellings) and the more-contrast variants: ${blocks.length}`);
  for (const b of blocks) {
    const r = ratio(b.red, b.on);
    assert.ok(r >= 4.5, `${b.selector}: ${b.on} on ${b.red} is ${r.toFixed(2)} to 1`);
  }
});

test("ct02 the danger button takes its text colour from the token, not a fixed white", () => {
  assert.match(css, /button\.danger\s*\{[^}]*color:\s*var\(--on-redline\)/);
  assert.doesNotMatch(css, /button\.danger\s*\{[^}]*color:\s*#fff/);
});
