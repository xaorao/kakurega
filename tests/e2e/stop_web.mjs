// Stopping a running review from its own page and from the Activity list, in a real browser against the real service
// (scripted model, real sandbox).
//   node tests/e2e/stop_web.mjs      screenshots in target/stop-web/

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { cpSync, mkdirSync, renameSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/stop-web");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(root, { recursive: true });
cpSync(join(repo, "fixtures/synthetic-repo"), join(root, "payments-service"), { recursive: true });
renameSync(join(root, "payments-service/_env"), join(root, "payments-service/.env"));
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, "--scripted", join(repo, "tests/e2e/script.json"), "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
process.on("exit", () => { try { server.kill(); } catch {} });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 1000 } });
const page = await ctx.newPage();
const problems = [];
page.on("console", (m) => { if (m.type() === "error" && !/status of 409/.test(m.text())) problems.push("console: " + m.text()); });
page.on("pageerror", (e) => problems.push("pageerror: " + e.message));
page.on("requestfailed", (r) => { if (!/ERR_ABORTED/.test(r.failure()?.errorText ?? "")) problems.push("requestfailed: " + r.url()); });
const shot = (n) => page.screenshot({ path: join(work, `${n}.png`), fullPage: true });
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };
const origin = new URL(link).origin;
const startReview = (recipient) => page.evaluate(async ([folder, recipient]) => {
  const csrf = (await (await fetch("/api/session", { credentials: "same-origin" })).json()).csrf;
  const r = await fetch("/api/jobs", { method: "POST", headers: { "Content-Type": "application/json", "x-kk-csrf": csrf }, credentials: "same-origin", body: JSON.stringify({ repo: folder, recipient }) });
  return (await r.json()).id;
}, [join(root, "payments-service"), recipient]);

await page.goto(link);
await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();

// ---- from the review's own page ----
const id = await startReview("acme-audit");
await page.goto(`${origin}/#/job/${id}`);
const stop = page.getByRole("button", { name: "Stop", exact: true });
await stop.waitFor({ timeout: 15000 });
await shot("1-running");
await stop.click();
await page.getByText("Stopping now. Nothing it found will be offered.").waitFor({ timeout: 5000 }).catch(() => undefined);
await page.getByText(/stopped by you/).first().waitFor({ timeout: 30000 });
check(true, "a running review shows a Stop, and pressing it ends the review as stopped by you");
check((await page.getByRole("button", { name: "Stop", exact: true }).count()) === 0, "a review that has ended offers no Stop");
check(!/Approve|Ask for approval/.test(await page.locator("main").innerText()), "and nothing is offered for approval");
await shot("2-stopped");

// ---- from the Activity list ----
const id2 = await startReview("second-audit");
await page.goto(`${origin}/#/activity`);
const row = page.locator("#working .work-item").filter({ hasText: "second-audit" });
await row.waitFor({ timeout: 15000 });
check(/Review/.test(await row.innerText()) && (await row.getByRole("button", { name: "Stop", exact: true }).count()) === 1, "the Activity list shows the running review with a Stop");
await shot("3-activity");
await row.getByRole("button", { name: "Stop", exact: true }).click();
await page.waitForFunction(() => /Nothing is working right now/.test(document.querySelector("#working")?.textContent ?? ""), null, { timeout: 30000 });
check(true, "stopping it there empties the list");
const state = await page.evaluate(async (id) => (await (await fetch(`/api/jobs/${id}`, { credentials: "same-origin" })).json()), id2);
check(state.state === "failed" && /stopped by you/.test(JSON.stringify(state)), "and the review ended as stopped by you");
check(problems.length === 0, "no console errors or failed requests: " + problems.join(" | "));
await browser.close();
server.kill();
