// The try-it page as a visitor uses it, LIVE on NEAR AI Cloud (one short answer; run by hand, not part of the gate):
// the detector withholds the keys in this tab, the hardware proof is checked in this tab, the question goes encrypted
// through the relay, and the model's signature is checked in this tab. The relay is watched: it must never see the
// code in plain text, and the withheld keys must never leave the page at all.
//   KAKU_TEST_NEAR_KEY_FILE=FILE node tests/e2e/site_try.mjs      screenshots in target/site-try/

import { chromium } from "playwright-core";
import { createServer } from "node:http";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/site-try");
rmSync(work, { recursive: true, force: true });
mkdirSync(work, { recursive: true });
const keyFile = process.env.KAKU_TEST_NEAR_KEY_FILE;
if (!keyFile) throw new Error("usage: KAKU_TEST_NEAR_KEY_FILE=FILE node tests/e2e/site_try.mjs");
const key = /NEAR_AI_API_KEY=(\S+)/.exec(readFileSync(keyFile, "utf8"))?.[1];
if (!key) throw new Error("the key file holds no NEAR_AI_API_KEY line");
const built = spawnSync("node", [join(repo, "apps/site/build.mjs")], { encoding: "utf8" });
if (built.status !== 0) throw new Error(built.stderr);
const dir = join(repo, "apps/site/dist");
process.env.NEAR_AI_RELAY_KEY = key;
// everything the relay passes on to NEAR is recorded here, to prove what it could and could not see
const relayed = [];
const realFetch = globalThis.fetch;
globalThis.fetch = async (url, init) => {
  if (String(url).startsWith("https://cloud-api.near.ai/")) relayed.push({ url: String(url), body: init?.body ? Buffer.from(init.body).toString("utf8") : "" });
  return realFetch(url, init);
};
const { default: relay } = await import(join(dir, "api/near.js"));
const config = JSON.parse(readFileSync(join(dir, "vercel.json"), "utf8"));
const rules = config.headers.map((h) => ({ re: new RegExp("^" + h.source + "$"), headers: h.headers }));
const types = { ".html": "text/html; charset=utf-8", ".css": "text/css", ".js": "text/javascript", ".json": "application/json", ".png": "image/png", ".wasm": "application/wasm" };
const srv = createServer((q, r) => {
  const path = q.url.split("?")[0];
  // the host's one rewrite: /api/near/v1/REST -> the function, with REST as `path`
  if (path.startsWith("/api/near/v1/")) {
    const rest = path.slice("/api/near/v1/".length);
    const query = q.url.includes("?") ? "&" + q.url.split("?")[1] : "";
    q.url = "/api/near?path=" + encodeURIComponent(rest).replace(/%2F/g, "/") + query;
    return void relay(q, r);
  }
  let p = join(dir, decodeURIComponent(path));
  if (!p.startsWith(dir)) { r.statusCode = 400; return r.end(); }
  if (existsSync(p) && statSync(p).isDirectory()) p = join(p, "index.html");
  if (!existsSync(p) || path.startsWith("/api/") || path === "/vercel.json" || path === "/package.json") { r.statusCode = 404; return r.end("not found"); }
  r.setHeader("content-type", types[extname(p)] ?? "application/octet-stream");
  for (const rule of rules) if (rule.re.test(path)) for (const h of rule.headers) r.setHeader(h.key, h.value);
  r.end(readFileSync(p));
}).listen(0, "127.0.0.1");
await new Promise((r) => srv.on("listening", r));
const origin = `http://127.0.0.1:${srv.address().port}`;
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };
const ctx = await browser.newContext({ viewport: { width: 1280, height: 1000 } });
const page = await ctx.newPage();
const problems = [];
const left = [];
page.on("console", (m) => { if (m.type() === "error") problems.push(m.text().slice(0, 200)); });
page.on("pageerror", (e) => problems.push(e.message.slice(0, 200)));
page.on("request", (r) => left.push({ url: r.url(), body: r.postData() ?? "" }));
await page.goto(origin + "/try/");
const code = await page.locator("[data-code]").inputValue();
const secrets = [/AKIA[A-Z0-9]{16}/.exec(code)?.[0], /sk_live_[A-Za-z0-9]+/.exec(code)?.[0]];
check(secrets.every(Boolean), "the sample holds two made-up keys of real shapes");
check(/3 free tries left/.test(await page.locator("[data-tries]").innerText()), "the page says how many free tries are left");
await page.screenshot({ path: join(work, "1-before.png") });
await page.getByRole("button", { name: "Withhold, check, ask" }).click();
const step = (name) => page.locator(`[data-step="${name}"]`);
await page.waitForFunction(() => ["ok", "warn", "bad"].includes(document.querySelector('[data-step="sign"]')?.getAttribute("data-state")) || document.querySelector('[data-state="bad"]'), null, { timeout: 180000 });
const texts = Object.fromEntries(await Promise.all(["withhold", "proof", "ask", "sign"].map(async (n) => [n, await step(n).innerText()])));
check(/2 lines were withheld/.test(texts.withhold) && /line 3: holds a/.test(texts.withhold) && /line 4: holds a/.test(texts.withhold), "step 1: the two lines with keys are withheld, by line: " + texts.withhold.replace(/\s+/g, " ").slice(0, 200));
check(/Gateway\s+Intel TDX verified/.test(texts.proof) && /Model\s+Qwen\/Qwen3\.8-27B: Intel TDX verified/.test(texts.proof) && /89154a43a4768474/.test(texts.proof), "step 2: gateway and model proofs are checked in the tab, and the model's key is the one the registry records: " + texts.proof.replace(/\s+/g, " ").slice(-160));
const answer = await step("ask").locator(".answer").innerText().catch(() => "");
check(answer.length > 40 && /logout|session|token|secret|hmac/i.test(answer), "step 3: the model answers about the code: " + answer.replace(/\s+/g, " ").slice(0, 200));
check(/Signed by the model's own hardware key, and the signature checks out/.test(texts.sign) && (await step("sign").getAttribute("data-state")) === "ok", "step 4: the model's own signature is checked in the tab" + ": " + texts.sign.replace(/\s+/g, " ").slice(-260));
check(/2 free tries left/.test(await page.locator("[data-tries]").innerText()), "one free try is counted");
// what left the page, and what the relay could see
const everything = left.map((r) => r.url + "\n" + r.body).join("\n");
check(secrets.every((s) => !everything.includes(s)), "the two withheld keys never left the page, in any request");
const chat = relayed.filter((r) => r.url.endsWith("/chat/completions"));
check(chat.length === 1 && !chat[0].body.includes("cookie.pop") && !chat[0].body.includes("dev-only-secret") && !chat[0].body.includes("ACTIVE"), "the relay passed on one chat and saw none of the code in plain text");
check(chat.length === 1 && JSON.parse(chat[0].body).model === "Qwen/Qwen3.8-27B" && JSON.parse(chat[0].body).max_tokens <= 500, "what it could read is the model's name and the length limit");
const hosts = [...new Set(left.map((r) => new URL(r.url).host))].filter((h) => h !== new URL(origin).host);
check(hosts.every((h) => ["cloud.near.ai", "nras.attestation.nvidia.com", "cloud-api.near.ai"].includes(h)), "the page spoke only to this site, NEAR and NVIDIA's verifier: " + hosts.join(", "));
check(!(await page.content()).includes(key) && !everything.includes(key), "the site's key is in neither the page nor anything the page sent");
check(problems.length === 0, "no console errors: " + problems.join(" | "));
await page.screenshot({ path: join(work, "2-after.png"), fullPage: true });
// with no free tries left the page asks for the visitor's own key and sends nothing
await page.evaluate(() => localStorage.setItem("kk-try-count", "3"));
await page.reload();
const before = relayed.length;
await page.getByRole("button", { name: "Withhold, check, ask" }).click();
await page.waitForTimeout(800);
check(/free tries are used/.test(await page.locator("[data-tries]").innerText()) && relayed.length === before && (await page.locator(".own-key").getAttribute("open")) !== null, "with the free tries used, nothing is sent and the page asks for your own key");
await browser.close();
srv.close();
