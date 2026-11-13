// Choosing a NEAR model from NEAR's own published list, as a person does it in Settings. LIVE: reads the public list
// from NEAR AI Cloud (no key, no spend), so it is run by hand and is not part of the gate.
//   node tests/e2e/near_models_web.mjs      screenshots in target/near-models-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/near-models-web");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root"), { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", join(work, "root"), "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
process.on("exit", () => { try { server.kill(); } catch {} });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);
const origin = new URL(link).origin;
const admin = JSON.parse(readFileSync(join(work, "data/admin.json"), "utf8"));
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };
for (const [name, viewport, scheme] of [["desktop-light", { width: 1280, height: 1000 }, "light"], ["phone-dark", { width: 390, height: 844 }, "dark"]]) {
  const ctx = await browser.newContext({ viewport, colorScheme: scheme });
  const page = await ctx.newPage();
  const problems = [];
  page.on("console", (m) => { if (m.type() === "error") problems.push(m.text()); });
  page.on("pageerror", (e) => problems.push(e.message));
  // each context pairs with a code of its own, as a second device would
  const code = (await (await fetch(origin + "/admin/pairing", { method: "POST", headers: { Authorization: "Bearer " + admin.token, "content-type": "application/json" }, body: "{}" })).json()).code;
  await page.goto(`${origin}/#pair=${code}`);
  await page.getByRole("button", { name: "Skip the rest" }).click({ timeout: 6000 }).catch(() => {});
  await page.goto(origin + "/#/settings");
  const fold = page.getByText("Choose from NEAR AI Cloud's list");
  await fold.waitFor({ timeout: 15000 });
  await fold.click();
  const rows = page.locator(".models-list .model-row");
  await rows.first().waitFor({ timeout: 30000 });
  const n = await rows.count();
  check(n >= 3, `${name}: NEAR's list is shown (${n} models)`);
  const first = await rows.first().innerText();
  const all = await rows.allInnerTexts();
  check(!all.some((t) => /FLUX|whisper|Embedding/i.test(t)), `${name}: image, speech and embedding models are not offered`);
  check(/hardware proof can be checked/.test(first) && /USD .* in, .* out per million tokens · window/.test(first), `${name}: a row gives the proof state, the prices and the window: ${first.replace(/\n/g, " | ")}`);
  const off = page.locator(".models-list .model-row:disabled");
  const offN = await off.count();
  check(offN === 0 || /no hardware proof: not used here/.test(await off.first().innerText()), `${name}: models without a proof (${offN}) are shown and cannot be chosen`);
  await page.getByPlaceholder("Find a model").fill("qwen");
  const found = await rows.allInnerTexts();
  check(found.length >= 1 && found.every((t) => /qwen/i.test(t)), `${name}: typing narrows the list (${found.length})`);
  await rows.first().click();
  const chosen = await page.locator("#route-model").inputValue();
  check(/qwen/i.test(chosen), `${name}: choosing a row fills the model field: ${chosen}`);
  await page.getByPlaceholder("Find a model").fill("zzzz-no-such");
  check((await page.getByText("No model by that name.").count()) === 1, `${name}: a name that matches nothing says so`);
  await page.getByPlaceholder("Find a model").fill("");
  await page.locator(".models-fold").scrollIntoViewIfNeeded();
  await page.screenshot({ path: join(work, `${name}.png`), fullPage: false });
  const wide = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1);
  check(!wide, `${name}: the page does not scroll sideways`);
  check(problems.length === 0, `${name}: no console errors: ${problems.join(" | ")}`);
  await ctx.close();
}
await browser.close();
server.kill();
