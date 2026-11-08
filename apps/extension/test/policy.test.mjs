import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { MAX_SELECTION } from "../lib.js";

const here = join(dirname(fileURLToPath(import.meta.url)), "..");
const root = join(here, "..", "..");
const manifest = JSON.parse(readFileSync(join(here, "manifest.json"), "utf8"));
const policy = readFileSync(join(root, "docs", "extension-privacy-policy.md"), "utf8");
const listing = readFileSync(join(root, "docs", "extension-store-listing.md"), "utf8");
const sources = readdirSync(here)
  .filter((f) => f.endsWith(".js"))
  .map((f) => [f, readFileSync(join(here, f), "utf8")]);

const withCommas = (n) => n.toLocaleString("en-US");

test("ex11 the privacy policy and the store listing state the limit and the address the code really uses", () => {
  assert.ok(policy.includes(`${withCommas(MAX_SELECTION)} characters`), "the policy names the selection limit the code enforces");
  assert.ok(listing.includes(`${withCommas(MAX_SELECTION)} characters`), "so does the listing");
  assert.deepEqual(manifest.host_permissions, ["http://127.0.0.1/*"]);
  for (const text of [policy, listing]) assert.ok(text.includes("127.0.0.1"));
  // the only address any source file mentions is the loopback one the documents name
  for (const [file, src] of sources) {
    for (const url of src.match(/https?:\/\/[^\s"'`)]+/g) ?? []) {
      assert.ok(/^http:\/\/127\.0\.0\.1/.test(url) || /\$\{/.test(url) || file === "formats.js", `${file} mentions ${url}`);
    }
  }
});

test("ex12 the short description is the manifest description and fits the store's limit", () => {
  const short = listing.split("## Short description (the manifest `description`)")[1].split("\n\n")[1].trim();
  assert.equal(short, manifest.description);
  assert.ok(manifest.description.length <= 132, `${manifest.description.length} characters`);
});

test("ex13 the permission justifications are exactly the permissions the manifest declares", () => {
  const block = listing.split("## Permission justifications (dashboard fields)")[1].split("## ")[0];
  const named = [...block.matchAll(/^- (?:Host permission )?`([^`]+)`/gm)].map((m) => m[1]).sort();
  const declared = [...manifest.permissions, ...manifest.host_permissions].sort();
  assert.deepEqual(named, declared);
});

test("ex14 what the policy says is saved in the browser is all the code saves, and none of it is synced", () => {
  const sets = sources.flatMap(([f, src]) => [...src.matchAll(/storage\.(local|sync|session)\.set\(\{([^}]*)\}/g)].map((m) => [f, m[1], m[2]]));
  assert.ok(sets.length >= 1);
  for (const [file, area, keys] of sets) {
    assert.equal(area, "local", `${file} saves outside local extension storage`);
    assert.deepEqual(keys.split(",").map((k) => k.split(":")[0].trim()).sort(), ["port", "token"], `${file} saves something the policy does not name`);
  }
  assert.ok(/not synced/.test(policy) && /port number/.test(policy) && /pairing token/.test(policy));
  for (const [file, src] of sources) assert.ok(!/storage\.sync/.test(src), `${file} uses synced storage`);
});

test("ex15 the claims that nothing is read until Send and that no analytics or remote code exists hold in the code", () => {
  assert.equal(manifest.content_scripts, undefined);
  assert.ok(!(manifest.permissions ?? []).some((p) => ["tabs", "history", "bookmarks", "cookies", "scripting", "activeTab", "webRequest"].includes(p)));
  for (const [file, src] of sources) {
    assert.ok(!/\b(gtag|analytics|ga\(|sendBeacon|XMLHttpRequest|WebSocket|EventSource)\b/i.test(src), `${file} has a network or analytics path the policy denies`);
  }
  for (const claim of ["no analytics", "no tracking", "loads no code from the internet"]) {
    assert.ok(listing.toLowerCase().includes(claim) || policy.toLowerCase().includes(claim), claim);
  }
});
