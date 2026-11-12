// The header when the model on this computer is not answering: the model badge says so in amber, the Private badge stays
// (where data goes has not changed), a reply that comes back clears it, and a failed reply shows it again.
//   node tests/e2e/model_down_web.mjs

import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const work = join(repo, "target/model-down-web");
rmSync(work, { recursive: true, force: true });
mkdirSync(join(work, "root"), { recursive: true });
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const check = (cond, what) => { console.log(cond ? "PASS" : "FAIL", what); if (!cond) process.exitCode = 1; };

// a stand-in for Ollama, started and stopped by the test on a port chosen now
const model = createServer((req, res) => {
  let body = "";
  req.on("data", (d) => (body += d));
  req.on("end", () => {
    if (req.url.startsWith("/api/tags")) return res.end(JSON.stringify({ models: [{ name: "fake:latest" }] }));
    if (!req.url.startsWith("/api/chat")) { res.statusCode = 404; return res.end("{}"); }
    const msg = { role: "assistant", content: "fake answer" };
    const end = { done: true, prompt_eval_count: 10, eval_count: 2 };
    if (JSON.parse(body || "{}").stream) {
      res.setHeader("content-type", "application/x-ndjson");
      return res.end(JSON.stringify({ message: msg, done: false }) + "\n" + JSON.stringify({ message: { role: "assistant", content: "" }, ...end }) + "\n");
    }
    res.end(JSON.stringify({ message: msg, ...end }));
  });
});
await new Promise((r) => model.listen(0, "127.0.0.1", r));
const port = model.address().port;
await new Promise((r) => model.close(r));

const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", join(work, "root"), "--model", "fake", "--ollama", `http://127.0.0.1:${port}`, "--ui-dir", join(repo, "apps/web/dist")]);
process.on("exit", () => { try { server.kill(); } catch {} });
let log = "";
server.stdout.on("data", (d) => (log += d));
server.stderr.on("data", (d) => (log += d));
const deadline = Date.now() + 30000;
while (!/#pair=/.test(log) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)?.[0];
if (!link) throw new Error("no pairing link:\n" + log);

const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1280, height: 900 } })).newPage();
await page.goto(link);
await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" });
if (await skip.count()) await skip.click();
const badges = page.locator(".badges");
await badges.getByText("not answering").waitFor({ timeout: 30000 });
check((await badges.locator(".badge.warn", { hasText: "not answering" }).count()) === 1, "with the model service stopped, the model badge says it is not answering, in amber");
check(/Private: model runs on this computer/.test(await badges.innerText()), "and the Private badge stays: where data goes has not changed");
await page.screenshot({ path: join(work, "model-down.png") });

await new Promise((r) => model.listen(port, "127.0.0.1", r));
await page.getByRole("link", { name: "Chat", exact: true }).click();
await page.fill("#chat-input", "are you there?");
await page.keyboard.press("Enter");
await page.getByText("fake answer").first().waitFor({ timeout: 30000 });
await page.waitForTimeout(300);
check((await badges.getByText("not answering").count()) === 0, "a reply from the model clears it");

await new Promise((r) => model.close(r));
model.closeAllConnections?.();
await page.fill("#chat-input", "and now?");
await page.keyboard.press("Enter");
await page.getByRole("alert").filter({ hasText: "not running" }).first().waitFor({ timeout: 60000 });
await badges.getByText("not answering").waitFor({ timeout: 30000 });
check(true, "a reply that fails because the model stopped shows it again, and the message says how to start it");
await browser.close();
server.kill();
