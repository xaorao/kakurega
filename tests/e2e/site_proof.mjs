// The proof page as a visitor meets it, with the real NEAR testnet behind it: all six checks match; the only host asked
// besides the site is the public RPC; and a site that states a wrong fact is caught (hostile cases: a wrong code hash, a
// registry whose record is missing, an RPC that is down), so a green row is never painted without NEAR's answer.
//   node tests/e2e/site_proof.mjs      screenshots in target/site-proof/    (reads NEAR testnet; set SITE_URL for the live site)

import { chromium } from "playwright-core";
import { createServer } from "node:http";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/site-proof");
rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });
const built = spawnSync("node", [join(repo, "apps/site/build.mjs")], { encoding: "utf8" });
if (built.status !== 0) throw new Error(built.stderr);
const dir = join(repo, "apps/site/dist");
const types = { ".html": "text/html; charset=utf-8", ".css": "text/css", ".js": "text/javascript", ".json": "application/json", ".png": "image/png", ".wasm": "application/wasm", ".kkg": "application/octet-stream" };
// the headers the host will send (vercel.json), sent here too, so the policy is proven not to break the pages
const rules = JSON.parse(readFileSync(join(dir, "vercel.json"), "utf8")).headers.map((h) => ({ re: new RegExp("^" + h.source + "$"), headers: h.headers }));
const asked = [];
const srv = createServer((q, r) => {
  asked.push(q.url);
  const path0 = q.url.split("?")[0];
  let p = join(dir, decodeURIComponent(path0));
  if (!p.startsWith(dir)) { r.statusCode = 400; return r.end(); }
  if (existsSync(p) && statSync(p).isDirectory()) p = join(p, "index.html");
  if (!existsSync(p) || path0.startsWith("/api/") || path0 === "/vercel.json" || path0 === "/package.json") { r.statusCode = 404; return r.end("not found"); }
  r.setHeader("content-type", types[extname(p)] ?? "application/octet-stream");
  const path = q.url.split("?")[0];
  for (const rule of rules) if (rule.re.test(path)) for (const h of rule.headers) r.setHeader(h.key, h.value);
  r.end(readFileSync(p));
}).listen(0, "127.0.0.1");
await new Promise((r) => srv.on("listening", r));
// SITE_URL checks the deployed site in place of the local copy: the same visitor, the real host and its real headers
const origin = (process.env.SITE_URL ?? `http://127.0.0.1:${srv.address().port}`).replace(/\/$/, "");
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };


const RPC = "https://rpc.testnet.fastnear.com";
async function open(name, viewport, tamper) {
  const ctx = await browser.newContext({ viewport, colorScheme: name.includes("dark") ? "dark" : "light" });
  const page = await ctx.newPage();
  const problems = [];
  const hosts = new Set();
  page.on("console", (m) => { if (m.type() === "error") problems.push(m.text().slice(0, 160)); });
  page.on("pageerror", (e) => problems.push(e.message.slice(0, 160)));
  page.on("request", (r) => { if (!r.url().startsWith(origin) && !r.url().startsWith("data:")) hosts.add(new URL(r.url()).origin); });
  if (tamper) await tamper(page);
  await page.goto(origin + "/proof/");
  await page.locator("[data-summary]").filter({ hasText: /checks|could not/ }).waitFor({ timeout: 60000 });
  const states = await page.locator("[data-check]").evaluateAll((rows) => Object.fromEntries(rows.map((r) => [r.getAttribute("data-check"), r.getAttribute("data-state")])));
  const summary = await page.locator("[data-summary]").innerText();
  return { ctx, page, problems, hosts: [...hosts], states, summary };
}

