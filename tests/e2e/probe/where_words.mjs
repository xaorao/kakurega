// On the NEAR route, prints every sentence on every screen that says "this computer" or "local", for a person to read
// against where the model really is (helper; not part of any gate).
//   KAKU_TEST_NEAR_MODEL=NAME KAKU_TEST_NEAR_KEY_FILE=FILE node tests/e2e/probe/where_words.mjs
import { chromium } from "playwright-core";
import { spawn } from "node:child_process";
import { cpSync, mkdirSync, renameSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";
const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const work = join(repo, "target/where-words");
rmSync(work, { recursive: true, force: true });
const root = join(work, "root");
mkdirSync(root, { recursive: true });
cpSync(join(repo, "fixtures/synthetic-repo"), join(root, "payments-service"), { recursive: true });
renameSync(join(root, "payments-service/_env"), join(root, "payments-service/.env"));
const chrome = join(homedir(), "Library/Caches/ms-playwright/chromium-1223/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing");
const server = spawn(join(repo, "target/debug/kaku"), ["serve", "--data-dir", join(work, "data"), "--root", root, "--model", "qwen2.5-coder:7b", "--route", "near", "--near-model", process.env.KAKU_TEST_NEAR_MODEL, "--near-key-file", process.env.KAKU_TEST_NEAR_KEY_FILE, "--ui-dir", join(repo, "apps/web/dist")], { stdio: ["ignore", "pipe", "pipe"] });
process.on("exit", () => { try { server.kill(); } catch {} });
let log = "";
server.stdout.on("data", (d) => (log += d)); server.stderr.on("data", (d) => (log += d));
while (!/#pair=/.test(log)) await new Promise((r) => setTimeout(r, 100));
const link = /http:\/\/127\.0\.0\.1:\d+\/#pair=[\w-]+/.exec(log)[0];
const browser = await chromium.launch({ executablePath: chrome, headless: true });
const page = await (await browser.newContext({ viewport: { width: 1280, height: 900 } })).newPage();
await page.goto(link); await page.waitForSelector("nav");
const skip = page.getByRole("button", { name: "Skip the rest" }); if (await skip.count()) await skip.click();
for (const r of ["", "chat", "docs", "meeting", "projects", "team", "roundtable", "memory", "instructions", "skills", "explore", "code", "handoff", "break", "agent", "activity", "privacy", "settings", "inbox"]) {
  await page.goto(new URL(link).origin + "/#/" + r); await page.waitForTimeout(1300);
  await page.locator("main details:not([open]) > summary").evaluateAll((els) => els.forEach((e) => e.click()));
  await page.waitForTimeout(300);
  const text = (await page.locator("body").innerText()).split(/\n+|(?<=[.!?])\s+/).map((s) => s.trim()).filter((s) => /this computer|\blocal\b|locally/i.test(s));
  console.log(`\n== ${r || "start"}\n` + [...new Set(text)].join("\n"));
}
await browser.close(); server.kill(); process.exit(0);