// 1. the real thing, at two sizes and both themes
for (const [name, viewport] of [["desktop-light", { width: 1440, height: 900 }], ["phone-dark", { width: 390, height: 844 }]]) {
  const v = await open(name, viewport);
  check(Object.values(v.states).length === 6 && Object.values(v.states).every((s) => s === "ok"), `${name}: all six checks are answered by NEAR and match: ${JSON.stringify(v.states)}`);
  check(/^All 6 checks match/.test(v.summary), `${name}: the summary says so: ${v.summary}`);
  check(v.hosts.length === 1 && v.hosts[0] === RPC, `${name}: the only other host asked is NEAR's public RPC: ${v.hosts.join(", ")}`);
  const text = await v.page.locator("main").innerText();
  check(/Access keys on the account:\s*0/.test(text) && /UpToDate, no advisories/.test(text) && /tee-verifier-/.test(text) && /Access keys on the verifier's account\s*0/.test(text) && /\(as pinned\)/.test(text) && /Paid; required a hardware record: yes/.test(text), `${name}: the rows show NEAR's own answers (0 keys, the verifier, patch level, a paid order)`);
  check(await v.page.evaluate(() => document.documentElement.scrollWidth <= document.documentElement.clientWidth), `${name}: no sideways scroll`);
  check(v.problems.length === 0, `${name}: no console errors: ${v.problems.join(" | ")}`);
  await v.page.screenshot({ path: join(work, `${name}.png`), fullPage: true });
  await v.ctx.close();
}

if (!process.env.SITE_URL) {
  // 2. a site that states a wrong code hash is caught by NEAR's answer
  let v = await open("wrong-hash", { width: 1280, height: 900 }, (page) => page.route("**/proof.json", async (route) => {
    const j = JSON.parse(readFileSync(join(dir, "proof.json"), "utf8"));
    j.code_sha256 = "00" + j.code_sha256.slice(2);
    await route.fulfill({ contentType: "application/json", body: JSON.stringify(j) });
  }));
  check(v.states.code === "bad" && /1 of 6 checks did not match/.test(v.summary), `a wrong code hash on the site is shown as a mismatch, not passed: ${v.states.code}, ${v.summary}`);
  await v.ctx.close();
  // 3. a capsule NEAR has no record of is not shown as registered
  v = await open("no-record", { width: 1280, height: 900 }, (page) => page.route("**/proof.json", async (route) => {
    const j = JSON.parse(readFileSync(join(dir, "proof.json"), "utf8"));
    j.registered_capsule.capsule_sha256 = "ff".repeat(32);
    await route.fulfill({ contentType: "application/json", body: JSON.stringify(j) });
  }));
  check(v.states.order === "bad", `a capsule the registry does not hold is shown as not registered: ${v.states.order}`);
  await v.ctx.close();
  // 4. an RPC that is down gives no green row for anything it was asked, and the summary says checks could not be asked
  v = await open("rpc-down", { width: 1280, height: 900 }, (page) => page.route(RPC + "/**", (route) => route.abort()));
  check(["keys", "code", "hardware", "order", "counts"].every((k) => v.states[k] === "warn") && v.states.capsule === "ok" && /could not be asked/.test(v.summary), `with the RPC down, nothing asked of it is green: ${JSON.stringify(v.states)}; ${v.summary}`);
  await v.ctx.close();
  // 5. an RPC that lies about the access keys is reported, not hidden
  v = await open("has-key", { width: 1280, height: 900 }, (page) => page.route(RPC + "/**", async (route) => {
    const body = route.request().postDataJSON();
    if (body?.params?.request_type !== "view_access_key_list") return route.continue();
    await route.fulfill({ contentType: "application/json", headers: { "access-control-allow-origin": "*" }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, result: { block_height: 1, keys: [{ public_key: "ed25519:x", access_key: {} }] } }) });
  }));
  check(v.states.keys === "bad", `an account that has an access key is shown as a mismatch: ${v.states.keys}`);
  await v.ctx.close();
  // 6. answers from NEAR that do not support a claim are shown as mismatches: each is one changed answer of the real RPC
  const answerOf = async (route) => { const r = await route.fetch(); return { r, body: await r.json() }; };
  const reply = (route, body) => route.fulfill({ contentType: "application/json", headers: { "access-control-allow-origin": "*" }, body: JSON.stringify(body) });
  const rewriteView = (method, change) => (page) => page.route(RPC + "/**", async (route) => {
    const asked = route.request().postDataJSON()?.params ?? {};
    if (asked.request_type !== "call_function" || asked.method_name !== method) return route.continue();
    const { body } = await answerOf(route);
    const value = JSON.parse(Buffer.from(body.result.result).toString("utf8"));
    body.result.result = [...Buffer.from(JSON.stringify(change(value)))];
    await reply(route, body);
  });
  for (const [name, row, tamper] of [
    ["a hardware record that is out of date", "hardware", rewriteView("hardware", (h) => ({ ...h, tcb_status: "OutOfDate" }))],
    ["a hardware record with an open advisory", "hardware", rewriteView("hardware", (h) => ({ ...h, advisory_ids: ["INTEL-SA-00000"] }))],
    ["a hardware record for another key", "hardware", rewriteView("hardware", (h) => ({ ...h, signer: "00".repeat(32) }))],
    ["a registry that names another verifier", "hardware", rewriteView("verifier", () => "someone-else.testnet")],
    ["an order that did not require a hardware record", "order", rewriteView("order", (o) => ({ ...o, require_hardware: false }))],
    ["an order that is earned but not paid", "order", rewriteView("order", (o) => ({ ...o, state: "Earned" }))],
    ["an order for another capsule", "order", rewriteView("order", (o) => ({ ...o, capsule_sha256: "ee".repeat(32) }))],
    ["a registry with another fee", "counts", rewriteView("fee_terms", (f) => ({ ...f, bps: 900 }))],
    ["a verifier account that has an access key", "hardware", (page) => page.route(RPC + "/**", async (route) => {
      const asked = route.request().postDataJSON()?.params ?? {};
      if (asked.request_type !== "view_access_key_list" || !String(asked.account_id).startsWith("tee-verifier")) return route.continue();
      await reply(route, { jsonrpc: "2.0", id: 1, result: { block_height: 1, keys: [{ public_key: "ed25519:x", access_key: {} }] } });
    })],
    ["a verifier that no longer runs the pinned code", "hardware", (page) => page.route("**/proof.json", async (route) => {
      const j = JSON.parse(readFileSync(join(dir, "proof.json"), "utf8"));
      j.verifier_code_sha256 = "11".repeat(32);
      await route.fulfill({ contentType: "application/json", body: JSON.stringify(j) });
    })],
    ["a sample capsule with one byte changed", "capsule", (page) => page.route("**/sample/review.kkg", async (route) => {
      const bytes = readFileSync(join(dir, "sample/review.kkg"));
      bytes[Math.floor(bytes.length / 2)] ^= 0x01;
      await route.fulfill({ contentType: "application/octet-stream", body: bytes });
    })],
  ]) {
    v = await open("hostile", { width: 1280, height: 900 }, tamper);
    check(v.states[row] === "bad" && !/^All 6 checks match/.test(v.summary), `${name}: shown as a mismatch (${row}: ${v.states[row]})`);
    await v.ctx.close();
  }
  // 7. the machine-readable copies exist and agree with the page's facts
  const proof = JSON.parse(readFileSync(join(dir, "proof.json"), "utf8"));
  const sent = await fetch(RPC, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(proof.claims.find((c) => c.id === "keys").request) }).then((r) => r.json());
  check(Array.isArray(sent.result?.keys) && sent.result.keys.length === 0, "the request written in proof.json for the first claim, sent as it stands, gets the expected answer from NEAR");
  check(proof.claims.length === 6 && proof.sample_capsule.model_key.length === 64 && /proof\.json/.test(readFileSync(join(dir, "llms.txt"), "utf8")), "proof.json lists its claims and llms.txt points at it");
}
await browser.close();
srv.close();
